<script lang="ts">
  import { getTranscriptEmptyStateLabelKey, session } from "$lib/stores/session.svelte";
  import { transcript } from "$lib/stores/transcript.svelte";
  import { t } from "$lib/i18n";

  let container: HTMLDivElement;
  let autoScroll = $state(true);

  function selectionIntersectsTranscriptText(selection: Selection): boolean {
    if (!container || selection.rangeCount === 0) return false;

    const range = selection.getRangeAt(0);

    return Array.from(container.querySelectorAll("[data-selectable-text]")).some((element) =>
      range.intersectsNode(element),
    );
  }

  function hasActiveTranscriptSelection(): boolean {
    if (typeof document === "undefined" || !container) return false;

    const selection = document.getSelection();

    if (
      !selection ||
      selection.rangeCount === 0 ||
      selection.isCollapsed ||
      selection.toString().length === 0
    ) {
      return false;
    }

    return selectionIntersectsTranscriptText(selection);
  }

  function formatSegmentTime(ms: number): string {
    const totalSeconds = Math.floor(ms / 1000);
    const minutes = Math.floor(totalSeconds / 60);
    const seconds = totalSeconds % 60;

    return `${String(minutes).padStart(2, "0")}:${String(seconds).padStart(2, "0")}`;
  }

  const displaySegments = $derived(transcript.segments);
  const emptyStateText = $derived(t(getTranscriptEmptyStateLabelKey(session.state)));

  // Auto-scroll when new transcript segments arrive
  $effect(() => {
    displaySegments.length;

    if (autoScroll && container && !hasActiveTranscriptSelection()) {
      requestAnimationFrame(() => {
        container.scrollTop = container.scrollHeight;
      });
    }
  });

  function handleScroll() {
    if (!container) return;
    const threshold = 60;
    const atBottom =
      container.scrollHeight - container.scrollTop - container.clientHeight <
      threshold;
    autoScroll = atBottom;
  }
</script>

<div
  bind:this={container}
  onscroll={handleScroll}
  class="h-full overflow-y-auto py-3"
  id="transcript-view"
>
  {#if transcript.isEmpty}
    <div
      class="min-h-full flex items-center justify-center px-4"
      style="color: var(--text-secondary);"
    >
      <p class="text-sm text-center leading-relaxed">
        {emptyStateText}
      </p>
    </div>
  {:else}
    <div class="flex flex-col px-4">
      {#each displaySegments as segment (segment.id)}
        <div
          class="mt-2 rounded-lg px-3 py-2 transition-opacity duration-200"
          class:opacity-50={!segment.is_final}
          style="background: var(--bg-elevated);"
        >
          <div class="flex items-start justify-between gap-3">
            <div class="min-w-0 flex-1">
              {#if segment.speaker}
                <div class="mb-1 text-xs font-medium" style="color: var(--color-accent-400);">
                  {segment.speaker}
                </div>
              {/if}

              <div data-selectable-text>
                {#if segment.translated_text}
                  <p
                    class="text-[13px] font-medium leading-5"
                    style="color: color-mix(in srgb, var(--text-primary) 88%, var(--text-secondary) 12%);"
                  >
                    {segment.translated_text}
                  </p>

                  {#if segment.original_text}
                    <p
                      class="mt-1 text-xs leading-5"
                      style="color: color-mix(in srgb, var(--text-secondary) 92%, transparent);"
                    >
                      {segment.original_text}
                    </p>
                  {/if}
                {:else if segment.original_text}
                  <p
                    class="text-[13px] font-medium leading-5"
                    style="color: color-mix(in srgb, var(--text-primary) 88%, var(--text-secondary) 12%);"
                  >
                    {segment.original_text}
                  </p>
                {/if}
              </div>
            </div>

            <span
              class="shrink-0 pt-[1px] text-[10px] font-medium tabular-nums"
              style="color: color-mix(in srgb, var(--text-secondary) 88%, transparent);"
            >
              {formatSegmentTime(segment.start_ms)} → {formatSegmentTime(segment.end_ms)}
            </span>
          </div>

        </div>
      {/each}
    </div>
  {/if}
</div>
