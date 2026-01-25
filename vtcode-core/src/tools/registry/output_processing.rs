//! Tool output processing helpers for ToolRegistry.

use serde_json::{Value, json};

use super::ToolRegistry;

impl ToolRegistry {
    fn sanitize_tool_output(value: Value, is_mcp: bool) -> Value {
        let (entry_fuse, depth_fuse, token_fuse, byte_fuse) = Self::fuse_limits();

        let trimmed = Self::clamp_value_recursive(&value, entry_fuse, depth_fuse);

        let serialized = trimmed.to_string();
        let approx_tokens = serialized.len() / 4;
        if serialized.len() > byte_fuse || approx_tokens > token_fuse {
            let truncated = serialized.chars().take(byte_fuse).collect::<String>();
            return json!({
                "content": truncated,
                "truncated": true,
                "note": if is_mcp {
                    "MCP tool result truncated to protect context budget"
                } else {
                    "Tool result truncated to protect context budget"
                },
                "approx_tokens": approx_tokens,
                "byte_fuse": byte_fuse
            });
        }
        trimmed
    }

    fn clamp_value_recursive(value: &Value, entry_fuse: usize, depth: usize) -> Value {
        if depth == 0 {
            return value.clone();
        }
        match value {
            Value::Array(arr) => {
                if arr.is_empty() {
                    return Value::Array(Vec::new());
                }
                let overflow = arr.len().saturating_sub(entry_fuse);
                let trimmed: Vec<Value> = arr
                    .iter()
                    .take(entry_fuse)
                    .map(|v| Self::clamp_value_recursive(v, entry_fuse, depth - 1))
                    .collect();
                if overflow > 0 {
                    let approx_tokens = trimmed
                        .iter()
                        .map(|v| v.to_string().len() / 4)
                        .sum::<usize>();
                    json!({
                        "truncated": true,
                        "note": "Array truncated to protect context budget",
                        "total_entries": arr.len(),
                        "entries": trimmed,
                        "overflow": overflow,
                        "approx_tokens": approx_tokens
                    })
                } else {
                    Value::Array(trimmed)
                }
            }
            Value::Object(map) => {
                if map.is_empty() {
                    return Value::Object(serde_json::Map::new());
                }
                let overflow = map.len().saturating_sub(entry_fuse);
                let mut head = serde_json::Map::new();
                for (k, v) in map.iter().take(entry_fuse) {
                    head.insert(
                        k.clone(),
                        Self::clamp_value_recursive(v, entry_fuse, depth - 1),
                    );
                }
                if overflow > 0 {
                    let approx_tokens = head
                        .iter()
                        .map(|(k, v)| (k.len() + v.to_string().len()) / 4)
                        .sum::<usize>();
                    json!({
                        "truncated": true,
                        "note": "Object truncated to protect context budget",
                        "total_entries": map.len(),
                        "entries": head,
                        "overflow": overflow,
                        "approx_tokens": approx_tokens
                    })
                } else {
                    Value::Object(head)
                }
            }
            _ => value.clone(),
        }
    }

    fn fuse_limits() -> (usize, usize, usize, usize) {
        let entry_fuse = std::env::var("VTCODE_FUSE_ENTRY")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .filter(|v| *v >= 10)
            .unwrap_or(200);
        let depth_fuse = std::env::var("VTCODE_FUSE_DEPTH")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .filter(|v| *v >= 1)
            .unwrap_or(3);
        let token_fuse = std::env::var("VTCODE_FUSE_TOKEN")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .filter(|v| *v >= 1_000)
            .unwrap_or(50_000);
        let byte_fuse = std::env::var("VTCODE_FUSE_BYTES")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .filter(|v| *v >= 10_000)
            .unwrap_or(200_000);
        (entry_fuse, depth_fuse, token_fuse, byte_fuse)
    }

    /// Process tool output with dynamic context discovery.
    ///
    /// This method implements Cursor-style dynamic context discovery:
    /// 1. First checks if output should be spooled to a file (large outputs)
    /// 2. If spooled, returns a file reference instead of truncated content
    /// 3. Otherwise, applies standard sanitization
    ///
    /// This is more token-efficient as agents can use read_file/grep_file
    /// to explore large outputs on demand.
    pub(super) async fn process_tool_output(
        &self,
        tool_name: &str,
        value: Value,
        is_mcp: bool,
    ) -> Value {
        // Check if output should be spooled to file
        if self.output_spooler.should_spool(&value) {
            match self
                .output_spooler
                .process_output(tool_name, value.clone(), is_mcp)
                .await
            {
                Ok(spooled) => return spooled,
                Err(e) => {
                    // Log error but fall back to standard sanitization
                    tracing::warn!(
                        tool = tool_name,
                        error = %e,
                        "Failed to spool tool output to file, falling back to truncation"
                    );
                }
            }
        }

        Self::sanitize_tool_output(value, is_mcp)
    }
}
