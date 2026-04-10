use chrono::{DateTime, Utc};

use crate::types::TranscriptSegment;
use crate::websocket_client::SonioxToken;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LateTranslationState {
    Accepting,
    Closed,
    Expired,
}

#[derive(Debug, Clone, Default)]
pub struct ParsedSonioxUpdate {
    pub original_final_delta: String,
    pub original_nonfinal_snapshot: String,
    pub translation_final_delta: String,
    pub translation_nonfinal_snapshot: String,
    pub speaker: Option<String>,
    pub start_ms: u64,
    pub end_ms: u64,
    pub has_spoken_text: bool,
    pub has_translation_text: bool,
    pub has_spoken_final: bool,
    pub is_translation_only: bool,
}

#[derive(Debug, Clone)]
pub struct ActiveUtterance {
    pub id: String,
    pub original_final_text: String,
    pub original_nonfinal_text: String,
    pub translation_final_text: String,
    pub translation_nonfinal_text: String,
    pub speaker: Option<String>,
    pub start_ms: u64,
    pub end_ms: u64,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct FinalizedUtteranceCandidate {
    pub segment_id: String,
    pub created_at: DateTime<Utc>,
    pub speaker: Option<String>,
    pub start_ms: u64,
    pub end_ms: u64,
    pub original_text: String,
    pub translation_final_text: String,
    pub translation_nonfinal_text: String,
    pub finalized_at: DateTime<Utc>,
    pub session_generation: u32,
    pub late_translation_state: LateTranslationState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TranslationOnlyMatch {
    Unique(usize),
    NoMatch,
    Ambiguous,
}

impl ActiveUtterance {
    #[must_use]
    pub fn new() -> Self {
        Self {
            id: format!("turn-{}", Utc::now().timestamp_millis()),
            original_final_text: String::new(),
            original_nonfinal_text: String::new(),
            translation_final_text: String::new(),
            translation_nonfinal_text: String::new(),
            speaker: None,
            start_ms: 0,
            end_ms: 0,
            created_at: Utc::now(),
        }
    }

    pub fn apply_update(&mut self, parsed: &ParsedSonioxUpdate) {
        if parsed.speaker.is_some() {
            self.speaker = parsed.speaker.clone();
        }

        if parsed.start_ms != 0 {
            self.start_ms = if self.start_ms == 0 {
                parsed.start_ms
            } else {
                self.start_ms.min(parsed.start_ms)
            };
        }

        if parsed.end_ms != 0 {
            self.end_ms = self.end_ms.max(parsed.end_ms);
        }

        self.original_final_text
            .push_str(&parsed.original_final_delta);
        self.original_nonfinal_text = parsed.original_nonfinal_snapshot.clone();
        self.translation_final_text
            .push_str(&parsed.translation_final_delta);
        self.translation_nonfinal_text = parsed.translation_nonfinal_snapshot.clone();
    }

    #[must_use]
    pub fn original_text(&self) -> String {
        format!("{}{}", self.original_final_text, self.original_nonfinal_text)
    }

    #[must_use]
    pub fn translated_text(&self) -> String {
        format!("{}{}", self.translation_final_text, self.translation_nonfinal_text)
    }

    #[must_use]
    fn build_segment(&self, is_final: bool) -> TranscriptSegment {
        TranscriptSegment {
            id: self.id.clone(),
            start_ms: self.start_ms,
            end_ms: self.end_ms,
            speaker: self.speaker.clone(),
            original_text: self.original_text(),
            translated_text: self.translated_text(),
            is_final,
            created_at: self.created_at,
        }
    }

    #[must_use]
    pub fn build_partial_segment(&self) -> Option<TranscriptSegment> {
        let segment = self.build_segment(false);
        if segment.original_text.is_empty() && segment.translated_text.is_empty() {
            None
        } else {
            Some(segment)
        }
    }

    #[must_use]
    pub fn into_finalized_candidate(
        self,
        session_generation: u32,
    ) -> Option<FinalizedUtteranceCandidate> {
        let original_text = self.original_text();
        let translated_text = self.translated_text();

        if original_text.is_empty() && translated_text.is_empty() {
            return None;
        }

        Some(FinalizedUtteranceCandidate {
            segment_id: self.id,
            created_at: self.created_at,
            speaker: self.speaker,
            start_ms: self.start_ms,
            end_ms: self.end_ms,
            original_text,
            translation_final_text: self.translation_final_text,
            translation_nonfinal_text: self.translation_nonfinal_text,
            finalized_at: Utc::now(),
            session_generation,
            late_translation_state: LateTranslationState::Accepting,
        })
    }

}

impl FinalizedUtteranceCandidate {
    pub fn apply_translation_update(&mut self, parsed: &ParsedSonioxUpdate) {
        self.translation_final_text
            .push_str(&parsed.translation_final_delta);
        self.translation_nonfinal_text = parsed.translation_nonfinal_snapshot.clone();
    }

    #[must_use]
    pub fn is_accepting(&self, session_generation: u32, now: DateTime<Utc>, ttl_ms: i64) -> bool {
        self.session_generation == session_generation
            && self.late_translation_state == LateTranslationState::Accepting
            && now.signed_duration_since(self.finalized_at).num_milliseconds() <= ttl_ms
    }

    #[must_use]
    pub fn translated_text(&self) -> String {
        format!(
            "{}{}",
            self.translation_final_text, self.translation_nonfinal_text
        )
    }

    #[must_use]
    pub fn build_final_segment(&self) -> TranscriptSegment {
        TranscriptSegment {
            id: self.segment_id.clone(),
            start_ms: self.start_ms,
            end_ms: self.end_ms,
            speaker: self.speaker.clone(),
            original_text: self.original_text.clone(),
            translated_text: self.translated_text(),
            is_final: true,
            created_at: self.created_at,
        }
    }
}

const TRANSLATION_MATCH_OVERLAP_SCORE: i32 = 100;
const TRANSLATION_MATCH_NEARBY_SCORE: i32 = 30;
const TRANSLATION_MATCH_MAX_TIMING_DISTANCE_MS: u64 = 300;
const TRANSLATION_MATCH_SPEAKER_MATCH_SCORE: i32 = 20;
const TRANSLATION_MATCH_SPEAKER_MISMATCH_PENALTY: i32 = 20;
const TRANSLATION_MATCH_RECENT_FINALIZATION_BONUS: i32 = 5;
const TRANSLATION_MATCH_RECENT_FINALIZATION_WINDOW_MS: i64 = 1_000;
const TRANSLATION_MATCH_NO_TIMING_BASE_SCORE: i32 = 140;
const TRANSLATION_MATCH_NO_TIMING_DECAY_MS: i64 = 10;
const TRANSLATION_MATCH_MIN_SCORE: i32 = 40;
const TRANSLATION_MATCH_MIN_WIN_MARGIN: i32 = 20;

fn has_complete_valid_interval(start_ms: u64, end_ms: u64) -> bool {
    start_ms != 0 && end_ms != 0 && start_ms <= end_ms
}

fn score_translation_only_candidate(
    parsed: &ParsedSonioxUpdate,
    candidate: &FinalizedUtteranceCandidate,
    now: DateTime<Utc>,
) -> i32 {
    let mut score = 0;
    let has_parsed_timing = has_complete_valid_interval(parsed.start_ms, parsed.end_ms);
    let has_candidate_timing = has_complete_valid_interval(candidate.start_ms, candidate.end_ms);

    if has_parsed_timing && has_candidate_timing {
        let overlaps = parsed.start_ms <= candidate.end_ms && parsed.end_ms >= candidate.start_ms;
        if overlaps {
            score += TRANSLATION_MATCH_OVERLAP_SCORE;
        } else {
            let distance = if parsed.end_ms < candidate.start_ms {
                candidate.start_ms.saturating_sub(parsed.end_ms)
            } else {
                parsed.start_ms.saturating_sub(candidate.end_ms)
            };
            if distance <= TRANSLATION_MATCH_MAX_TIMING_DISTANCE_MS {
                score += TRANSLATION_MATCH_NEARBY_SCORE;
            }
        }
    }

    let recency_ms = now.signed_duration_since(candidate.finalized_at).num_milliseconds();

    if !has_parsed_timing {
        let no_timing_decay_steps = (recency_ms.max(0) / TRANSLATION_MATCH_NO_TIMING_DECAY_MS) as i32;
        score += (TRANSLATION_MATCH_NO_TIMING_BASE_SCORE - no_timing_decay_steps).max(0);
    }

    if let (Some(update_speaker), Some(candidate_speaker)) =
        (parsed.speaker.as_ref(), candidate.speaker.as_ref())
    {
        if update_speaker == candidate_speaker {
            score += TRANSLATION_MATCH_SPEAKER_MATCH_SCORE;
        } else {
            score -= TRANSLATION_MATCH_SPEAKER_MISMATCH_PENALTY;
        }
    }

    if recency_ms <= TRANSLATION_MATCH_RECENT_FINALIZATION_WINDOW_MS {
        score += TRANSLATION_MATCH_RECENT_FINALIZATION_BONUS;
    }

    score
}

#[must_use]
pub fn match_translation_only_update(
    parsed: &ParsedSonioxUpdate,
    candidates: &[FinalizedUtteranceCandidate],
    session_generation: u32,
    now: DateTime<Utc>,
    ttl_ms: i64,
) -> TranslationOnlyMatch {
    let mut scored: Vec<(usize, i32)> = candidates
        .iter()
        .enumerate()
        .filter(|(_, candidate)| candidate.is_accepting(session_generation, now, ttl_ms))
        .map(|(idx, candidate)| (idx, score_translation_only_candidate(parsed, candidate, now)))
        .filter(|(_, score)| *score >= TRANSLATION_MATCH_MIN_SCORE)
        .collect();

    scored.sort_by(|a, b| b.1.cmp(&a.1));

    let Some((winner_idx, winner_score)) = scored.first().copied() else {
        return TranslationOnlyMatch::NoMatch;
    };

    if let Some((_, runner_up_score)) = scored.get(1).copied()
        && winner_score - runner_up_score < TRANSLATION_MATCH_MIN_WIN_MARGIN
    {
        return TranslationOnlyMatch::Ambiguous;
    }

    TranslationOnlyMatch::Unique(winner_idx)
}

#[must_use]
fn should_ignore_transcript_marker(text: &str) -> bool {
    matches!(text.trim(), "." | "<end>")
}

#[must_use]
pub fn parse_soniox_update(tokens: &[SonioxToken]) -> ParsedSonioxUpdate {
    let mut parsed = ParsedSonioxUpdate::default();

    let mut spoken_min_start = u64::MAX;
    let mut spoken_max_end = 0;
    let mut translation_min_start = u64::MAX;
    let mut translation_max_end = 0;
    let mut has_spoken_timing = false;
    let mut has_translation_timing = false;

    for token in tokens {
        let is_translation = matches!(token.translation_status.as_deref(), Some("translation"));
        let text = token.text.clone();
        let has_valid_timing = token.start_ms != 0 || token.end_ms != 0;

        if should_ignore_transcript_marker(&text) {
            continue;
        }

        if token.speaker.is_some() {
            parsed.speaker = token.speaker.clone();
        }

        if is_translation {
            parsed.has_translation_text = true;
            if token.is_final {
                parsed.translation_final_delta.push_str(&text);
            } else {
                parsed.translation_nonfinal_snapshot.push_str(&text);
            }
            if has_valid_timing {
                translation_min_start = translation_min_start.min(token.start_ms);
                translation_max_end = translation_max_end.max(token.end_ms);
                has_translation_timing = true;
            }
        } else {
            parsed.has_spoken_text = true;
            if token.is_final {
                parsed.original_final_delta.push_str(&text);
                parsed.has_spoken_final = true;
            } else {
                parsed.original_nonfinal_snapshot.push_str(&text);
            }
            if has_valid_timing {
                spoken_min_start = spoken_min_start.min(token.start_ms);
                spoken_max_end = spoken_max_end.max(token.end_ms);
                has_spoken_timing = true;
            }
        }
    }

    parsed.is_translation_only = parsed.has_translation_text && !parsed.has_spoken_text;

    let (start_ms, end_ms) = if has_spoken_timing {
        (spoken_min_start, spoken_max_end)
    } else if has_translation_timing {
        (translation_min_start, translation_max_end)
    } else {
        (0, 0)
    };

    parsed.start_ms = start_ms;
    parsed.end_ms = end_ms;
    parsed
}
