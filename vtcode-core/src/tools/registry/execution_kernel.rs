use anyhow::{Result, anyhow};
use serde_json::Value;

use crate::config::constants::tools as tool_names;
use crate::tools::error_messages::agent_execution;
use crate::tools::names::canonical_tool_name;
use crate::tools::validation::{commands, paths};

use super::ToolRegistry;

#[derive(Debug, Clone)]
pub struct ToolPreflightOutcome {
    pub normalized_tool_name: String,
    pub validated_args: Value,
    pub readonly_classification: bool,
    pub plan_mode_allowed: bool,
    pub validation_warnings: Vec<String>,
}

fn required_args_for_tool(tool_name: &str) -> &'static [&'static str] {
    match tool_name {
        tool_names::READ_FILE => &["path"],
        tool_names::WRITE_FILE => &["path", "content"],
        tool_names::EDIT_FILE => &["path", "old_string", "new_string"],
        tool_names::LIST_FILES => &["path"],
        tool_names::GREP_FILE => &["pattern", "path"],
        tool_names::CODE_INTELLIGENCE => &["operation"],
        tool_names::RUN_PTY_CMD => &["command"],
        tool_names::APPLY_PATCH => &["patch"],
        _ => &[],
    }
}

pub(super) fn preflight_validate_call(
    registry: &ToolRegistry,
    name: &str,
    args: &Value,
) -> Result<ToolPreflightOutcome> {
    let normalized_tool_name = registry
        .inventory
        .registration_for(name)
        .map(|registration| registration.name().to_string())
        .unwrap_or_else(|| canonical_tool_name(name).to_string());

    let required = required_args_for_tool(&normalized_tool_name);
    let mut failures = Vec::new();
    for key in required {
        let is_missing = match args.get(*key) {
            Some(v) => {
                v.is_null() || (v.is_string() && v.as_str().is_none_or(|s| s.trim().is_empty()))
            }
            None => true,
        };
        if is_missing {
            failures.push(format!("Missing required argument: {}", key));
        }
    }

    if let Some(path) = args.get("path").and_then(|v| v.as_str())
        && let Err(err) = paths::validate_path_safety(path)
    {
        failures.push(format!("Path security check failed: {}", err));
    }

    let should_validate_command = normalized_tool_name == tool_names::RUN_PTY_CMD
        || normalized_tool_name == tool_names::UNIFIED_EXEC
        || normalized_tool_name == "shell";
    if should_validate_command
        && let Some(command) = args.get("command").and_then(|v| v.as_str())
        && let Err(err) = commands::validate_command_safety(command)
    {
        failures.push(format!("Command security check failed: {}", err));
    }

    if !failures.is_empty() {
        return Err(anyhow!(
            "Tool preflight validation failed for '{}': {}",
            normalized_tool_name,
            failures.join("; ")
        ));
    }

    if let Some(registration) = registry.inventory.registration_for(&normalized_tool_name)
        && let Some(schema) = registration.parameter_schema()
        && let Err(errors) = jsonschema::validate(schema, args)
    {
        return Err(anyhow!(
            "Invalid arguments for tool '{}': {}",
            normalized_tool_name,
            errors
        ));
    }

    let intent = crate::tools::tool_intent::classify_tool_intent(&normalized_tool_name, args);
    let readonly_classification = !intent.mutating;
    let plan_mode_allowed =
        !registry.is_plan_mode() || registry.is_plan_mode_allowed(&normalized_tool_name, args);
    if !plan_mode_allowed {
        let msg = agent_execution::plan_mode_denial_message(&normalized_tool_name);
        return Err(anyhow!(msg).context(agent_execution::PLAN_MODE_DENIED_CONTEXT));
    }

    Ok(ToolPreflightOutcome {
        normalized_tool_name,
        validated_args: args.clone(),
        readonly_classification,
        plan_mode_allowed,
        validation_warnings: Vec::new(),
    })
}
