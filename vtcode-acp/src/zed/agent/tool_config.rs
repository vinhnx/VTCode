use super::ZedAgent;
use crate::reports::TOOL_FAILURE_PREFIX;
use crate::tooling::{SupportedTool, TOOL_READ_FILE_PATH_ARG, TOOL_READ_FILE_URI_ARG};
use agent_client_protocol as acp;
use anyhow::Result;
use serde_json::Value;
use std::path::{Path, PathBuf};
use tracing::warn;
use vtcode_core::core::interfaces::SessionMode;
use vtcode_core::llm::provider::ToolChoice;
use vtcode_core::llm::provider::ToolDefinition;
use vtcode_core::tools::command_args;
use vtcode_core::utils::path::ensure_path_within_workspace;

use super::super::constants::*;
use super::super::helpers::{session_mode_allows_local_tools, text_chunk};
use super::super::types::{RunTerminalMode, ToolDisableReason, ToolRuntime};

impl ZedAgent {
    pub(super) fn local_tools_available(&self, mode: SessionMode) -> bool {
        session_mode_allows_local_tools(mode) && self.acp_tool_registry.has_local_tools()
    }

    pub(super) fn tool_definitions(
        &self,
        provider_supports_tools: bool,
        enabled_tools: &[SupportedTool],
        mode: SessionMode,
    ) -> Option<Vec<ToolDefinition>> {
        if !provider_supports_tools {
            return None;
        }

        let include_local = self.local_tools_available(mode);
        if enabled_tools.is_empty() && !include_local {
            None
        } else {
            Some(
                self.acp_tool_registry
                    .definitions_for(enabled_tools, include_local),
            )
        }
    }

    pub(super) fn tool_choice(&self, tools_available: bool) -> Option<ToolChoice> {
        if tools_available {
            Some(ToolChoice::auto())
        } else {
            Some(ToolChoice::none())
        }
    }

    pub(super) fn client_supports_read_text_file(&self) -> bool {
        self.client_capabilities
            .borrow()
            .as_ref()
            .map(|capabilities| capabilities.fs.read_text_file)
            .unwrap_or(false)
    }

    pub(super) fn client_supports_terminal(&self) -> bool {
        self.client_capabilities
            .borrow()
            .as_ref()
            .map(|capabilities| capabilities.terminal)
            .unwrap_or(false)
    }

    pub(super) fn tool_availability<'a>(
        &'a self,
        provider_supports_tools: bool,
        client_supports_read_text_file: bool,
    ) -> Vec<(SupportedTool, ToolRuntime<'a>)> {
        self.acp_tool_registry
            .registered_tools()
            .into_iter()
            .map(|tool| {
                let runtime = if !provider_supports_tools {
                    ToolRuntime::Disabled(ToolDisableReason::Provider {
                        provider: self.config.provider.as_str(),
                        model: self.config.model.as_str(),
                    })
                } else {
                    match tool {
                        SupportedTool::ReadFile => {
                            if client_supports_read_text_file {
                                ToolRuntime::Enabled
                            } else {
                                ToolRuntime::Disabled(ToolDisableReason::ClientCapabilities)
                            }
                        }
                        SupportedTool::ListFiles => ToolRuntime::Enabled,
                        SupportedTool::SwitchMode => ToolRuntime::Enabled,
                    }
                };
                (tool, runtime)
            })
            .collect()
    }

    pub(super) fn requested_terminal_mode(args: &Value) -> Result<RunTerminalMode, String> {
        if let Some(mode_value) = args.get("mode").and_then(Value::as_str) {
            let normalized = mode_value.trim().to_lowercase();
            match normalized.as_str() {
                "pty" => return Ok(RunTerminalMode::Pty),
                "terminal" | "" => return Ok(RunTerminalMode::Terminal),
                "streaming" => {
                    return Err("command sessions do not support streaming mode".to_string());
                }
                _ => {}
            }
        }

        if args.get("tty").and_then(Value::as_bool).unwrap_or(false) {
            return Ok(RunTerminalMode::Pty);
        }

        Ok(RunTerminalMode::Terminal)
    }

    pub(crate) fn parse_terminal_command(args: &Value) -> Result<Vec<String>, String> {
        fn validate_command_parts(parts: Vec<String>) -> Result<Vec<String>, String> {
            if parts.is_empty() {
                return Err("command array cannot be empty".to_string());
            }
            if parts[0].trim().is_empty() {
                return Err("command executable cannot be empty".to_string());
            }
            Ok(parts)
        }

        match command_args::normalized_command_value(args).map_err(str::to_string)? {
            Some(Value::String(command)) if command.trim().is_empty() => {
                return Err("command string cannot be empty".to_string());
            }
            Some(Value::Array(values)) if values.is_empty() => {
                return Err("command array cannot be empty".to_string());
            }
            _ => {}
        }

        let parts = command_args::command_words(args)
            .map_err(str::to_string)?
            .ok_or_else(|| {
                "command execution requires a 'command' field (string/array or indexed command.N entries)"
                    .to_string()
            })?;
        validate_command_parts(parts)
    }

    pub(super) fn resolve_terminal_working_dir(
        &self,
        args: &Value,
    ) -> Result<Option<PathBuf>, String> {
        let requested = command_args::working_dir_text(args);

        let Some(raw_dir) = requested else {
            return Ok(None);
        };

        let candidate = Path::new(raw_dir);
        let resolved = if candidate.is_absolute() {
            candidate.to_path_buf()
        } else {
            self.config.workspace.join(candidate)
        };

        let normalized = ensure_path_within_workspace(&resolved, &self.config.workspace)
            .map_err(|_| "working_dir must stay within the workspace".to_string())?;

        Ok(Some(normalized))
    }

    pub(super) fn describe_terminal_location(
        &self,
        working_dir: Option<&PathBuf>,
    ) -> Option<String> {
        let workspace = &self.config.workspace;
        working_dir.and_then(|path| {
            path.strip_prefix(workspace).ok().map(|relative| {
                if relative.as_os_str().is_empty() {
                    ".".to_string()
                } else {
                    format!("./{}", relative.to_string_lossy())
                }
            })
        })
    }

    pub(super) fn truncate_text(&self, input: &str) -> (String, bool) {
        if input.chars().count() <= MAX_TOOL_RESPONSE_CHARS {
            return (input.to_string(), false);
        }

        let truncated: String = input.chars().take(MAX_TOOL_RESPONSE_CHARS).collect();
        (truncated, true)
    }

    pub(super) fn argument_message(template: &str, argument: &str) -> String {
        template.replace("{argument}", argument)
    }

    pub(super) fn render_tool_disable_notice(
        &self,
        tool: SupportedTool,
        reason: &ToolDisableReason<'_>,
    ) -> String {
        let tool_name = tool.function_name();
        match reason {
            ToolDisableReason::Provider { provider, model } => TOOL_DISABLED_PROVIDER_NOTICE
                .replace("{tool}", tool_name)
                .replace("{model}", model)
                .replace("{provider}", provider),
            ToolDisableReason::ClientCapabilities => {
                TOOL_DISABLED_CAPABILITY_NOTICE.replace("{tool}", tool_name)
            }
        }
    }

    pub(super) fn log_tool_disable_reason(
        &self,
        tool: SupportedTool,
        reason: &ToolDisableReason<'_>,
    ) {
        match reason {
            ToolDisableReason::Provider { provider, model } => {
                warn!(
                    tool = tool.function_name(),
                    provider = %provider,
                    model = %model,
                    "{}",
                    TOOL_DISABLED_PROVIDER_LOG_MESSAGE
                );
            }
            ToolDisableReason::ClientCapabilities => {
                warn!(
                    tool = tool.function_name(),
                    "{}", TOOL_DISABLED_CAPABILITY_LOG_MESSAGE
                );
            }
        }
    }

    pub(super) async fn send_tool_disable_notices(
        &self,
        session_id: &acp::SessionId,
        reasons: &[(SupportedTool, ToolDisableReason<'_>)],
    ) -> Result<(), acp::Error> {
        if reasons.is_empty() {
            return Ok(());
        }

        let mut combined = String::new();
        for (index, (tool, reason)) in reasons.iter().enumerate() {
            let mut notice = self.render_tool_disable_notice(*tool, reason);
            if !notice.ends_with('.') {
                notice.push('.');
            }
            if index > 0 {
                combined.push(' ');
            }
            combined.push_str(&notice);
        }

        self.send_update(
            session_id,
            acp::SessionUpdate::AgentThoughtChunk(text_chunk(combined)),
        )
        .await
    }

    pub(super) fn workspace_root(&self) -> &Path {
        self.config.workspace.as_path()
    }

    pub(super) fn resolve_workspace_path(
        &self,
        candidate: PathBuf,
        argument: &str,
    ) -> Result<PathBuf, String> {
        let resolved_candidate = if candidate.is_absolute() {
            candidate
        } else {
            self.workspace_root().join(candidate)
        };
        let normalized = ensure_path_within_workspace(&resolved_candidate, self.workspace_root())
            .map_err(|_| {
            Self::argument_message(TOOL_READ_FILE_WORKSPACE_ESCAPE_TEMPLATE, argument)
        })?;

        if !normalized.is_absolute() {
            return Err(Self::argument_message(
                TOOL_READ_FILE_ABSOLUTE_PATH_TEMPLATE,
                argument,
            ));
        }

        Ok(normalized)
    }

    pub(super) fn parse_positive_argument(args: &Value, key: &str) -> Result<Option<u32>, String> {
        let Some(raw_value) = args.get(key) else {
            return Ok(None);
        };

        if raw_value.is_null() {
            return Ok(None);
        }

        let Some(value) = raw_value.as_u64() else {
            return Err(Self::argument_message(
                TOOL_READ_FILE_INVALID_INTEGER_TEMPLATE,
                key,
            ));
        };

        if value == 0 {
            return Err(Self::argument_message(
                TOOL_READ_FILE_INVALID_INTEGER_TEMPLATE,
                key,
            ));
        }

        if value > u32::MAX as u64 {
            return Err(Self::argument_message(
                TOOL_READ_FILE_INTEGER_RANGE_TEMPLATE,
                key,
            ));
        }

        Ok(Some(value as u32))
    }

    pub(super) fn parse_tool_path(&self, args: &Value) -> Result<PathBuf, String> {
        if let Some(path) = args
            .get(TOOL_READ_FILE_PATH_ARG)
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
        {
            let candidate = PathBuf::from(path);
            return self.resolve_workspace_path(candidate, TOOL_READ_FILE_PATH_ARG);
        }

        if let Some(uri) = args
            .get(TOOL_READ_FILE_URI_ARG)
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
        {
            return self.parse_resource_path(uri);
        }

        Err(format!(
            "{TOOL_FAILURE_PREFIX}: missing {TOOL_READ_FILE_PATH_ARG} or {TOOL_READ_FILE_URI_ARG}"
        ))
    }
}
