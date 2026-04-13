use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};

use chrono::Utc;
use crossbeam_channel::Sender;
use libpulse_binding as pulse;
use libpulse_binding::callbacks::ListResult;
use libpulse_binding::context::introspect::SourceInfo;
use libpulse_binding::context::{Context, FlagSet as ContextFlagSet, State as ContextState};
use libpulse_binding::mainloop::standard::{IterateResult, Mainloop};
use libpulse_binding::operation::State as OperationState;
use libpulse_binding::proplist::Proplist;
use libpulse_binding::sample::{Format as PulseSampleFormat, Spec};
use libpulse_binding::stream::Direction;
use libpulse_simple_binding::Simple;
use tokio_util::sync::CancellationToken;

use crate::audio_backend::device_identity::{
    backend_namespace, split_namespaced_id, split_versioned_payload, versioned_id,
};
use crate::audio_backend::{SourceStreamFormat, SystemAudioBackend};
use crate::error::{AppError, AppResult};
use crate::types::{
    AudioBackendKind, AudioSource, AudioSourceDevice, CapturedAudioFrame, PhysicalAudioSource,
};

pub struct LinuxSystemBackend;

const LINUX_SYSTEM_ID_VERSION: &str = "v3";
const DEFAULT_RECORD_FORMAT: PulseSampleFormat = PulseSampleFormat::S16le;
const TARGET_CHUNK_MS: u32 = 20;

#[derive(Debug, Clone)]
struct PulseMonitorSource {
    name: String,
    description: String,
    rate: u32,
    channels: u8,
    is_default: bool,
}

impl SystemAudioBackend for LinuxSystemBackend {
    fn enumerate_system_devices(&self) -> AppResult<Vec<AudioSourceDevice>> {
        let sources = enumerate_monitor_sources()?;
        Ok(sources
            .into_iter()
            .map(|source| AudioSourceDevice {
                id: linux_monitor_device_id(&source.name),
                label: source.description,
                source: PhysicalAudioSource::SystemOutput,
                backend: AudioBackendKind::LinuxSystem,
                is_default: source.is_default,
                usable: true,
            })
            .collect())
    }

    fn preflight_system_capture(&self, device_id: Option<&str>) -> AppResult<()> {
        let source = resolve_monitor_source(device_id)?;
        let spec = Spec {
            format: DEFAULT_RECORD_FORMAT,
            channels: source.channels,
            rate: source.rate,
        };
        if !spec.is_valid() {
            return Err(AppError::AudioCapture(format!(
                "Invalid Linux PulseAudio sample spec for `{}`",
                source.name
            )));
        }
        Ok(())
    }

    fn start_system_capture(
        &self,
        device_id: Option<&str>,
        tx: Sender<CapturedAudioFrame>,
        cancel_token: CancellationToken,
    ) -> AppResult<SourceStreamFormat> {
        let source = resolve_monitor_source(device_id)?;
        let sample_rate = source.rate;
        let channels = u16::from(source.channels);
        let stream_spec = Spec {
            format: DEFAULT_RECORD_FORMAT,
            channels: source.channels,
            rate: source.rate,
        };
        if !stream_spec.is_valid() {
            return Err(AppError::AudioCapture(format!(
                "Invalid Linux PulseAudio sample spec for `{}`",
                source.name
            )));
        }

        let (startup_tx, startup_rx) = mpsc::sync_channel(1);
        std::thread::spawn(move || {
            let startup_result = start_recording_thread(&source, &stream_spec, tx, cancel_token, startup_tx);
            if let Err(err) = startup_result {
                tracing::error!(source = %source.name, "Linux system capture thread exited: {err}");
            }
        });

        startup_rx.recv().map_err(|_| {
            AppError::AudioCapture(
                "Linux system capture thread exited before startup completed".into(),
            )
        })??;

        Ok(SourceStreamFormat {
            source: PhysicalAudioSource::SystemOutput,
            sample_rate,
            channels,
        })
    }
}

fn linux_monitor_device_id(source_name: &str) -> String {
    versioned_id(
        backend_namespace(AudioBackendKind::LinuxSystem),
        LINUX_SYSTEM_ID_VERSION,
        source_name,
    )
}

fn parse_linux_monitor_device_id(device_id: &str) -> Option<&str> {
    let (namespace, payload) = split_namespaced_id(device_id)?;
    if namespace != backend_namespace(AudioBackendKind::LinuxSystem) {
        return None;
    }
    let (version, value) = split_versioned_payload(payload)?;
    (version == LINUX_SYSTEM_ID_VERSION && !value.is_empty()).then_some(value)
}

fn normalize_linux_system_selection(device_id: &str) -> String {
    if let Some(source_name) = parse_linux_monitor_device_id(device_id) {
        return source_name.to_string();
    }

    match split_namespaced_id(device_id) {
        Some((namespace, raw_value)) if namespace == backend_namespace(AudioBackendKind::LinuxSystem) => {
            raw_value.to_string()
        }
        _ => device_id.to_string(),
    }
}

fn resolve_monitor_source(device_id: Option<&str>) -> AppResult<PulseMonitorSource> {
    let sources = enumerate_monitor_sources()?;
    if sources.is_empty() {
        return Err(AppError::AudioCapture(
            "No Linux system monitor source found. Ensure PulseAudio or PipeWire Pulse exposes a monitor source."
                .into(),
        ));
    }

    if let Some(device_id) = device_id.filter(|value| *value != "default") {
        let requested = normalize_linux_system_selection(device_id);
        return sources
            .into_iter()
            .find(|source| source.name == requested || source.description == requested)
            .ok_or_else(|| {
                AppError::AudioCapture(format!(
                    "Linux system audio device not found or not a monitor source: {device_id}"
                ))
            });
    }

    if let Some(default_source) = sources.iter().find(|source| source.is_default) {
        return Ok(default_source.clone());
    }

    sources.into_iter().next().ok_or_else(|| {
        AppError::AudioCapture(
            "No Linux system monitor source found. Ensure PulseAudio or PipeWire Pulse exposes a monitor source."
                .into(),
        )
    })
}

fn start_recording_thread(
    source: &PulseMonitorSource,
    spec: &Spec,
    tx: Sender<CapturedAudioFrame>,
    cancel_token: CancellationToken,
    startup_tx: mpsc::SyncSender<AppResult<()>>,
) -> AppResult<()> {
    let stream = Simple::new(
        None,
        "carry-talk",
        Direction::Record,
        Some(source.name.as_str()),
        "system-capture",
        spec,
        None,
        None,
    )
    .map_err(|err| {
        AppError::AudioCapture(format!(
            "Failed to open Linux PulseAudio monitor `{}`: {err:?}",
            source.name
        ))
    })?;

    tracing::info!(
        source = %source.name,
        rate = spec.rate,
        channels = spec.channels,
        "Linux system audio capture started via PulseAudio."
    );

    let _ = startup_tx.send(Ok(()));

    let _ = tx.send(CapturedAudioFrame {
        captured_at: Utc::now(),
        source: AudioSource::System,
        samples: Vec::new(),
    });

    let bytes_per_sample = match spec.format {
        PulseSampleFormat::S16le => 2usize,
        _ => {
            return Err(AppError::AudioCapture(format!(
                "Unsupported Linux PulseAudio sample format: {:?}",
                spec.format
            )));
        }
    };
    let frame_bytes = usize::from(spec.channels) * bytes_per_sample;
    let frames_per_chunk = ((u64::from(spec.rate) * u64::from(TARGET_CHUNK_MS)) / 1_000).max(1) as usize;
    let mut buffer = vec![0u8; frame_bytes * frames_per_chunk];

    loop {
        if cancel_token.is_cancelled() {
            break;
        }

        stream.read(&mut buffer).map_err(|err| {
            AppError::AudioCapture(format!(
                "Linux PulseAudio monitor read failed for `{}`: {err:?}",
                source.name
            ))
        })?;

        let samples = buffer
            .chunks_exact(2)
            .map(|chunk| i16::from_le_bytes([chunk[0], chunk[1]]) as f32 / i16::MAX as f32)
            .collect();
        let frame = CapturedAudioFrame {
            captured_at: Utc::now(),
            source: AudioSource::System,
            samples,
        };
        if let Err(error) = tx.send(frame) {
            tracing::debug!(
                "Stopping Linux PulseAudio callback because receiver closed: {error}"
            );
            break;
        }
    }

    let _ = stream.drain();
    tracing::debug!(source = %source.name, "Linux system audio capture stopped.");
    Ok(())
}

fn enumerate_monitor_sources() -> AppResult<Vec<PulseMonitorSource>> {
    let default_monitor = default_monitor_source_name().ok();
    let (mut mainloop, context) = connect_pulse_context()?;
    let introspector = context.introspect();
    let completed = Arc::new(AtomicBool::new(false));
    let collected = Arc::new(Mutex::new(Vec::<PulseMonitorSource>::new()));

    let completed_clone = completed.clone();
    let collected_clone = collected.clone();
    let default_monitor_clone = default_monitor.clone();

    let operation = introspector.get_source_info_list(move |result: ListResult<&SourceInfo>| {
        match result {
            ListResult::Item(info) => {
                let Some(name) = info.name.as_ref().map(|value| value.to_string()) else {
                    return;
                };
                if info.monitor_of_sink.is_none() {
                    return;
                }

                let spec = info.sample_spec;
                if !spec.is_valid() {
                    return;
                }

                let description = info
                    .description
                    .as_ref()
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| name.clone());
                let source = PulseMonitorSource {
                    is_default: default_monitor_clone.as_deref() == Some(name.as_str()),
                    name,
                    description,
                    rate: spec.rate,
                    channels: spec.channels,
                };
                if let Ok(mut devices) = collected_clone.lock() {
                    devices.push(source);
                }
            }
            ListResult::End | ListResult::Error => {
                completed_clone.store(true, Ordering::SeqCst);
            }
        }
    });

    wait_for_operation(&mut mainloop, &operation, &completed)?;

    let mut devices = collected
        .lock()
        .map_err(|_| AppError::AudioCapture("Failed to lock PulseAudio monitor list".into()))?
        .clone();
    devices.sort_by(|left, right| left.description.cmp(&right.description));
    Ok(devices)
}

fn default_monitor_source_name() -> AppResult<String> {
    let (mut mainloop, context) = connect_pulse_context()?;
    let introspector = context.introspect();
    let completed = Arc::new(AtomicBool::new(false));
    let result = Arc::new(Mutex::new(None::<String>));

    let completed_clone = completed.clone();
    let result_clone = result.clone();
    let operation = introspector.get_server_info(move |info| {
        if let Some(default_source) = info.default_source_name.as_ref().map(|value| value.to_string()) {
            if let Ok(mut slot) = result_clone.lock() {
                *slot = Some(default_source);
            }
        }
        completed_clone.store(true, Ordering::SeqCst);
    });

    wait_for_operation(&mut mainloop, &operation, &completed)?;

    result
        .lock()
        .map_err(|_| AppError::AudioCapture("Failed to lock PulseAudio default source".into()))?
        .clone()
        .ok_or_else(|| AppError::AudioCapture("PulseAudio default source is unavailable".into()))
}

fn connect_pulse_context() -> AppResult<(Mainloop, Context)> {
    let mut proplist = Proplist::new().ok_or_else(|| {
        AppError::AudioCapture("Failed to initialize PulseAudio client properties".into())
    })?;
    proplist
        .set_str(pulse::proplist::properties::APPLICATION_NAME, "carry-talk")
        .map_err(|_| AppError::AudioCapture("Failed to set PulseAudio application name".into()))?;

    let mut mainloop = Mainloop::new().ok_or_else(|| {
        AppError::AudioCapture("Failed to initialize PulseAudio mainloop".into())
    })?;
    let mut context = Context::new_with_proplist(&mainloop, "carry-talk-context", &proplist)
        .ok_or_else(|| AppError::AudioCapture("Failed to initialize PulseAudio context".into()))?;

    context
        .connect(None, ContextFlagSet::NOFLAGS, None)
        .map_err(|err| AppError::AudioCapture(format!("Failed to connect PulseAudio context: {err:?}")))?;

    loop {
        match mainloop.iterate(true) {
            IterateResult::Success(_) => {}
            IterateResult::Quit(_) | IterateResult::Err(_) => {
                return Err(AppError::AudioCapture(
                    "PulseAudio mainloop exited while connecting".into(),
                ));
            }
        }

        match context.get_state() {
            ContextState::Ready => break,
            ContextState::Failed | ContextState::Terminated => {
                return Err(AppError::AudioCapture(format!(
                    "PulseAudio context state is {:?}",
                    context.get_state()
                )));
            }
            _ => {}
        }
    }

    Ok((mainloop, context))
}

fn wait_for_operation<T: ?Sized>(
    mainloop: &mut Mainloop,
    operation: &pulse::operation::Operation<T>,
    completed: &AtomicBool,
) -> AppResult<()> {
    while operation.get_state() == OperationState::Running || !completed.load(Ordering::SeqCst) {
        match mainloop.iterate(true) {
            IterateResult::Success(_) => {}
            IterateResult::Quit(_) | IterateResult::Err(_) => {
                return Err(AppError::AudioCapture(
                    "PulseAudio mainloop exited during operation".into(),
                ));
            }
        }
    }
    Ok(())
}
