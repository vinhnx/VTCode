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

/// Approximate token count from character length (~4 chars per token).
fn heuristic_token_count(text: &str) -> usize {
    text.len().div_ceil(4)
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
        let end = (max_tokens * 4).min(text.len());
        let mut end = end;
        while end > 0 && !text.is_char_boundary(end) {
            end -= 1;
        }
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
}
