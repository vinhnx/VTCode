#![expect(
    clippy::string_slice,
    reason = "Cargo manifest offsets come from ASCII markers and are therefore UTF-8 boundaries."
)]

//! Generic utility functions

use anyhow::{Context, Result};
use regex::Regex;
use sha2::{Digest, Sha256};
use std::time::{SystemTime, UNIX_EPOCH};

/// Get current Unix timestamp in seconds
#[inline]
pub fn current_timestamp() -> u64 {
    current_timestamp_result().unwrap_or(0)
}

/// Get current Unix timestamp in seconds as a fallible operation.
#[inline]
fn current_timestamp_result() -> Result<u64> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("System clock is before UNIX_EPOCH while generating timestamp")?
        .as_secs())
}

/// Calculate the SHA256 hash of `content` and return it as a 64-character
/// lowercase hex string (the standard hex encoding of the 32-byte digest).
///
/// Use this helper whenever a caller needs a stable, ASCII-safe fingerprint
/// of arbitrary bytes - for example, hashing file contents for change
/// detection, config fingerprints, or cache keys.
pub fn calculate_sha256(content: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content);
    let digest = hasher.finalize();
    let mut output = String::with_capacity(digest.len() * 2);

    for byte in digest {
        output.push(nibble_to_hex(byte >> 4));
        output.push(nibble_to_hex(byte & 0x0f));
    }

    output
}

#[allow(
    clippy::unreachable,
    reason = "Intentional compatibility, platform, or test-only suppression."
)]
fn nibble_to_hex(nibble: u8) -> char {
    match nibble {
        0..=9 => char::from(b'0' + nibble),
        10..=15 => char::from(b'a' + (nibble - 10)),
        _ => unreachable!("nibble must be in 0..=15"),
    }
}

/// Extract a string value from a simple TOML key assignment within the `[package]` section
pub fn extract_toml_str(content: &str, key: &str) -> Option<String> {
    // Only consider the [package] section to avoid matching other tables
    let pkg_section = if let Some(start) = content.find("[package]") {
        let rest = &content[start + "[package]".len()..];
        // Stop at next section header or end
        if let Some(_next) = rest.find('\n') {
            &content[start..]
        } else {
            &content[start..]
        }
    } else {
        content
    };

    // Example target: name = "vtcode"
    let pattern = format!(r#"(?m)^\s*{}\s*=\s*"([^"]+)"\s*$"#, regex::escape(key));
    let re = Regex::new(&pattern).ok()?;
    re.captures(pkg_section)
        .and_then(|caps| caps.get(1).map(|m| m.as_str().to_owned()))
}

/// Get the first meaningful section of the README/markdown as an excerpt
pub fn extract_readme_excerpt(md: &str, max_len: usize) -> String {
    // Take from start until we pass the first major sections or hit max_len
    let mut excerpt = String::with_capacity(max_len.min(md.len()));
    for line in md.lines() {
        // Stop if we reach a deep section far into the doc
        if excerpt.len() > max_len {
            break;
        }
        excerpt.push_str(line);
        excerpt.push('\n');
        // Prefer stopping after an initial overview section
        if line.trim().starts_with("## ") && excerpt.len() > (max_len / 2) {
            break;
        }
    }
    crate::formatting::truncate_byte_budget(&excerpt, max_len, "...\n")
}

/// Whether the environment variable `var_name` is set to an affirmative flag.
///
/// Accepts `1`, `true`, `yes`, `on`, or `debug` (case-insensitive, surrounding
/// whitespace ignored). This is the canonical env-flag parser; do not fork the
/// accepted-value set per crate.
///
/// ```
/// # use vtcode_commons::utils::env_flag_enabled;
/// // Unset variables are disabled.
/// assert!(!env_flag_enabled("VTCODE_ENV_FLAG_DOCTEST_DEFINITELY_UNSET"));
/// ```
#[must_use]
pub fn env_flag_enabled(var_name: &str) -> bool {
    std::env::var(var_name).ok().is_some_and(|value| {
        matches!(value.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on" | "debug")
    })
}

/// Parse a boolean env-var value, returning `None` when unrecognized.
///
/// Accepted truthy values: `1`, `true`, `yes`, `on`. Accepted falsy values:
/// `0`, `false`, `no`, `off`. Matching is case-insensitive after trimming
/// whitespace. This is the canonical tri-state parser; do not fork the
/// accepted-value set per crate.
#[must_use]
pub fn parse_bool_env_value(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

/// Read the environment variable `name` as a boolean, returning `default`
/// when it is unset or unrecognized.
///
/// ```
/// # use vtcode_commons::utils::parse_env_bool;
/// // Unset variables fall back to the caller-supplied default.
/// assert!(!parse_env_bool("VTCODE_ENV_BOOL_DOCTEST_DEFINITELY_UNSET", false));
/// assert!(parse_env_bool("VTCODE_ENV_BOOL_DOCTEST_DEFINITELY_UNSET", true));
/// ```
#[must_use]
pub fn parse_env_bool(name: &str, default: bool) -> bool {
    std::env::var(name)
        .ok()
        .and_then(|value| parse_bool_env_value(&value))
        .unwrap_or(default)
}

/// Safe text replacement with validation
pub fn safe_replace_text(content: &str, old_str: &str, new_str: &str) -> Result<String> {
    if old_str.is_empty() {
        return Err(anyhow::anyhow!("old_string cannot be empty"));
    }

    if !content.contains(old_str) {
        return Err(anyhow::anyhow!("Text '{old_str}' not found in content"));
    }

    Ok(content.replace(old_str, new_str))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readme_excerpt_does_not_split_utf8() {
        let markdown = "你".repeat(700);

        assert_eq!(extract_readme_excerpt(&markdown, 1201), format!("{}...\n", "你".repeat(400)));
    }

    #[test]
    fn parse_bool_env_value_accepts_canonical_sets() {
        for truthy in ["1", "true", "YES", " On ", "on"] {
            assert_eq!(parse_bool_env_value(truthy), Some(true), "{truthy}");
        }
        for falsy in ["0", "false", "NO", " Off ", "off"] {
            assert_eq!(parse_bool_env_value(falsy), Some(false), "{falsy}");
        }
        for unrecognized in ["", "debug", "2", "maybe"] {
            assert_eq!(parse_bool_env_value(unrecognized), None, "{unrecognized}");
        }
    }

    #[test]
    fn parse_env_bool_falls_back_to_default_when_unset_or_unrecognized() {
        assert!(!parse_env_bool("VTCODE_ENV_BOOL_TEST_DEFINITELY_UNSET", false));
        assert!(parse_env_bool("VTCODE_ENV_BOOL_TEST_DEFINITELY_UNSET", true));

        let guard = crate::env_lock::lock();
        guard.set_var("VTCODE_ENV_BOOL_TEST_VALUE", "yes");
        assert!(parse_env_bool("VTCODE_ENV_BOOL_TEST_VALUE", false));
        guard.set_var("VTCODE_ENV_BOOL_TEST_VALUE", "bogus");
        assert!(!parse_env_bool("VTCODE_ENV_BOOL_TEST_VALUE", false));
    }
}
