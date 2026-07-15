use super::ZedAgent;
use crate::reports::TOOL_FAILURE_PREFIX;
use crate::tooling::{SupportedTool, TOOL_READ_FILE_PATH_ARG, TOOL_READ_FILE_URI_ARG};
use anyhow::Result;
use serde_json::Value;
use std::path::{Path, PathBuf};
use vtcode_core::config::tool_loop_limit_reached;
use vtcode_core::llm::provider::ToolChoice;
use vtcode_core::llm::provider::ToolDefinition;
use vtcode_core::tools::command_args;
use vtcode_core::utils::path::ensure_path_within_workspace;

use super::super::constants::*;
use super::super::types::{RunTerminalMode, ToolRuntime};

impl ZedAgent {
    pub(super) fn local_tools_available(&self, primary_agent: &str) -> bool {
        self.acp_tool_registry
            .definitions_for(&[], true)
            .iter()
            .any(|definition| {
                self.primary_agents
                    .allows_local_tool(primary_agent, definition.function_name())
            })
    }

    pub(super) fn tool_definitions(
        &self,
        provider_supports_tools: bool,
        enabled_tools: &[SupportedTool],
        primary_agent: &str,
    ) -> Option<Vec<ToolDefinition>> {
        if !provider_supports_tools {
            return None;
        }

        let include_local = self.local_tools_available(primary_agent);
        if enabled_tools.is_empty() && !include_local {
            None
        } else {
            let mut definitions = self.acp_tool_registry.definitions_for(enabled_tools, false);
            if include_local {
                definitions.extend(
                    self.acp_tool_registry
                        .definitions_for(&[], true)
                        .into_iter()
                        .filter(|definition| {
                            self.primary_agents
                                .allows_local_tool(primary_agent, definition.function_name())
                        }),
                );
            }
            Some(definitions)
        }
    }

    pub(super) fn tool_choice(&self, tools_available: bool) -> Option<ToolChoice> {
        if tools_available {
            Some(ToolChoice::auto())
        } else {
            Some(ToolChoice::none())
        }
    }

    pub(super) fn tool_loop_limit_reached(&self, completed_tool_loops: usize) -> bool {
        tool_loop_limit_reached(completed_tool_loops, self.tool_loop_limit)
    }

    pub(super) fn tool_loop_limit_message(&self) -> String {
        format!(
            "Reached maximum tool loops ({}); stopping this turn to avoid an unsafe autonomous loop.",
            self.tool_loop_limit
        )
    }

    pub(super) fn client_supports_read_text_file(&self) -> bool {
        self.client_capabilities
            .lock()
            .ok()
            .and_then(|guard| {
                guard
                    .as_ref()
                    .map(|capabilities| capabilities.fs.read_text_file)
            })
            .unwrap_or(false)
    }

    pub(super) fn client_supports_terminal(&self) -> bool {
        self.client_capabilities
            .lock()
            .ok()
            .and_then(|guard| guard.as_ref().map(|capabilities| capabilities.terminal))
            .unwrap_or(false)
    }

    pub(super) fn tool_availability(
        &self,
        provider_supports_tools: bool,
        client_supports_read_text_file: bool,
    ) -> Vec<(SupportedTool, ToolRuntime)> {
        self.acp_tool_registry
            .registered_tools()
            .into_iter()
            .map(|tool| {
                let runtime = if provider_supports_tools
                    && (tool != SupportedTool::ReadFile || client_supports_read_text_file)
                {
                    ToolRuntime::Enabled
                } else {
                    ToolRuntime::Disabled
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
            .map_err(|_err| "working_dir must stay within the workspace".to_string())?;

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
            .map_err(|_err| {
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
