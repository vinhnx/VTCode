//! Stream-level sanitization for provider noise and control tokens.
//!
//! Extracted from `ui_interaction_stream` to make the sanitization logic
//! independently testable and to provide a single, guarded interface for all
//! provider-specific stream noise. This module is the **sole owner** of
//! stream-sanitization state — the main stream loop delegates to
//! [`StreamSanitizer::process_delta`] / [`StreamSanitizer::finalize`] and
//! never touches provider-specific details directly.
//!
//! ## Two categories of noise
//!
//! - **Harmony control tokens** (gpt-oss / OpenAI harmony format):
//!   `<|start|>...<|end|>` block structure that must be parsed and
//!   selectively retained (only `final` channel content is kept).
//! - **Flat noise tokens** (MiniMax): `]<]minimax[>[` fragments that are
//!   simply removed wherever they appear — in deltas, commentary, or final
//!   answers.
//!
//! Both use an accumulator + prefix-diff pattern so that tokens split across
//! stream deltas are handled correctly.

use crate::agent::runloop::unified::turn::harmony::strip_harmony_syntax;
use crate::agent::runloop::unified::turn::provider_noise::{
    contains_provider_noise, noise_token_partial_suffix, strip_provider_noise,
};
use crate::agent::runloop::unified::ui_interaction_stream_helpers::common_prefix_len;

// Harmony marker detection and constants are centralized in `harmony.rs`
// alongside `strip_harmony_syntax`. This module imports them rather than
// duplicating the marker list.
use crate::agent::runloop::unified::turn::harmony::{HARMONY_END_TAGS, contains_harmony_marker};

fn incomplete_harmony_block_start(raw: &str) -> Option<usize> {
    let start_pos = raw.rfind("<|start|>")?;
    let tail = &raw[start_pos..];
    let has_terminator = HARMONY_END_TAGS.iter().any(|terminator| tail.contains(terminator));
    if has_terminator {
        None
    } else {
        Some(start_pos)
    }
}

fn sanitize_harmony_stream_text(raw: &str) -> String {
    let stable_raw = if let Some(start_pos) = incomplete_harmony_block_start(raw) {
        &raw[..start_pos]
    } else {
        raw
    };
    strip_harmony_syntax(stable_raw)
}

/// Result of processing a single stream delta through the sanitizer.
pub(crate) struct SanitizedDelta {
    /// The visible portion to render/emit for this delta.
    pub visible_delta: String,
    /// When `Some`, the caller should **replace** the entire `aggregated`
    /// string with this value (harmony mode retroactively rewrites content).
    /// When `None`, the caller should **append** `visible_delta` to
    /// `aggregated` normally.
    pub aggregated_override: Option<String>,
}

/// Encapsulates all stream-level sanitization state for a single response.
///
/// Create one [`StreamSanitizer`] per response stream and call
/// [`process_delta`](Self::process_delta) for each `LLMStreamEvent::Token`
/// delta. At stream end (or for non-streamed responses), call
/// [`finalize`](Self::finalize) to clean the complete text.
pub(crate) struct StreamSanitizer {
    // Harmony block-parsing state
    harmony_mode: bool,
    harmony_raw: String,
    harmony_visible: String,
    // Flat-noise (MiniMax) accumulator state
    noise_mode: bool,
    noise_raw: String,
    noise_visible: String,
}

impl StreamSanitizer {
    pub(crate) fn new() -> Self {
        Self {
            harmony_mode: false,
            harmony_raw: String::new(),
            harmony_visible: String::new(),
            noise_mode: false,
            noise_raw: String::new(),
            noise_visible: String::new(),
        }
    }

    /// Process a raw stream delta and return the sanitized visible portion.
    ///
    /// The delta first passes through flat-noise stripping (MiniMax), then
    /// through harmony block parsing. This order ensures noise tokens don't
    /// corrupt harmony tag boundaries.
    ///
    /// # Performance
    ///
    /// The fast path (no noise detected) is O(delta_size) — it does a quick
    /// `contains` scan and passes through without accumulation. Only when noise
    /// is detected (or a partial token is at the delta boundary) does the
    /// accumulator engage, which is O(accumulated_size) per delta. This keeps
    /// non-MiniMax providers at zero overhead beyond a substring check.
    pub(crate) fn process_delta(&mut self, delta: &str) -> SanitizedDelta {
        // --- Phase 1: Flat noise stripping (MiniMax `]<]minimax[>[`) ---
        let (after_noise, noise_aggregated) = if !self.noise_mode {
            // Fast path: check if this delta contains a complete noise token.
            if contains_provider_noise(delta) {
                // Complete token found — activate noise mode and strip this delta.
                self.noise_mode = true;
                self.noise_raw.push_str(delta);
                let cleaned = strip_provider_noise(&self.noise_raw);
                self.noise_visible = cleaned.clone();
                (cleaned.clone(), Some(cleaned))
            } else if let Some(suffix_len) = noise_token_partial_suffix(delta) {
                // The delta ends with a prefix of a noise token (e.g. `]<]mini`
                // is a prefix of `]<]minimax[>[`). Activate noise mode and hold
                // back the partial token — it may complete on the next delta.
                self.noise_mode = true;
                let visible_len = delta.len().saturating_sub(suffix_len);
                let visible = delta[..visible_len].to_string();
                self.noise_raw = delta.to_string();
                self.noise_visible = visible.clone();
                (visible.clone(), Some(visible))
            } else {
                // No noise at all — fast pass-through, zero accumulation.
                (delta.to_string(), None)
            }
        } else {
            // Noise mode active: accumulate and strip the full buffer.
            // This is only reached for MiniMax (or after a partial token
            // detection), so the O(n) cost is acceptable.
            self.noise_raw.push_str(delta);
            let cleaned = strip_provider_noise(&self.noise_raw);
            let prefix_len = common_prefix_len(&self.noise_visible, &cleaned);
            self.noise_visible = cleaned.clone();
            let visible = cleaned.get(prefix_len..).unwrap_or_default().to_string();
            (visible, Some(cleaned))
        };

        // --- Phase 2: Harmony block parsing ---
        if !self.harmony_mode && contains_harmony_marker(&after_noise) {
            self.harmony_mode = true;
        }
        if self.harmony_mode {
            self.harmony_raw.push_str(&after_noise);
            let sanitized = sanitize_harmony_stream_text(&self.harmony_raw);
            let prefix_len = common_prefix_len(&self.harmony_visible, &sanitized);
            let visible_delta = sanitized.get(prefix_len..).unwrap_or_default().to_string();
            self.harmony_visible = sanitized.clone();
            SanitizedDelta {
                visible_delta,
                aggregated_override: Some(sanitized),
            }
        } else {
            SanitizedDelta {
                visible_delta: after_noise,
                aggregated_override: noise_aggregated,
            }
        }
    }

    /// Sanitize final (complete) text — called once at stream end or for
    /// non-streamed responses. Strips flat noise tokens, then harmony syntax.
    pub(crate) fn finalize(&self, text: &str) -> String {
        let stripped = strip_provider_noise(text);
        if contains_harmony_marker(&stripped) {
            strip_harmony_syntax(&stripped)
        } else {
            stripped
        }
    }

    /// Whether any sanitization mode is currently active.
    #[allow(dead_code)]
    pub(crate) fn mode_active(&self) -> bool {
        self.harmony_mode || self.noise_mode
    }
}

impl Default for StreamSanitizer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::StreamSanitizer;

    #[test]
    fn clean_text_passes_through_unchanged() {
        let mut s = StreamSanitizer::new();
        let result = s.process_delta("Hello, world!");
        assert_eq!(result.visible_delta, "Hello, world!");
        assert!(result.aggregated_override.is_none());
    }

    #[test]
    fn minimax_noise_stripped_from_stream_delta() {
        let mut s = StreamSanitizer::new();
        let result = s.process_delta("]<]minimax[>[Real content here");
        assert_eq!(result.visible_delta, "Real content here");
        // Noise mode provides aggregated_override for retroactive correction
        assert!(result.aggregated_override.is_some());
        assert_eq!(result.aggregated_override.as_deref(), Some("Real content here"));
    }

    #[test]
    fn minimax_noise_split_across_deltas() {
        let mut s = StreamSanitizer::new();
        let r1 = s.process_delta("Before ]<]mini");
        let r2 = s.process_delta("max[>[ after");
        // The first delta passes the partial token through (noise mode not
        // yet activated). The second delta completes the token; noise mode
        // activates and `aggregated_override` retroactively corrects the
        // full accumulated text, removing the now-complete noise token.
        let _ = r1;
        assert!(r2.aggregated_override.is_some());
        assert_eq!(r2.aggregated_override.as_deref(), Some("Before  after"));
    }

    #[test]
    fn repeated_minimax_noise_all_stripped() {
        let mut s = StreamSanitizer::new();
        let result = s.process_delta("]<]minimax[>[]<]minimax[>[Final");
        assert_eq!(result.visible_delta, "Final");
    }

    #[test]
    fn harmony_block_stripped_from_stream() {
        let mut s = StreamSanitizer::new();
        let result =
            s.process_delta("<|start|>assistant<|channel|>commentary to=tool<|message|>{}<|call|>");
        assert_eq!(result.visible_delta, "");
        // Harmony mode replaces aggregated entirely
        assert_eq!(result.aggregated_override.as_deref(), Some(""));
    }

    #[test]
    fn harmony_final_channel_content_retained() {
        let mut s = StreamSanitizer::new();
        let input = "<|start|>assistant<|channel|>final<|message|>Visible answer<|end|>";
        let result = s.process_delta(input);
        assert_eq!(result.visible_delta, "Visible answer");
    }

    #[test]
    fn minimax_noise_then_harmony_both_stripped() {
        let mut s = StreamSanitizer::new();
        let input = "]<]minimax[>[<|start|>assistant<|channel|>final<|message|>Clean<|end|>";
        let result = s.process_delta(input);
        assert_eq!(result.visible_delta, "Clean");
    }

    #[test]
    fn finalize_strips_both_noise_types() {
        let s = StreamSanitizer::new();
        let input =
            "]<]minimax[>[<|start|>assistant<|channel|>commentary<|message|>hidden<|call|> visible";
        let result = s.finalize(input);
        assert_eq!(result, "visible");
    }

    #[test]
    fn finalize_preserves_clean_text() {
        let s = StreamSanitizer::new();
        assert_eq!(s.finalize("Just normal text"), "Just normal text");
    }

    #[test]
    fn finalize_strips_standalone_minimax_noise() {
        let s = StreamSanitizer::new();
        assert_eq!(s.finalize("]<]minimax[>[Hello"), "Hello");
    }

    #[test]
    fn mode_active_reflects_state() {
        let mut s = StreamSanitizer::new();
        assert!(!s.mode_active());
        s.process_delta("]<]minimax[>[text");
        assert!(s.mode_active());
    }
}
