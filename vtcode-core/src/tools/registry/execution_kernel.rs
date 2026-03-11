use anyhow::{Result, anyhow};
use serde_json::Map;
use serde_json::Value;

use crate::config::constants::tools as tool_names;
use crate::tools::error_messages::agent_execution;
use crate::tools::names::canonical_tool_name;
use crate::tools::validation::{commands, paths};

use super::ToolRegistry;

pub(super) const UNIFIED_FILE_MAX_PAYLOAD_BYTES: usize = 1024 * 1024;
const UNIFIED_FILE_MAX_PAYLOAD_BYTES_ENV: &str = "VTCODE_UNIFIED_FILE_MAX_PAYLOAD_BYTES";
const DESCRIPTION_FIELD: &str = "description";
const DETAILS_ALIAS_FIELD: &str = "details";

#[derive(Debug, Clone)]
pub struct ToolPreflightOutcome {
    pub normalized_tool_name: String,
    pub readonly_classification: bool,
}

fn required_args_for_tool(tool_name: &str) -> &'static [&'static str] {
    match tool_name {
        tool_names::READ_FILE => &["path"],
        tool_names::WRITE_FILE => &["path", "content"],
        tool_names::EDIT_FILE => &["path", "old_str", "new_str"],
        tool_names::RUN_PTY_CMD | tool_names::CREATE_PTY_SESSION => &["command"],
        tool_names::APPLY_PATCH => &["patch"],
        _ => &[],
    }
}

fn is_missing_arg_value(args: &Value, key: &str) -> bool {
    match args.get(key) {
        Some(v) => v.is_null() || (v.is_string() && v.as_str().is_none_or(|s| s.trim().is_empty())),
        None => true,
    }
}

fn is_missing_apply_patch_payload(args: &Value) -> bool {
    if args.is_string() {
        return false;
    }

    let has_object_payload = |key: &str| args.get(key).is_some_and(|value| !value.is_null());
    !(has_object_payload("patch") || has_object_payload("input"))
}

fn is_missing_required_arg(tool_name: &str, args: &Value, key: &str) -> bool {
    if tool_name == tool_names::EDIT_FILE {
        return match key {
            "old_str" => {
                is_missing_arg_value(args, "old_str") && is_missing_arg_value(args, "old_string")
            }
            "new_str" => {
                is_missing_arg_value(args, "new_str") && is_missing_arg_value(args, "new_string")
            }
            _ => is_missing_arg_value(args, key),
        };
    }
    if tool_name == tool_names::APPLY_PATCH && key == "patch" {
        return is_missing_apply_patch_payload(args);
    }
    is_missing_arg_value(args, key)
}

fn parse_unified_file_max_payload_bytes(raw: Option<&str>) -> Option<usize> {
    raw.and_then(|value| value.trim().parse::<usize>().ok())
        .filter(|value| *value >= 1024)
}

fn configured_unified_file_max_payload_bytes() -> usize {
    parse_unified_file_max_payload_bytes(
        std::env::var(UNIFIED_FILE_MAX_PAYLOAD_BYTES_ENV)
            .ok()
            .as_deref(),
    )
    .unwrap_or(UNIFIED_FILE_MAX_PAYLOAD_BYTES)
}

fn schema_uses_description_alias(schema_properties: &Map<String, Value>) -> bool {
    schema_properties.contains_key(DESCRIPTION_FIELD)
        && !schema_properties.contains_key(DETAILS_ALIAS_FIELD)
}

fn normalize_description_alias(
    object: &mut Map<String, Value>,
    schema_properties: &Map<String, Value>,
) -> bool {
    if !schema_uses_description_alias(schema_properties) || object.contains_key(DESCRIPTION_FIELD) {
        return false;
    }

    let Some(details) = object.remove(DETAILS_ALIAS_FIELD) else {
        return false;
    };
    object.insert(DESCRIPTION_FIELD.to_string(), details);
    true
}

fn normalize_schema_aliases_in_place(value: &mut Value, schema: &Value) -> bool {
    let Some(schema_object) = schema.as_object() else {
        return false;
    };

    let mut changed = false;

    if let Value::Object(object) = value
        && let Some(properties) = schema_object.get("properties").and_then(Value::as_object)
    {
        changed |= normalize_description_alias(object, properties);
        for (property_name, property_schema) in properties {
            if let Some(property_value) = object.get_mut(property_name) {
                changed |= normalize_schema_aliases_in_place(property_value, property_schema);
            }
        }
    }

    if let Value::Array(items) = value
        && let Some(items_schema) = schema_object.get("items")
    {
        for item in items {
            changed |= normalize_schema_aliases_in_place(item, items_schema);
        }
    }

    for keyword in ["allOf", "anyOf", "oneOf"] {
        if let Some(branches) = schema_object.get(keyword).and_then(Value::as_array) {
            for branch in branches {
                changed |= normalize_schema_aliases_in_place(value, branch);
            }
        }
    }
    for keyword in ["if", "then", "else"] {
        if let Some(branch) = schema_object.get(keyword) {
            changed |= normalize_schema_aliases_in_place(value, branch);
        }
    }

    changed
}

fn normalize_details_aliases(args: &Value, parameter_schema: Option<&Value>) -> Option<Value> {
    let schema = parameter_schema?;
    let mut normalized = args.clone();
    normalize_schema_aliases_in_place(&mut normalized, schema).then_some(normalized)
}

fn serialized_payload_size_bytes(args: &Value) -> usize {
    serde_json::to_vec(args)
        .map(|bytes| bytes.len())
        .unwrap_or_else(|_| args.to_string().len())
}

fn unified_file_action_for_limit(normalized_tool_name: &str, args: &Value) -> Option<String> {
    if normalized_tool_name == tool_names::UNIFIED_FILE {
        return crate::tools::tool_intent::unified_file_action(args)
            .map(|a| a.to_ascii_lowercase());
    }
    if normalized_tool_name == tool_names::APPLY_PATCH {
        return Some("patch".to_string());
    }
    if normalized_tool_name == tool_names::EDIT_FILE {
        return Some("edit".to_string());
    }
    None
}

fn enforce_unified_file_payload_limit(
    normalized_tool_name: &str,
    args: &Value,
    max_payload_bytes: usize,
    failures: &mut Vec<String>,
) {
    let Some(action) = unified_file_action_for_limit(normalized_tool_name, args) else {
        return;
    };
    if action != "patch" && action != "edit" {
        return;
    }

    let payload_bytes = serialized_payload_size_bytes(args);
    if payload_bytes <= max_payload_bytes {
        return;
    }

    tracing::warn!(
        tool = %normalized_tool_name,
        action = %action,
        payload_bytes,
        max_payload_bytes,
        "Rejected oversized patch/edit payload during preflight"
    );

    failures.push(format!(
        "Patch/edit payload too large for '{}': action='{}', payload={} bytes exceeds {} bytes. \
         Split the change into smaller patch/edit calls, or raise {} for intentional large edits.",
        normalized_tool_name,
        action,
        payload_bytes,
        max_payload_bytes,
        UNIFIED_FILE_MAX_PAYLOAD_BYTES_ENV
    ));
}

pub(super) fn normalize_tool_args<'a>(
    normalized_tool_name: &str,
    args: &'a Value,
    parameter_schema: Option<&Value>,
) -> Result<std::borrow::Cow<'a, Value>> {
    let mut normalized = std::borrow::Cow::Borrowed(args);

    if matches!(
        normalized_tool_name,
        tool_names::RUN_PTY_CMD
            | tool_names::CREATE_PTY_SESSION
            | tool_names::UNIFIED_EXEC
            | "shell"
    ) {
        let shell_args = crate::tools::command_args::normalize_shell_args(normalized.as_ref())
            .map_err(|error| anyhow!(error))?;
        if shell_args != *normalized.as_ref() {
            normalized = std::borrow::Cow::Owned(shell_args);
        }
    }

    if normalized_tool_name == tool_names::UNIFIED_SEARCH {
        let search_args =
            crate::tools::tool_intent::normalize_unified_search_args(normalized.as_ref());
        if search_args != *normalized.as_ref() {
            normalized = std::borrow::Cow::Owned(search_args);
        }
    }

    if let Some(alias_args) = normalize_details_aliases(normalized.as_ref(), parameter_schema) {
        normalized = std::borrow::Cow::Owned(alias_args);
    }

    Ok(normalized)
}

pub(super) fn preflight_validate_call(
    registry: &ToolRegistry,
    name: &str,
    args: &Value,
) -> Result<ToolPreflightOutcome> {
    let normalized_tool_name = registry
        .resolve_public_tool(name)
        .map(|resolution| resolution.registration_name().to_string())
        .map_err(|_| anyhow!("Unknown tool: {}", canonical_tool_name(name)))?;

    preflight_validate_resolved_call(registry, &normalized_tool_name, args)
}

pub(super) fn preflight_validate_resolved_call(
    registry: &ToolRegistry,
    normalized_tool_name: &str,
    args: &Value,
) -> Result<ToolPreflightOutcome> {
    let parameter_schema = registry
        .inventory
        .registration_for(normalized_tool_name)
        .and_then(|registration| registration.parameter_schema().cloned());
    let validation_args =
        normalize_tool_args(normalized_tool_name, args, parameter_schema.as_ref())?;
    let required = required_args_for_tool(normalized_tool_name);
    let mut failures = Vec::new();
    for key in required {
        if is_missing_required_arg(normalized_tool_name, validation_args.as_ref(), key) {
            failures.push(format!("Missing required argument: {}", key));
        }
    }

    if let Some(path) = validation_args
        .as_ref()
        .get("path")
        .and_then(|v| v.as_str())
        && let Err(err) = paths::validate_path_safety(path)
    {
        failures.push(format!("Path security check failed: {}", err));
    }

    let should_validate_command = normalized_tool_name == tool_names::RUN_PTY_CMD
        || normalized_tool_name == tool_names::CREATE_PTY_SESSION
        || normalized_tool_name == tool_names::UNIFIED_EXEC
        || normalized_tool_name == "shell";
    if should_validate_command
        && let Some(command) = validation_args
            .as_ref()
            .get("command")
            .and_then(|v| v.as_str())
        && let Err(err) = commands::validate_command_safety(command)
    {
        failures.push(format!("Command security check failed: {}", err));
    }
    enforce_unified_file_payload_limit(
        normalized_tool_name,
        validation_args.as_ref(),
        configured_unified_file_max_payload_bytes(),
        &mut failures,
    );

    if !failures.is_empty() {
        return Err(anyhow!(
            "Tool preflight validation failed for '{}': {}",
            normalized_tool_name,
            failures.join("; ")
        ));
    }

    if normalized_tool_name == tool_names::UNIFIED_SEARCH
        && crate::tools::tool_intent::unified_search_action(validation_args.as_ref()).is_none()
    {
        return Err(anyhow!(
            "Invalid arguments for tool '{}': missing action; provide `action` or inferable search arguments",
            normalized_tool_name
        ));
    }
    if let Some(schema) = parameter_schema.as_ref()
        && let Err(errors) = jsonschema::validate(schema, validation_args.as_ref())
    {
        return Err(anyhow!(
            "Invalid arguments for tool '{}': {}",
            normalized_tool_name,
            errors
        ));
    }

    let intent = crate::tools::tool_intent::classify_tool_intent(normalized_tool_name, args);
    let readonly_classification = !intent.mutating;
    if registry.is_plan_mode() && !registry.is_plan_mode_allowed(normalized_tool_name, args) {
        let msg = agent_execution::plan_mode_denial_message(normalized_tool_name);
        return Err(anyhow!(msg).context(agent_execution::PLAN_MODE_DENIED_CONTEXT));
    }

    Ok(ToolPreflightOutcome {
        normalized_tool_name: normalized_tool_name.to_string(),
        readonly_classification,
    })
}

#[cfg(test)]
mod tests {
    use super::super::assembly::public_tool_name_candidates;
    use super::{
        configured_unified_file_max_payload_bytes, enforce_unified_file_payload_limit,
        is_missing_required_arg, normalize_tool_args, parse_unified_file_max_payload_bytes,
    };
    use crate::config::constants::tools as tool_names;
    use crate::tools::command_args::parse_indexed_command_parts;
    use crate::tools::request_user_input::RequestUserInputTool;
    use crate::tools::traits::Tool;
    use anyhow::Result;
    use serde_json::{Value, json};

    #[test]
    fn patch_action_within_limit_is_allowed() {
        let mut failures = Vec::new();
        let args = json!({
            "action": "patch",
            "patch": "*** Begin Patch\n*** End Patch\n"
        });

        enforce_unified_file_payload_limit(tool_names::UNIFIED_FILE, &args, 1024, &mut failures);
        assert!(failures.is_empty());
    }

    #[test]
    fn patch_action_over_limit_is_rejected() {
        let mut failures = Vec::new();
        let args = json!({
            "action": "patch",
            "patch": "x".repeat(512)
        });

        enforce_unified_file_payload_limit(tool_names::UNIFIED_FILE, &args, 128, &mut failures);
        assert_eq!(failures.len(), 1);
        assert!(failures[0].contains("payload too large"));
        assert!(failures[0].contains("Split the change"));
    }

    #[test]
    fn edit_tool_over_limit_is_rejected() {
        let mut failures = Vec::new();
        let args = json!({
            "path": "file.txt",
            "old_str": "old",
            "new_str": "x".repeat(512)
        });

        enforce_unified_file_payload_limit(tool_names::EDIT_FILE, &args, 128, &mut failures);
        assert_eq!(failures.len(), 1);
        assert!(failures[0].contains("action='edit'"));
    }

    #[test]
    fn read_action_is_not_limited() {
        let mut failures = Vec::new();
        let args = json!({
            "action": "read",
            "path": "README.md"
        });

        enforce_unified_file_payload_limit(tool_names::UNIFIED_FILE, &args, 1, &mut failures);
        assert!(failures.is_empty());
    }

    #[test]
    fn edit_file_required_args_accept_legacy_key_names() {
        let args = json!({
            "path": "file.txt",
            "old_string": "old",
            "new_string": "new"
        });

        assert!(!is_missing_required_arg(
            tool_names::EDIT_FILE,
            &args,
            "path"
        ));
        assert!(!is_missing_required_arg(
            tool_names::EDIT_FILE,
            &args,
            "old_str"
        ));
        assert!(!is_missing_required_arg(
            tool_names::EDIT_FILE,
            &args,
            "new_str"
        ));
    }

    #[test]
    fn parse_payload_limit_accepts_safe_override() {
        let parsed = parse_unified_file_max_payload_bytes(Some("2048"));
        assert_eq!(parsed, Some(2048));
    }

    #[test]
    fn parse_payload_limit_rejects_too_small_values() {
        let parsed = parse_unified_file_max_payload_bytes(Some("512"));
        assert_eq!(parsed, None);
    }

    #[test]
    fn parse_payload_limit_rejects_invalid_values() {
        let parsed = parse_unified_file_max_payload_bytes(Some("not-a-number"));
        assert_eq!(parsed, None);
    }

    #[test]
    fn configured_payload_limit_is_always_safe() {
        let configured = configured_unified_file_max_payload_bytes();
        assert!(configured >= 1024);
    }

    #[test]
    fn apply_patch_required_arg_accepts_input_alias() {
        assert!(!is_missing_required_arg(
            tool_names::APPLY_PATCH,
            &json!({"input": ""}),
            "patch"
        ));
    }

    #[test]
    fn apply_patch_required_arg_accepts_raw_string_payload() {
        assert!(!is_missing_required_arg(
            tool_names::APPLY_PATCH,
            &json!(""),
            "patch"
        ));
    }

    #[test]
    fn run_pty_cmd_required_arg_accepts_zero_based_indexed_command() -> Result<()> {
        let input = json!({
            "command.0": "ls",
            "command.1": "-a"
        });
        let args = normalize_tool_args(tool_names::RUN_PTY_CMD, &input, None)?;

        assert!(!is_missing_required_arg(
            tool_names::RUN_PTY_CMD,
            args.as_ref(),
            "command"
        ));
        assert_eq!(
            args.get("command").and_then(|value| value.as_str()),
            Some("ls -a")
        );
        Ok(())
    }

    #[test]
    fn run_pty_cmd_required_arg_accepts_one_based_indexed_command() -> Result<()> {
        let input = json!({
            "command.1": "ls",
            "command.2": "-a"
        });
        let args = normalize_tool_args(tool_names::RUN_PTY_CMD, &input, None)?;

        assert!(!is_missing_required_arg(
            tool_names::RUN_PTY_CMD,
            args.as_ref(),
            "command"
        ));
        assert_eq!(
            args.get("command").and_then(|value| value.as_str()),
            Some("ls -a")
        );
        Ok(())
    }

    #[test]
    fn indexed_command_parts_require_zero_or_one_based_sequences() {
        assert_eq!(
            parse_indexed_command_parts(
                json!({
                    "command.0": "ls",
                    "command.1": "-a"
                })
                .as_object()
                .expect("object"),
            )
            .expect("valid indexed args"),
            Some(vec!["ls".to_string(), "-a".to_string()])
        );
        assert_eq!(
            parse_indexed_command_parts(
                json!({
                    "command.1": "ls",
                    "command.2": "-a"
                })
                .as_object()
                .expect("object"),
            )
            .expect("valid indexed args"),
            Some(vec!["ls".to_string(), "-a".to_string()])
        );
        assert_eq!(
            parse_indexed_command_parts(json!({"command.2": "ls"}).as_object().expect("object"))
                .expect("valid indexed args"),
            None
        );
    }

    #[test]
    fn tool_name_candidates_extract_channel_suffix_alias() {
        let candidates = public_tool_name_candidates("assistant<|channel|>apply_patch");
        assert!(candidates.iter().any(|c| c == "apply_patch"));
    }

    #[test]
    fn tool_name_candidates_normalize_humanized_name() {
        let candidates = public_tool_name_candidates("Read file");
        assert!(candidates.iter().any(|c| c == "read_file"));
    }

    #[test]
    fn unified_search_schema_args_infers_action_from_pattern() -> Result<()> {
        let args = json!({
            "pattern": "LLMStreamEvent::",
            "path": "."
        });

        let normalized = normalize_tool_args(tool_names::UNIFIED_SEARCH, &args, None)?;
        assert_eq!(
            normalized.get("action").and_then(|v| v.as_str()),
            Some("grep")
        );
        Ok(())
    }

    #[test]
    fn unified_search_schema_args_preserves_non_inferable_payload() -> Result<()> {
        let args = json!({
            "max_results": 10
        });

        let normalized = normalize_tool_args(tool_names::UNIFIED_SEARCH, &args, None)?;
        assert!(normalized.get("action").is_none());
        Ok(())
    }

    #[test]
    fn unified_search_schema_args_normalizes_case_variants() -> Result<()> {
        let args = json!({
            "Pattern": "ReasoningStage",
            "Path": "."
        });

        let normalized = normalize_tool_args(tool_names::UNIFIED_SEARCH, &args, None)?;
        assert_eq!(
            normalized.get("pattern").and_then(|v| v.as_str()),
            Some("ReasoningStage")
        );
        assert_eq!(normalized.get("path").and_then(|v| v.as_str()), Some("."));
        assert_eq!(
            normalized.get("action").and_then(|v| v.as_str()),
            Some("grep")
        );
        Ok(())
    }

    #[test]
    fn request_user_input_args_accept_details_alias() -> Result<()> {
        let schema = RequestUserInputTool
            .parameter_schema()
            .expect("request_user_input schema");
        let args = json!({
            "questions": [{
                "id": "scope",
                "header": "Scope",
                "question": "Which direction should we take?",
                "options": [
                    {
                        "label": "Minimal",
                        "details": "Ship the smallest viable slice."
                    },
                    {
                        "label": "Full",
                        "details": "Ship the full implementation."
                    }
                ]
            }]
        });

        let normalized = normalize_tool_args(tool_names::REQUEST_USER_INPUT, &args, Some(&schema))?;
        let option = &normalized["questions"][0]["options"][0];
        assert_eq!(
            option.get("description").and_then(Value::as_str),
            Some("Ship the smallest viable slice.")
        );
        assert!(option.get("details").is_none());
        Ok(())
    }

    #[test]
    fn task_tracker_args_accept_details_alias() -> Result<()> {
        let schema = json!({
            "type": "object",
            "properties": {
                "action": { "type": "string" },
                "description": { "type": "string" }
            }
        });
        let args = json!({
            "action": "add",
            "details": "Add regression coverage"
        });

        let normalized = normalize_tool_args(tool_names::TASK_TRACKER, &args, Some(&schema))?;
        assert_eq!(
            normalized.get("description").and_then(Value::as_str),
            Some("Add regression coverage")
        );
        assert!(normalized.get("details").is_none());
        Ok(())
    }

    #[test]
    fn details_alias_does_not_shadow_real_details_field() -> Result<()> {
        let schema = json!({
            "type": "object",
            "properties": {
                "description": { "type": "string" },
                "details": { "type": "string" }
            }
        });
        let args = json!({
            "details": "Keep the real details field."
        });

        let normalized = normalize_tool_args(tool_names::TASK_TRACKER, &args, Some(&schema))?;
        assert!(normalized.get("description").is_none());
        assert_eq!(
            normalized.get("details").and_then(Value::as_str),
            Some("Keep the real details field.")
        );
        Ok(())
    }
}
