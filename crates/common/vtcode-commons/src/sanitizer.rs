//! Secret sanitization utilities for redacting sensitive information.
//!
//! Provides regex-based secret redaction for:
//! - OpenAI API keys (`sk-...`)
//! - AWS Access Key IDs (`AKIA...`)
//! - GitHub App installation tokens (`ghs_...`, including ~520-char stateless JWTs)
//! - Other GitHub token prefixes (`ghp_`/`gho_`/`ghu_`/`ghr_`/`github_pat_...`)
//! - Bearer tokens (`Bearer ...`)
//! - Generic secret assignments (`api_key=...`, `password:...`, etc.)
//!
//! Use this module to sanitize text before logging, displaying in UI,
//! or storing in session archives.
//!
//! GitHub App installation tokens are treated as opaque strings per
//! <https://github.blog/changelog/2026-05-15-github-app-installation-tokens-per-request-override-header/>
//! and <https://github.blog/changelog/2026-10-02-stateless-github-app-installation-tokens-rolled-out>:
//! `ghs_`-prefixed JWTs (~520 chars, two dots) must not be subject to length
//! assumptions and must be redacted even without a `Bearer` prefix.

use regex::Regex;
use std::sync::LazyLock;

/// OpenAI API key pattern: sk- followed by alphanumeric characters
static OPENAI_KEY_REGEX: LazyLock<Regex> = LazyLock::new(|| compile_regex(r"sk-[A-Za-z0-9_-]{16,}"));

/// AWS Access Key ID pattern: AKIA followed by 16 alphanumeric characters
static AWS_ACCESS_KEY_ID_REGEX: LazyLock<Regex> = LazyLock::new(|| compile_regex(r"\bAKIA[0-9A-Z]{16}\b"));

/// GitHub App installation token pattern (recommended by GitHub):
/// `ghs_[A-Za-z0-9\.\-_]{36,}` matches both legacy opaque tokens (~40 chars)
/// and stateless JWT-format tokens (~520 chars, two dots).
static GITHUB_APP_TOKEN_REGEX: LazyLock<Regex> = LazyLock::new(|| compile_regex(r"ghs_[A-Za-z0-9.\-_]{36,}"));

/// Remaining GitHub token family: personal (`ghp_`), OAuth (`gho_`),
/// user-to-server (`ghu_`), and refresh (`ghr_`) tokens share the classic
/// 36+ char suffix shape; fine-grained PATs use the distinctive
/// `github_pat_` prefix with a 22+ char suffix. Same opaque-string
/// treatment as `ghs_`: redact on prefix alone, without length oracles.
static GITHUB_TOKEN_REGEX: LazyLock<Regex> =
    LazyLock::new(|| compile_regex(r"(?:gh[pour]_[A-Za-z0-9.\-_]{36,}|github_pat_[A-Za-z0-9.\-_]{22,})"));

/// Bearer token pattern: "Bearer " followed by token characters
static BEARER_TOKEN_REGEX: LazyLock<Regex> = LazyLock::new(|| compile_regex(r"(?i)\bBearer\s+[A-Za-z0-9.\-_]{16,}\b"));

/// Generic secret assignment pattern: key=value or key: value format
/// Matches common secret key names like api_key, token, secret, password
static SECRET_ASSIGNMENT_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    compile_regex(
        r#"(?i)\b((?:[a-z0-9][a-z0-9_-]*?)?(?:api[\-_]?key|access[\-_]?key|client[\-_]?secret|credential|private[\-_]?key|token|secret|password|auth)[a-z0-9_-]*)\b(\s*[:=]\s*)(["']?)[^\s"']{8,}"#,
    )
});

/// Maximum serialized size of a provider diagnostic after redaction.
pub const PROVIDER_DIAGNOSTIC_MAX_BYTES: usize = 8 * 1024;
const PROVIDER_DIAGNOSTIC_TRUNCATION_MARKER: &str = "… [diagnostic truncated]";

/// Replacement marker for control characters in terminal-bound text.
const TERMINAL_CONTROL_REPLACEMENT: char = '\u{FFFD}';

/// Bidi controls that reorder terminal output while occupying no visible space.
///
/// A crafted override (U+202E) in a displayed URI or domain can visually spoof a
/// trusted value, so these are neutralized like control characters. Joiners and
/// other format characters stay intact: legitimate names may contain emoji.
fn is_bidi_control(character: char) -> bool {
    matches!(
        character,
        '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}'
    )
}

fn is_neutralized_terminal_char(character: char) -> bool {
    character.is_control() || is_bidi_control(character)
}

/// Neutralize control and bidi-reordering characters and bound length for terminal display.
///
/// Server-originated strings (resource URIs, domains, names) must pass through
/// here before reaching terminal output: raw newlines split rows, BEL/CR/ESC
/// enable terminal mangling, bidi controls visually reorder text, and unbounded
/// values blow up layouts. Shell metacharacters (`$`, backticks) are inert in
/// terminal output and preserved.
pub fn sanitize_terminal_text(input: &str, max_chars: usize) -> String {
    if input.chars().all(|character| !is_neutralized_terminal_char(character)) {
        return crate::formatting::truncate_text(input, max_chars, "…");
    }
    let neutralized: String = input
        .chars()
        .map(|character| {
            if is_neutralized_terminal_char(character) {
                TERMINAL_CONTROL_REPLACEMENT
            } else {
                character
            }
        })
        .collect();
    crate::formatting::truncate_text(&neutralized, max_chars, "…")
}

/// Join disclosed entries with per-item sanitizing and a count-capped tail.
///
/// Each item is neutralized via [`sanitize_terminal_text`] with `max_item_chars`;
/// at most `max_items` are shown before the remainder folds into a count.
pub fn join_sanitized_truncated(items: &[String], max_items: usize, max_item_chars: usize) -> String {
    let sanitized: Vec<String> = items.iter().map(|item| sanitize_terminal_text(item, max_item_chars)).collect();
    if sanitized.len() <= max_items {
        return sanitized.join(", ");
    }
    let shown: Vec<&str> = sanitized.iter().take(max_items).map(String::as_str).collect();
    format!("{} … +{} more", shown.join(", "), sanitized.len() - max_items)
}

/// Redact secrets and sensitive keys from a string.
///
/// This is a best-effort operation using well-known regex patterns.
/// Redacted values are replaced with `[REDACTED_SECRET]`.
///
/// # Examples
///
/// ```
/// use vtcode_commons::sanitizer::redact_secrets;
///
/// let input = format!("Found key: {}", concat!("sk-", "test1234567890abcdef"));
/// let output = redact_secrets(input);
/// assert_eq!(output, "Found key: [REDACTED_SECRET]");
/// ```
pub fn redact_secrets(input: String) -> String {
    let r1 = OPENAI_KEY_REGEX.replace_all(&input, "[REDACTED_SECRET]");
    let r2 = AWS_ACCESS_KEY_ID_REGEX.replace_all(&r1, "[REDACTED_SECRET]");
    let r3 = GITHUB_APP_TOKEN_REGEX.replace_all(&r2, "[REDACTED_SECRET]");
    let r4 = GITHUB_TOKEN_REGEX.replace_all(&r3, "[REDACTED_SECRET]");
    let r5 = BEARER_TOKEN_REGEX.replace_all(&r4, "Bearer [REDACTED_SECRET]");
    let r6 = SECRET_ASSIGNMENT_REGEX.replace_all(&r5, "$1$2$3[REDACTED_SECRET]");
    // `into_owned` clones only when the final result is `Borrowed` (no regex
    // matched at all); when any redaction occurred it moves the owned string
    // without an extra allocation. Do NOT short-circuit on `Cow::Borrowed` —
    // the final Cow is `Borrowed` whenever the *last* regex doesn't match,
    // even if earlier regexes did, which would silently discard redactions.
    r6.into_owned()
}

/// Redact secrets and return a bounded, UTF-8-safe provider diagnostic.
///
/// The input is sampled with a carry window so a secret beginning near the
/// output boundary is still redacted before the final size limit is applied.
pub fn sanitize_provider_diagnostic(input: impl AsRef<[u8]>) -> String {
    let input = input.as_ref();
    let sample_len = input.len().min(PROVIDER_DIAGNOSTIC_MAX_BYTES + STREAMING_REDACTION_CARRY_BYTES);
    let sample = String::from_utf8_lossy(input.get(..sample_len).unwrap_or(input));
    let redacted = redact_secrets(sample.into_owned());
    if redacted.len() <= PROVIDER_DIAGNOSTIC_MAX_BYTES {
        return redacted;
    }

    let content_limit = PROVIDER_DIAGNOSTIC_MAX_BYTES.saturating_sub(PROVIDER_DIAGNOSTIC_TRUNCATION_MARKER.len());
    let end = redacted.floor_char_boundary(content_limit);
    format!("{}{}", redacted.get(..end).unwrap_or(&redacted), PROVIDER_DIAGNOSTIC_TRUNCATION_MARKER)
}

/// Incrementally redact streamed output without retaining the full stream.
///
/// A bounded suffix is held between chunks so a secret split at an IO
/// boundary is still matched by the same redaction rules as a complete line.
#[derive(Debug, Default)]
pub struct StreamingSecretRedactor {
    pending: String,
}

const STREAMING_REDACTION_CARRY_BYTES: usize = 1_024;

impl StreamingSecretRedactor {
    /// Redact and return the safe prefix of `chunk`. The returned string may
    /// be empty while the bounded carry window is being filled.
    pub fn push(&mut self, chunk: &str) -> String {
        self.pending.push_str(chunk);
        if self.pending.len() <= STREAMING_REDACTION_CARRY_BYTES {
            if !self.pending.contains('\n') {
                return String::new();
            }
        }

        let carry_split = self.pending.len().saturating_sub(STREAMING_REDACTION_CARRY_BYTES);
        let line_split = self.pending.rfind('\n').map(|index| index + 1).unwrap_or(0);
        let split_at = self.pending.floor_char_boundary(carry_split.max(line_split));
        let prefix: String = self.pending.drain(..split_at).collect();
        redact_secrets(prefix)
    }

    /// Redact and return the final carried suffix.
    pub fn finish(self) -> String {
        redact_secrets(self.pending)
    }
}

#[allow(
    clippy::panic,
    reason = "Intentional compatibility, platform, or test-only suppression."
)]
fn compile_regex(pattern: &str) -> Regex {
    match Regex::new(pattern) {
        Ok(regex) => regex,
        // Panic is acceptable thanks to the `load_regex` test
        Err(err) => panic!("invalid regex pattern `{pattern}`: {err}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_regex() {
        // Verify all regex patterns compile without panicking
        let _ = redact_secrets("test".to_string());
    }

    #[test]
    fn redacts_openai_key() {
        let input = format!("Found key: {}", concat!("sk-", "test1234567890abcdef"));
        let output = redact_secrets(input);
        assert_eq!(output, "Found key: [REDACTED_SECRET]");
    }

    #[test]
    fn redacts_aws_access_key() {
        // Assemble the documentation fixture at runtime so repository scans do not
        // mistake it for a live credential.
        let aws_key = concat!("AKIA", "IOSFODNN7EXAMPLE");
        let input = format!(" creds: {aws_key} ");
        let output = redact_secrets(input);
        assert_eq!(output, " creds: [REDACTED_SECRET] ");
    }

    #[test]
    fn redacts_bearer_token() {
        let input = "Authorization: Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9".to_string();
        let output = redact_secrets(input);
        assert_eq!(output, "Authorization: Bearer [REDACTED_SECRET]");
    }

    #[test]
    fn redacts_github_app_installation_tokens() {
        // Legacy opaque format (~40 chars, no dots) and stateless JWT format
        // (~520 chars, two dots) per May/Oct 2026 changelogs. Both must be
        // treated as opaque and redacted even without a Bearer prefix.
        // Fixtures are assembled at runtime so scans do not flag them.
        let legacy = format!("ghs_{}", "a".repeat(36));
        let header = format!("ghs_{}", "b".repeat(20));
        let payload = format!("{}.", "c".repeat(250));
        let signature = "d".repeat(250);
        let stateless = format!("{header}.{payload}{signature}");
        assert!(stateless.len() >= 500, "fixture should model ~520-char token");
        assert_eq!(stateless.matches('.').count(), 2);

        for token in [&legacy, &stateless] {
            let bare = redact_secrets(format!("saw token {token} in logs"));
            assert_eq!(bare, "saw token [REDACTED_SECRET] in logs");
            assert!(!bare.contains(token.as_str()));

            let bearer = redact_secrets(format!("Authorization: Bearer {token}"));
            assert!(!bearer.contains(token.as_str()));
        }
    }

    #[test]
    fn github_app_token_threshold_is_36_chars() {
        // Locks the GitHub-recommended `ghs_[A-Za-z0-9.\-_]{36,}` boundary:
        // 35 chars must not redact on its own, 36 chars must.
        let short = format!("ghs_{}", "a".repeat(35));
        let exact = format!("ghs_{}", "a".repeat(36));
        assert_eq!(redact_secrets(format!("saw {short} here")), format!("saw {short} here"));
        assert_eq!(redact_secrets(format!("saw {exact} here")), "saw [REDACTED_SECRET] here");
    }

    #[test]
    fn redacts_github_token_family_besides_app_tokens() {
        // Fixtures are assembled at runtime so scans do not flag them.
        for prefix in ["ghp_", "gho_", "ghu_", "ghr_"] {
            let token = format!("{prefix}{}", "e".repeat(36));
            let output = redact_secrets(format!("pasted {token} in chat"));
            assert_eq!(output, "pasted [REDACTED_SECRET] in chat", "{prefix}");
            assert!(!output.contains(token.as_str()));
        }
        let fine_grained = format!("github_pat_{}", "f".repeat(22));
        let output = redact_secrets(format!("pasted {fine_grained} in chat"));
        assert_eq!(output, "pasted [REDACTED_SECRET] in chat");
        assert!(!output.contains(fine_grained.as_str()));
    }

    #[test]
    fn github_token_family_thresholds_match_app_token_policy() {
        // `gh?_` prefixes need a 36+ char suffix, `github_pat_` needs 22+;
        // shorter lookalikes (docs, placeholders) must survive.
        let short_classic = format!("ghp_{}", "a".repeat(35));
        let exact_classic = format!("ghp_{}", "a".repeat(36));
        assert_eq!(redact_secrets(format!("saw {short_classic} here")), format!("saw {short_classic} here"));
        assert_eq!(redact_secrets(format!("saw {exact_classic} here")), "saw [REDACTED_SECRET] here");
        let short_pat = format!("github_pat_{}", "a".repeat(21));
        let exact_pat = format!("github_pat_{}", "a".repeat(22));
        assert_eq!(redact_secrets(format!("saw {short_pat} here")), format!("saw {short_pat} here"));
        assert_eq!(redact_secrets(format!("saw {exact_pat} here")), "saw [REDACTED_SECRET] here");
    }

    #[test]
    fn redacts_github_app_token_split_across_stream_chunks() {
        // A ~526-char stateless token fits in the 1_024-byte carry window, so
        // halves split at an IO boundary must still redact on finish.
        let token = format!("ghs_{}.{}.{}", "b".repeat(20), "c".repeat(250), "d".repeat(250));
        let mid = token.floor_char_boundary(token.len() / 2);
        let (first, second) = token.split_at(mid);
        let mut redactor = StreamingSecretRedactor::default();
        let mut output = redactor.push(first);
        output.push_str(&redactor.push(&format!("{second}\n")));
        output.push_str(&redactor.finish());
        assert!(!output.contains(token.as_str()));
        assert!(output.contains("[REDACTED_SECRET]"));
    }

    #[test]
    fn redacts_api_key_assignment() {
        let input = "api_key=sk-test12345678".to_string();
        let output = redact_secrets(input);
        assert_eq!(output, "api_key=[REDACTED_SECRET]");
    }

    #[test]
    fn redacts_password_assignment() {
        let input = "password: mysecretvalue".to_string();
        let output = redact_secrets(input);
        assert_eq!(output, "password: [REDACTED_SECRET]");
    }

    #[test]
    fn redacts_token_in_quotes() {
        let input = r#"token="abc123xyz789abcdef""#.to_string();
        let output = redact_secrets(input);
        assert_eq!(output, r#"token="[REDACTED_SECRET]""#);
    }

    #[test]
    fn preserves_short_values() {
        // Values under 8 characters should not be redacted
        let input = "password: short".to_string();
        let output = redact_secrets(input);
        assert_eq!(output, "password: short");
    }

    #[test]
    fn redacts_multiple_secrets() {
        let openai_key = concat!("sk-", "test1234567890abcdef");
        let aws_key = concat!("AKIA", "IOSFODNN7EXAMPLE");
        let input = format!("Keys: {openai_key} and {aws_key}");
        let output = redact_secrets(input);
        // Verify both secrets are redacted
        assert!(output.contains("[REDACTED_SECRET]"));
        assert!(!output.contains(openai_key));
        assert!(!output.contains(aws_key));
    }

    #[test]
    fn preserves_non_secret_text() {
        let input = "Hello world, this is normal text".to_string();
        let output = redact_secrets(input);
        assert_eq!(output, "Hello world, this is normal text");
    }

    #[test]
    fn redacts_secrets_split_across_stream_chunks() {
        let mut redactor = StreamingSecretRedactor::default();
        let mut output = redactor.push("password=superse");
        output.push_str(&redactor.push("cretvalue\n"));
        output.push_str(&redactor.finish());

        assert_eq!(output, "password=[REDACTED_SECRET]\n");
        assert!(!output.contains("supersecretvalue"));
    }

    #[test]
    fn terminal_text_neutralizes_controls_and_bounds_length() {
        assert_eq!(sanitize_terminal_text("", 8), "");
        assert_eq!(sanitize_terminal_text("abc", 8), "abc");
        assert_eq!(sanitize_terminal_text("abcdefgh", 8), "abcdefgh");
        assert_eq!(sanitize_terminal_text("abcdefghi", 8), "abcdefgh…");
        assert_eq!(sanitize_terminal_text("ab", 0), "…");
    }

    #[test]
    fn terminal_text_replaces_controls_but_preserves_shell_metachars() {
        assert_eq!(sanitize_terminal_text("a\nb\rc\td\0e\x07f\x1bg", 64), "a�b�c�d�e�f�g");
        assert_eq!(sanitize_terminal_text("price is $5 `code` ~user", 64), "price is $5 `code` ~user");
        assert_eq!(sanitize_terminal_text("ui://x/\u{7f}app", 64), "ui://x/�app");
    }

    #[test]
    fn terminal_text_truncates_on_char_boundaries() {
        assert_eq!(sanitize_terminal_text("日本語テスト", 4), "日本語テ…");
        assert_eq!(sanitize_terminal_text("日本語", 4), "日本語");
    }

    #[test]
    fn terminal_text_neutralizes_bidi_controls_but_preserves_joiners() {
        assert_eq!(sanitize_terminal_text("ui://\u{202e}evil/app", 64), "ui://�evil/app");
        assert_eq!(sanitize_terminal_text("a\u{2066}b\u{2069}c", 64), "a�b�c");
        assert_eq!(sanitize_terminal_text("x\u{200e}y\u{200f}z", 64), "x�y�z");
        // Zero-width joiners keep emoji sequences intact.
        assert_eq!(sanitize_terminal_text("fam 👨\u{200d}👩\u{200d}👧 ok", 64), "fam 👨\u{200d}👩\u{200d}👧 ok");
    }

    #[test]
    fn join_sanitized_truncated_folds_only_past_max() {
        let items: Vec<String> = (0..8).map(|n| format!("d{n}")).collect();
        assert_eq!(join_sanitized_truncated(&items, 8, usize::MAX), "d0, d1, d2, d3, d4, d5, d6, d7");
        let items: Vec<String> = (0..9).map(|n| format!("d{n}")).collect();
        assert_eq!(join_sanitized_truncated(&items, 8, usize::MAX), "d0, d1, d2, d3, d4, d5, d6, d7 … +1 more");
        let items: Vec<String> = (0..11).map(|n| format!("d{n}")).collect();
        assert_eq!(join_sanitized_truncated(&items, 8, usize::MAX), "d0, d1, d2, d3, d4, d5, d6, d7 … +3 more");
        // Per-item caps apply before joining; controls in any item are neutralized.
        let items = vec!["abcdefghij".to_string(), "x\ny".to_string()];
        assert_eq!(join_sanitized_truncated(&items, 8, 4), "abcd…, x�y");
        assert!(join_sanitized_truncated(&[], 8, usize::MAX).is_empty());
    }

    #[test]
    fn provider_diagnostic_is_bounded_utf8_safe_and_redacted() {
        let mut input = b"api_key=diagnostic-secret-value Bearer abcdefghijklmnop ".to_vec();
        input.extend(std::iter::repeat_n(b'x', 20_000));
        input.extend_from_slice("終端".as_bytes());
        input.push(0xff);

        let output = sanitize_provider_diagnostic(input);

        assert!(output.len() <= PROVIDER_DIAGNOSTIC_MAX_BYTES);
        assert!(output.is_char_boundary(output.len()));
        assert!(!output.contains("diagnostic-secret-value"));
        assert!(!output.contains("abcdefghijklmnop"));
    }
}
