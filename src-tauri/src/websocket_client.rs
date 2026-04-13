use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tokio::time::{Instant, sleep_until, timeout};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::protocol::Message;
use tokio_util::sync::CancellationToken;

use crate::error::{AppError, AppResult};
use crate::types::AudioChunk;

const SONIOX_WS_URL: &str = "wss://stt-rt.soniox.com/transcribe-websocket";
const SONIOX_WS_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

// ── Models ───────────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct SonioxTranslationConfig {
    pub r#type: String,
    pub target_language: String,
}

#[derive(Serialize)]
pub struct SonioxStartRequest {
    pub api_key: String,
    pub model: String,
    pub audio_format: String,
    pub sample_rate: u32,
    pub num_channels: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language_hints: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub translation: Option<SonioxTranslationConfig>,
    pub enable_endpoint_detection: bool,
}

#[derive(Deserialize, Debug)]
pub struct SonioxResponse {
    pub tokens: Option<Vec<SonioxToken>>,
    pub finished: Option<bool>,
    pub error_code: Option<u16>,
    pub error_message: Option<String>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct SonioxToken {
    pub text: String,
    #[serde(default)]
    pub start_ms: u64,
    #[serde(default)]
    pub end_ms: u64,
    pub speaker: Option<String>,
    pub is_final: bool,
    #[serde(default)]
    pub translation_status: Option<String>,
}

pub enum WsEvent {
    Update(Vec<SonioxToken>),
    Finished,
    Error(String),
    AudioChunkSent { sequence: u64 },
}

// ── Pipeline Orchestrator ────────────────────────────────────────────────────

/// Spawns the bidirectional WebSocket pipeline to Soniox.
/// Automatically handles the JSON auth handshake, binary audio pushing, and response parsing.
pub async fn spawn_soniox_pipeline(
    api_key: &str,
    source_language: &str,
    target_language: &str,
    mut audio_rx: mpsc::Receiver<AudioChunk>,
    event_tx: mpsc::Sender<WsEvent>,
    shutdown_token: CancellationToken,
) -> AppResult<()> {
    tracing::info!("Connecting to Soniox API at {}", SONIOX_WS_URL);

    let (ws_stream, _) = timeout(SONIOX_WS_CONNECT_TIMEOUT, connect_async(SONIOX_WS_URL))
        .await
        .map_err(|_| {
            AppError::WebSocket(format!(
                "Timed out connecting to {} after {} seconds",
                SONIOX_WS_URL,
                SONIOX_WS_CONNECT_TIMEOUT.as_secs()
            ))
        })?
        .map_err(|e| AppError::WebSocket(format!("Failed to connect to {}: {e}", SONIOX_WS_URL)))?;

    let (mut write_half, mut read_half) = ws_stream.split();

    let trimmed_source_language = source_language.trim();
    let trimmed_target_language = target_language.trim();
    let req = SonioxStartRequest {
        api_key: api_key.to_string(),
        model: "stt-rt-v4".to_string(),
        audio_format: "pcm_s16le".to_string(),
        sample_rate: 16000,
        num_channels: 1,
        language_hints: if trimmed_source_language.is_empty() || trimmed_source_language == "auto" {
            None
        } else {
            Some(vec![trimmed_source_language.to_string()])
        },
        translation: if trimmed_target_language.is_empty() {
            None
        } else {
            Some(SonioxTranslationConfig {
                r#type: "one_way".to_string(),
                target_language: trimmed_target_language.to_string(),
            })
        },
        enable_endpoint_detection: true,
    };

    let config_json = serde_json::to_string(&req)?;
    write_half
        .send(Message::Text(config_json.into()))
        .await
        .map_err(|e| AppError::WebSocket(format!("Failed to send config: {e}")))?;
    tracing::debug!("Soniox Handshake pushed");

    let mut sent_audio_frames: u64 = 0;

    let read_event_tx = event_tx.clone();
    let read_shutdown_token = shutdown_token.clone();
    tokio::spawn(async move {
        while let Some(msg) = read_half.next().await {
            match msg {
                Ok(Message::Text(txt)) => {
                    tracing::debug!(payload_len = txt.len(), "Received Soniox text frame");
                    match serde_json::from_str::<SonioxResponse>(&txt) {
                    Ok(res) => {
                        let tokens_len = res.tokens.as_ref().map_or(0, Vec::len);
                        tracing::debug!(
                            payload_len = txt.len(),
                            finished = res.finished.unwrap_or(false),
                            error_code = res.error_code,
                            tokens_len,
                            "Parsed Soniox text frame"
                        );
                        if let Some(err_msg) = res.error_message {
                            let message = match res.error_code {
                                Some(error_code) => format!("Soniox error {error_code}: {err_msg}"),
                                None => err_msg,
                            };
                            if read_shutdown_token.is_cancelled() {
                                tracing::debug!("Suppressing Soniox error after cancellation: {message}");
                            } else {
                                let _ = read_event_tx.send(WsEvent::Error(message)).await;
                            }
                            break;
                        }
                        if res.finished == Some(true) {
                            tracing::info!("Received Soniox finished frame");
                            let _ = read_event_tx.send(WsEvent::Finished).await;
                            break;
                        }
                        if let Some(tokens) = res.tokens {
                            if let Some(first_token) = tokens.first() {
                                tracing::debug!(
                                    first_text = %first_token.text,
                                    first_is_final = first_token.is_final,
                                    first_translation_status = ?first_token.translation_status,
                                    first_start_ms = first_token.start_ms,
                                    first_end_ms = first_token.end_ms,
                                    "Soniox token preview"
                                );
                            }
                            let tokens: Vec<_> = tokens
                                .into_iter()
                                .filter(|token| !token.text.trim().is_empty())
                                .collect();
                            if !tokens.is_empty() {
                                tracing::debug!(tokens_len = tokens.len(), "Forwarding Soniox update tokens");
                                let _ = read_event_tx.send(WsEvent::Update(tokens)).await;
                            } else {
                                tracing::debug!("Dropping Soniox text frame because all tokens were empty after filtering");
                            }
                        } else {
                            tracing::warn!(payload = %txt, "Soniox text frame matched schema but had no tokens, finished, or error_message");
                        }
                    }
                    Err(error) => {
                        tracing::warn!(
                            payload = %txt,
                            payload_len = txt.len(),
                            "Ignoring unparseable Soniox text frame: {error}"
                        );
                    }
                }
                },
                Ok(Message::Close(frame)) => {
                    if read_shutdown_token.is_cancelled() {
                        tracing::debug!("Soniox WS closed after cancellation");
                    } else {
                        let close_reason = frame
                            .as_ref()
                            .map(|close_frame| format!("code={} reason={}", close_frame.code, close_frame.reason))
                            .unwrap_or_else(|| "no close frame".to_string());
                        tracing::warn!(close_reason, "Soniox WS closed remotely before reporting a terminal event");
                        let _ = read_event_tx
                            .send(WsEvent::Error(format!("WebSocket Closed prematurely ({close_reason})")))
                            .await;
                    }
                    break;
                }
                Err(e) => {
                    if read_shutdown_token.is_cancelled() {
                        tracing::debug!("Suppressing Soniox socket error after cancellation: {e}");
                    } else {
                        let _ = read_event_tx.send(WsEvent::Error(e.to_string())).await;
                    }
                    break;
                }
                _ => {}
            }
        }
    });

    tokio::spawn(async move {
        let mut next_send_deadline = Instant::now();
        let mut eof_sent = false;

        loop {
            tokio::select! {
                chunk_opt = audio_rx.recv(), if !eof_sent => {
                    match chunk_opt {
                        Some(chunk) => {
                            let sequence = chunk.sequence;
                            let duration_ms = chunk.duration_ms;
                            let scheduled_deadline = next_send_deadline;
                            sleep_until(scheduled_deadline).await;
                            if let Err(e) = write_half.send(Message::Binary(chunk.pcm_bytes.into())).await {
                                let message = format!("Failed sending audio frame: {}", e);
                                tracing::error!("{message}");
                                if !shutdown_token.is_cancelled() {
                                    let _ = event_tx.send(WsEvent::Error(message)).await;
                                }
                                break;
                            }

                            let send_duration = Duration::from_millis(u64::from(duration_ms));
                            let now = Instant::now();
                            next_send_deadline = scheduled_deadline.max(now) + send_duration;
                            let drift_ms = now.checked_duration_since(scheduled_deadline).map(|drift| drift.as_millis() as u64).unwrap_or(0);
                            tracing::debug!(sequence, duration_ms, drift_ms, "Audio frame sent to Soniox websocket");
                            sent_audio_frames = sent_audio_frames.saturating_add(1);
                            if sent_audio_frames == 1 {
                                tracing::info!(sequence, duration_ms, "Sent first audio frame to Soniox websocket");
                            }

                            if event_tx.send(WsEvent::AudioChunkSent { sequence }).await.is_err() {
                                break;
                            }
                        }
                        None => {
                            tracing::info!("Audio sender closed, sending EOF frame to Soniox");
                            let _ = write_half.send(Message::Binary(Vec::new().into())).await;
                            eof_sent = true;
                        }
                    }
                }
                _ = shutdown_token.cancelled(), if !eof_sent => {
                    tracing::info!("Shutdown requested, sending EOF frame to Soniox...");
                    let _ = write_half.send(Message::Binary(Vec::new().into())).await;
                    eof_sent = true;
                }
                else => {
                    break;
                }
            }
        }
    });

    Ok(())
}
