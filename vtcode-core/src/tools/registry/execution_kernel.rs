use anyhow::{Result, anyhow};
use serde_json::Value;

use crate::config::constants::tools as tool_names;
use crate::tools::error_messages::agent_execution;
use crate::tools::names::canonical_tool_name;
use crate::tools::validation::{commands, paths};

use super::ToolRegistry;

pub(super) const UNIFIED_FILE_MAX_PAYLOAD_BYTES: usize = 1024 * 1024;
const UNIFIED_FILE_MAX_PAYLOAD_BYTES_ENV: &str = "VTCODE_UNIFIED_FILE_MAX_PAYLOAD_BYTES";

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
        tool_names::LIST_FILES => &["path"],
        tool_names::GREP_FILE => &["pattern", "path"],
        tool_names::RUN_PTY_CMD => &["command"],
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

fn strip_wrapping_quotes(value: &str) -> &str {
    value
        .trim()
        .trim_matches('"')
        .trim_matches('\'')
        .trim_matches('`')
}

fn strip_tool_namespace_prefix(value: &str) -> &str {
    for prefix in [
        "functions.",
        "function.",
        "tools.",
        "tool.",
        "assistant.",
        "recipient_name.",
    ] {
        if let Some(stripped) = value.strip_prefix(prefix) {
            return stripped;
        }
    }
    value
}

fn push_candidate(candidates: &mut Vec<String>, value: &str) {
    let trimmed = strip_wrapping_quotes(value);
    if trimmed.is_empty() {
        return;
    }
    if !candidates.iter().any(|existing| existing == trimmed) {
        candidates.push(trimmed.to_string());
    }

    let stripped = strip_tool_namespace_prefix(trimmed);
    if stripped != trimmed && !candidates.iter().any(|existing| existing == stripped) {
        candidates.push(stripped.to_string());
    }

    let underscored = stripped
        .trim()
        .to_ascii_lowercase()
        .replace([' ', '-'], "_");
    if !underscored.is_empty() && !candidates.iter().any(|existing| existing == &underscored) {
        candidates.push(underscored);
    }
}

fn tool_name_candidates(name: &str) -> Vec<String> {
    let mut candidates = Vec::new();
    let raw = strip_wrapping_quotes(name);
    if raw.is_empty() {
        return candidates;
    }

    push_candidate(&mut candidates, raw);

    if let Some((lhs, rhs)) = raw.split_once("<|channel|>") {
        push_candidate(&mut candidates, rhs);
        push_candidate(&mut candidates, lhs);
    }

    if let Some((_, suffix)) = raw.rsplit_once(':') {
        push_candidate(&mut candidates, suffix);
    }

    candidates
}

fn schema_validation_args<'a>(
    normalized_tool_name: &str,
    args: &'a Value,
) -> std::borrow::Cow<'a, Value> {
    if normalized_tool_name != tool_names::UNIFIED_SEARCH {
        return std::borrow::Cow::Borrowed(args);
    }

    let normalized = crate::tools::tool_intent::normalize_unified_search_args(args);
    if normalized == *args {
        return std::borrow::Cow::Borrowed(args);
    }
    std::borrow::Cow::Owned(normalized)
}

pub(super) fn preflight_validate_call(
    registry: &ToolRegistry,
    name: &str,
    args: &Value,
) -> Result<ToolPreflightOutcome> {
    let candidates = tool_name_candidates(name);
    let normalized_tool_name = candidates
        .iter()
        .find_map(|candidate| {
            registry
                .inventory
                .registration_for(candidate)
                .map(|registration| registration.name().to_string())
        })
        .or_else(|| {
            candidates
                .first()
                .map(|candidate| canonical_tool_name(candidate).to_string())
        })
        .unwrap_or_else(|| canonical_tool_name(name).to_string());

    let required = required_args_for_tool(&normalized_tool_name);
    let mut failures = Vec::new();
    for key in required {
        if is_missing_required_arg(&normalized_tool_name, args, key) {
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
    enforce_unified_file_payload_limit(
        &normalized_tool_name,
        args,
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

    let validation_args = schema_validation_args(&normalized_tool_name, args);
    if let Some(registration) = registry.inventory.registration_for(&normalized_tool_name)
        && let Some(schema) = registration.parameter_schema()
        && let Err(errors) = jsonschema::validate(schema, validation_args.as_ref())
    {
        return Err(anyhow!(
            "Invalid arguments for tool '{}': {}",
            normalized_tool_name,
            errors
        ));
    }

    let intent = crate::tools::tool_intent::classify_tool_intent(&normalized_tool_name, args);
    let readonly_classification = !intent.mutating;
    if registry.is_plan_mode() && !registry.is_plan_mode_allowed(&normalized_tool_name, args) {
        let msg = agent_execution::plan_mode_denial_message(&normalized_tool_name);
        return Err(anyhow!(msg).context(agent_execution::PLAN_MODE_DENIED_CONTEXT));
    }

    Ok(ToolPreflightOutcome {
        normalized_tool_name,
        readonly_classification,
    })
}

#[cfg(test)]
mod tests {
    use super::{
        configured_unified_file_max_payload_bytes, enforce_unified_file_payload_limit,
        is_missing_required_arg, parse_unified_file_max_payload_bytes, schema_validation_args,
        tool_name_candidates,
    };
    use crate::config::constants::tools as tool_names;
    use serde_json::json;

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
    fn tool_name_candidates_extract_channel_suffix_alias() {
        let candidates = tool_name_candidates("assistant<|channel|>apply_patch");
        assert!(candidates.iter().any(|c| c == "apply_patch"));
    }

    #[test]
    fn tool_name_candidates_normalize_humanized_name() {
        let candidates = tool_name_candidates("Read file");
        assert!(candidates.iter().any(|c| c == "read_file"));
    }

    #[test]
    fn unified_search_schema_args_infers_action_from_pattern() {
        let args = json!({
            "pattern": "LLMStreamEvent::",
            "path": "."
        });

        let normalized = schema_validation_args(tool_names::UNIFIED_SEARCH, &args);
        assert_eq!(
            normalized.get("action").and_then(|v| v.as_str()),
            Some("grep")
        );
    }

    #[test]
    fn unified_search_schema_args_preserves_non_inferable_payload() {
        let args = json!({
            "max_results": 10
        });

        let normalized = schema_validation_args(tool_names::UNIFIED_SEARCH, &args);
        assert!(normalized.get("action").is_none());
    }

    #[test]
    fn unified_search_schema_args_normalizes_case_variants() {
        let args = json!({
            "Pattern": "ReasoningStage",
            "Path": "."
        });

        let normalized = schema_validation_args(tool_names::UNIFIED_SEARCH, &args);
        assert_eq!(
            normalized.get("pattern").and_then(|v| v.as_str()),
            Some("ReasoningStage")
        );
        assert_eq!(normalized.get("path").and_then(|v| v.as_str()), Some("."));
        assert_eq!(
            normalized.get("action").and_then(|v| v.as_str()),
            Some("grep")
        );
    }
}
