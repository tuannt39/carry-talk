import type { SessionState } from "$lib/types";

type SessionStatus = SessionState["status"];

/** Reactive session state using Svelte 5 runes. */
let sessionState = $state<SessionState>({ status: "Idle" });
let recordingTimer = $state(0);
let timerInterval: ReturnType<typeof setInterval> | null = null;

const sessionStatusLabelKeys: Record<SessionStatus, string> = {
  Idle: "session.idle",
  Connecting: "session.connecting",
  Buffering: "session.buffering",
  Recording: "session.recording",
  Paused: "session.paused",
  Reconnecting: "session.reconnecting",
  Draining: "session.draining",
  Error: "session.error",
};

function getStartedAt(state: SessionState): number | null {
  if (
    state.status === "Buffering" ||
    state.status === "Recording" ||
    state.status === "Paused" ||
    state.status === "Reconnecting" ||
    state.status === "Draining"
  ) {
    return new Date(state.detail.started_at).getTime();
  }

  return null;
}

function syncElapsedSeconds(state: SessionState) {
  const startedAt = getStartedAt(state);
  if (startedAt === null) {
    recordingTimer = 0;
    return;
  }

  recordingTimer = Math.max(0, Math.floor((Date.now() - startedAt) / 1000));
}

export function getSessionStatusLabelKey(
  state: SessionState,
  transcriptIsEmpty: boolean,
): string {
  if (state.status === "Recording" && transcriptIsEmpty) {
    return "transcript.waiting_for_speech";
  }

  return sessionStatusLabelKeys[state.status];
}

export function getTranscriptEmptyStateLabelKey(state: SessionState): string {
  if (state.status === "Paused") {
    return "session.paused";
  }

  if (
    state.status === "Connecting" ||
    state.status === "Buffering" ||
    state.status === "Recording" ||
    state.status === "Reconnecting" ||
    state.status === "Draining"
  ) {
    return "transcript.waiting_for_speech";
  }

  return "transcript.empty";
}

export const session = {
  get state(): SessionState {
    return sessionState;
  },
  set state(v: SessionState) {
    sessionState = v;
    syncElapsedSeconds(v);

    if (
      v.status === "Buffering" ||
      v.status === "Recording" ||
      v.status === "Paused" ||
      v.status === "Reconnecting" ||
      v.status === "Draining"
    ) {
      startTimer();
    } else {
      stopTimer();
    }
  },

  get isIdle(): boolean {
    return sessionState.status === "Idle";
  },
  get isConnecting(): boolean {
    return sessionState.status === "Connecting";
  },
  get isRecording(): boolean {
    return sessionState.status === "Recording";
  },
  get isBuffering(): boolean {
    return sessionState.status === "Buffering";
  },
  get isPaused(): boolean {
    return sessionState.status === "Paused";
  },
  get isReconnecting(): boolean {
    return sessionState.status === "Reconnecting";
  },
  get isDraining(): boolean {
    return sessionState.status === "Draining";
  },
  get isError(): boolean {
    return sessionState.status === "Error";
  },
  get canStart(): boolean {
    return sessionState.status === "Idle" || sessionState.status === "Error";
  },
  get canPause(): boolean {
    return (
      sessionState.status === "Recording" ||
      sessionState.status === "Buffering" ||
      sessionState.status === "Reconnecting"
    );
  },
  get canResume(): boolean {
    return sessionState.status === "Paused";
  },
  get canStop(): boolean {
    return (
      sessionState.status === "Connecting" ||
      sessionState.status === "Buffering" ||
      sessionState.status === "Recording" ||
      sessionState.status === "Paused" ||
      sessionState.status === "Reconnecting" ||
      sessionState.status === "Draining"
    );
  },

  get elapsedSeconds(): number {
    return recordingTimer;
  },

  get sessionId(): string | null {
    if (
      sessionState.status === "Buffering" ||
      sessionState.status === "Recording" ||
      sessionState.status === "Paused" ||
      sessionState.status === "Reconnecting" ||
      sessionState.status === "Draining"
    ) {
      return sessionState.detail.session_id;
    }
    return null;
  },

  get errorMessage(): string | null {
    if (sessionState.status === "Error") {
      return sessionState.detail.message;
    }
    return null;
  },
};

function startTimer() {
  stopTimer();
  syncElapsedSeconds(sessionState);
  timerInterval = setInterval(() => {
    syncElapsedSeconds(sessionState);
  }, 1000);
}

function stopTimer() {
  if (timerInterval) {
    clearInterval(timerInterval);
    timerInterval = null;
  }
  if (
    sessionState.status !== "Buffering" &&
    sessionState.status !== "Recording" &&
    sessionState.status !== "Paused" &&
    sessionState.status !== "Reconnecting" &&
    sessionState.status !== "Draining"
  ) {
    recordingTimer = 0;
  }
}
