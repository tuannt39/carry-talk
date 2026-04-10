import type { TranscriptSegment } from "$lib/types";

/** Reactive transcript store using Svelte 5 runes. */
let segments = $state<TranscriptSegment[]>([]);

export const transcript = {
  get segments(): TranscriptSegment[] {
    return segments;
  },

  /** Replace all segments (full snapshot from backend). */
  setAll(newSegments: TranscriptSegment[]) {
    segments = newSegments;
  },

  /** Clear all segments (on session stop). */
  clear() {
    segments = [];
  },

  get count(): number {
    return segments.length;
  },

  get finalCount(): number {
    return segments.filter((s) => s.is_final).length;
  },

  get isEmpty(): boolean {
    return segments.length === 0;
  },
};
