use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

// ── Transcript Segment ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptSegment {
    pub id: String,
    pub start_ms: u64,
    pub end_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speaker: Option<String>,
    pub original_text: String,
    #[serde(default)]
    pub translated_text: String,
    pub is_final: bool,
    pub created_at: DateTime<Utc>,
}

impl TranscriptSegment {
    pub fn new(start_ms: u64, end_ms: u64, original_text: String) -> Self {
        let id = format!("seg_{}", uuid::Uuid::new_v4().as_simple());
        Self {
            id,
            start_ms,
            end_ms,
            speaker: None,
            original_text,
            translated_text: String::new(),
            is_final: false,
            created_at: Utc::now(),
        }
    }
}

// ── Audio Metadata / Runtime State ─────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AudioChunk {
    pub sequence: u64,
    pub captured_at: DateTime<Utc>,
    pub duration_ms: u32,
    pub source: AudioSource,
    pub session_generation: u32,
    pub pcm_bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CapturedAudioFrame {
    pub captured_at: DateTime<Utc>,
    pub source: AudioSource,
    pub samples: Vec<f32>,
}

impl SourceActivity {
    pub fn from_source(source: AudioSource, active: bool) -> Self {
        match source {
            AudioSource::Mic => Self {
                mic_active: active,
                system_active: false,
            },
            AudioSource::System => Self {
                mic_active: false,
                system_active: active,
            },
            AudioSource::Mixed => Self {
                mic_active: active,
                system_active: active,
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResampledAudioFrame {
    pub captured_at: DateTime<Utc>,
    pub duration_ms: u32,
    pub source: AudioSource,
    pub pcm_bytes: Vec<u8>,
    pub activity: SourceActivity,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum AudioSource {
    Mic,
    System,
    Mixed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PhysicalAudioSource {
    Microphone,
    SystemOutput,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AudioBackendKind {
    Cpal,
    WindowsSystem,
    MacosSystem,
    LinuxSystem,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceActivity {
    pub mic_active: bool,
    pub system_active: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedAudioFrame {
    pub captured_at: DateTime<Utc>,
    pub duration_ms: u32,
    pub source: AudioSource,
    pub pcm_bytes: Vec<u8>,
    pub activity: SourceActivity,
}

// ── Session State ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "status", content = "detail")]
pub enum SessionState {
    Idle,
    Connecting,
    Buffering {
        session_id: String,
        started_at: DateTime<Utc>,
        backlog_ms: u32,
        session_generation: u32,
    },
    Recording {
        session_id: String,
        started_at: DateTime<Utc>,
    },
    Paused {
        session_id: String,
        started_at: DateTime<Utc>,
    },
    Reconnecting {
        session_id: String,
        started_at: DateTime<Utc>,
        backlog_ms: u32,
        session_generation: u32,
    },
    Draining {
        session_id: String,
        started_at: DateTime<Utc>,
        backlog_ms: u32,
    },
    Error {
        message: String,
        recoverable: bool,
    },
}

impl Default for SessionState {
    fn default() -> Self {
        Self::Idle
    }
}

// ── App Settings (serialized to config.toml) ────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    #[serde(default = "default_general")]
    pub general: GeneralSettings,
    #[serde(default = "default_audio")]
    pub audio: AudioSettings,
    #[serde(default = "default_provider")]
    pub provider: ProviderSettings,
    #[serde(default = "default_session")]
    pub session: SessionSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneralSettings {
    #[serde(default = "default_language")]
    pub language: String,
    #[serde(default = "default_theme")]
    pub theme: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AudioCaptureMode {
    Mic,
    System,
    Mixed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioSettings {
    #[serde(default = "default_capture_mode")]
    pub capture_mode: AudioCaptureMode,
    #[serde(default = "default_input_device")]
    pub mic_device_id: String,
    #[serde(default = "default_input_device")]
    pub system_device_id: String,
    #[serde(default = "default_sample_rate")]
    pub sample_rate: u32,
    #[serde(default = "default_chunk_duration_ms")]
    pub chunk_duration_ms: u32,
    #[serde(default = "default_mic_gain")]
    pub mic_gain: f32,
    #[serde(default = "default_system_gain")]
    pub system_gain: f32,
    #[serde(default = "default_mic_silence_threshold")]
    pub mic_silence_threshold: i16,
    #[serde(default = "default_system_silence_threshold")]
    pub system_silence_threshold: i16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderSettings {
    #[serde(default = "default_provider_name")]
    pub name: String,
    #[serde(default = "default_source_language")]
    pub source_language: String,
    #[serde(default = "default_true")]
    pub translation_enabled: bool,
    #[serde(default = "default_target_language")]
    pub translation_target_language: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSettings {
    #[serde(default = "default_true")]
    pub auto_save: bool,
    #[serde(default = "default_flush_interval")]
    pub flush_interval_segments: u32,
    #[serde(default = "default_max_segments")]
    pub max_segments_in_memory: u32,
    #[serde(default = "default_archive_days")]
    pub archive_after_days: u32,
    #[serde(default = "default_max_storage_mb")]
    pub max_total_sessions_mb: u32,
}

// ── Audio Device Info ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioDevice {
    pub name: String,
    pub is_default: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioSourceDevice {
    pub id: String,
    pub label: String,
    pub source: PhysicalAudioSource,
    pub backend: AudioBackendKind,
    pub is_default: bool,
    pub usable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioCapabilityReason {
    pub code: String,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioSourceCapability {
    pub source: PhysicalAudioSource,
    pub backend: AudioBackendKind,
    pub supported: bool,
    pub usable: bool,
    pub reason: Option<AudioCapabilityReason>,
    pub devices: Vec<AudioSourceDevice>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioRuntimeCapabilities {
    pub microphone: AudioSourceCapability,
    pub system_output: AudioSourceCapability,
    pub mixed_supported: bool,
    pub mixed_reason: Option<AudioCapabilityReason>,
}

// ── Session Summary (for listing saved sessions) ────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSummary {
    pub session_id: String,
    pub file_path: String,
    pub started_at: Option<DateTime<Utc>>,
    pub segment_count: u32,
    pub is_complete: bool,
}

// ── JSONL Segment & Manifest Models ──────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    Active,
    Completed,
    Recovered,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionManifest {
    pub session_id: String,
    pub status: SessionStatus,
    pub started_at: DateTime<Utc>,
    pub provider: String,
    pub source_language: String,
    pub target_language: String,
    pub parts: Vec<SessionPartMeta>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionPartMeta {
    pub file: String,
    pub status: SessionStatus,
    pub segments: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum JsonlLine {
    #[serde(rename = "part_header")]
    Header {
        part_index: u32,
        session_id: String,
        created_at: DateTime<Utc>,
    },
    #[serde(rename = "segment")]
    Segment(TranscriptSegment),
    #[serde(rename = "part_footer")]
    Footer {
        status: SessionStatus,
        segment_count: u32,
        closed_at: Option<DateTime<Utc>>,
    },
}

// ── Default value functions ─────────────────────────────────────────────────

fn default_general() -> GeneralSettings {
    GeneralSettings {
        language: default_language(),
        theme: default_theme(),
    }
}

fn default_audio() -> AudioSettings {
    AudioSettings {
        capture_mode: default_capture_mode(),
        mic_device_id: default_input_device(),
        system_device_id: default_input_device(),
        sample_rate: default_sample_rate(),
        chunk_duration_ms: default_chunk_duration_ms(),
        mic_gain: default_mic_gain(),
        system_gain: default_system_gain(),
        mic_silence_threshold: default_mic_silence_threshold(),
        system_silence_threshold: default_system_silence_threshold(),
    }
}

impl Default for AudioSettings {
    fn default() -> Self {
        default_audio()
    }
}

fn default_provider() -> ProviderSettings {
    ProviderSettings {
        name: default_provider_name(),
        source_language: default_source_language(),
        translation_enabled: true,
        translation_target_language: default_target_language(),
    }
}

fn default_session() -> SessionSettings {
    SessionSettings {
        auto_save: true,
        flush_interval_segments: default_flush_interval(),
        max_segments_in_memory: default_max_segments(),
        archive_after_days: default_archive_days(),
        max_total_sessions_mb: default_max_storage_mb(),
    }
}

fn default_language() -> String {
    "en".into()
}
fn default_theme() -> String {
    "light".into()
}
fn default_capture_mode() -> AudioCaptureMode {
    AudioCaptureMode::Mixed
}
fn default_input_device() -> String {
    "default".into()
}
fn default_sample_rate() -> u32 {
    16000
}
fn default_chunk_duration_ms() -> u32 {
    100
}
fn default_mic_gain() -> f32 {
    1.0
}
fn default_system_gain() -> f32 {
    1.0
}
fn default_mic_silence_threshold() -> i16 {
    800
}
fn default_system_silence_threshold() -> i16 {
    800
}
fn default_provider_name() -> String {
    "soniox".into()
}
fn default_source_language() -> String {
    "auto".into()
}
fn default_target_language() -> String {
    "en".into()
}
fn default_true() -> bool {
    true
}
fn default_flush_interval() -> u32 {
    10
}
fn default_max_segments() -> u32 {
    500
}
fn default_archive_days() -> u32 {
    30
}
fn default_max_storage_mb() -> u32 {
    1000
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            general: default_general(),
            audio: default_audio(),
            provider: default_provider(),
            session: default_session(),
        }
    }
}
