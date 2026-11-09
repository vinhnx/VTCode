#![expect(
    clippy::indexing_slicing,
    clippy::string_slice,
    reason = "Token and UTF-8 boundaries come from tokenizer output and the byte truncation helper."
)]

//! Token counting via tiktoken BPE tokenizer.
//!
//! All token estimation goes through [`tiktoken`]'s `cl100k_base` encoding
//! (GPT-4, GPT-3.5-turbo). BPE tokenizers are similar enough across providers
//! that this gives reasonable accuracy for Anthropic, Gemini, and others.
//!
//! Provider-reported exact token counts (from API responses) should always be
//! preferred when available. This module is for pre-call budget estimation and
//! offline token sizing where no provider response exists yet.

use std::sync::OnceLock;
use tiktoken::CoreBpe;

/// Return the process-global `cl100k_base` BPE instance, if it could be loaded.
///
/// Loaded once on first call; all subsequent calls return the same reference.
/// Returns `None` only if the builtin encoding fails to load, in which case
/// callers fall back to a character-based heuristic rather than panicking.
fn bpe() -> Option<&'static CoreBpe> {
    static BPE: OnceLock<Option<&'static CoreBpe>> = OnceLock::new();
    *BPE.get_or_init(|| tiktoken::get_encoding("cl100k_base"))
}

/// Approximate bytes per token used by the byte-length heuristic.
///
/// Single source of truth for the `~4 chars/token` convention shared by
/// preview budgets, schema estimates, and message metadata. BPE-backed
/// [`estimate_tokens`] stays the default; the heuristic exists for hot paths
/// and serialized payloads where running the tokenizer costs more than the
/// accuracy is worth.
pub const CHARS_PER_TOKEN: usize = 4;

/// Approximate token count from a byte length without running the tokenizer.
///
/// Rounds UP (`div_ceil`): budget gates must never undercount, so any
/// non-empty payload costs at least one token. Prefer [`estimate_tokens`]
/// when accuracy matters more than speed.
#[must_use]
pub fn heuristic_tokens_from_byte_len(byte_len: usize) -> usize {
    byte_len.div_ceil(CHARS_PER_TOKEN)
}

/// `u64` twin of [`heuristic_tokens_from_byte_len`] for lifetime byte
/// counters that never narrow to `usize`.
#[must_use]
pub fn heuristic_tokens_from_byte_len_u64(byte_len: u64) -> u64 {
    byte_len.div_ceil(CHARS_PER_TOKEN as u64)
}

/// Approximate token count from text length without running the tokenizer.
///
/// `text.len()` counts bytes, so multibyte text overestimates slightly; that
/// is the safe direction for a budget fallback.
fn heuristic_token_count(text: &str) -> usize {
    heuristic_tokens_from_byte_len(text.len())
}

/// Count the number of tokens in `text` using tiktoken BPE.
///
/// Returns 0 for empty strings. Falls back to a character-based heuristic if
/// the BPE tokenizer is unavailable.
pub fn estimate_tokens(text: &str) -> usize {
    if text.is_empty() {
        return 0;
    }
    match bpe() {
        Some(bpe) => bpe.count(text),
        None => heuristic_token_count(text),
    }
}

/// Format a token count with a compact decimal suffix for transcript labels.
///
/// Counts below one thousand stay exact. Larger counts are rounded to one
/// decimal place using `k`, `M`, or `B` suffixes.
pub fn format_token_count(tokens: usize) -> String {
    let (divisor, suffix) = match tokens {
        999_500_000.. => (1_000_000_000, "B"),
        999_500.. => (1_000_000, "M"),
        1_000.. => (1_000, "k"),
        _ => return tokens.to_string(),
    };

    let whole = tokens / divisor;
    let decimal_step = divisor / 10;
    // Divide into tenths before rounding; multiplying a billion-scale
    // remainder by ten would overflow on 32-bit targets.
    let tenths = (tokens % divisor + decimal_step / 2) / decimal_step;
    let (whole, tenths) = if tenths >= 10 {
        (whole.saturating_add(1), 0)
    } else {
        (whole, tenths)
    };
    if tenths == 0 {
        format!("{whole}{suffix}")
    } else {
        format!("{whole}.{tenths}{suffix}")
    }
}

/// Truncate `text` to at most `max_tokens` tokens.
///
/// Decodes the truncated token sequence back to text so the result is always
/// valid UTF-8 with no mid-token corruption. Falls back to byte-level
/// truncation if BPE decode fails (should not happen in practice).
pub fn truncate_to_tokens(text: &str, max_tokens: usize) -> String {
    if max_tokens == 0 || text.is_empty() {
        return String::new();
    }
    // Byte-level fallback used when BPE is unavailable or decode fails.
    let byte_truncate = || {
        let end = text.floor_char_boundary(max_tokens.saturating_mul(CHARS_PER_TOKEN));
        let mut result = text[..end].to_string();
        result.push_str("...");
        result
    };
    let Some(bpe) = bpe() else {
        return byte_truncate();
    };
    let tokens = bpe.encode_with_special_tokens(text);
    if tokens.len() <= max_tokens {
        return text.to_string();
    }
    bpe.decode_to_string(&tokens[..max_tokens]).unwrap_or_else(|_| byte_truncate())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_string_returns_zero() {
        assert_eq!(estimate_tokens(""), 0);
        assert_eq!(truncate_to_tokens("", 10), "");
    }

    #[test]
    fn count_is_reasonable() {
        let count = estimate_tokens("Hello, how are you today?");
        assert!((4..=12).contains(&count), "count={count}");
    }

    #[test]
    fn heuristic_byte_len_rounds_up_at_boundaries() {
        assert_eq!(heuristic_tokens_from_byte_len(0), 0);
        assert_eq!(heuristic_tokens_from_byte_len(1), 1);
        assert_eq!(heuristic_tokens_from_byte_len(3), 1);
        assert_eq!(heuristic_tokens_from_byte_len(4), 1);
        assert_eq!(heuristic_tokens_from_byte_len(5), 2);
        assert_eq!(heuristic_tokens_from_byte_len(8), 2);
        assert_eq!(heuristic_tokens_from_byte_len(9), 3);
    }

    #[test]
    fn heuristic_u64_twin_matches_usize_flavor() {
        for len in [0usize, 1, 3, 4, 5, 1_000, 1_000_003] {
            assert_eq!(
                heuristic_tokens_from_byte_len_u64(len as u64),
                heuristic_tokens_from_byte_len(len) as u64,
                "len={len}"
            );
        }
        // `div_ceil` cannot overflow: even the largest input converts exactly.
        assert_eq!(heuristic_tokens_from_byte_len_u64(u64::MAX), u64::MAX.div_ceil(4));
    }

    #[test]
    fn bpe_fallback_heuristic_delegates_to_byte_len_helper() {
        for text in ["", "a", "abcd", "abcde", "Hello, world!", "\u{1f44b}\u{1f30d}"] {
            assert_eq!(heuristic_token_count(text), heuristic_tokens_from_byte_len(text.len()), "text={text:?}");
        }
    }

    #[test]
    fn truncate_respects_limit() {
        let text = "the quick brown fox jumps over the lazy dog";
        let truncated = truncate_to_tokens(text, 5);
        let count = estimate_tokens(&truncated);
        assert!(count <= 5 + 1, "count={count} should be <= 6");
    }

    #[test]
    fn truncate_zero_returns_empty() {
        assert_eq!(truncate_to_tokens("hello", 0), "");
    }

    #[test]
    fn code_and_prose_tokenize() {
        let code = "fn main() { println!(\"hello\"); }";
        let prose = "the main function prints hello to console";
        assert!(estimate_tokens(code) > 0);
        assert!(estimate_tokens(prose) > 0);
    }

    #[test]
    fn json_tokenizes() {
        let json = r#"{"name":"test","value":123,"nested":{"key":"value"}}"#;
        let count = estimate_tokens(json);
        assert!((10..=40).contains(&count), "json count={count}");
    }

    #[test]
    fn token_count_format_is_compact_and_rounds_across_suffix_boundaries() {
        assert_eq!(format_token_count(999), "999");
        assert_eq!(format_token_count(1_000), "1k");
        assert_eq!(format_token_count(1_050), "1.1k");
        assert_eq!(format_token_count(6_100), "6.1k");
        assert_eq!(format_token_count(999_500), "1M");
        assert_eq!(format_token_count(15_600_000), "15.6M");
        assert_eq!(format_token_count(999_500_000), "1B");
        assert_eq!(format_token_count(1_550_000_000), "1.6B");
        assert_eq!(format_token_count(1_549_999_999), "1.5B");
    }
}
