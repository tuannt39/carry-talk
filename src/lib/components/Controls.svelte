<script lang="ts">
  import { session } from "$lib/stores/session.svelte";
  import { transcript } from "$lib/stores/transcript.svelte";
  import {
    hasApiKey,
    pauseSession,
    resumeSession,
    startSession,
    stopSession,
  } from "$lib/services/commands";
  import { t } from "$lib/i18n";
  import { devError } from "$lib/utils/devLogger";

  type NotifyKind = "info" | "success" | "warning" | "error";

  let {
    onNotify = () => {},
    onRequireApiKey = () => {},
  }: {
    onNotify?: (kind: NotifyKind, message: string) => void;
    onRequireApiKey?: () => void;
  } = $props();

  let pendingAction = $state<"start" | "pause" | "resume" | "stop" | null>(null);

  function formatError(error: unknown): string {
    const message = error instanceof Error ? error.message : String(error);
    const normalized = message.replace(/^Error:\s*/, "");
    const authMatch = normalized.match(/^Auth\((.*)\)$/);
    return authMatch ? authMatch[1] : normalized;
  }

  function isMissingApiKeyError(error: unknown): boolean {
    const message = formatError(error);
    return (
      message === "API key not configured" ||
      message === "Authentication: API key not configured"
    );
  }

  function promptForApiKey() {
    onRequireApiKey();
  }

  async function handleStart() {
    pendingAction = "start";

    try {
      const apiKeyExists = await hasApiKey();
      if (!apiKeyExists) {
        promptForApiKey();
        return;
      }

      await startSession();
    } catch (error) {
      devError("Failed to start session:", error);
      if (isMissingApiKeyError(error)) {
        promptForApiKey();
      } else {
        onNotify("error", formatError(error));
      }
    } finally {
      pendingAction = null;
    }
  }

  async function handlePause() {
    pendingAction = "pause";

    try {
      await pauseSession();
    } catch (error) {
      devError("Failed to pause session:", error);
      onNotify("error", formatError(error));
    } finally {
      pendingAction = null;
    }
  }

  async function handleResume() {
    pendingAction = "resume";

    try {
      await resumeSession();
    } catch (error) {
      devError("Failed to resume session:", error);
      onNotify("error", formatError(error));
    } finally {
      pendingAction = null;
    }
  }

  async function handleStop() {
    pendingAction = "stop";

    try {
      await stopSession();
      transcript.clear();
    } catch (error) {
      devError("Failed to stop session:", error);
      onNotify("error", formatError(error));
    } finally {
      pendingAction = null;
    }
  }

  const showStopButton = $derived(session.canStop);
  const showPauseControls = $derived(session.canPause);
  const showResumeControls = $derived(session.isPaused);
  const showStartingState = $derived(pendingAction === "start" && !session.canStop);
  const showStopOnlyControls = $derived(session.canStop && !session.canPause && !session.isPaused);
  const stopDisabled = $derived(pendingAction === "stop");
</script>

<div class="flex items-center justify-center gap-3">
  {#if showPauseControls}
    <button
      id="btn-pause"
      type="button"
      onclick={handlePause}
      disabled={pendingAction !== null}
      class="px-6 py-2.5 rounded-full text-sm font-medium text-white transition-all duration-150 disabled:opacity-50 disabled:cursor-not-allowed"
      style="background: var(--color-accent-500);"
    >
      {#if pendingAction === "pause"}
        <span class="inline-block w-4 h-4 border-2 border-white/30 border-t-white rounded-full animate-spin"></span>
      {:else}
        {t("controls.pause")}
      {/if}
    </button>

    <button
      id="btn-stop"
      type="button"
      onclick={handleStop}
      disabled={stopDisabled}
      class="px-4 py-2.5 rounded-full border text-sm font-medium transition-all duration-150 disabled:opacity-50 disabled:cursor-not-allowed"
      style="border-color: var(--border-color); background: color-mix(in srgb, var(--bg-secondary) 82%, var(--bg-elevated) 18%); color: var(--text-secondary);"
    >
      {#if pendingAction === "stop"}
        <span class="inline-block w-4 h-4 border-2 border-white/30 border-t-white rounded-full animate-spin"></span>
      {:else}
        {t("controls.stop")}
      {/if}
    </button>
  {:else if showResumeControls}
    <button
      id="btn-continue"
      type="button"
      onclick={handleResume}
      disabled={pendingAction !== null}
      class="px-6 py-2.5 rounded-full text-sm font-medium text-white transition-all duration-150 disabled:opacity-50 disabled:cursor-not-allowed"
      style="background: var(--color-accent-500);"
    >
      {#if pendingAction === "resume"}
        <span class="inline-block w-4 h-4 border-2 border-white/30 border-t-white rounded-full animate-spin"></span>
      {:else}
        {t("controls.continue")}
      {/if}
    </button>

    <button
      id="btn-stop"
      type="button"
      onclick={handleStop}
      disabled={stopDisabled}
      class="px-4 py-2.5 rounded-full border text-sm font-medium transition-all duration-150 disabled:opacity-50 disabled:cursor-not-allowed"
      style="border-color: var(--border-color); background: color-mix(in srgb, var(--bg-secondary) 82%, var(--bg-elevated) 18%); color: var(--text-secondary);"
    >
      {#if pendingAction === "stop"}
        <span class="inline-block w-4 h-4 border-2 border-white/30 border-t-white rounded-full animate-spin"></span>
      {:else}
        {t("controls.stop")}
      {/if}
    </button>
  {:else if showStartingState}
    <button
      id="btn-starting"
      type="button"
      disabled={true}
      aria-label={t("session.connecting")}
      class="px-6 py-2.5 rounded-full text-sm font-medium text-white transition-all duration-150 disabled:opacity-50 disabled:cursor-not-allowed"
      style="background: var(--color-accent-500);"
    >
      <div class="flex items-center">
        <span class="inline-block w-4 h-4 border-2 border-white/30 border-t-white rounded-full animate-spin"></span>
      </div>
    </button>
  {:else if showStopOnlyControls}
    <button
      id="btn-stop"
      type="button"
      onclick={handleStop}
      disabled={stopDisabled}
      class="px-4 py-2.5 rounded-full border text-sm font-medium transition-all duration-150 disabled:opacity-50 disabled:cursor-not-allowed"
      style="border-color: var(--border-color); background: color-mix(in srgb, var(--bg-secondary) 82%, var(--bg-elevated) 18%); color: var(--text-secondary);"
    >
      {#if pendingAction === "stop"}
        <span class="inline-block w-4 h-4 border-2 border-white/30 border-t-white rounded-full animate-spin"></span>
      {:else}
        {t("controls.stop")}
      {/if}
    </button>
  {:else if session.canStart}
    <button
      id="btn-start"
      type="button"
      onclick={handleStart}
      disabled={pendingAction !== null}
      class="px-6 py-2.5 rounded-full text-sm font-medium text-white transition-all duration-150 disabled:opacity-50 disabled:cursor-not-allowed"
      style="background: var(--color-accent-500);"
    >
      <div class="flex items-center gap-2">
        <span>{t("controls.start")}</span>
      </div>
    </button>
  {/if}

  {#if showStopButton && transcript.count > 0}
    <span class="text-xs font-mono" style="color: var(--text-secondary);">
      {transcript.finalCount} {t("controls.segments")}
    </span>
  {/if}
</div>
