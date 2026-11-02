use super::ZedAgent;
use crate::reports::TOOL_FAILURE_PREFIX;
use crate::tooling::{SupportedTool, TOOL_READ_FILE_PATH_ARG, TOOL_READ_FILE_URI_ARG};
use anyhow::Result;
use serde_json::Value;
use std::path::{Path, PathBuf};
use vtcode_core::config::tool_loop_limit_reached;
use vtcode_core::llm::provider::ToolChoice;
use vtcode_core::llm::provider::ToolDefinition;
use vtcode_core::utils::path::ensure_path_within_workspace;

use super::super::constants::{
    MAX_TOOL_RESPONSE_CHARS, TOOL_READ_FILE_ABSOLUTE_PATH_TEMPLATE, TOOL_READ_FILE_INTEGER_RANGE_TEMPLATE,
    TOOL_READ_FILE_INVALID_INTEGER_TEMPLATE, TOOL_READ_FILE_WORKSPACE_ESCAPE_TEMPLATE,
};
use super::super::types::ToolRuntime;

impl ZedAgent {
    pub(super) fn local_tools_available(&self, primary_agent: &str) -> bool {
        self.acp_tool_registry
            .definitions_for(&[], true)
            .iter()
            .any(|definition| self.primary_agents.allows_local_tool(primary_agent, definition.function_name()))
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
            let workspace_root = self.config.workspace.clone();
            Some(
                self.acp_tool_registry
                    .definitions_for_filtered(enabled_tools, include_local, |tool_name| {
                        self.primary_agents
                            .allows_tool(primary_agent, tool_name, workspace_root.as_path())
                    }),
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
            .and_then(|guard| guard.as_ref().map(|capabilities| capabilities.fs.read_text_file))
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
                let runtime =
                    if provider_supports_tools && (tool != SupportedTool::ReadFile || client_supports_read_text_file) {
                        ToolRuntime::Enabled
                    } else {
                        ToolRuntime::Disabled
                    };
                (tool, runtime)
            })
            .collect()
    }

    pub(super) fn truncate_text(&self, input: &str) -> (String, bool) {
        if input.chars().count() <= MAX_TOOL_RESPONSE_CHARS {
            return (input.to_string(), false);
        }

        (vtcode_commons::formatting::truncate_text(input, MAX_TOOL_RESPONSE_CHARS, ""), true)
    }

    fn argument_message(template: &str, argument: &str) -> String {
        template.replace("{argument}", argument)
    }

    pub(super) fn workspace_root(&self) -> &Path {
        self.config.workspace.as_path()
    }

    pub(super) fn resolve_workspace_path(&self, candidate: PathBuf, argument: &str) -> Result<PathBuf, String> {
        let resolved_candidate = if candidate.is_absolute() {
            candidate
        } else {
            self.workspace_root().join(candidate)
        };
        let normalized = ensure_path_within_workspace(&resolved_candidate, self.workspace_root())
            .map_err(|_err| Self::argument_message(TOOL_READ_FILE_WORKSPACE_ESCAPE_TEMPLATE, argument))?;

        if !normalized.is_absolute() {
            return Err(Self::argument_message(TOOL_READ_FILE_ABSOLUTE_PATH_TEMPLATE, argument));
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
            return Err(Self::argument_message(TOOL_READ_FILE_INVALID_INTEGER_TEMPLATE, key));
        };

        if value == 0 {
            return Err(Self::argument_message(TOOL_READ_FILE_INVALID_INTEGER_TEMPLATE, key));
        }

        let value = match u32::try_from(value) {
            Ok(value) => value,
            Err(_conversion_error) => return Err(Self::argument_message(TOOL_READ_FILE_INTEGER_RANGE_TEMPLATE, key)),
        };

        Ok(Some(value))
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

        Err(format!("{TOOL_FAILURE_PREFIX}: missing {TOOL_READ_FILE_PATH_ARG} or {TOOL_READ_FILE_URI_ARG}"))
    }
}
