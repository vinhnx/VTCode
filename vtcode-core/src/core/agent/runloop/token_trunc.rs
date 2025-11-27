//! Shared token-based truncation utilities for model input aggregation
use crate::core::token_budget::TokenBudgetManager;
use crate::core::token_constants::{
    CODE_DETECTION_THRESHOLD, CODE_HEAD_RATIO_PERCENT, CODE_INDICATOR_CHARS,
    LOG_HEAD_RATIO_PERCENT, TOKENS_PER_CHARACTER,
};
use serde_json::Value;
use std::fmt::Write;

/// Truncate content by tokens using a head+tail strategy with code-aware ratios.
pub async fn truncate_content_by_tokens(
    content: &str,
    max_tokens: usize,
    token_budget: &TokenBudgetManager,
) -> (String, bool) {
    let total_tokens = match token_budget.count_tokens(content).await {
        Ok(n) => n,
        Err(_) => (content.len() as f64 / TOKENS_PER_CHARACTER).ceil() as usize,
    };
    if total_tokens <= max_tokens {
        return (content.to_string(), false);
    }

    let char_count = content.len();
    let bracket_count: usize = content
        .chars()
        .filter(|c| CODE_INDICATOR_CHARS.contains(*c))
        .count();
    let is_code = bracket_count > (char_count / CODE_DETECTION_THRESHOLD);
    let head_ratio = if is_code {
        CODE_HEAD_RATIO_PERCENT
    } else {
        LOG_HEAD_RATIO_PERCENT
    };

    let head_tokens = (max_tokens * head_ratio) / 100;
    let tail_tokens = max_tokens - head_tokens;

    let lines: Vec<&str> = content.lines().collect();

    // Head
    let mut head_lines = Vec::new();
    let mut acc = 0usize;
    for line in &lines {
        if acc >= head_tokens {
            break;
        }
        let line_tokens = (line.len() as f64 / TOKENS_PER_CHARACTER).ceil() as usize;
        if acc + line_tokens <= head_tokens || head_lines.is_empty() {
            head_lines.push(*line);
            acc += line_tokens;
        } else {
            break;
        }
    }

    // Tail
    let mut tail_lines = Vec::new();
    let mut acc_tail = 0usize;
    let mut tail_start_idx = lines.len();
    for line in lines.iter().rev() {
        if acc_tail >= tail_tokens {
            break;
        }
        let line_tokens = (line.len() as f64 / TOKENS_PER_CHARACTER).ceil() as usize;
        if acc_tail + line_tokens <= tail_tokens || tail_lines.is_empty() {
            tail_lines.push(*line);
            acc_tail += line_tokens;
            tail_start_idx -= 1;
        } else {
            break;
        }
    }
    tail_lines.reverse();

    let head_content = if head_lines.is_empty() {
        String::new()
    } else {
        head_lines.join("\n")
    };
    let tail_content = if tail_lines.is_empty() {
        String::new()
    } else {
        tail_lines.join("\n")
    };

    if tail_start_idx > head_lines.len() {
        let truncated_lines = tail_start_idx.saturating_sub(head_lines.len());
        let mut out = String::with_capacity(head_content.len() + tail_content.len() + 64);
        if !head_content.is_empty() {
            out.push_str(head_content.trim_end());
            out.push('\n');
        }
        let _ = writeln!(out, "[... {} lines truncated ...]", truncated_lines);
        out.push_str(&tail_content);
        (out.trim_end().to_string(), true)
    } else {
        (head_content.trim_end().to_string(), true)
    }
}

/// Byte fuse truncation with a clear marker, preserving UTF-8 boundaries.
pub fn safe_truncate_to_bytes_with_marker(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    let marker = "\n[... content truncated by byte fuse ...]";
    let budget = max_bytes.saturating_sub(marker.len());
    let cutoff = s
        .char_indices()
        .take_while(|(idx, _)| *idx < budget)
        .map(|(idx, _)| idx)
        .last()
        .unwrap_or(budget);
    let mut out = String::with_capacity(cutoff + marker.len());
    out.push_str(&s[..cutoff]);
    out.push_str(marker);
    out
}

/// Aggregate relevant textual fields from a tool output JSON in a stable order
/// and apply token-based truncation plus byte fuse. Returns a model-friendly string.
pub async fn aggregate_tool_output_for_model(
    tool_name: &str,
    output: &Value,
    max_tokens: usize,
    byte_fuse: usize,
    token_budget: &TokenBudgetManager,
) -> String {
    // Collect likely text fields in preferred order (max 5 standard fields)
    let mut parts: Vec<(String, String)> = Vec::with_capacity(5);

    // Common fields
    if let Some(s) = output.get("output").and_then(Value::as_str) {
        parts.push(("output".to_owned(), s.to_owned()));
    }
    if let Some(s) = output.get("stdout").and_then(Value::as_str) {
        parts.push(("stdout".to_owned(), s.to_owned()));
    }
    if let Some(s) = output.get("stderr").and_then(Value::as_str) {
        parts.push(("stderr".to_owned(), s.to_owned()));
    }
    if let Some(s) = output.get("content").and_then(Value::as_str) {
        parts.push(("content".to_owned(), s.to_owned()));
    }
    if let Some(s) = output.get("message").and_then(Value::as_str) {
        parts.push(("message".to_owned(), s.to_owned()));
    }

    // Fallback: if nothing obvious, serialize concisely
    if parts.is_empty() {
        let compact = if output.is_object() || output.is_array() {
            serde_json::to_string(output).unwrap_or_else(|_| "{}".to_owned())
        } else {
            output.to_string()
        };
        let (mut text, _tr) = truncate_content_by_tokens(&compact, max_tokens, token_budget).await;
        if text.len() > byte_fuse {
            text = safe_truncate_to_bytes_with_marker(&text, byte_fuse);
        }
        return text;
    }

    // Build a readable aggregate with section headers to preserve provenance/order
    let mut aggregate = String::with_capacity(parts.iter().map(|(_, s)| s.len() + 16).sum());
    let _ = writeln!(aggregate, "[tool: {}]", tool_name);
    for (label, s) in parts.iter() {
        let _ = writeln!(aggregate, "--- {} ---", label);
        aggregate.push_str(s);
        if !aggregate.ends_with('\n') {
            aggregate.push('\n');
        }
    }

    let (mut text, _tr) = truncate_content_by_tokens(&aggregate, max_tokens, token_budget).await;
    if text.len() > byte_fuse {
        text = safe_truncate_to_bytes_with_marker(&text, byte_fuse);
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::token_budget::TokenBudgetManager;

    #[tokio::test]
    async fn test_truncate_content_by_tokens_preserves_head_tail() {
        let mgr = TokenBudgetManager::default();
        // Create a content block where important data is at head and tail
        let head = (1..=10)
            .map(|i| format!("head-{}", i))
            .collect::<Vec<_>>()
            .join("\n");
        let tail = (1..=10)
            .map(|i| format!("tail-{}", i))
            .collect::<Vec<_>>()
            .join("\n");
        let middle = (1..=500)
            .map(|i| format!("middle-{}", i))
            .collect::<Vec<_>>()
            .join("\n");
        let content = format!("{}\n{}\n{}", head, middle, tail);
        let (res, truncated) = truncate_content_by_tokens(&content, 200, &mgr).await;
        assert!(truncated);
        assert!(res.contains("head-1"));
        assert!(res.contains("tail-1"));
    }
    #[tokio::test]
    async fn test_safe_truncate_to_bytes_with_marker_marker_present() {
        let marker = "\n[... content truncated by byte fuse ...]";
        let s = "a".repeat(5000);
        let out = safe_truncate_to_bytes_with_marker(&s, 100);
        assert!(out.ends_with(marker));
        assert!(out.len() <= 100 + marker.len());
    }

    #[tokio::test]
    async fn test_aggregate_tool_output_honors_max_tokens_and_preserves_tail() {
        let mgr = TokenBudgetManager::default();
        let mut output = serde_json::json!({});
        // Build a large stdout-like content with start/middle/end
        let mut buf = String::new();
        buf.push_str("START_LINE\n");
        for i in 0..100 {
            buf.push_str(&format!("MIDDLE {}\n", i));
        }
        buf.push_str("END_LINE\n");
        output["stdout"] = serde_json::json!(buf);

        // Small max_tokens to force truncation
        let result = aggregate_tool_output_for_model("test_tool", &output, 50, 200, &mgr).await;
        assert!(result.contains("START_LINE"));
        assert!(result.contains("END_LINE"));
        // Should be truncated (contain our truncation marker)
        assert!(result.contains("[..."));
    }
}
