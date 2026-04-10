use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use tauri::{AppHandle, Emitter};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::audio_capture;
use crate::audio_combine::mixed_is_active;
use crate::audio_resampler::{AudioProcessingConfig, spawn_resampler_pipeline};
use crate::error::{AppError, AppResult};
use crate::soniox_runtime::{
    ActiveUtterance, FinalizedUtteranceCandidate, LateTranslationState,
    TranslationOnlyMatch, match_translation_only_update, parse_soniox_update,
};
use crate::storage;
use crate::transcript_buffer::TranscriptBuffer;
use crate::types::{
    AudioChunk, AudioSource, ResampledAudioFrame, SessionManifest, SessionState, SessionStatus,
    TranscriptSegment,
};
use crate::websocket_client::{WsEvent, spawn_soniox_pipeline};

const AUDIO_BACKLOG_CHANNEL_CAPACITY: usize = 256;
const AUDIO_SEND_CHANNEL_CAPACITY: usize = 256;
const DEFAULT_SILENCE_FLUSH_MS: u32 = 250;
const MIN_TARGET_CHUNK_MS: u32 = 60;
const MAX_TARGET_CHUNK_MS: u32 = 400;
const MIN_MAX_INFLIGHT_MS: u32 = 800;
const DEFAULT_MAX_INFLIGHT_MULTIPLIER: u32 = 8;
const DRAIN_COMPLETION_POLL_MS: u64 = 20;
const DRAIN_TIMEOUT_MS: u64 = 2_000;
const LATE_TRANSLATION_WINDOW_CAPACITY: usize = 6;
const LATE_TRANSLATION_TTL_MS: i64 = 3_000;

#[derive(Debug, Clone, Copy)]
struct SilenceConfig {
    mic_threshold: i16,
    system_threshold: i16,
}

impl Default for SilenceConfig {
    fn default() -> Self {
        Self {
            mic_threshold: 800,
            system_threshold: 800,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct AudioEgressPolicy {
    target_chunk_ms: u32,
    max_chunk_bytes: usize,
    silence_flush_ms: u32,
    max_inflight_ms: u32,
}

impl AudioEgressPolicy {
    fn from_audio_settings(audio_settings: &crate::types::AudioSettings) -> Self {
        let target_chunk_ms = audio_settings
            .chunk_duration_ms
            .clamp(MIN_TARGET_CHUNK_MS, MAX_TARGET_CHUNK_MS);
        let max_chunk_bytes = ((target_chunk_ms as usize * 16000) / 1000) * 2;
        let max_inflight_ms = (target_chunk_ms.saturating_mul(DEFAULT_MAX_INFLIGHT_MULTIPLIER))
            .max(MIN_MAX_INFLIGHT_MS);

        Self {
            target_chunk_ms,
            max_chunk_bytes,
            silence_flush_ms: DEFAULT_SILENCE_FLUSH_MS.max(target_chunk_ms),
            max_inflight_ms,
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum FlushReason {
    TargetDuration,
    MaxBytes,
    Silence,
    Tail,
}

impl FlushReason {
    fn as_str(self) -> &'static str {
        match self {
            FlushReason::TargetDuration => "target_duration",
            FlushReason::MaxBytes => "max_bytes",
            FlushReason::Silence => "silence",
            FlushReason::Tail => "tail",
        }
    }
}

#[derive(Debug)]
struct PendingChunkSeed {
    captured_at: chrono::DateTime<chrono::Utc>,
    duration_ms: u32,
    source: crate::types::AudioSource,
    pcm_bytes: Vec<u8>,
    flush_reason: FlushReason,
}

#[derive(Debug)]
struct AudioCollector {
    frames: Vec<ResampledAudioFrame>,
    buffered_duration_ms: u32,
    buffered_bytes: usize,
    trailing_silence_ms: u32,
    silence_config: SilenceConfig,
    policy: AudioEgressPolicy,
}

impl Default for AudioCollector {
    fn default() -> Self {
        Self {
            frames: Vec::new(),
            buffered_duration_ms: 0,
            buffered_bytes: 0,
            trailing_silence_ms: 0,
            silence_config: SilenceConfig::default(),
            policy: AudioEgressPolicy {
                target_chunk_ms: 100,
                max_chunk_bytes: ((100usize * 16000) / 1000) * 2,
                silence_flush_ms: DEFAULT_SILENCE_FLUSH_MS,
                max_inflight_ms: MIN_MAX_INFLIGHT_MS,
            },
        }
    }
}

impl AudioCollector {
    fn configure(&mut self, silence_config: SilenceConfig, policy: AudioEgressPolicy) {
        self.silence_config = silence_config;
        self.policy = policy;
    }

    fn push(&mut self, frame: ResampledAudioFrame) -> Option<PendingChunkSeed> {
        if frame.pcm_bytes.is_empty() || frame.duration_ms == 0 {
            return None;
        }

        if self.is_silent(&frame) {
            self.trailing_silence_ms = self.trailing_silence_ms.saturating_add(frame.duration_ms);
        } else {
            self.trailing_silence_ms = 0;
        }

        self.buffered_duration_ms = self.buffered_duration_ms.saturating_add(frame.duration_ms);
        self.buffered_bytes = self.buffered_bytes.saturating_add(frame.pcm_bytes.len());
        self.frames.push(frame);

        let flush_reason = if self.buffered_duration_ms >= self.policy.target_chunk_ms {
            Some(FlushReason::TargetDuration)
        } else if self.buffered_bytes >= self.policy.max_chunk_bytes {
            Some(FlushReason::MaxBytes)
        } else if self.trailing_silence_ms >= self.policy.silence_flush_ms {
            Some(FlushReason::Silence)
        } else {
            None
        };

        flush_reason.and_then(|reason| self.flush(reason))
    }

    fn flush_tail(&mut self) -> Option<PendingChunkSeed> {
        self.flush(FlushReason::Tail)
    }

    fn flush(&mut self, flush_reason: FlushReason) -> Option<PendingChunkSeed> {
        let first = self.frames.first()?.clone();
        let duration_ms = self.buffered_duration_ms;
        let source = first.source;
        let captured_at = first.captured_at;
        let mut pcm_bytes = Vec::with_capacity(self.buffered_bytes);
        for frame in self.frames.drain(..) {
            pcm_bytes.extend_from_slice(&frame.pcm_bytes);
        }

        self.buffered_duration_ms = 0;
        self.buffered_bytes = 0;
        self.trailing_silence_ms = 0;

        Some(PendingChunkSeed {
            captured_at,
            duration_ms,
            source,
            pcm_bytes,
            flush_reason,
        })
    }

    fn clear(&mut self) {
        self.frames.clear();
        self.buffered_duration_ms = 0;
        self.buffered_bytes = 0;
        self.trailing_silence_ms = 0;
    }

    fn max_inflight_ms(&self) -> u32 {
        self.policy.max_inflight_ms
    }

    fn is_silent(&self, frame: &ResampledAudioFrame) -> bool {
        if frame.pcm_bytes.len() < 2 {
            return true;
        }

        if frame.source == AudioSource::Mixed {
            return !mixed_is_active(&frame.activity);
        }

        let threshold = match frame.source {
            AudioSource::Mic => self.silence_config.mic_threshold,
            AudioSource::System => self.silence_config.system_threshold,
            AudioSource::Mixed => self
                .silence_config
                .mic_threshold
                .min(self.silence_config.system_threshold),
        };

        let mut peak = 0_i16;
        for sample in frame.pcm_bytes.chunks_exact(2) {
            let value = i16::from_le_bytes([sample[0], sample[1]]).abs();
            if value > peak {
                peak = value;
                if peak >= threshold {
                    return false;
                }
            }
        }

        true
    }
}

fn refresh_finalized_candidates(
    window: &mut VecDeque<FinalizedUtteranceCandidate>,
    session_generation: u32,
    now: DateTime<Utc>,
    ttl_ms: i64,
) {
    for candidate in window.iter_mut() {
        if candidate.late_translation_state == LateTranslationState::Accepting
            && (candidate.session_generation != session_generation
                || now
                    .signed_duration_since(candidate.finalized_at)
                    .num_milliseconds()
                    > ttl_ms)
        {
            candidate.late_translation_state = LateTranslationState::Closed;
        }
    }

    while window.front().is_some_and(|candidate| {
        matches!(
            candidate.late_translation_state,
            LateTranslationState::Closed | LateTranslationState::Expired
        )
    }) {
        if let Some(candidate) = window.front_mut() {
            candidate.late_translation_state = LateTranslationState::Expired;
        }
        let _ = window.pop_front();
    }
}

pub(crate) struct StartSessionContext {
    pub(crate) session_id: String,
    folder_path: PathBuf,
    started_at: chrono::DateTime<chrono::Utc>,
}

pub(crate) struct PipelineStartContext {
    pub(crate) pipeline_id: u64,
    pub(crate) session_generation: u32,
    pub(crate) shutdown_token: CancellationToken,
    pub(crate) resampled_rx: mpsc::Receiver<ResampledAudioFrame>,
    pub(crate) ws_tx: mpsc::Sender<AudioChunk>,
    pub(crate) ws_rx: mpsc::Receiver<AudioChunk>,
    pub(crate) event_tx: mpsc::Sender<WsEvent>,
    pub(crate) event_rx: mpsc::Receiver<WsEvent>,
}

struct StopSessionContext {
    should_wait_for_drain: bool,
}

/// Orchestrates the recording lifecycle.
/// Owns the state machine and coordinates audio capture, WebSocket, and storage.
pub struct SessionManager {
    state: SessionState,
    buffer: TranscriptBuffer,
    current_session_id: Option<String>,
    current_folder_path: Option<PathBuf>,
    manifest: Option<SessionManifest>,
    capture_cancel_token: Option<CancellationToken>,
    shutdown_token: Option<CancellationToken>,
    active_pipeline_id: Option<u64>,
    next_pipeline_id: u64,
    total_flushed: u32,
    session_generation: u32,
    next_audio_sequence: u64,
    pending_audio: VecDeque<AudioChunk>,
    inflight_audio: VecDeque<AudioChunk>,
    inflight_audio_ms: u32,
    audio_collector: AudioCollector,
}

impl SessionManager {
    pub fn new() -> Self {
        Self {
            state: SessionState::Idle,
            buffer: TranscriptBuffer::new(500),
            current_session_id: None,
            current_folder_path: None,
            manifest: None,
            capture_cancel_token: None,
            shutdown_token: None,
            active_pipeline_id: None,
            next_pipeline_id: 1,
            total_flushed: 0,
            session_generation: 0,
            next_audio_sequence: 1,
            pending_audio: VecDeque::new(),
            inflight_audio: VecDeque::new(),
            inflight_audio_ms: 0,
            audio_collector: AudioCollector::default(),
        }
    }

    pub fn state(&self) -> &SessionState {
        &self.state
    }

    pub fn resume_context(&self) -> AppResult<(String, String, String)> {
        match &self.state {
            SessionState::Paused { .. } => {}
            _ => {
                return Err(AppError::Session(
                    "Cannot resume: session is not paused".into(),
                ));
            }
        }

        let manifest = self
            .manifest
            .as_ref()
            .ok_or_else(|| AppError::Session("Missing session manifest for resume".into()))?;

        Ok((
            manifest.provider.clone(),
            manifest.source_language.clone(),
            manifest.target_language.clone(),
        ))
    }

    pub(crate) fn prepare_streaming_pipeline(
        &mut self,
        handle: &AppHandle,
        audio_settings: &crate::types::AudioSettings,
    ) -> AppResult<PipelineStartContext> {
        let pipeline_id = self.next_pipeline_id;
        self.next_pipeline_id += 1;
        self.session_generation = self.session_generation.saturating_add(1);
        let session_generation = self.session_generation;
        tracing::info!(pipeline_id, session_generation, "Starting streaming pipeline");

        let capture_cancel_token = CancellationToken::new();
        let shutdown_token = CancellationToken::new();
        self.capture_cancel_token = Some(capture_cancel_token.clone());
        self.shutdown_token = Some(shutdown_token.clone());
        self.active_pipeline_id = Some(pipeline_id);
        self.restore_inflight_to_pending();
        self.audio_collector.configure(
            SilenceConfig {
                mic_threshold: audio_settings.mic_silence_threshold,
                system_threshold: audio_settings.system_silence_threshold,
            },
            AudioEgressPolicy::from_audio_settings(audio_settings),
        );

        let capture_config = audio_capture::CaptureConfig {
            mode: audio_settings.capture_mode.clone(),
            mic_device_id: Some(audio_settings.mic_device_id.clone()),
            system_device_id: Some(audio_settings.system_device_id.clone()),
        };

        let (audio_tx, audio_rx) = crossbeam_channel::bounded(AUDIO_BACKLOG_CHANNEL_CAPACITY);
        let capture_runtime = audio_capture::start_capture_runtime(
            capture_config,
            audio_tx,
            capture_cancel_token.clone(),
        )?;
        for warning in &capture_runtime.warnings {
            self.emit_session_error(handle, warning, true);
        }

        let (resampled_tx, resampled_rx) = mpsc::channel::<ResampledAudioFrame>(AUDIO_SEND_CHANNEL_CAPACITY);
        let (ws_tx, ws_rx) = mpsc::channel::<AudioChunk>(AUDIO_SEND_CHANNEL_CAPACITY);
        let (event_tx, event_rx) = mpsc::channel::<WsEvent>(AUDIO_SEND_CHANNEL_CAPACITY);

        spawn_resampler_pipeline(
            capture_runtime,
            AudioProcessingConfig {
                mic_gain: audio_settings.mic_gain,
                system_gain: audio_settings.system_gain,
                mic_silence_threshold: audio_settings.mic_silence_threshold,
                system_silence_threshold: audio_settings.system_silence_threshold,
            },
            audio_rx,
            resampled_tx,
            event_tx.clone(),
            shutdown_token.clone(),
        )?;

        Ok(PipelineStartContext {
            pipeline_id,
            session_generation,
            shutdown_token,
            resampled_rx,
            ws_tx,
            ws_rx,
            event_tx,
            event_rx,
        })
    }

    pub(crate) async fn start_streaming_pipeline(
        handle: &AppHandle,
        sm_arc: Arc<tokio::sync::Mutex<SessionManager>>,
        api_key: &str,
        source_lang: &str,
        target_lang: &str,
        pipeline: PipelineStartContext,
        start_context: &StartSessionContext,
    ) -> AppResult<()> {
        let PipelineStartContext {
            pipeline_id,
            session_generation,
            shutdown_token,
            mut resampled_rx,
            ws_tx,
            ws_rx,
            event_tx,
            mut event_rx,
        } = pipeline;

        {
            let mut sm = sm_arc.lock().await;
            sm.finalize_start(handle, start_context)?;
        }

        spawn_soniox_pipeline(
            api_key,
            source_lang,
            target_lang,
            ws_rx,
            event_tx,
            shutdown_token.clone(),
        )
        .await?;

        let loop_handle = handle.clone();
        let loop_token = shutdown_token.clone();
        let loop_sm_arc = sm_arc.clone();
        let state_sm_arc = sm_arc.clone();
        tauri::async_runtime::spawn(async move {
            let mut active_utterance = ActiveUtterance::new();
            let mut finalized_window: VecDeque<FinalizedUtteranceCandidate> = VecDeque::new();
            let mut dropped_translation_only_updates: u64 = 0;
            let mut ambiguous_translation_only_updates: u64 = 0;
            let mut resampler_closed = false;
            let mut ws_tx = Some(ws_tx);

            loop {
                tokio::select! {
                    maybe_frame = resampled_rx.recv(), if !resampler_closed => {
                        match maybe_frame {
                            Some(frame) => {
                                {
                                    let mut sm = loop_sm_arc.lock().await;
                                    if !sm.is_pipeline_active(pipeline_id, &loop_token) {
                                        tracing::debug!("Ignoring resampled audio from stale pipeline {}", pipeline_id);
                                        break;
                                    }

                                    sm.enqueue_resampled_audio(frame, session_generation);
                                }
                                if let Some(ws_sender) = ws_tx.as_ref() {
                                    if let Err(err) = SessionManager::dispatch_audio_backlog(
                                        &loop_handle,
                                        &loop_sm_arc,
                                        pipeline_id,
                                        &loop_token,
                                        ws_sender,
                                    ).await {
                                        let message = format!("Failed dispatching audio backlog: {err}");
                                        tracing::error!("{message}");
                                        let mut sm = loop_sm_arc.lock().await;
                                        sm.emit_session_error(&loop_handle, &message, false);
                                        let _ = sm.stop_pipeline(&loop_handle, pipeline_id).await;
                                        break;
                                    }
                                }
                            }
                            None => {
                                resampler_closed = true;
                                {
                                    let mut sm = loop_sm_arc.lock().await;
                                    if !sm.is_pipeline_active(pipeline_id, &loop_token) {
                                        break;
                                    }

                                    sm.flush_audio_tail(session_generation);
                                }
                                if let Some(ws_sender) = ws_tx.as_ref() {
                                    if let Err(err) = SessionManager::dispatch_audio_backlog(
                                        &loop_handle,
                                        &loop_sm_arc,
                                        pipeline_id,
                                        &loop_token,
                                        ws_sender,
                                    ).await {
                                        let message = format!("Failed dispatching audio tail: {err}");
                                        tracing::error!("{message}");
                                        let sm = &mut *loop_sm_arc.lock().await;
                                        sm.emit_session_error(&loop_handle, &message, false);
                                        let _ = sm.stop_pipeline(&loop_handle, pipeline_id).await;
                                        break;
                                    }
                                }

                                let sm = &mut *loop_sm_arc.lock().await;
                                if resampler_closed && sm.is_drain_complete() {
                                    tracing::info!(pipeline_id, session_generation, "Closing Soniox audio sender after resampler shutdown and full drain");
                                    let _ = ws_tx.take();
                                }
                            }
                        }
                    }
                    maybe_event = event_rx.recv() => {
                        let Some(event) = maybe_event else {
                            break;
                        };

                        if loop_token.is_cancelled() && matches!(event, WsEvent::Finished) {
                            tracing::debug!("Soniox cleanly acknowledged gracefully closing token");
                            break;
                        }

                        match event {
                            WsEvent::AudioChunkSent { sequence } => {
                                {
                                    let mut sm = loop_sm_arc.lock().await;
                                    if !sm.is_pipeline_active(pipeline_id, &loop_token) {
                                        tracing::debug!("Ignoring audio ack from stale pipeline {}", pipeline_id);
                                        break;
                                    }

                                    sm.mark_audio_sent(sequence);
                                }
                                if let Some(ws_sender) = ws_tx.as_ref() {
                                    let _ = SessionManager::dispatch_audio_backlog(
                                        &loop_handle,
                                        &loop_sm_arc,
                                        pipeline_id,
                                        &loop_token,
                                        ws_sender,
                                    ).await;
                                }
                                let sm = &mut *loop_sm_arc.lock().await;
                                if resampler_closed && sm.is_drain_complete() {
                                    tracing::info!(pipeline_id, session_generation, "Closing Soniox audio sender after final audio ack and full drain");
                                    let _ = ws_tx.take();
                                }
                            }
                            WsEvent::Update(tokens) => {
                                if tokens.is_empty() {
                                    tracing::debug!(pipeline_id, session_generation, "Ignoring empty Soniox update");
                                    continue;
                                }

                                tracing::debug!(pipeline_id, session_generation, tokens_len = tokens.len(), "Received Soniox update in session manager");
                                let parsed = parse_soniox_update(&tokens);
                                tracing::debug!(
                                    pipeline_id,
                                    session_generation,
                                    has_spoken_text = parsed.has_spoken_text,
                                    has_translation_text = parsed.has_translation_text,
                                    has_spoken_final = parsed.has_spoken_final,
                                    is_translation_only = parsed.is_translation_only,
                                    start_ms = parsed.start_ms,
                                    end_ms = parsed.end_ms,
                                    original_final_len = parsed.original_final_delta.len(),
                                    original_nonfinal_len = parsed.original_nonfinal_snapshot.len(),
                                    translation_final_len = parsed.translation_final_delta.len(),
                                    translation_nonfinal_len = parsed.translation_nonfinal_snapshot.len(),
                                    "Parsed Soniox update summary"
                                );
                                let now = Utc::now();
                                refresh_finalized_candidates(
                                    &mut finalized_window,
                                    session_generation,
                                    now,
                                    LATE_TRANSLATION_TTL_MS,
                                );

                                if parsed.is_translation_only {
                                    match match_translation_only_update(
                                        &parsed,
                                        finalized_window.make_contiguous(),
                                        session_generation,
                                        now,
                                        LATE_TRANSLATION_TTL_MS,
                                    ) {
                                        TranslationOnlyMatch::Unique(index) => {
                                            let candidate = finalized_window
                                                .get_mut(index)
                                                .expect("matcher returned valid finalized candidate index");
                                            candidate.apply_translation_update(&parsed);

                                            let final_segment = candidate.build_final_segment();

                                            let mut sm = loop_sm_arc.lock().await;
                                            if !sm.is_pipeline_active(pipeline_id, &loop_token) {
                                                tracing::debug!(
                                                    "Ignoring late translation update from stale pipeline {}",
                                                    pipeline_id
                                                );
                                                break;
                                            }
                                            if let Err(e) =
                                                sm.on_transcript_update(vec![final_segment], &loop_handle)
                                            {
                                                let message = format!(
                                                    "Failed applying late translated transcript segment: {e}"
                                                );
                                                tracing::error!("{message}");
                                                sm.emit_session_error(&loop_handle, &message, false);
                                                let _ = sm.stop_pipeline(&loop_handle, pipeline_id).await;
                                                break;
                                            }
                                        }
                                        TranslationOnlyMatch::Ambiguous => {
                                            ambiguous_translation_only_updates += 1;
                                            tracing::warn!(
                                                pipeline_id,
                                                session_generation,
                                                start_ms = parsed.start_ms,
                                                end_ms = parsed.end_ms,
                                                ambiguous_translation_only_updates,
                                                "Dropping ambiguous translation-only Soniox update"
                                            );
                                        }
                                        TranslationOnlyMatch::NoMatch => {
                                            dropped_translation_only_updates += 1;
                                            tracing::warn!(
                                                pipeline_id,
                                                session_generation,
                                                start_ms = parsed.start_ms,
                                                end_ms = parsed.end_ms,
                                                dropped_translation_only_updates,
                                                "Dropping translation-only Soniox update without safe finalized match"
                                            );
                                        }
                                    }

                                    refresh_finalized_candidates(
                                        &mut finalized_window,
                                        session_generation,
                                        now,
                                        LATE_TRANSLATION_TTL_MS,
                                    );
                                    continue;
                                }

                                active_utterance.apply_update(&parsed);

                                if parsed.has_spoken_final {
                                    let Some(candidate) = active_utterance
                                        .clone()
                                        .into_finalized_candidate(session_generation)
                                    else {
                                        active_utterance = ActiveUtterance::new();
                                        continue;
                                    };

                                    let final_segment = candidate.build_final_segment();

                                    let mut sm = loop_sm_arc.lock().await;
                                    if !sm.is_pipeline_active(pipeline_id, &loop_token) {
                                        tracing::debug!(
                                            "Ignoring final utterance from stale pipeline {}",
                                            pipeline_id
                                        );
                                        break;
                                    }

                                    if let Err(e) = sm.on_transcript_update(vec![final_segment], &loop_handle)
                                    {
                                        let message = format!("Failed applying transcript segment: {e}");
                                        tracing::error!("{message}");
                                        sm.emit_session_error(&loop_handle, &message, false);
                                        let _ = sm.stop_pipeline(&loop_handle, pipeline_id).await;
                                        break;
                                    }

                                    finalized_window.push_back(candidate);
                                    while finalized_window.len() > LATE_TRANSLATION_WINDOW_CAPACITY {
                                        let _ = finalized_window.pop_front();
                                    }

                                    active_utterance = ActiveUtterance::new();
                                    continue;
                                }

                                let Some(partial_segment) = active_utterance.build_partial_segment() else {
                                    continue;
                                };

                                let mut sm = loop_sm_arc.lock().await;
                                if !sm.is_pipeline_active(pipeline_id, &loop_token) {
                                    tracing::debug!(
                                        "Ignoring transcript update from stale pipeline {}",
                                        pipeline_id
                                    );
                                    break;
                                }

                                if let Err(e) = sm.on_transcript_update(vec![partial_segment], &loop_handle)
                                {
                                    let message = format!("Failed applying transcript segment: {e}");
                                    tracing::error!("{message}");
                                    sm.emit_session_error(&loop_handle, &message, false);
                                    let _ = sm.stop_pipeline(&loop_handle, pipeline_id).await;
                                    break;
                                }
                            }
                            WsEvent::Finished => {
                                tracing::info!(pipeline_id, session_generation, "Transcription remote finished flag triggered");
                                break;
                            }
                            WsEvent::Error(err) => {
                                let mut sm = loop_sm_arc.lock().await;
                                if !sm.is_pipeline_active(pipeline_id, &loop_token) {
                                    tracing::debug!(
                                        "Suppressing session error from stale pipeline {}: {}",
                                        pipeline_id,
                                        err
                                    );
                                    break;
                                }

                                tracing::error!("WebSocket fatal error: {err}");
                                sm.handle_pipeline_runtime_error(&loop_handle, pipeline_id, &err);
                                break;
                            }
                        }
                    }
                }
            }
        });

        {
            let mut sm = state_sm_arc.lock().await;
            sm.update_active_audio_state(handle);
        }
        Ok(())
    }

    pub(crate) fn prepare_start(
        &mut self,
        handle: &AppHandle,
        source_lang: &str,
        target_lang: &str,
    ) -> AppResult<StartSessionContext> {
        if self.state != SessionState::Idle {
            return Err(AppError::Session(
                "Cannot start: session is not idle".into(),
            ));
        }

        self.set_state(SessionState::Connecting, handle);
        let (session_id, folder_path, manifest) =
            match storage::create_session_folder("soniox", source_lang, target_lang) {
                Ok(value) => value,
                Err(err) => {
                    self.set_state(SessionState::Idle, handle);
                    return Err(err);
                }
            };
        self.current_session_id = Some(session_id.clone());
        self.current_folder_path = Some(folder_path.clone());
        self.manifest = Some(manifest);
        self.total_flushed = 0;
        self.session_generation = 0;
        self.next_audio_sequence = 1;
        self.pending_audio.clear();
        self.inflight_audio.clear();
        self.audio_collector.clear();
        self.buffer.clear();

        Ok(StartSessionContext {
            session_id,
            folder_path,
            started_at: Utc::now(),
        })
    }

    pub(crate) fn finalize_start(
        &mut self,
        handle: &AppHandle,
        context: &StartSessionContext,
    ) -> AppResult<String> {
        let active_session_id = self.current_session_id.as_deref();
        let can_finalize = match &self.state {
            SessionState::Connecting => true,
            SessionState::Paused { session_id, .. } => session_id == &context.session_id,
            _ => false,
        };

        if !can_finalize || active_session_id != Some(context.session_id.as_str()) {
            return Err(AppError::Session("Session start was superseded before completion".into()));
        }

        self.set_state(
            SessionState::Recording {
                session_id: context.session_id.clone(),
                started_at: context.started_at,
            },
            handle,
        );

        tracing::info!("Session effectively running: {}", context.session_id);
        Ok(context.session_id.clone())
    }

    pub(crate) fn rollback_prepared_start(
        &mut self,
        handle: &AppHandle,
        context: &StartSessionContext,
    ) {
        if self.current_session_id.as_deref() != Some(context.session_id.as_str()) {
            return;
        }

        self.rollback_failed_start(handle, &context.folder_path);
    }

    pub fn pause(&mut self, handle: &AppHandle) -> AppResult<()> {
        let (session_id, started_at) = match &self.state {
            SessionState::Recording {
                session_id,
                started_at,
            } => (session_id.clone(), *started_at),
            SessionState::Buffering {
                session_id,
                started_at,
                ..
            }
            | SessionState::Reconnecting {
                session_id,
                started_at,
                ..
            } => (session_id.clone(), *started_at),
            _ => {
                return Err(AppError::Session(
                    "Cannot pause: session is not recording".into(),
                ));
            }
        };

        self.flush_audio_tail(self.session_generation);
        if let Some(token) = self.capture_cancel_token.take() {
            token.cancel();
        }
        if let Some(token) = self.shutdown_token.take() {
            token.cancel();
        }
        self.active_pipeline_id = None;
        self.restore_inflight_to_pending();

        self.persist_pending_segments()?;

        self.set_state(
            SessionState::Paused {
                session_id,
                started_at,
            },
            handle,
        );

        Ok(())
    }

    pub(crate) fn prepare_resume(
        &mut self,
        handle: &AppHandle,
        audio_settings: &crate::types::AudioSettings,
    ) -> AppResult<(StartSessionContext, PipelineStartContext)> {
        let (session_id, started_at) = match &self.state {
            SessionState::Paused {
                session_id,
                started_at,
            } => (session_id.clone(), *started_at),
            _ => {
                return Err(AppError::Session(
                    "Cannot resume: session is not paused".into(),
                ));
            }
        };

        let start_context = StartSessionContext {
            session_id,
            folder_path: self
                .current_folder_path
                .clone()
                .ok_or_else(|| AppError::Session("Missing session folder for resume pipeline start".into()))?,
            started_at,
        };
        let pipeline = self.prepare_streaming_pipeline(handle, audio_settings)?;
        Ok((start_context, pipeline))
    }

    /// Stop the current recording session.
    ///
    /// Transitions: Recording → Idle
    pub async fn stop(&mut self, handle: &AppHandle) -> AppResult<()> {
        self.stop_active_session(handle).await
    }

    /// Called when new transcript data arrives from the WebSocket.
    /// Updates the buffer and emits events to the frontend.
    pub fn on_transcript_update(
        &mut self,
        segments: Vec<TranscriptSegment>,
        handle: &AppHandle,
    ) -> AppResult<()> {
        let has_new_finalized_segment = segments.iter().any(|seg| seg.is_final);
        let segment_count = segments.len();
        let final_count = segments.iter().filter(|seg| seg.is_final).count();
        let preview = segments
            .iter()
            .map(|seg| format!(
                "id={} final={} original_len={} translated_len={}",
                seg.id,
                seg.is_final,
                seg.original_text.len(),
                seg.translated_text.len()
            ))
            .collect::<Vec<_>>()
            .join(" | ");

        tracing::debug!(segment_count, final_count, %preview, "Applying transcript update to buffer");

        for seg in segments {
            self.buffer.upsert(seg);
        }

        if has_new_finalized_segment {
            self.persist_pending_segments()?;
        }

        let snapshot = self.buffer.snapshot();
        tracing::debug!(snapshot_len = snapshot.len(), has_new_finalized_segment, "Emitting transcript_update event");
        let _ = handle.emit(
            "transcript_update",
            serde_json::json!({
                "segments": snapshot,
                "is_partial": true,
            }),
        );

        Ok(())
    }

    fn enqueue_resampled_audio(&mut self, frame: ResampledAudioFrame, session_generation: u32) {
        if let Some(seed) = self.audio_collector.push(frame) {
            self.push_pending_audio(seed, session_generation);
        }
        self.update_active_audio_state_internal();
    }

    fn flush_audio_tail(&mut self, session_generation: u32) {
        if let Some(seed) = self.audio_collector.flush_tail() {
            tracing::debug!(session_generation, duration_ms = seed.duration_ms, bytes = seed.pcm_bytes.len(), "Flushing tail audio chunk");
            self.push_pending_audio(seed, session_generation);
        } else {
            tracing::debug!(session_generation, "No buffered audio to flush at tail");
        }
        self.update_active_audio_state_internal();
    }

    fn push_pending_audio(&mut self, seed: PendingChunkSeed, session_generation: u32) {
        if seed.pcm_bytes.is_empty() || seed.duration_ms == 0 {
            return;
        }

        tracing::debug!(
            flush_reason = seed.flush_reason.as_str(),
            duration_ms = seed.duration_ms,
            bytes = seed.pcm_bytes.len(),
            "Queueing audio chunk"
        );

        let chunk = AudioChunk {
            sequence: self.next_audio_sequence,
            captured_at: seed.captured_at,
            duration_ms: seed.duration_ms,
            source: seed.source,
            session_generation,
            pcm_bytes: seed.pcm_bytes,
        };
        self.next_audio_sequence = self.next_audio_sequence.saturating_add(1);
        self.pending_audio.push_back(chunk);
    }

    fn take_next_dispatchable_chunk(
        &mut self,
        handle: &AppHandle,
        pipeline_id: u64,
    ) -> Option<AudioChunk> {
        if self.active_pipeline_id != Some(pipeline_id) {
            return None;
        }

        let max_inflight_ms = self.audio_collector.max_inflight_ms();
        while self.inflight_audio_ms < max_inflight_ms {
            let Some(chunk) = self.pending_audio.pop_front() else {
                break;
            };

            if chunk.session_generation != self.session_generation {
                tracing::warn!(
                    sequence = chunk.sequence,
                    chunk_generation = chunk.session_generation,
                    current_generation = self.session_generation,
                    pending_len = self.pending_audio.len(),
                    inflight_len = self.inflight_audio.len(),
                    "Dropping stale audio chunk from previous session generation"
                );
                continue;
            }

            self.inflight_audio_ms = self.inflight_audio_ms.saturating_add(chunk.duration_ms);
            self.inflight_audio.push_back(chunk.clone());
            self.update_active_audio_state(handle);
            return Some(chunk);
        }

        tracing::debug!(
            pending_ms = self.pending_audio_duration_ms(),
            inflight_ms = self.inflight_audio_ms,
            max_inflight_ms,
            "Audio backlog dispatched"
        );
        None
    }

    fn requeue_failed_dispatch(
        &mut self,
        handle: &AppHandle,
        chunk: AudioChunk,
    ) {
        if let Some(index) = self
            .inflight_audio
            .iter()
            .position(|queued| queued.sequence == chunk.sequence)
        {
            if let Some(removed) = self.inflight_audio.remove(index) {
                self.inflight_audio_ms = self.inflight_audio_ms.saturating_sub(removed.duration_ms);
                self.pending_audio.push_front(removed);
            }
        }
        self.update_active_audio_state(handle);
    }

    async fn dispatch_audio_backlog(
        handle: &AppHandle,
        sm_arc: &Arc<tokio::sync::Mutex<SessionManager>>,
        pipeline_id: u64,
        loop_token: &CancellationToken,
        ws_tx: &mpsc::Sender<AudioChunk>,
    ) -> AppResult<bool> {
        loop {
            let next_chunk = {
                let mut sm = sm_arc.lock().await;
                if !sm.is_pipeline_active(pipeline_id, loop_token) {
                    return Ok(false);
                }
                sm.take_next_dispatchable_chunk(handle, pipeline_id)
            };

            let Some(chunk) = next_chunk else {
                return Ok(true);
            };

            if ws_tx.send(chunk.clone()).await.is_err() {
                let mut sm = sm_arc.lock().await;
                if sm.active_pipeline_id == Some(pipeline_id) {
                    sm.requeue_failed_dispatch(handle, chunk);
                }
                return Err(AppError::Session(
                    "WebSocket sender closed while dispatching audio backlog".into(),
                ));
            }
        }
    }

    fn mark_audio_sent(&mut self, sequence: u64) {
        while let Some(front) = self.inflight_audio.front() {
            if front.sequence < sequence {
                let removed = self.inflight_audio.pop_front().expect("front exists");
                self.inflight_audio_ms = self.inflight_audio_ms.saturating_sub(removed.duration_ms);
                continue;
            }
            break;
        }

        if self
            .inflight_audio
            .front()
            .is_some_and(|chunk| chunk.sequence == sequence)
        {
            if let Some(removed) = self.inflight_audio.pop_front() {
                self.inflight_audio_ms = self.inflight_audio_ms.saturating_sub(removed.duration_ms);
            }
        }

        tracing::debug!(
            pending_ms = self.pending_audio_duration_ms(),
            inflight_ms = self.inflight_audio_ms,
            ack_sequence = sequence,
            "Audio chunk acknowledged"
        );

        self.update_active_audio_state_internal();
    }

    fn restore_inflight_to_pending(&mut self) {
        while let Some(chunk) = self.inflight_audio.pop_back() {
            self.inflight_audio_ms = self.inflight_audio_ms.saturating_sub(chunk.duration_ms);
            self.pending_audio.push_front(chunk);
        }
        self.update_active_audio_state_internal();
    }

    fn is_drain_complete(&self) -> bool {
        let is_complete = self.pending_audio.is_empty()
            && self.inflight_audio_ms == 0
            && self.audio_collector.buffered_duration_ms == 0;
        tracing::debug!(
            is_complete,
            pending_len = self.pending_audio.len(),
            pending_ms = self.pending_audio_duration_ms(),
            inflight_len = self.inflight_audio.len(),
            inflight_ms = self.inflight_audio_ms,
            buffered_ms = self.audio_collector.buffered_duration_ms,
            "Evaluated session drain state"
        );
        is_complete
    }

    fn clear_audio_runtime_state(&mut self) {
        self.pending_audio.clear();
        self.inflight_audio.clear();
        self.inflight_audio_ms = 0;
        self.audio_collector.clear();
    }

    fn pending_audio_duration_ms(&self) -> u32 {
        self.pending_audio.iter().map(|chunk| chunk.duration_ms).sum()
    }

    fn pending_backlog_ms(&self) -> u32 {
        self.pending_audio_duration_ms()
            .saturating_add(self.inflight_audio_ms)
            .saturating_add(self.audio_collector.buffered_duration_ms)
    }

    fn update_active_audio_state(&mut self, handle: &AppHandle) {
        self.update_active_audio_state_internal();
        let emitted_state = self.state.clone();
        let _ = handle.emit("session_state_changed", &emitted_state);
    }

    fn update_active_audio_state_internal(&mut self) {
        let Some(session_id) = self.current_session_id.clone() else {
            return;
        };

        let started_at = match &self.state {
            SessionState::Recording { started_at, .. }
            | SessionState::Buffering { started_at, .. }
            | SessionState::Reconnecting { started_at, .. }
            | SessionState::Draining { started_at, .. }
            | SessionState::Paused { started_at, .. } => *started_at,
            _ => return,
        };

        let backlog_ms = self.pending_backlog_ms();
        self.state = match &self.state {
            SessionState::Paused { .. } => SessionState::Paused {
                session_id,
                started_at,
            },
            SessionState::Draining { .. } => SessionState::Draining {
                session_id,
                started_at,
                backlog_ms,
            },
            SessionState::Reconnecting { .. } => SessionState::Reconnecting {
                session_id,
                started_at,
                backlog_ms,
                session_generation: self.session_generation,
            },
            SessionState::Connecting | SessionState::Buffering { .. } if backlog_ms > 0 => {
                SessionState::Buffering {
                    session_id,
                    started_at,
                    backlog_ms,
                    session_generation: self.session_generation,
                }
            }
            SessionState::Connecting | SessionState::Buffering { .. } => SessionState::Recording {
                session_id,
                started_at,
            },
            SessionState::Recording { .. } if backlog_ms > 0 => SessionState::Buffering {
                session_id,
                started_at,
                backlog_ms,
                session_generation: self.session_generation,
            },
            SessionState::Recording { .. } => SessionState::Recording {
                session_id,
                started_at,
            },
            other => other.clone(),
        };
    }

    fn handle_pipeline_runtime_error(&mut self, handle: &AppHandle, pipeline_id: u64, message: &str) {
        if self.active_pipeline_id != Some(pipeline_id) {
            return;
        }

        if let Some(token) = self.capture_cancel_token.take() {
            token.cancel();
        }
        self.flush_audio_tail(self.session_generation);
        self.active_pipeline_id = None;
        self.restore_inflight_to_pending();
        if let Some(token) = self.shutdown_token.take() {
            token.cancel();
        }

        if let Err(err) = self.persist_pending_segments() {
            tracing::error!("Failed persisting pending segments after recoverable runtime error: {err}");
        }

        if let Some((session_id, started_at)) = self.active_session_identity() {
            self.set_state(
                SessionState::Paused {
                    session_id,
                    started_at,
                },
                handle,
            );
        }

        self.emit_session_error(handle, message, true);
    }

    fn active_session_identity(&self) -> Option<(String, chrono::DateTime<chrono::Utc>)> {
        let started_at = self
            .manifest
            .as_ref()
            .map(|manifest| manifest.started_at)
            .unwrap_or_else(Utc::now);

        match &self.state {
            SessionState::Connecting => self
                .current_session_id
                .as_ref()
                .map(|session_id| (session_id.clone(), started_at)),
            SessionState::Buffering {
                session_id,
                started_at,
                ..
            }
            | SessionState::Recording {
                session_id,
                started_at,
            }
            | SessionState::Paused {
                session_id,
                started_at,
            }
            | SessionState::Reconnecting {
                session_id,
                started_at,
                ..
            }
            | SessionState::Draining {
                session_id,
                started_at,
                ..
            } => Some((session_id.clone(), *started_at)),
            _ => None,
        }
    }

    fn persist_pending_segments(&mut self) -> AppResult<()> {
        let pending = self.buffer.take_pending();
        if pending.is_empty() {
            return Ok(());
        }

        let persist_result = match (self.current_folder_path.as_ref(), self.manifest.as_mut()) {
            (Some(folder_path), Some(manifest)) => Self::append_segments_to_active_part(
                folder_path,
                manifest,
                &pending,
                &mut self.total_flushed,
            ),
            (None, None) => Err(AppError::Session(
                "Missing session folder path and manifest while persisting pending segments".into(),
            )),
            (None, Some(_)) => Err(AppError::Session(
                "Missing session folder path while persisting pending segments".into(),
            )),
            (Some(_), None) => Err(AppError::Session(
                "Missing session manifest while persisting pending segments".into(),
            )),
        };

        if persist_result.is_err() {
            self.buffer.restore_pending(pending);
        }

        persist_result
    }

    fn append_segments_to_active_part(
        folder_path: &Path,
        manifest: &mut SessionManifest,
        segments: &[TranscriptSegment],
        total_flushed: &mut u32,
    ) -> AppResult<()> {
        if segments.is_empty() {
            return Ok(());
        }

        let active_part = manifest
            .parts
            .last_mut()
            .ok_or_else(|| AppError::Session("No active part found".into()))?;
        let part_path = folder_path.join(&active_part.file);

        storage::append_segments(&part_path, segments)?;
        active_part.segments += segments.len() as u32;
        *total_flushed += segments.len() as u32;

        if active_part.segments >= 2000 {
            storage::finalize_part(&part_path, active_part.segments, SessionStatus::Completed)?;
            active_part.status = SessionStatus::Completed;

            let next_idx = manifest.parts.len() + 1;
            let next_file = format!("part-{:04}.jsonl", next_idx);
            let next_path = folder_path.join(&next_file);

            let header_line = crate::types::JsonlLine::Header {
                part_index: next_idx as u32,
                session_id: manifest.session_id.clone(),
                created_at: Utc::now(),
            };
            let mut file = std::fs::File::create(&next_path)?;
            std::io::Write::write_fmt(
                &mut file,
                format_args!("{}\n", serde_json::to_string(&header_line)?),
            )?;

            manifest.parts.push(crate::types::SessionPartMeta {
                file: next_file,
                status: SessionStatus::Active,
                segments: 0,
            });

            tracing::info!("Rotated transcript part to index {}", next_idx);
        }

        storage::atomic_write_manifest(folder_path, manifest)?;
        Ok(())
    }

    fn rollback_failed_start(&mut self, handle: &AppHandle, folder_path: &Path) {
        if let Some(token) = self.capture_cancel_token.take() {
            token.cancel();
        }
        if let Some(token) = self.shutdown_token.take() {
            token.cancel();
        }
        self.active_pipeline_id = None;

        let session_id = self.current_session_id.clone();
        self.current_session_id = None;
        self.current_folder_path = None;
        self.manifest = None;
        self.total_flushed = 0;
        self.session_generation = 0;
        self.next_audio_sequence = 1;
        self.clear_audio_runtime_state();
        self.buffer.clear();
        self.set_state(SessionState::Idle, handle);

        if let Some(id) = session_id {
            tracing::warn!("Rolled back failed session start: {id}");
        }

        if let Err(err) = std::fs::remove_dir_all(folder_path) {
            tracing::warn!(
                "Failed to remove orphaned session folder after start rollback ({}): {err}",
                folder_path.display()
            );
        }
    }

    fn is_pipeline_active(&self, pipeline_id: u64, pipeline_token: &CancellationToken) -> bool {
        !pipeline_token.is_cancelled() && self.active_pipeline_id == Some(pipeline_id)
    }

    async fn stop_pipeline(&mut self, handle: &AppHandle, pipeline_id: u64) -> AppResult<()> {
        if self.active_pipeline_id != Some(pipeline_id) {
            tracing::debug!(
                "Ignoring stop request from stale pipeline {} while active pipeline is {:?}",
                pipeline_id,
                self.active_pipeline_id
            );
            return Ok(());
        }

        self.stop_active_session_with_context(
            handle,
            StopSessionContext {
                should_wait_for_drain: false,
            },
        )
        .await
    }

    async fn stop_active_session(&mut self, handle: &AppHandle) -> AppResult<()> {
        self.stop_active_session_with_context(
            handle,
            StopSessionContext {
                should_wait_for_drain: true,
            },
        )
        .await
    }

    async fn stop_active_session_with_context(
        &mut self,
        handle: &AppHandle,
        context: StopSessionContext,
    ) -> AppResult<()> {
        match &self.state {
            SessionState::Connecting
            | SessionState::Buffering { .. }
            | SessionState::Recording { .. }
            | SessionState::Paused { .. }
            | SessionState::Reconnecting { .. }
            | SessionState::Draining { .. } => {}
            _ => {
                return Err(AppError::Session("Cannot stop: no active session".into()));
            }
        }

        let session_identity = self.active_session_identity();
        if let Some((session_id, started_at)) = session_identity {
            self.set_state(
                SessionState::Draining {
                    session_id,
                    started_at,
                    backlog_ms: self.pending_backlog_ms(),
                },
                handle,
            );
        }

        tracing::info!(
            should_wait_for_drain = context.should_wait_for_drain,
            session_generation = self.session_generation,
            pending_len = self.pending_audio.len(),
            inflight_len = self.inflight_audio.len(),
            pending_ms = self.pending_audio_duration_ms(),
            inflight_ms = self.inflight_audio_ms,
            buffered_ms = self.audio_collector.buffered_duration_ms,
            "Stopping active session"
        );

        self.flush_audio_tail(self.session_generation);

        if let Some(token) = self.capture_cancel_token.take() {
            token.cancel();
        }

        if context.should_wait_for_drain {
            let drain_deadline = tokio::time::Instant::now() + Duration::from_millis(DRAIN_TIMEOUT_MS);
            while !self.is_drain_complete() && tokio::time::Instant::now() < drain_deadline {
                tokio::time::sleep(Duration::from_millis(DRAIN_COMPLETION_POLL_MS)).await;
            }
        }

        self.active_pipeline_id = None;
        self.restore_inflight_to_pending();

        if let Some(token) = self.shutdown_token.take() {
            tracing::info!("Cancelling websocket shutdown token");
            token.cancel();
        }

        let remaining = self.buffer.flush_all();
        let session_id = self.current_session_id.take();
        let folder_path = self.current_folder_path.take();
        let mut manifest = self.manifest.take();

        let persist_result =
            Self::persist_stopped_session(folder_path.as_deref(), manifest.as_mut(), &remaining);

        self.buffer.clear();
        self.total_flushed = 0;
        self.session_generation = 0;
        self.next_audio_sequence = 1;
        self.clear_audio_runtime_state();
        self.set_state(SessionState::Idle, handle);

        if let Some(id) = &session_id {
            tracing::info!("Session stopped: {id}");
        }

        if let Err(err) = &persist_result {
            tracing::error!("Session persistence failed during stop: {err}");
        }

        persist_result
    }

    fn persist_stopped_session(
        folder_path: Option<&Path>,
        manifest: Option<&mut SessionManifest>,
        remaining: &[TranscriptSegment],
    ) -> AppResult<()> {
        match (folder_path, manifest) {
            (Some(folder_path), Some(manifest)) => {
                let active_part = manifest
                    .parts
                    .last_mut()
                    .ok_or_else(|| AppError::Session("Missing active part in manifest".into()))?;
                let part_path = folder_path.join(&active_part.file);

                if !remaining.is_empty() {
                    storage::append_segments(&part_path, remaining)?;
                    active_part.segments += remaining.len() as u32;
                }

                storage::finalize_part(&part_path, active_part.segments, SessionStatus::Completed)?;

                active_part.status = SessionStatus::Completed;
                manifest.status = SessionStatus::Completed;
                storage::atomic_write_manifest(folder_path, manifest)?;
                Ok(())
            }
            (None, None) => Ok(()),
            _ => Err(AppError::Session(
                "Session persistence state is inconsistent during stop".into(),
            )),
        }
    }

    fn emit_session_error(&self, handle: &AppHandle, message: &str, recoverable: bool) {
        let _ = handle.emit(
            "session_error",
            serde_json::json!({
                "message": message,
                "recoverable": recoverable,
            }),
        );
    }

    fn set_state(&mut self, new_state: SessionState, handle: &AppHandle) {
        tracing::info!("Session state: {:?} → {:?}", self.state, new_state);
        self.state = new_state.clone();
        let _ = handle.emit("session_state_changed", &new_state);
    }
}
