import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  SessionState,
  TranscriptUpdatePayload,
  SessionSummary,
} from "$lib/types";

/** Listen for session state changes from the Rust backend. */
export function onSessionStateChanged(
  handler: (state: SessionState) => void,
): Promise<UnlistenFn> {
  return listen<SessionState>("session_state_changed", (event) => {
    handler(event.payload);
  });
}

/** Listen for real-time transcript updates. */
export function onTranscriptUpdate(
  handler: (payload: TranscriptUpdatePayload) => void,
): Promise<UnlistenFn> {
  return listen<TranscriptUpdatePayload>("transcript_update", (event) => {
    handler(event.payload);
  });
}

/** Listen for audio level updates (RMS + peak). */
export function onAudioLevel(
  handler: (level: { rms: number; peak: number }) => void,
): Promise<UnlistenFn> {
  return listen<{ rms: number; peak: number }>("audio_level", (event) => {
    handler(event.payload);
  });
}

/** Listen for session error events. */
export function onSessionError(
  handler: (error: { message: string; recoverable: boolean }) => void,
): Promise<UnlistenFn> {
  return listen<{ message: string; recoverable: boolean }>(
    "session_error",
    (event) => {
      handler(event.payload);
    },
  );
}

/** Listen for crash-recovered session notifications. */
export function onSessionRecovered(
  handler: (summary: SessionSummary) => void,
): Promise<UnlistenFn> {
  return listen<SessionSummary>("session_recovered", (event) => {
    handler(event.payload);
  });
}
