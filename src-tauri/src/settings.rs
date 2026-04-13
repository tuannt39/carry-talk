use std::path::PathBuf;

use cpal::traits::{DeviceTrait, HostTrait};
use serde::Deserialize;

use crate::audio_backend::device_identity::{
    backend_namespace, fingerprint_payload, namespaced_id, normalize_label,
    parse_v2_fingerprint_payload, split_namespaced_id, versioned_id, InputFingerprint,
};
#[cfg(target_os = "linux")]
use crate::audio_backend::device_identity::{parse_v2_fingerprint, split_versioned_payload};
use crate::audio_backend::host_selection::selected_input_host;
use crate::error::{AppError, AppResult};
use crate::types::{
    AppSettings, AudioBackendKind, AudioCaptureMode, AudioSettings, GeneralSettings,
    ProviderSettings, SessionSettings,
};

/// Runtime settings manager.
/// Loads from and persists to ./carrytalk-data/config.toml (portable path).
pub struct Settings {
    pub current: AppSettings,
    config_path: PathBuf,
}

impl Settings {
    /// Load settings from config.toml, or create defaults if missing.
    pub fn load_or_default() -> Self {
        let config_path = data_dir().join("config.toml");
        let current = Self::read_from_file(&config_path).unwrap_or_default();
        Self {
            current,
            config_path,
        }
    }

    /// Read and parse config.toml
    fn read_from_file(path: &PathBuf) -> AppResult<AppSettings> {
        if !path.exists() {
            return Ok(AppSettings::default());
        }
        let content = std::fs::read_to_string(path)
            .map_err(|e| AppError::Settings(format!("Cannot read config: {e}")))?;

        read_compatible_settings(&content).map_err(|error| {
            tracing::warn!("Invalid config.toml, using defaults: {error}");
            AppError::Settings(format!("Parse error: {error}"))
        })
    }

    /// Save current settings to config.toml.
    /// Uses write-to-temp + rename for atomicity.
    pub fn save(&self) -> AppResult<()> {
        let dir = self
            .config_path
            .parent()
            .ok_or_else(|| AppError::Settings("Invalid config path".into()))?;
        std::fs::create_dir_all(dir)?;

        let content = toml::to_string_pretty(&self.current)
            .map_err(|e| AppError::Settings(format!("Serialize error: {e}")))?;

        // Atomic write: temp file then rename
        let tmp_path = self.config_path.with_extension("toml.tmp");
        std::fs::write(&tmp_path, &content)?;
        std::fs::rename(&tmp_path, &self.config_path)?;

        tracing::info!("Settings saved to {}", self.config_path.display());
        Ok(())
    }

    /// Apply a partial update from the frontend.
    pub fn update(&mut self, patch: AppSettings) -> AppResult<()> {
        self.current = patch;
        self.save()
    }

    pub fn get(&self) -> &AppSettings {
        &self.current
    }
}

/// Resolve the portable data directory relative to the executable.
/// In dev mode, falls back to ./carrytalk-data/ relative to the workspace root.
#[derive(Debug, Deserialize)]
struct LegacyAppSettings {
    #[serde(default)]
    general: Option<GeneralSettings>,
    #[serde(default)]
    audio: Option<LegacyAudioSettings>,
    #[serde(default)]
    provider: Option<ProviderSettings>,
    #[serde(default)]
    session: Option<SessionSettings>,
}

#[derive(Debug, Deserialize, Default)]
struct LegacyAudioSettings {
    #[serde(default)]
    capture_mode: Option<AudioCaptureMode>,
    #[serde(default)]
    mic_input_device: Option<String>,
    #[serde(default)]
    system_input_device: Option<String>,
    #[serde(default)]
    mic_device_id: Option<String>,
    #[serde(default)]
    system_device_id: Option<String>,
    #[serde(default)]
    sample_rate: Option<u32>,
    #[serde(default)]
    chunk_duration_ms: Option<u32>,
    #[serde(default)]
    mic_gain: Option<f32>,
    #[serde(default)]
    system_gain: Option<f32>,
    #[serde(default)]
    mic_silence_threshold: Option<i16>,
    #[serde(default)]
    system_silence_threshold: Option<i16>,
}

fn read_compatible_settings(content: &str) -> Result<AppSettings, toml::de::Error> {
    let legacy = toml::from_str::<LegacyAppSettings>(content)?;
    let mut settings = AppSettings::default();

    if let Some(general) = legacy.general {
        settings.general = general;
    }

    if let Some(provider) = legacy.provider {
        settings.provider = provider;
    }

    if let Some(session) = legacy.session {
        settings.session = session;
    }

    if let Some(audio) = legacy.audio {
        settings.audio = bridge_legacy_audio_settings(audio);
    }

    Ok(settings)
}

fn bridge_legacy_audio_settings(legacy: LegacyAudioSettings) -> AudioSettings {
    let defaults = AudioSettings::default();

    AudioSettings {
        capture_mode: legacy.capture_mode.unwrap_or(defaults.capture_mode),
        mic_device_id: canonicalize_microphone_device_id(
            legacy.mic_device_id.or(legacy.mic_input_device),
            &defaults.mic_device_id,
        ),
        system_device_id: canonicalize_system_device_id(
            legacy.system_device_id.or(legacy.system_input_device),
            &defaults.system_device_id,
        ),
        sample_rate: legacy.sample_rate.unwrap_or(defaults.sample_rate),
        chunk_duration_ms: legacy
            .chunk_duration_ms
            .unwrap_or(defaults.chunk_duration_ms),
        mic_gain: legacy.mic_gain.unwrap_or(defaults.mic_gain),
        system_gain: legacy.system_gain.unwrap_or(defaults.system_gain),
        mic_silence_threshold: legacy
            .mic_silence_threshold
            .unwrap_or(defaults.mic_silence_threshold),
        system_silence_threshold: legacy
            .system_silence_threshold
            .unwrap_or(defaults.system_silence_threshold),
    }
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

fn canonicalize_microphone_device_id(value: Option<String>, default_value: &str) -> String {
    let canonical = canonicalize_namespaced_device_id(value, AudioBackendKind::Cpal, default_value);
    if canonical == "default" {
        return canonical;
    }

    let Some((namespace, raw_value)) = split_backend_device_id(&canonical) else {
        return canonical;
    };
    if namespace != backend_namespace(AudioBackendKind::Cpal) {
        return default_value.to_string();
    }
    if parse_v2_fingerprint_payload(raw_value).is_some() {
        return canonical;
    }

    let Ok(host) = selected_input_host() else {
        return canonical;
    };
    let Ok(devices) = host.input_devices() else {
        return canonical;
    };

    for device in devices {
        let matches_legacy_name = device
            .description()
            .ok()
            .map(|desc| desc.name() == raw_value)
            .unwrap_or(false);
        if matches_legacy_name {
            return cpal_input_device_id(&device).unwrap_or(canonical);
        }
    }

    canonical
}

fn canonicalize_system_device_id(value: Option<String>, default_value: &str) -> String {
    let Some(value) = normalize_device_selection(value) else {
        return default_value.to_string();
    };

    if value == "default" {
        return value;
    }

    #[cfg(target_os = "macos")]
    {
        if let Some((namespace, raw_value)) = split_backend_device_id(&value) {
            if namespace == backend_namespace(AudioBackendKind::MacosSystem) {
                return value;
            }

            return if raw_value.starts_with("display:") {
                namespaced_id(backend_namespace(AudioBackendKind::MacosSystem), raw_value)
            } else {
                default_value.to_string()
            };
        }

        return if value.starts_with("display:") {
            namespaced_id(backend_namespace(AudioBackendKind::MacosSystem), &value)
        } else {
            default_value.to_string()
        };
    }

    #[cfg(target_os = "windows")]
    {
        if let Some((namespace, _)) = split_backend_device_id(&value) {
            return if namespace == backend_namespace(AudioBackendKind::WindowsSystem) {
                value
            } else {
                default_value.to_string()
            };
        }

        return canonicalize_namespaced_device_id(
            Some(value),
            AudioBackendKind::WindowsSystem,
            default_value,
        );
    }

    #[cfg(target_os = "linux")]
    {
        let canonical = canonicalize_linux_system_device_id(Some(value), default_value);
        return canonical;
    }

    #[allow(unreachable_code)]
    default_value.to_string()
}

#[cfg(target_os = "linux")]
fn canonicalize_linux_system_device_id(value: Option<String>, default_value: &str) -> String {
    let Some(value) = normalize_device_selection(value) else {
        return default_value.to_string();
    };

    if value == "default" {
        return value;
    }

    let legacy_candidate = match split_backend_device_id(&value) {
        Some((namespace, raw_value)) if namespace == backend_namespace(AudioBackendKind::LinuxSystem) => {
            if split_versioned_payload(raw_value)
                .map(|(version, payload)| version == "v3" && !payload.is_empty())
                .unwrap_or(false)
            {
                return value;
            }
            raw_value.to_string()
        }
        Some(_) => return default_value.to_string(),
        None => value.clone(),
    };

    let Ok(devices) = crate::audio_backend::enumerate_system_devices() else {
        return namespaced_id(backend_namespace(AudioBackendKind::LinuxSystem), &legacy_candidate);
    };

    if let Some(device) = devices.iter().find(|device| {
        device.id == value || device.id == legacy_candidate || device.label == legacy_candidate
    }) {
        return device.id.clone();
    }

    let Some(legacy_payload) = split_backend_device_id(&value)
        .and_then(|(namespace, raw_value)| {
            (namespace == backend_namespace(AudioBackendKind::LinuxSystem))
                .then_some(raw_value)
        })
    else {
        return namespaced_id(backend_namespace(AudioBackendKind::LinuxSystem), &legacy_candidate);
    };

    if let Some(fingerprint) = parse_v2_fingerprint(legacy_payload) {
        if let Some(device) = devices
            .iter()
            .find(|device| normalize_label(&device.label) == fingerprint.normalized_label)
        {
            return device.id.clone();
        }
    }

    default_value.to_string()
}

fn canonicalize_namespaced_device_id(
    value: Option<String>,
    backend: AudioBackendKind,
    default_value: &str,
) -> String {
    let Some(value) = normalize_device_selection(value) else {
        return default_value.to_string();
    };

    if value == "default" {
        return value;
    }

    if let Some((namespace, raw_value)) = split_backend_device_id(&value) {
        if namespace == backend_namespace(backend.clone()) {
            return value;
        }

        return if raw_value.is_empty() {
            default_value.to_string()
        } else {
            namespaced_id(backend_namespace(backend), raw_value)
        };
    }

    namespaced_id(backend_namespace(backend), &value)
}

fn normalize_device_selection(value: Option<String>) -> Option<String> {
    let normalized = value?.trim().to_string();
    if normalized.is_empty() {
        return None;
    }

    Some(normalized)
}

fn split_backend_device_id(id: &str) -> Option<(&str, &str)> {
    let (namespace, raw_value) = split_namespaced_id(id)?;
    match namespace {
        "cpal" | "windows-system" | "macos-system" | "linux-system" => {
            Some((namespace, raw_value))
        }
        _ => None,
    }
}

pub fn data_dir() -> PathBuf {
    // In development, use CARGO_MANIFEST_DIR parent (workspace root)
    if cfg!(debug_assertions) {
        if let Ok(manifest_dir) = std::env::var("CARGO_MANIFEST_DIR") {
            let workspace = PathBuf::from(manifest_dir)
                .parent()
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|| PathBuf::from("."));
            return workspace.join("carrytalk-data");
        }
    }

    // Production: relative to executable
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."))
        .join("carrytalk-data")
}
