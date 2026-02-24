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
pub fn current_timestamp_result() -> Result<u64> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("System clock is before UNIX_EPOCH while generating timestamp")?
        .as_secs())
}

/// Calculate SHA256 hash of the given content
pub fn calculate_sha256(content: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content);
    format!("{:x}", hasher.finalize())
}

/// Extract a string value from a simple TOML key assignment within [package]
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
    let mut excerpt = String::new();
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
    if excerpt.len() > max_len {
        excerpt.truncate(max_len);
        excerpt.push_str("...\n");
    }
    excerpt
}

/// Safe text replacement with validation
pub fn safe_replace_text(content: &str, old_str: &str, new_str: &str) -> Result<String> {
    if old_str.is_empty() {
        return Err(anyhow::anyhow!("old_string cannot be empty"));
    }

    if !content.contains(old_str) {
        return Err(anyhow::anyhow!("Text '{}' not found in content", old_str));
    }

    Ok(content.replace(old_str, new_str))
}
