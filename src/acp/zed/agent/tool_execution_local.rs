use super::ZedAgent;
use crate::acp::reports::{
    TOOL_RESPONSE_KEY_CONTENT, TOOL_RESPONSE_KEY_PATH, TOOL_RESPONSE_KEY_STATUS,
    TOOL_RESPONSE_KEY_TOOL, TOOL_RESPONSE_KEY_TRUNCATED, TOOL_SUCCESS_LABEL, ToolExecutionReport,
    create_diff_content,
};
use crate::acp::tooling::{
    TOOL_LIST_FILES_ITEMS_KEY, TOOL_LIST_FILES_MESSAGE_KEY, TOOL_LIST_FILES_PATH_ARG,
    TOOL_LIST_FILES_RESULT_KEY, TOOL_LIST_FILES_SUMMARY_MAX_ITEMS, TOOL_LIST_FILES_URI_ARG,
    TOOL_READ_FILE_LIMIT_ARG, TOOL_READ_FILE_LINE_ARG,
};
use agent_client_protocol::{self as acp, AgentSideConnection, Client};
use anyhow::Result;
use path_clean::PathClean;
use serde_json::{Value, json};
use std::path::PathBuf;
use tracing::warn;
use vtcode_core::config::constants::tools;
use vtcode_core::tools::traits::Tool;
use vtcode_core::utils::ansi_parser::strip_ansi;

impl ZedAgent {
    pub(super) async fn execute_local_tool(
        &self,
        tool_name: &str,
        args: &Value,
    ) -> ToolExecutionReport {
        // SECURITY FIX: Block sensitive tools from external ACP clients
        // These are internal diagnostic and code execution tools that should not be exposed
        let restricted_tools = [
            "debug_agent",   // Internal diagnostic tool
            "analyze_agent", // Internal diagnostic tool
            "execute_code",  // Code execution tool - dangerous for external clients
        ];

        if restricted_tools.contains(&tool_name) {
            warn!(
                tool = tool_name,
                "Attempted execution of restricted tool from external ACP client"
            );
            return ToolExecutionReport::failure(
                tool_name,
                &format!("Tool '{}' is not available to external clients", tool_name),
            );
        }

        let result = {
            let registry = self.local_tool_registry.lock().await;
            registry.execute_tool_ref(tool_name, args).await
        };
        match result {
            Ok(output) => {
                if let Some(error_value) = output.get("error") {
                    let message = error_value
                        .get("message")
                        .and_then(Value::as_str)
                        .unwrap_or("Tool execution failed");
                    return ToolExecutionReport::failure(tool_name, message);
                }

                let content = self.render_local_tool_content(tool_name, &output);
                let payload = json!({
                    TOOL_RESPONSE_KEY_STATUS: TOOL_SUCCESS_LABEL,
                    TOOL_RESPONSE_KEY_TOOL: tool_name,
                    "result": output.clone(),
                });
                ToolExecutionReport::success(content, Vec::with_capacity(0), payload) // Use with_capacity(0)
            }
            Err(error) => {
                warn!(%error, tool = tool_name, "Failed to execute local tool");
                let message = format!("Unable to execute {tool_name}: {error}");
                ToolExecutionReport::failure(tool_name, &message)
            }
        }
    }

    fn render_local_tool_content(
        &self,
        tool_name: &str,
        output: &Value,
    ) -> Vec<acp::ToolCallContent> {
        if tool_name == tools::EDIT_FILE
            || tool_name == tools::WRITE_FILE
            || tool_name == tools::CREATE_FILE
        {
            if let (Some(path), Some(old_text), Some(new_text)) = (
                output.get("path").and_then(Value::as_str),
                output.get("old_text").and_then(Value::as_str),
                output.get("new_text").and_then(Value::as_str),
            ) {
                return vec![create_diff_content(path, Some(old_text), new_text)];
            }
            if let (Some(path), Some(new_text)) = (
                output.get("path").and_then(Value::as_str),
                output.get("new_text").and_then(Value::as_str),
            ) {
                return vec![create_diff_content(path, None, new_text)];
            }
        }

        let mut sections = Vec::with_capacity(10);

        if let Some(stdout) = output
            .get("stdout")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
        {
            let plain = strip_ansi(stdout);
            let (rendered, truncated) = self.truncate_text(&plain);
            sections.push(format!("stdout:\n{rendered}"));
            if truncated {
                sections.push("[stdout truncated]".to_string());
            }
        }

        if let Some(stderr) = output
            .get("stderr")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
        {
            let plain = strip_ansi(stderr);
            let (rendered, truncated) = self.truncate_text(&plain);
            sections.push(format!("stderr:\n{rendered}"));
            if truncated {
                sections.push("[stderr truncated]".to_string());
            }
        }

        if sections.is_empty() {
            if let Some(message) = output
                .get("message")
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
            {
                let plain = strip_ansi(message);
                let (rendered, truncated) = self.truncate_text(&plain);
                sections.push(rendered);
                if truncated {
                    sections.push("[message truncated]".to_string());
                }
            } else {
                let summary =
                    serde_json::to_string_pretty(output).unwrap_or_else(|_| output.to_string());
                let plain = strip_ansi(&summary);
                let (rendered, truncated) = self.truncate_text(&plain);
                sections.push(rendered);
                if truncated {
                    sections.push("[response truncated]".to_string());
                }
            }
        }

        if sections.is_empty() {
            sections.push(format!("{tool_name} completed without output"));
        }

        vec![acp::ToolCallContent::from(sections.join("\n"))]
    }

    pub(super) async fn run_read_file(
        &self,
        client: &AgentSideConnection,
        session_id: &acp::SessionId,
        args: &Value,
    ) -> Result<ToolExecutionReport, String> {
        let path = self.parse_tool_path(args)?;
        let line = Self::parse_positive_argument(args, TOOL_READ_FILE_LINE_ARG)?;
        let limit = Self::parse_positive_argument(args, TOOL_READ_FILE_LIMIT_ARG)?;

        let request = acp::ReadTextFileRequest::new(session_id.clone(), path.clone())
            .line(line)
            .limit(limit);

        let response = client.read_text_file(request).await.map_err(|error| {
            warn!(%error, path = ?path, "Failed to read file via ACP client");
            format!("Unable to read file: {error}")
        })?;

        let plain_response = strip_ansi(&response.content);
        let (truncated_content, truncated) = self.truncate_text(&plain_response);
        let mut tool_content = truncated_content.clone();
        if truncated {
            tool_content.push_str("\n\n[truncated]");
        }

        let payload = json!({
            TOOL_RESPONSE_KEY_STATUS: TOOL_SUCCESS_LABEL,
            TOOL_RESPONSE_KEY_TOOL: tools::READ_FILE,
            TOOL_RESPONSE_KEY_PATH: path.to_string_lossy(),
            TOOL_RESPONSE_KEY_CONTENT: truncated_content,
            TOOL_RESPONSE_KEY_TRUNCATED: truncated,
        });

        let locations = vec![acp::ToolCallLocation::new(path.clone()).line(line)];

        Ok(ToolExecutionReport::success(
            vec![acp::ToolCallContent::from(tool_content)],
            locations,
            payload,
        ))
    }

    pub(crate) async fn run_list_files(&self, args: &Value) -> Result<ToolExecutionReport, String> {
        let Some(tool) = &self.file_ops_tool else {
            return Err("List files tool is unavailable".to_string());
        };

        let resolved_path = self
            .resolve_list_files_path(args)?
            .unwrap_or_else(|| ".".into());

        let mut normalized_args = match args.clone() {
            Value::Object(map) => map,
            _ => serde_json::Map::new(),
        };
        normalized_args.insert(
            TOOL_LIST_FILES_PATH_ARG.to_string(),
            Value::String(resolved_path),
        );
        let normalized_args = Value::Object(normalized_args);

        let listing = tool.execute(normalized_args).await.map_err(|error| {
            let detail = error.to_string();
            warn!(error = %detail, "Failed to execute list_files tool");
            format!("Unable to list files: {detail}")
        })?;

        let content = Self::list_files_content(&listing);
        let locations = Self::list_files_locations(&listing);
        let payload = json!({
            TOOL_RESPONSE_KEY_STATUS: TOOL_SUCCESS_LABEL,
            TOOL_RESPONSE_KEY_TOOL: tools::LIST_FILES,
            TOOL_LIST_FILES_RESULT_KEY: listing,
        });

        Ok(ToolExecutionReport::success(content, locations, payload))
    }

    fn resolve_list_files_path(&self, args: &Value) -> Result<Option<String>, String> {
        if let Some(path) = args
            .get(TOOL_LIST_FILES_PATH_ARG)
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
        {
            return Ok(Some(path.to_string()));
        }

        if let Some(uri) = args
            .get(TOOL_LIST_FILES_URI_ARG)
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
        {
            let resolved = self.parse_resource_path(uri)?;
            let workspace_root = self.workspace_root().to_path_buf().clean();
            let normalized = resolved.clean();

            if normalized == workspace_root {
                return Ok(Some(".".into()));
            }

            if let Ok(relative) = normalized.strip_prefix(&workspace_root) {
                if relative.as_os_str().is_empty() {
                    return Ok(Some(".".into()));
                }
                return Ok(Some(relative.to_string_lossy().into()));
            }

            return Ok(Some(normalized.to_string_lossy().into()));
        }

        Ok(None)
    }

    fn list_files_content(output: &Value) -> Vec<acp::ToolCallContent> {
        let mut lines = Vec::with_capacity(100); // Pre-allocate for typical line count

        if let (Some(count), Some(total)) = (
            output.get("count").and_then(Value::as_u64),
            output.get("total").and_then(Value::as_u64),
        ) {
            lines.push(format!("Showing {} of {} items", count, total));
        }

        if let Some(items) = output
            .get(TOOL_LIST_FILES_ITEMS_KEY)
            .and_then(Value::as_array)
        {
            if items.is_empty() {
                lines.push("No items found.".to_string());
            } else {
                for item in items.iter().take(TOOL_LIST_FILES_SUMMARY_MAX_ITEMS) {
                    let path = item
                        .get("path")
                        .and_then(Value::as_str)
                        .or_else(|| item.get("name").and_then(Value::as_str))
                        .unwrap_or("(unknown)");
                    let item_type = item.get("type").and_then(Value::as_str).unwrap_or("file");
                    let prefix = match item_type {
                        "directory" => "[dir]",
                        "file" => "[file]",
                        other => other,
                    };
                    lines.push(format!("{prefix} {path}"));
                }

                if items.len() > TOOL_LIST_FILES_SUMMARY_MAX_ITEMS {
                    let remaining = items.len() - TOOL_LIST_FILES_SUMMARY_MAX_ITEMS;
                    lines.push(format!("… and {remaining} more"));
                }
            }
        } else {
            lines.push("No results returned.".to_string());
        }

        if let Some(has_more) = output.get("has_more").and_then(Value::as_bool)
            && has_more
        {
            lines.push(
                "Additional results available (adjust page or per_page to view more).".to_string(),
            );
        }

        if let Some(message) = output
            .get(TOOL_LIST_FILES_MESSAGE_KEY)
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
        {
            lines.push(message.to_string());
        }

        if lines.is_empty() {
            lines.push("No results.".to_string());
        }

        vec![acp::ToolCallContent::from(lines.join(" "))]
    }

    fn list_files_locations(output: &Value) -> Vec<acp::ToolCallLocation> {
        let Some(items) = output
            .get(TOOL_LIST_FILES_ITEMS_KEY)
            .and_then(Value::as_array)
        else {
            return Vec::with_capacity(0); // Use with_capacity(0) instead of Vec::new()
        };

        items
            .iter()
            .filter_map(|item| {
                item.get("path")
                    .or_else(|| item.get("name"))
                    .and_then(Value::as_str)
                    .map(PathBuf::from)
            })
            .take(TOOL_LIST_FILES_SUMMARY_MAX_ITEMS)
            .map(acp::ToolCallLocation::new)
            .collect()
    }
}
