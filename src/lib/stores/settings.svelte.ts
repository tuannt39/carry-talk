import type { AppSettings, AudioRuntimeCapabilities } from "$lib/types";

/** Default settings matching Rust defaults. */
const defaultSettings: AppSettings = {
  general: { language: "en", theme: "light" },
  audio: {
    capture_mode: "mixed",
    mic_device_id: "default",
    system_device_id: "default",
    sample_rate: 16000,
    chunk_duration_ms: 100,
    mic_gain: 1.0,
    system_gain: 1.0,
    mic_silence_threshold: 800,
    system_silence_threshold: 800,
  },
  provider: {
    name: "soniox",
    source_language: "auto",
    translation_enabled: true,
    translation_target_language: "en",
  },
  session: {
    auto_save: true,
    flush_interval_segments: 10,
    max_segments_in_memory: 500,
    archive_after_days: 30,
    max_total_sessions_mb: 1000,
  },
};

let current = $state<AppSettings>(structuredClone(defaultSettings));
let loaded = $state(false);
let audioRuntimeCapabilities = $state<AudioRuntimeCapabilities | null>(null);
let audioRuntimeCapabilitiesLoaded = $state(false);

export const settings = {
  get current(): AppSettings {
    return current;
  },
  set current(v: AppSettings) {
    current = v;
  },

  get loaded(): boolean {
    return loaded;
  },
  set loaded(v: boolean) {
    loaded = v;
  },

  get theme(): string {
    return current.general.theme;
  },

  get audioRuntimeCapabilities(): AudioRuntimeCapabilities | null {
    return audioRuntimeCapabilities;
  },
  set audioRuntimeCapabilities(v: AudioRuntimeCapabilities | null) {
    audioRuntimeCapabilities = v;
  },

  get audioRuntimeCapabilitiesLoaded(): boolean {
    return audioRuntimeCapabilitiesLoaded;
  },
  set audioRuntimeCapabilitiesLoaded(v: boolean) {
    audioRuntimeCapabilitiesLoaded = v;
  },
};
