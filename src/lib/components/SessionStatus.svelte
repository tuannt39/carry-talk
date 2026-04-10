<script lang="ts">
  import { t } from "$lib/i18n";
  import { getSessionStatusLabelKey, session } from "$lib/stores/session.svelte";
  import { transcript } from "$lib/stores/transcript.svelte";

  const statusConfig: Record<string, { color: string; pulse: boolean }> = {
    Idle: { color: "var(--color-surface-400)", pulse: false },
    Connecting: {
      color: "var(--color-warning-500)",
      pulse: true,
    },
    Buffering: {
      color: "var(--color-warning-500)",
      pulse: true,
    },
    Recording: {
      color: "var(--color-success-500)",
      pulse: true,
    },
    Paused: {
      color: "var(--color-warning-500)",
      pulse: false,
    },
    Reconnecting: {
      color: "var(--color-warning-500)",
      pulse: true,
    },
    Draining: {
      color: "var(--color-warning-500)",
      pulse: true,
    },
    Error: {
      color: "var(--color-error-500)",
      pulse: false,
    },
  };

  let config = $derived(statusConfig[session.state.status] ?? statusConfig.Idle);
  let statusLabelKey = $derived(getSessionStatusLabelKey(session.state, transcript.isEmpty));
  let statusLabel = $derived(t(statusLabelKey));
</script>

<div class="flex items-center gap-2" style="color: var(--text-secondary);">
  <span
    class="inline-block w-2 h-2 rounded-full"
    class:animate-pulse={config.pulse}
    style="background-color: {config.color};"
  ></span>
  <span class="text-xs font-medium">{statusLabel}</span>
</div>
