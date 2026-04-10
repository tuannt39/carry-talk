use std::collections::VecDeque;

use crate::types::TranscriptSegment;

/// Bounded in-memory buffer for transcript segments.
/// Keeps partial (non-final) segments and recent final segments.
/// Evicts oldest final segments when capacity is reached.
pub struct TranscriptBuffer {
    segments: VecDeque<TranscriptSegment>,
    max_capacity: usize,
    /// Segments that have been finalized but not yet flushed to disk.
    pending_flush: Vec<TranscriptSegment>,
}

impl TranscriptBuffer {
    pub fn new(max_capacity: usize) -> Self {
        Self {
            segments: VecDeque::with_capacity(max_capacity),
            max_capacity,
            pending_flush: Vec::new(),
        }
    }

    fn merge_segment(existing: &TranscriptSegment, incoming: TranscriptSegment) -> TranscriptSegment {
        let TranscriptSegment {
            id: _,
            start_ms,
            end_ms,
            speaker,
            original_text,
            translated_text,
            is_final,
            created_at: _,
        } = incoming;

        let incoming_has_original_text = !original_text.is_empty();
        let merged_original_text = if incoming_has_original_text {
            original_text
        } else {
            existing.original_text.clone()
        };

        let merged_translated_text = if translated_text.is_empty() {
            existing.translated_text.clone()
        } else {
            translated_text
        };

        TranscriptSegment {
            id: existing.id.clone(),
            start_ms: if start_ms == 0 {
                existing.start_ms
            } else {
                start_ms
            },
            end_ms: if end_ms == 0 {
                existing.end_ms
            } else {
                end_ms
            },
            speaker: speaker.or_else(|| existing.speaker.clone()),
            original_text: merged_original_text,
            translated_text: merged_translated_text,
            is_final: existing.is_final || is_final,
            created_at: existing.created_at,
        }
    }

    /// Insert or update a segment by id.
    /// If a segment with the same id exists, it is merged.
    /// If the segment is new and final, or if an already-final segment receives
    /// a meaningful merged update, it goes to pending_flush.
    pub fn upsert(&mut self, segment: TranscriptSegment) {
        // Check if segment already exists (update case)
        if let Some(pos) = self.segments.iter().position(|s| s.id == segment.id) {
            let existing = self.segments[pos].clone();
            let was_final = existing.is_final;
            let merged = Self::merge_segment(&existing, segment);
            let became_final = !was_final && merged.is_final;
            let updated_final_segment = was_final
                && merged.is_final
                && (merged.start_ms != existing.start_ms
                    || merged.end_ms != existing.end_ms
                    || merged.speaker != existing.speaker
                    || merged.original_text != existing.original_text
                    || merged.translated_text != existing.translated_text);
            self.segments[pos] = merged.clone();

            // If segment just became final, or a final segment changed, queue for flush
            if became_final || updated_final_segment {
                self.pending_flush.push(merged);
            }
        } else {
            // New segment
            if segment.is_final {
                self.pending_flush.push(segment.clone());
            }
            self.segments.push_back(segment);

            // Evict oldest final segments if over capacity
            while self.segments.len() > self.max_capacity {
                if let Some(front) = self.segments.front() {
                    if front.is_final {
                        self.segments.pop_front();
                    } else {
                        break; // Don't evict non-final segments
                    }
                }
            }
        }
    }

    /// Get all current segments (for UI rendering).
    pub fn snapshot(&self) -> Vec<TranscriptSegment> {
        self.segments.iter().cloned().collect()
    }

    /// Take all pending segments for flushing to JSONL.
    /// Clears the pending buffer.
    pub fn take_pending(&mut self) -> Vec<TranscriptSegment> {
        std::mem::take(&mut self.pending_flush)
    }

    /// Restore pending segments after a failed flush attempt.
    /// Restored segments are placed back at the front to preserve flush order.
    pub fn restore_pending(&mut self, segments: Vec<TranscriptSegment>) {
        if segments.is_empty() {
            return;
        }

        let mut restored = segments;
        restored.append(&mut self.pending_flush);
        self.pending_flush = restored;
    }

    /// Force-take all remaining pending segments (for session end).
    pub fn flush_all(&mut self) -> Vec<TranscriptSegment> {
        self.take_pending()
    }

    /// Clear all segments (on session stop).
    pub fn clear(&mut self) {
        self.segments.clear();
        self.pending_flush.clear();
    }
}
