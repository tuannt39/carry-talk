use cpal::traits::{DeviceTrait, HostTrait};

use crate::audio_backend;
use crate::audio_backend::host_selection::selected_input_host;
use crate::audio_backend::device_identity::{backend_namespace, fingerprint_payload, namespaced_id, normalize_label, versioned_id, InputFingerprint};
use crate::error::{AppError, AppResult};
use crate::types::{
    AudioBackendKind, AudioCapabilityReason, AudioRuntimeCapabilities, AudioSourceCapability,
    AudioSourceDevice, PhysicalAudioSource,
};

pub fn query_audio_runtime_capabilities() -> AppResult<AudioRuntimeCapabilities> {
    let microphone_devices = enumerate_microphone_devices()?;
    let microphone_usable = microphone_devices.iter().any(|device| device.usable);
    let microphone_reason = if microphone_usable {
        None
    } else {
        Some(reason("audio.microphone.no_usable_device", None))
    };

    let system_output = query_system_output_capability();
    let mixed_supported = microphone_usable && system_output.usable;
    let mixed_reason = if mixed_supported {
        None
    } else if !microphone_usable {
        Some(reason("audio.mixed.requires_microphone", None))
    } else {
        Some(reason(
            "audio.mixed.requires_system_audio",
            system_output.reason.as_ref().and_then(|reason| reason.detail.clone()),
        ))
    };

    Ok(AudioRuntimeCapabilities {
        microphone: AudioSourceCapability {
            source: PhysicalAudioSource::Microphone,
            backend: AudioBackendKind::Cpal,
            supported: true,
            usable: microphone_usable,
            reason: microphone_reason,
            devices: microphone_devices,
        },
        system_output,
        mixed_supported,
        mixed_reason,
    })
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

fn enumerate_microphone_devices() -> AppResult<Vec<AudioSourceDevice>> {
    let host = selected_input_host()?;

    let default_name = host
        .default_input_device()
        .and_then(|d| d.description().ok().map(|desc| desc.name().to_string()))
        .unwrap_or_default();

    let devices = host
        .input_devices()
        .map_err(|e| AppError::AudioCapture(format!("Cannot enumerate microphone devices: {e}")))?;

    let mut result = Vec::new();
    for device in devices {
        if let Ok(description) = device.description() {
            let name = description.name().to_string();
            result.push(AudioSourceDevice {
                id: cpal_input_device_id(&device)
                    .unwrap_or_else(|| namespaced_id(backend_namespace(AudioBackendKind::Cpal), &name)),
                label: name.clone(),
                source: PhysicalAudioSource::Microphone,
                backend: AudioBackendKind::Cpal,
                is_default: name == default_name,
                usable: true,
            });
        }
    }

    Ok(result)
}

fn query_system_output_capability() -> AudioSourceCapability {
    let backend = audio_backend::current_platform_system_backend();
    match audio_backend::enumerate_system_devices() {
        Ok(devices) => {
            let usable = devices.iter().any(|device| device.usable);
            AudioSourceCapability {
                source: PhysicalAudioSource::SystemOutput,
                backend: backend.clone(),
                supported: true,
                usable,
                reason: if usable {
                    None
                } else {
                    Some(default_system_backend_reason(&backend, devices.is_empty()))
                },
                devices,
            }
        }
        Err(err) => AudioSourceCapability {
            source: PhysicalAudioSource::SystemOutput,
            backend: backend.clone(),
            supported: true,
            usable: false,
            reason: Some(dynamic_system_backend_reason(&backend, err.to_string())),
            devices: Vec::new(),
        },
    }
}

fn default_system_backend_reason(
    kind: &AudioBackendKind,
    has_no_devices: bool,
) -> AudioCapabilityReason {
    match kind {
        AudioBackendKind::LinuxSystem => reason(
            "audio.system.no_monitor_source",
            Some(
                "No usable Linux monitor-source input found. Ensure PipeWire/PulseAudio monitor devices are available.".into(),
            ),
        ),
        AudioBackendKind::MacosSystem if has_no_devices => reason(
            "audio.system.no_display_available",
            Some(
                "macOS system audio capture expects at least one active display for the ScreenCaptureKit path".into(),
            ),
        ),
        AudioBackendKind::MacosSystem => reason(
            "audio.system.runtime_verification_pending",
            Some(
                "macOS system audio capture uses ScreenCaptureKit and still requires runtime verification on a real macOS build with Screen Recording permission".into(),
            ),
        ),
        _ => reason(
            "audio.system.runtime_not_wired",
            Some(format!(
                "System audio backend `{}` is not wired on this runtime yet",
                backend_label(kind)
            )),
        ),
    }
}

fn dynamic_system_backend_reason(
    kind: &AudioBackendKind,
    detail: String,
) -> AudioCapabilityReason {
    let code = match kind {
        AudioBackendKind::LinuxSystem => "audio.system.enumeration_failed",
        AudioBackendKind::MacosSystem => "audio.system.enumeration_failed",
        _ => "audio.system.runtime_error",
    };
    reason(code, Some(detail))
}

fn reason(code: &str, detail: Option<String>) -> AudioCapabilityReason {
    AudioCapabilityReason {
        code: code.into(),
        detail,
    }
}

fn backend_label(kind: &AudioBackendKind) -> &'static str {
    match kind {
        AudioBackendKind::Cpal => "cpal",
        AudioBackendKind::WindowsSystem => "windows_system",
        AudioBackendKind::MacosSystem => "macos_system",
        AudioBackendKind::LinuxSystem => "linux_system",
    }
}
