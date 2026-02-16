//! Secret sanitization utilities for redacting sensitive information.
//!
//! Provides regex-based secret redaction for:
//! - OpenAI API keys (`sk-...`)
//! - AWS Access Key IDs (`AKIA...`)
//! - Bearer tokens (`Bearer ...`)
//! - Generic secret assignments (`api_key=...`, `password:...`, etc.)
//!
//! Use this module to sanitize text before logging, displaying in UI,
//! or storing in session archives.

use regex::Regex;
use std::sync::LazyLock;

/// OpenAI API key pattern: sk- followed by alphanumeric characters
static OPENAI_KEY_REGEX: LazyLock<Regex> = LazyLock::new(|| compile_regex(r"sk-[A-Za-z0-9]{20,}"));

/// AWS Access Key ID pattern: AKIA followed by 16 alphanumeric characters
static AWS_ACCESS_KEY_ID_REGEX: LazyLock<Regex> =
    LazyLock::new(|| compile_regex(r"\bAKIA[0-9A-Z]{16}\b"));

/// Bearer token pattern: "Bearer " followed by token characters
static BEARER_TOKEN_REGEX: LazyLock<Regex> =
    LazyLock::new(|| compile_regex(r"(?i)\bBearer\s+[A-Za-z0-9.\-_]{16,}\b"));

/// Generic secret assignment pattern: key=value or key: value format
/// Matches common secret key names like api_key, token, secret, password
static SECRET_ASSIGNMENT_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    compile_regex(r#"(?i)\b(api[\-_]?key|token|secret|password)\b(\s*[:=]\s*)(["']?)[^\s"']{8,}"#)
});

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
/// let input = "API key is sk-abc123xyz789".to_string();
/// let output = redact_secrets(input);
/// assert_eq!(output, "API key is [REDACTED_SECRET]");
/// ```
pub fn redact_secrets(input: String) -> String {
    let redacted = OPENAI_KEY_REGEX.replace_all(&input, "[REDACTED_SECRET]");
    let redacted = AWS_ACCESS_KEY_ID_REGEX.replace_all(&redacted, "[REDACTED_SECRET]");
    let redacted = BEARER_TOKEN_REGEX.replace_all(&redacted, "Bearer [REDACTED_SECRET]");
    let redacted = SECRET_ASSIGNMENT_REGEX.replace_all(&redacted, "$1$2$3[REDACTED_SECRET]");

    redacted.to_string()
}

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
        let input = "sk-abcdefghijklmnopqrstuvwxyz123456".to_string();
        let output = redact_secrets(input);
        assert_eq!(output, "[REDACTED_SECRET]");
    }

    #[test]
    fn redacts_aws_access_key() {
        let input = "AKIAIOSFODNN7EXAMPLE".to_string();
        let output = redact_secrets(input);
        assert_eq!(output, "[REDACTED_SECRET]");
    }

    #[test]
    fn redacts_bearer_token() {
        let input = "Authorization: Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9".to_string();
        let output = redact_secrets(input);
        assert_eq!(output, "Authorization: Bearer [REDACTED_SECRET]");
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
        let input = "Keys: sk-test12345678901234567890 and AKIAIOSFODNN7EXAMPLE".to_string();
        let output = redact_secrets(input);
        // Verify both secrets are redacted
        assert!(output.contains("[REDACTED_SECRET]"));
        assert!(!output.contains("AKIAIOSFODNN7EXAMPLE"));
        assert!(!output.contains("sk-test12345678901234567890"));
    }

    #[test]
    fn preserves_non_secret_text() {
        let input = "Hello world, this is normal text".to_string();
        let output = redact_secrets(input);
        assert_eq!(output, "Hello world, this is normal text");
    }
}
