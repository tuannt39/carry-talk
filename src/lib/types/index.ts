/** Session state from Rust backend */
export type SessionStatus =
  | "Idle"
  | "Connecting"
  | "Buffering"
  | "Recording"
  | "Paused"
  | "Reconnecting"
  | "Draining"
  | "Error";

export interface SessionStateIdle {
  status: "Idle";
}

export interface SessionStateConnecting {
  status: "Connecting";
}

export interface SessionStateBuffering {
  status: "Buffering";
  detail: {
    session_id: string;
    started_at: string;
    backlog_ms: number;
    session_generation: number;
  };
}

export interface SessionStateRecording {
  status: "Recording";
  detail: {
    session_id: string;
    started_at: string;
  };
}

export interface SessionStatePaused {
  status: "Paused";
  detail: {
    session_id: string;
    started_at: string;
  };
}

export interface SessionStateReconnecting {
  status: "Reconnecting";
  detail: {
    session_id: string;
    started_at: string;
    backlog_ms: number;
    session_generation: number;
  };
}

export interface SessionStateDraining {
  status: "Draining";
  detail: {
    session_id: string;
    started_at: string;
    backlog_ms: number;
  };
}

export interface SessionStateError {
  status: "Error";
  detail: {
    message: string;
    recoverable: boolean;
  };
}

export type SessionState =
  | SessionStateIdle
  | SessionStateConnecting
  | SessionStateBuffering
  | SessionStateRecording
  | SessionStatePaused
  | SessionStateReconnecting
  | SessionStateDraining
  | SessionStateError;

/** Audio source mirroring Rust AudioSource */
export type AudioSource = "Mic" | "System" | "Mixed";

/** Audio chunk mirroring Rust AudioChunk */
export interface AudioChunk {
  sequence: number;
  captured_at: string;
  duration_ms: number;
  source: AudioSource;
  session_generation: number;
  pcm_bytes: number[];
}

/** Transcript segment mirroring Rust TranscriptSegment */
export interface TranscriptSegment {
  id: string;
  start_ms: number;
  end_ms: number;
  speaker?: string;
  original_text: string;
  translated_text: string;
  is_final: boolean;
  created_at: string;
}

/** Audio capture mode mirroring Rust AudioCaptureMode */
export type AudioCaptureMode = "mic" | "system" | "mixed";

/** App settings mirroring Rust AppSettings */
export interface AppSettings {
  general: {
    language: string;
    theme: string;
  };
  audio: {
    capture_mode: AudioCaptureMode;
    mic_device_id: string;
    system_device_id: string;
    sample_rate: number;
    chunk_duration_ms: number;
    mic_gain: number;
    system_gain: number;
    mic_silence_threshold: number;
    system_silence_threshold: number;
  };
  provider: {
    name: string;
    source_language: string;
    translation_enabled: boolean;
    translation_target_language: string;
  };
  session: {
    auto_save: boolean;
    flush_interval_segments: number;
    max_segments_in_memory: number;
    archive_after_days: number;
    max_total_sessions_mb: number;
  };
}

/** Audio device info */
export interface AudioDevice {
  name: string;
  is_default: boolean;
}

export type PhysicalAudioSource = "microphone" | "system_output";
export type AudioBackendKind =
  | "cpal"
  | "windows_system"
  | "macos_system"
  | "linux_system";

export interface AudioSourceDevice {
  id: string;
  label: string;
  source: PhysicalAudioSource;
  backend: AudioBackendKind;
  is_default: boolean;
  usable: boolean;
}

export interface AudioCapabilityReason {
  code: string;
  detail: string | null;
}

export interface AudioSourceCapability {
  source: PhysicalAudioSource;
  backend: AudioBackendKind;
  supported: boolean;
  usable: boolean;
  reason: AudioCapabilityReason | null;
  devices: AudioSourceDevice[];
}

export interface AudioRuntimeCapabilities {
  microphone: AudioSourceCapability;
  system_output: AudioSourceCapability;
  mixed_supported: boolean;
  mixed_reason: AudioCapabilityReason | null;
}

/** Session summary for history list */
export interface SessionSummary {
  session_id: string;
  file_path: string;
  started_at: string | null;
  segment_count: number;
  is_complete: boolean;
}

/** Transcript update event payload */
export interface TranscriptUpdatePayload {
  segments: TranscriptSegment[];
  is_partial: boolean;
}
