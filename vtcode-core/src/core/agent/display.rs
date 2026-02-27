use crate::config::constants::tools;
use serde_json::Value;

/// Format tool result for display in the TUI.
/// Limits verbose output from web_fetch to avoid overwhelming the terminal.
#[inline]
pub fn format_tool_result_for_display(tool_name: &str, result: &Value) -> String {
    match tool_name {
        tools::WEB_FETCH => {
            // For web_fetch, show minimal info instead of the full content
            if let Some(obj) = result.as_object() {
                if obj.contains_key("error") {
                    format!(
                        "Tool {} result: {{\"error\": {}}}",
                        tool_name,
                        obj.get("error")
                            .map(|v| v.to_string())
                            .unwrap_or_else(|| "unknown error".into())
                    )
                } else {
                    let status = serde_json::json!({
                        "status": "fetched",
                        "content_length": obj.get("content_length"),
                        "truncated": obj.get("truncated"),
                        "url": obj.get("url")
                    });
                    format!("Tool {} result: {}", tool_name, status)
                }
            } else {
                format!("Tool {} result: {}", tool_name, result)
            }
        }
        tools::GREP_FILE => {
            // Show max 5 matches, indicate overflow
            if let Some(obj) = result.as_object()
                && let Some(matches) = obj.get("matches").and_then(|v| v.as_array())
                && matches.len() > 5
            {
                let truncated: Vec<_> = matches.iter().take(5).cloned().collect();
                let overflow = matches.len() - 5;
                let summary = serde_json::json!({
                    "matches": truncated,
                    "overflow": format!("[+{} more matches]", overflow),
                    "total": matches.len()
                });
                return format!("Tool {} result: {}", tool_name, summary);
            }
            format!("Tool {} result: {}", tool_name, result)
        }
        tools::LIST_FILES => {
            // Summarize if 50+ items
            if let Some(obj) = result.as_object()
                && let Some(files) = obj.get("files").and_then(|v| v.as_array())
                && files.len() > 50
            {
                let sample: Vec<_> = files.iter().take(5).cloned().collect();
                let summary = serde_json::json!({
                    "total_files": files.len(),
                    "sample": sample,
                    "note": format!("Showing 5 of {} files", files.len())
                });
                return format!("Tool {} result: {}", tool_name, summary);
            }
            format!("Tool {} result: {}", tool_name, result)
        }
        tools::RUN_PTY_CMD | tools::UNIFIED_EXEC | "shell" => {
            // Extract errors + 2 context lines for build output
            if let Some(obj) = result.as_object()
                && let Some(stdout) = obj
                    .get("stdout")
                    .or_else(|| obj.get("output"))
                    .and_then(|v| v.as_str())
                && stdout.len() > 2000
                && (stdout.contains("error") || stdout.contains("Error"))
            {
                let lines: Vec<&str> = stdout.lines().collect();
                let mut extracted = Vec::new();
                for (i, line) in lines.iter().enumerate() {
                    if line.to_lowercase().contains("error") {
                        let start = i.saturating_sub(2);
                        let end = (i + 3).min(lines.len());
                        extracted.extend_from_slice(&lines[start..end]);
                        extracted.push("...");
                    }
                }
                if !extracted.is_empty() {
                    let compact = serde_json::json!({
                        "exit_code": obj.get("exit_code"),
                        "errors": extracted.join("\n"),
                        "note": "Showing error lines + context only"
                    });
                    return format!("Tool {} result: {}", tool_name, compact);
                }
            }
            format!("Tool {} result: {}", tool_name, result)
        }
        _ => format!("Tool {} result: {}", tool_name, result),
    }
}
