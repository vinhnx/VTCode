use crate::config::constants::tools;
use crate::tools::tool_intent;
use serde_json::Value;

/// Format tool result for display in the TUI.
/// Limits verbose output from web_fetch to avoid overwhelming the terminal.
#[inline]
pub fn format_tool_result_for_display(tool_name: &str, result: &Value) -> String {
    let display_tool_name =
        tool_intent::canonical_command_session_tool_name(tool_name).unwrap_or(tool_name);
    let is_command_session_tool = display_tool_name == tools::UNIFIED_EXEC;

    if is_command_session_tool {
        // Extract errors + 2 context lines for build output
        if let Some(obj) = result.as_object()
            && let Some(stdout) =
                obj.get("stdout").or_else(|| obj.get("output")).and_then(|v| v.as_str())
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
                return format!("Tool {display_tool_name} result: {compact}");
            }
        }
        return format!("Tool {display_tool_name} result: {result}");
    }

    format!("Tool {display_tool_name} result: {result}")
}
