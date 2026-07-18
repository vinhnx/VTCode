//! Centralized provider-noise stripping.
//!
//! Some LLM providers emit spurious control tokens into their streamed output.
//! MiniMax (the Anthropic-compatible endpoint) is the primary offender: it
//! interleaves `]<]minimax[>[` fragments throughout text deltas, tool-call
//! bodies, and final answers. Left unchecked this noise leaks into:
//!
//! - The user-visible live stream (commentary text rendered in the TUI).
//! - `working_history`, which is echoed back to the API on follow-up calls,
//!   polluting context and degrading subsequent responses.
//! - Textual tool-call parsing, where the noise corrupts tag boundaries.
//!
//! Previously this stripping was duplicated across three uncoordinated sites
//! (`text_tools::parse_tagged`, `context::response_handling`, and the stream
//! renderer), each with a different approach. This module is the **single
//! source of truth**: all call sites delegate here so the noise vocabulary and
//! stripping semantics stay consistent and extensible.
//!
//! ## Design
//!
//! - [`strip_provider_noise`] is a pure, allocation-light function that removes
//!   every known noise token from a string. It is safe to call on any text —
//!   tool-call bodies, commentary, final answers, or raw stream deltas.
//! - [`contains_provider_noise`] is a cheap predicate for deciding whether to
//!   engage stream-mode stripping (mirrors `contains_harmony_marker`).
//! - [`RECOVERY_NOISE_FALLBACK`] and [`sanitize_recovery_answer`] handle the
//!   special case where a tool-free recovery pass produces *only* noise: the
//!   user gets an actionable message instead of empty/garbage text.

/// The canonical MiniMax streaming-noise token. It appears as a bracketed
/// pseudo-XML fragment that the model emits between (and sometimes inside)
/// content deltas, reasoning blocks, and tool-call invocations.
pub(crate) const MINIMAX_NOISE_TOKEN: &str = "]<]minimax[>[";

/// All known provider-noise tokens. Add new entries here when a new provider
/// starts emitting stray control tokens; every call site updates automatically.
const PROVIDER_NOISE_TOKENS: &[&str] = &[MINIMAX_NOISE_TOKEN];

/// Deterministic fallback shown to the user when a tool-free recovery pass
/// produces only provider noise or empty text. Mirrors the intent of
/// `turn_loop::RECOVERY_SYNTHESIS_FALLBACK_FINAL_ANSWER` without coupling
/// these modules.
pub(crate) const RECOVERY_NOISE_FALLBACK: &str = "I was unable to complete this task from the available context. Re-run the request, \
or provide more specific guidance so I can proceed without re-reading the same resources.";

/// Returns `true` if `text` contains any known provider-noise token.
///
/// Use this to decide whether to engage stream-mode stripping (analogous to
/// `contains_harmony_marker`). It is a simple substring scan — cheap enough to
/// call per-delta on the hot stream path.
#[inline]
pub(crate) fn contains_provider_noise(text: &str) -> bool {
    PROVIDER_NOISE_TOKENS.iter().any(|token| text.contains(token))
}

/// Check if the end of `text` is a prefix of any known noise token.
///
/// This is used by `StreamSanitizer` to detect noise tokens that are split
/// across stream deltas. Returns the byte length of the matching suffix, or
/// `None` if the text doesn't end with a partial noise token.
///
/// # Example
/// ```ignore
/// // "]<]mini" is a prefix of "]<]minimax[>[" — returns 8
/// assert_eq!(noise_token_partial_suffix("Before ]<]mini"), Some(8));
/// // "clean text" doesn't end with a partial token — returns None
/// assert_eq!(noise_token_partial_suffix("clean text"), None);
/// ```
#[inline]
pub(crate) fn noise_token_partial_suffix(text: &str) -> Option<usize> {
    for token in PROVIDER_NOISE_TOKENS {
        // Check progressively shorter prefixes of the token (longest first)
        // to find the longest suffix of `text` that matches a token prefix.
        for prefix_len in (1..token.len()).rev() {
            if text.ends_with(&token[..prefix_len]) {
                return Some(prefix_len);
            }
        }
    }
    None
}

/// Remove every occurrence of every known provider-noise token from `text`.
///
/// Noise tokens may appear at the start, mid-text, or repeated. This function
/// strips them all in a single pass per token. It does **not** trim or
/// substitute a fallback — callers that need empty-input handling (recovery
/// passes) should use [`sanitize_recovery_answer`] instead.
#[inline]
pub(crate) fn strip_provider_noise(text: &str) -> String {
    let mut cleaned = text.to_string();
    for token in PROVIDER_NOISE_TOKENS {
        if cleaned.contains(token) {
            cleaned = cleaned.replace(token, "");
        }
    }
    cleaned
}

/// Strip provider noise from a **recovery** final answer and, if nothing
/// meaningful remains, substitute [`RECOVERY_NOISE_FALLBACK`].
///
/// This is the safety net for the "agent just stops with garbage" symptom
/// (checkpoints turn_609/613): a guard forced a tool-free recovery pass, the
/// model emitted only a noise prefix, and the noise became the user-visible
/// answer. Non-recovery callers should use [`strip_provider_noise`] directly
/// — commentary and normal final answers should be cleaned but *not* replaced
/// with a fallback (the model may legitimately produce empty prose during
/// thinking).
#[inline]
pub(crate) fn sanitize_recovery_answer(text: String) -> String {
    let cleaned = strip_provider_noise(&text);
    if cleaned.trim().is_empty() {
        RECOVERY_NOISE_FALLBACK.to_string()
    } else {
        cleaned
    }
}

#[cfg(test)]
mod tests {
    use super::{
        RECOVERY_NOISE_FALLBACK, contains_provider_noise, noise_token_partial_suffix, sanitize_recovery_answer,
        strip_provider_noise,
    };

    #[test]
    fn detects_noise_presence() {
        assert!(contains_provider_noise("]<]minimax[>[hello"));
        assert!(contains_provider_noise("text ]<]minimax[>[ more"));
        assert!(!contains_provider_noise("clean text"));
        assert!(!contains_provider_noise(""));
    }

    #[test]
    fn strips_single_occurrence() {
        assert_eq!(strip_provider_noise("]<]minimax[>[Here is my summary."), "Here is my summary.");
    }

    #[test]
    fn strips_repeated_occurrences() {
        assert_eq!(strip_provider_noise("]<]minimax[>[]<]minimax[>[Real answer"), "Real answer");
    }

    #[test]
    fn strips_mid_text_occurrences() {
        assert_eq!(strip_provider_noise("Before ]<]minimax[>[ after"), "Before  after");
    }

    #[test]
    fn preserves_clean_text() {
        assert_eq!(strip_provider_noise("All tests pass."), "All tests pass.");
    }

    #[test]
    fn preserves_empty_text() {
        assert_eq!(strip_provider_noise(""), "");
    }

    #[test]
    fn recovery_strips_and_keeps_content() {
        assert_eq!(sanitize_recovery_answer("]<]minimax[>[Here is my summary.".to_string()), "Here is my summary.");
    }

    #[test]
    fn recovery_substitutes_fallback_for_noise_only() {
        assert_eq!(sanitize_recovery_answer("]<]minimax[>[".to_string()), RECOVERY_NOISE_FALLBACK);
    }

    #[test]
    fn recovery_substitutes_fallback_for_empty() {
        assert_eq!(sanitize_recovery_answer(String::new()), RECOVERY_NOISE_FALLBACK);
    }

    #[test]
    fn recovery_substitutes_fallback_for_whitespace() {
        assert_eq!(sanitize_recovery_answer("   \n\t ".to_string()), RECOVERY_NOISE_FALLBACK);
    }

    #[test]
    fn recovery_preserves_legitimate_text() {
        assert_eq!(sanitize_recovery_answer("All tests pass.".to_string()), "All tests pass.");
    }

    #[test]
    fn recovery_strips_repeated_noise() {
        assert_eq!(sanitize_recovery_answer("]<]minimax[>[]<]minimax[>[Real answer".to_string()), "Real answer");
    }

    #[test]
    fn partial_suffix_detects_token_start() {
        // "]<]mini" is a 7-byte prefix of "]<]minimax[>[" (13 bytes total)
        assert_eq!(noise_token_partial_suffix("Before ]<]mini"), Some(7));
    }

    #[test]
    fn partial_suffix_detects_short_prefix() {
        // "]" is a 1-byte prefix of "]<]minimax[>["
        assert_eq!(noise_token_partial_suffix("text ]"), Some(1));
    }

    #[test]
    fn partial_suffix_returns_none_for_clean_text() {
        assert_eq!(noise_token_partial_suffix("clean text"), None);
        assert_eq!(noise_token_partial_suffix(""), None);
    }

    #[test]
    fn partial_suffix_returns_none_for_complete_token() {
        // A complete token is not a "partial" suffix — `contains_provider_noise`
        // handles complete tokens. `noise_token_partial_suffix` only checks
        // prefixes shorter than the full token.
        assert_eq!(noise_token_partial_suffix("]<]minimax[>["), None);
    }

    #[test]
    fn partial_suffix_finds_longest_match() {
        // "]<]minimax" is a 10-byte prefix of "]<]minimax[>[" (13 bytes)
        assert_eq!(noise_token_partial_suffix("Before ]<]minimax"), Some(10));
    }
}
