import { invoke } from "@tauri-apps/api/core";
import type {
  AppSettings,
  AudioDevice,
  AudioRuntimeCapabilities,
  SessionState,
  SessionSummary,
} from "$lib/types";

// ── Settings ──────────────────────────────────────────────────────────────

export async function getSettings(): Promise<AppSettings> {
  return invoke<AppSettings>("get_settings");
}

export async function saveSettings(settings: AppSettings): Promise<void> {
  return invoke<void>("save_settings", { settings });
}

export async function saveSettingsAndApiKey(
  settings: AppSettings,
  apiKey?: string,
): Promise<void> {
  return invoke<void>("save_settings_and_api_key", {
    settings,
    apiKey,
  });
}

// ── Session Lifecycle ─────────────────────────────────────────────────────

export async function startSession(opts?: {
  sourceLang?: string;
  targetLang?: string;
}): Promise<string> {
  return invoke<string>("start_session", {
    sourceLang: opts?.sourceLang,
    targetLang: opts?.targetLang,
  });
}

export async function stopSession(): Promise<void> {
  return invoke<void>("stop_session");
}

export async function pauseSession(): Promise<void> {
  return invoke<void>("pause_session");
}

export async function resumeSession(): Promise<void> {
  return invoke<void>("resume_session");
}

export async function getSessionState(): Promise<SessionState> {
  return invoke<SessionState>("get_session_state");
}

export async function getAudioRuntimeCapabilities(): Promise<AudioRuntimeCapabilities> {
  return invoke<AudioRuntimeCapabilities>("get_audio_runtime_capabilities");
}

// ── Audio Devices ─────────────────────────────────────────────────────────

export async function listAudioDevices(): Promise<AudioDevice[]> {
  return invoke<AudioDevice[]>("list_audio_devices");
}

// ── Secrets ───────────────────────────────────────────────────────────────

export async function setApiKey(key: string): Promise<void> {
  return invoke<void>("set_api_key", { key });
}

export async function hasApiKey(): Promise<boolean> {
  return invoke<boolean>("has_api_key");
}


// ── Session History ───────────────────────────────────────────────────────

export async function listSessions(): Promise<SessionSummary[]> {
  return invoke<SessionSummary[]>("list_sessions");
}
