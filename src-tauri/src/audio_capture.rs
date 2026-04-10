use std::sync::mpsc;
use std::time::Duration;

use chrono::Utc;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Sample, SampleFormat, Stream, StreamConfig};
use crossbeam_channel::Sender;
use tokio_util::sync::CancellationToken;

use crate::audio_backend::device_identity::{
    backend_namespace, fingerprint_payload, microphone_rematch_score, namespaced_id,
    normalize_label, parse_v2_fingerprint, parse_v2_fingerprint_payload, split_namespaced_id,
    versioned_id, InputFingerprint,
};
use crate::audio_backend::host_selection::selected_input_host;
use crate::audio_backend::{self, CaptureRuntime, SourceStreamFormat};
use crate::error::{AppError, AppResult};
use crate::types::{
    AudioBackendKind, AudioCaptureMode, AudioDevice, AudioSource, CapturedAudioFrame,
    PhysicalAudioSource,
};

#[derive(Debug, Clone)]
pub struct CaptureConfig {
    pub mode: AudioCaptureMode,
    pub mic_device_id: Option<String>,
    pub system_device_id: Option<String>,
}

/// List available audio input devices.
pub fn list_devices() -> AppResult<Vec<AudioDevice>> {
    let host = selected_input_host()?;

    let default_name = host
        .default_input_device()
        .and_then(|d| d.description().ok().map(|desc| desc.name().to_string()))
        .unwrap_or_default();

    let devices = host
        .input_devices()
        .map_err(|e| AppError::AudioCapture(format!("Cannot enumerate devices: {e}")))?;

    let mut result = Vec::new();
    for device in devices {
        if let Ok(description) = device.description() {
            let name = description.name().to_string();
            result.push(AudioDevice {
                is_default: name == default_name,
                name: namespaced_id(backend_namespace(AudioBackendKind::Cpal), &name),
            });
        }
    }

    Ok(result)
}

fn build_input_capability_signature(device: &cpal::Device) -> String {
    let Ok(configs) = device.supported_input_configs() else {
        return "unknown".into();
    };

    let mut signatures = Vec::new();
    for config in configs {
        signatures.push(format!(
            "{}:{}-{}:{:?}",
            config.channels(),
            config.min_sample_rate(),
            config.max_sample_rate(),
            config.sample_format()
        ));
    }
    signatures.sort();
    signatures.join(",")
}

fn cpal_input_fingerprint(device: &cpal::Device) -> Option<InputFingerprint> {
    let description = device.description().ok()?;
    let name = description.name().to_string();
    let default = device.default_input_config().ok();
    let default_sample_rate = default.as_ref().map(|cfg| cfg.sample_rate()).unwrap_or_default();
    let default_channels = default.as_ref().map(|cfg| cfg.channels()).unwrap_or_default();
    let default_sample_format = default
        .as_ref()
        .map(|cfg| format!("{:?}", cfg.sample_format()))
        .unwrap_or_else(|| "unknown".into());

    Some(InputFingerprint {
        normalized_label: normalize_label(&name),
        default_sample_rate,
        default_channels,
        default_sample_format,
        capability_signature: build_input_capability_signature(device),
    })
}

fn cpal_input_device_id(device: &cpal::Device) -> Option<String> {
    let fingerprint = cpal_input_fingerprint(device)?;
    Some(versioned_id("cpal", "v2", &fingerprint_payload(&fingerprint)))
}

fn resolve_input_device(host: &cpal::Host, device_id: Option<&str>) -> AppResult<cpal::Device> {
    if let Some(device_id) = device_id.filter(|value| *value != "default") {
        if let Some((namespace, payload)) = split_namespaced_id(device_id) {
            if namespace == backend_namespace(AudioBackendKind::Cpal)
                && parse_v2_fingerprint_payload(payload).is_some()
            {
                if let Some(device) = host
                    .input_devices()
                    .map_err(|e| AppError::AudioCapture(e.to_string()))?
                    .find(|device| cpal_input_device_id(device).as_deref() == Some(device_id))
                {
                    return Ok(device);
                }

                let saved = parse_v2_fingerprint(payload).ok_or_else(|| {
                    AppError::AudioCapture(format!("Invalid CPAL microphone fingerprint: {device_id}"))
                })?;
                let mut best: Option<(u32, cpal::Device)> = None;
                for candidate in host
                    .input_devices()
                    .map_err(|e| AppError::AudioCapture(e.to_string()))?
                {
                    let Some(candidate_fp) = cpal_input_fingerprint(&candidate) else {
                        continue;
                    };
                    let score = microphone_rematch_score(&saved, &candidate_fp);
                    if score >= 75 {
                        match &best {
                            Some((best_score, _)) if *best_score >= score => {}
                            _ => best = Some((score, candidate)),
                        }
                    }
                }
                if let Some((_, device)) = best {
                    return Ok(device);
                }

                return host.default_input_device().ok_or_else(|| {
                    AppError::AudioCapture("No default input device available".into())
                });
            }
        }

        let raw_device_id = match split_namespaced_id(device_id) {
            Some((namespace, raw_device_id)) if namespace == backend_namespace(AudioBackendKind::Cpal) => {
                raw_device_id
            }
            _ => device_id,
        };

        host.input_devices()
            .map_err(|e| AppError::AudioCapture(e.to_string()))?
            .find(|device| {
                device
                    .description()
                    .map(|desc| desc.name() == raw_device_id)
                    .unwrap_or(false)
            })
            .ok_or_else(|| AppError::AudioCapture(format!("Device not found: {device_id}")))
    } else {
        host.default_input_device()
            .ok_or_else(|| AppError::AudioCapture("No default input device available".into()))
    }
}

/// Start capturing audio.
/// Opens the CPAL stream, forces formatting checks, and dispatches chunks of f32 to the `Sender`.
/// Moves the non-Send stream into a detached OS thread and keeps it alive until the CancellationToken is cancelled.
pub fn preflight_capture(config: &CaptureConfig) -> AppResult<()> {
    match config.mode {
        AudioCaptureMode::Mic => {
            let host = selected_input_host()?;
            let _ = resolve_input_device(&host, config.mic_device_id.as_deref())?;
            Ok(())
        }
        AudioCaptureMode::System => {
            #[cfg(target_os = "linux")]
            {
                return preflight_linux_system_capture_relaxed();
            }

            #[cfg(not(target_os = "linux"))]
            {
                match audio_backend::preflight_system_capture(config.system_device_id.as_deref()) {
                    Ok(()) => Ok(()),
                    Err(system_error) => {
                        let host = selected_input_host()?;
                        let _ = resolve_input_device(&host, config.mic_device_id.as_deref())?;
                        tracing::debug!(error = %system_error, "System audio preflight failed; allowing microphone fallback.");
                        Ok(())
                    }
                }
            }
        }
        AudioCaptureMode::Mixed => {
            let host = selected_input_host()?;
            let _ = resolve_input_device(&host, config.mic_device_id.as_deref())?;

            #[cfg(target_os = "linux")]
            {
                return preflight_linux_system_capture_relaxed();
            }

            #[cfg(not(target_os = "linux"))]
            {
                if let Err(error) = audio_backend::preflight_system_capture(config.system_device_id.as_deref()) {
                    tracing::debug!(error = %error, "System audio preflight failed; mixed mode will continue with microphone fallback if needed.");
                }
                Ok(())
            }
        }
    }
}

#[cfg(target_os = "linux")]
fn preflight_linux_system_capture_relaxed() -> AppResult<()> {
    let has_monitor_candidate = audio_backend::enumerate_system_devices()?
        .into_iter()
        .any(|device| device.backend == AudioBackendKind::LinuxSystem);

    if has_monitor_candidate {
        Ok(())
    } else {
        audio_backend::preflight_system_capture(None)
    }
}

fn fallback_warning_message(selected_device_id: Option<&str>, fallback_target: &str, error: &AppError) -> String {
    let selected = describe_system_device_for_warning(selected_device_id);
    format!(
        "Failed to start system audio from {selected}: {error}. Switched to {fallback_target}."
    )
}

fn describe_system_device_for_warning(device_id: Option<&str>) -> String {
    match device_id.filter(|value| !value.is_empty() && *value != "default") {
        None => {
            #[cfg(target_os = "linux")]
            {
                "the default system monitor".into()
            }

            #[cfg(not(target_os = "linux"))]
            {
                "the default system audio source".into()
            }
        }
        Some(value) => match split_namespaced_id(value) {
            Some((namespace, _)) if namespace == backend_namespace(AudioBackendKind::LinuxSystem) => {
                "the selected system monitor".into()
            }
            _ => "the selected system device".into(),
        },
    }
}

#[cfg(target_os = "linux")]
fn linux_system_candidates(selected_device_id: Option<&str>) -> AppResult<Vec<String>> {
    let selected_device_id = selected_device_id.filter(|value| !value.is_empty() && *value != "default");
    let devices = audio_backend::enumerate_system_devices()?;
    let mut candidates = Vec::new();

    if let Some(selected_device_id) = selected_device_id {
        candidates.push(selected_device_id.to_string());
    }

    for device in devices {
        if device.backend != AudioBackendKind::LinuxSystem {
            continue;
        }

        if selected_device_id.is_some_and(|current| current == device.id) {
            continue;
        }

        candidates.push(device.id);
    }

    if candidates.is_empty() {
        candidates.push("default".into());
    }

    Ok(candidates)
}

#[cfg(target_os = "linux")]
fn start_linux_system_with_fallback(
    selected_device_id: Option<&str>,
    mic_device_id: Option<&str>,
    tx: Sender<CapturedAudioFrame>,
    cancel_token: CancellationToken,
) -> AppResult<CaptureRuntime> {
    let candidates = linux_system_candidates(selected_device_id)?;
    let mut warnings = Vec::new();
    let mut last_system_error: Option<AppError> = None;

    for (index, candidate) in candidates.iter().enumerate() {
        match audio_backend::start_system_capture(Some(candidate.as_str()), tx.clone(), cancel_token.clone()) {
            Ok(format) => {
                if index > 0 {
                    let warning = match &last_system_error {
                        Some(error) => fallback_warning_message(selected_device_id, "another available system monitor", error),
                        None => "Switched system audio capture to another available system monitor.".into(),
                    };
                    warnings.push(warning);
                }

                return Ok(CaptureRuntime {
                    formats: vec![format],
                    mixed: false,
                    warnings,
                });
            }
            Err(err) => {
                last_system_error = Some(err);
            }
        }
    }

    let fallback_error = last_system_error.unwrap_or_else(|| {
        AppError::AudioCapture("No Linux system monitor source candidates were available".into())
    });
    let (sample_rate, channels) = start_single_input_capture(
        mic_device_id,
        AudioSource::Mic,
        tx,
        cancel_token,
    )?;
    warnings.push(fallback_warning_message(
        selected_device_id,
        "microphone capture",
        &fallback_error,
    ));

    Ok(CaptureRuntime {
        formats: vec![SourceStreamFormat {
            source: PhysicalAudioSource::Microphone,
            sample_rate,
            channels,
        }],
        mixed: false,
        warnings,
    })
}

#[cfg(target_os = "linux")]
fn start_linux_mixed_with_fallback(
    mic_device_id: Option<&str>,
    system_device_id: Option<&str>,
    tx: Sender<CapturedAudioFrame>,
    cancel_token: CancellationToken,
) -> AppResult<CaptureRuntime> {
    let mut formats = Vec::with_capacity(2);
    let mut warnings = Vec::new();
    let (mic_rate, mic_channels) = start_single_input_capture(
        mic_device_id,
        AudioSource::Mic,
        tx.clone(),
        cancel_token.clone(),
    )?;
    formats.push(SourceStreamFormat {
        source: PhysicalAudioSource::Microphone,
        sample_rate: mic_rate,
        channels: mic_channels,
    });

    let candidates = linux_system_candidates(system_device_id)?;
    let mut last_system_error: Option<AppError> = None;

    for (index, candidate) in candidates.iter().enumerate() {
        match audio_backend::start_system_capture(Some(candidate.as_str()), tx.clone(), cancel_token.clone()) {
            Ok(format) => {
                formats.push(format);
                if index > 0 {
                    let warning = match &last_system_error {
                        Some(error) => fallback_warning_message(system_device_id, "another available system monitor", error),
                        None => "Switched system audio capture to another available system monitor while keeping microphone capture active.".into(),
                    };
                    warnings.push(warning);
                }

                return Ok(CaptureRuntime {
                    formats,
                    mixed: true,
                    warnings,
                });
            }
            Err(err) => {
                last_system_error = Some(err);
            }
        }
    }

    let fallback_error = last_system_error.unwrap_or_else(|| {
        AppError::AudioCapture("No Linux system monitor source candidates were available".into())
    });
    warnings.push(fallback_warning_message(
        system_device_id,
        "microphone-only capture",
        &fallback_error,
    ));

    Ok(CaptureRuntime {
        formats,
        mixed: false,
        warnings,
    })
}

pub fn start_capture_runtime(
    config: CaptureConfig,
    tx: Sender<CapturedAudioFrame>,
    cancel_token: CancellationToken,
) -> AppResult<CaptureRuntime> {
    match config.mode {
        AudioCaptureMode::Mic => {
            let (sample_rate, channels) = start_single_input_capture(
                config.mic_device_id.as_deref(),
                AudioSource::Mic,
                tx,
                cancel_token,
            )?;
            Ok(CaptureRuntime {
                formats: vec![SourceStreamFormat {
                    source: PhysicalAudioSource::Microphone,
                    sample_rate,
                    channels,
                }],
                mixed: false,
                warnings: Vec::new(),
            })
        }
        AudioCaptureMode::System => {
            #[cfg(target_os = "linux")]
            {
                return start_linux_system_with_fallback(
                    config.system_device_id.as_deref(),
                    config.mic_device_id.as_deref(),
                    tx,
                    cancel_token,
                );
            }

            #[cfg(not(target_os = "linux"))]
            {
                match audio_backend::start_system_capture(
                    config.system_device_id.as_deref(),
                    tx.clone(),
                    cancel_token.clone(),
                ) {
                    Ok(format) => Ok(CaptureRuntime {
                        formats: vec![format],
                        mixed: false,
                        warnings: Vec::new(),
                    }),
                    Err(system_error) => {
                        let (sample_rate, channels) = start_single_input_capture(
                            config.mic_device_id.as_deref(),
                            AudioSource::Mic,
                            tx,
                            cancel_token,
                        )?;
                        Ok(CaptureRuntime {
                            formats: vec![SourceStreamFormat {
                                source: PhysicalAudioSource::Microphone,
                                sample_rate,
                                channels,
                            }],
                            mixed: false,
                            warnings: vec![fallback_warning_message(
                                config.system_device_id.as_deref(),
                                "microphone capture",
                                &system_error,
                            )],
                        })
                    }
                }
            }
        }
        AudioCaptureMode::Mixed => {
            #[cfg(target_os = "linux")]
            {
                return start_linux_mixed_with_fallback(
                    config.mic_device_id.as_deref(),
                    config.system_device_id.as_deref(),
                    tx,
                    cancel_token,
                );
            }

            #[cfg(not(target_os = "linux"))]
            {
                let mut formats = Vec::with_capacity(2);
                let (mic_rate, mic_channels) = start_single_input_capture(
                    config.mic_device_id.as_deref(),
                    AudioSource::Mic,
                    tx.clone(),
                    cancel_token.clone(),
                )?;
                formats.push(SourceStreamFormat {
                    source: PhysicalAudioSource::Microphone,
                    sample_rate: mic_rate,
                    channels: mic_channels,
                });

                match audio_backend::start_system_capture(
                    config.system_device_id.as_deref(),
                    tx,
                    cancel_token,
                ) {
                    Ok(system_format) => {
                        formats.push(system_format);
                        Ok(CaptureRuntime {
                            formats,
                            mixed: true,
                            warnings: Vec::new(),
                        })
                    }
                    Err(system_error) => Ok(CaptureRuntime {
                        formats,
                        mixed: false,
                        warnings: vec![fallback_warning_message(
                            config.system_device_id.as_deref(),
                            "microphone-only capture",
                            &system_error,
                        )],
                    }),
                }
            }
        }
    }
}

fn start_single_input_capture(
    device_id: Option<&str>,
    source: AudioSource,
    tx: Sender<CapturedAudioFrame>,
    cancel_token: CancellationToken,
) -> AppResult<(u32, u16)> {
    let host = selected_input_host()?;
    let device = resolve_input_device(&host, device_id)?;

    let config = device
        .default_input_config()
        .map_err(|e| AppError::AudioCapture(format!("Failed to get default input config: {e}")))?;

    let sample_rate = config.sample_rate();
    let channels = config.channels();
    let sample_format = config.sample_format();

    let stream_config: StreamConfig = config.into();
    let (startup_tx, startup_rx) = mpsc::sync_channel(1);

    // We must spawn a dedicated std thread because cpal::Stream is !Send on Windows.
    // Therefore, the Stream must be BUILT and OWNED strictly inside the thread.
    std::thread::spawn(move || {
        let startup_result = match sample_format {
            SampleFormat::F32 => build_stream::<f32>(&device, &stream_config, source, tx),
            SampleFormat::I16 => build_stream::<i16>(&device, &stream_config, source, tx),
            SampleFormat::U16 => build_stream::<u16>(&device, &stream_config, source, tx),
            _ => Err(AppError::AudioCapture(format!(
                "Unsupported input sample format: {sample_format:?}"
            ))),
        }
        .and_then(|stream| {
            stream
                .play()
                .map_err(|e| AppError::AudioCapture(format!("Failed to play audio stream: {e}")))?;
            Ok(stream)
        });

        match startup_result {
            Ok(stream) => {
                let _ = startup_tx.send(Ok(()));
                tracing::info!(?source, "Audio capture started natively.");

                while !cancel_token.is_cancelled() {
                    std::thread::sleep(Duration::from_millis(20));
                }

                let _ = stream.pause();
                drop(stream);
                tracing::debug!(?source, "Audio capture natively torn down.");
            }
            Err(err) => {
                tracing::error!(?source, "Audio capture failed during startup: {err}");
                let _ = startup_tx.send(Err(err));
            }
        }
    });

    startup_rx.recv().map_err(|_| {
        AppError::AudioCapture("Audio capture thread exited before startup completed".into())
    })??;

    Ok((sample_rate, channels))
}

fn build_stream<T>(
    device: &cpal::Device,
    config: &StreamConfig,
    source: AudioSource,
    tx: Sender<CapturedAudioFrame>,
) -> AppResult<Stream>
where
    T: cpal::Sample + cpal::SizedSample,
    f32: cpal::FromSample<T>,
{
    let err_fn = |err| tracing::error!("An error occurred on the input audio stream: {}", err);

    let stream = device
        .build_input_stream(
            config,
            move |data: &[T], _: &cpal::InputCallbackInfo| {
                let samples: Vec<f32> = data.iter().map(|&s| f32::from_sample(s)).collect();
                let frame = CapturedAudioFrame {
                    captured_at: Utc::now(),
                    source,
                    samples,
                };
                if let Err(error) = tx.send(frame) {
                    tracing::debug!("Stopping audio capture callback because receiver closed: {error}");
                }
            },
            err_fn,
            None,
        )
        .map_err(|e| AppError::AudioCapture(format!("Build stream error: {e}")))?;

    Ok(stream)
}
