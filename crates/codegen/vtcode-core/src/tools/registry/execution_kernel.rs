use anyhow::{Result, anyhow};
use serde_json::Map;
use serde_json::Value;
use serde_json::json;

use crate::config::constants::tools as tool_names;
use crate::tools::apply_patch::{UNIFIED_FILE_MAX_PAYLOAD_BYTES_ENV, effective_max_payload_bytes};
use crate::tools::error_messages::agent_execution;
use crate::tools::names::canonical_tool_name;
use crate::tools::registry::ToolCatalogSource;
use crate::tools::tool_intent::ToolIntent;
use crate::tools::validation::{commands, condensed_schema_hint, paths};
use crate::utils::tool_name_parsing::MCP_QUALIFIED_TOOL_PREFIX;

use super::ToolRegistry;

const DESCRIPTION_FIELD: &str = "description";
const DETAILS_ALIAS_FIELD: &str = "details";

/// Path-field aliases accepted by file tools, in priority order.
///
/// `read_file`/`edit_file`/`unified_file` historically accept several path-key
/// spellings. The required-arg check, the preflight path-safety check, and the
/// execution-history path extraction must all agree on the same set (and order)
/// so a value accepted as "present" by one stage is validated by the next.
/// Shared across the `registry` module via `pub(super)`.
pub(super) const PATH_ALIAS_KEYS: [&str; 5] = ["path", "file_path", "filepath", "target_path", "file"];

/// The explicit set of model-hidden tools the harness path is allowed to
/// dispatch when the public route lookup misses.
///
/// These are registered as real builtins (`with_llm_visibility(false)`) but
/// deliberately excluded from the public route table
/// (`assembly::is_removed_public_tool_name`). Only names in this allowlist may
/// fall back to the inventory registration on the harness path; every other
/// `with_llm_visibility(false)` helper stays inaccessible through the public
/// execution entrypoint. This turns internal dispatch into an explicit,
/// reviewed surface instead of "any inventory registration not in the public
/// routes".
const HARNESS_DISPATCHABLE_INTERNAL_TOOLS: [&str; 4] = [
    tool_names::READ_FILE,
    tool_names::WRITE_FILE,
    tool_names::EDIT_FILE,
    tool_names::LIST_FILES,
];

/// Entry point classification for a tool dispatch.
///
/// The public model surface exposes only the Codex baseline (apply_patch,
/// exec_command, write_stdin, code_search). File helpers
/// (read_file/write_file/list_files/edit_file) are registered builtins hidden
/// from the model and excluded from the public route table, but remain
/// dispatchable for the harness path. `DispatchMode` encodes that distinction in
/// the type system so the "public is strict, harness is the only fallback"
/// invariant lives in exactly one place and cannot be re-derived incorrectly by
/// a future caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DispatchMode {
    /// Direct model-originated entry (e.g. `execute_public_tool_ref`,
    /// integration tests). Rejects anything not in the public route table.
    ModelPublic,
    /// Harness entry (via `admit_public_tool_call`). Falls back to the internal
    /// builtin registration for the harness-dispatchable allowlist when the
    /// public route lookup misses.
    Harness,
}

impl DispatchMode {
    fn allows_internal_dispatch(self) -> bool {
        matches!(self, DispatchMode::Harness)
    }
}

#[derive(Debug, Clone)]
pub struct ToolPreflightOutcome {
    pub normalized_tool_name: String,
    pub readonly_classification: bool,
    pub parallel_safe_after_preflight: bool,
    pub effective_args: Value,
    /// The intent classified on the validation args that become the executed
    /// text. Consumers must reuse it instead of re-classifying, so planning
    /// enforcement cannot disagree with `readonly_classification`.
    pub intent: ToolIntent,
}

fn required_args_for_tool(tool_name: &str) -> &'static [&'static str] {
    match tool_name {
        tool_names::READ_FILE => &["path"],
        tool_names::WRITE_FILE => &["path", "content"],
        tool_names::EDIT_FILE => &["path", "old_str", "new_str"],
        tool_names::RUN_PTY_CMD | tool_names::CREATE_PTY_SESSION => &["command"],
        tool_names::APPLY_PATCH => &["input"],
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

    let has_object_payload = |key: &str| args.get(key).is_some_and(Value::is_string);
    !(has_object_payload("patch") || has_object_payload("input"))
}

fn is_missing_required_arg(tool_name: &str, args: &Value, key: &str) -> bool {
    if tool_name == tool_names::READ_FILE && key == "path" {
        return PATH_ALIAS_KEYS.iter().all(|candidate| is_missing_arg_value(args, candidate));
    }
    if tool_name == tool_names::EDIT_FILE {
        return match key {
            "old_str" => is_missing_arg_value(args, "old_str") && is_missing_arg_value(args, "old_string"),
            "new_str" => is_missing_arg_value(args, "new_str") && is_missing_arg_value(args, "new_string"),
            _ => is_missing_arg_value(args, key),
        };
    }
    if tool_name == tool_names::APPLY_PATCH && key == "input" {
        return is_missing_apply_patch_payload(args);
    }
    is_missing_arg_value(args, key)
}

/// Format a missing-required-argument failure with the canonical wording.
///
/// Single source of truth for the preflight failure message so the
/// tool-specific and command-session required-arg checks cannot drift in
/// phrasing (both feed into the same joined `failures` list).
fn missing_required_arg_failure(key: &str) -> String {
    format!("Missing required argument: {key}")
}

#[cfg(test)]
fn parse_file_operation_max_payload_bytes(raw: Option<&str>) -> Option<usize> {
    raw.and_then(|value| value.trim().parse::<usize>().ok())
        .filter(|value| *value >= 1024)
}

fn configured_file_operation_max_payload_bytes() -> usize {
    // Single source of truth for the cap: both preflight and the post-decode
    // size check in `apply_patch` resolve the env-var override the same way
    // (including the 1 KiB safety floor), so the two stages always agree.
    effective_max_payload_bytes()
}

fn schema_uses_description_alias(schema_properties: &Map<String, Value>) -> bool {
    schema_properties.contains_key(DESCRIPTION_FIELD) && !schema_properties.contains_key(DETAILS_ALIAS_FIELD)
}

fn normalize_description_alias(object: &mut Map<String, Value>, schema_properties: &Map<String, Value>) -> bool {
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
                // Coerce string-encoded JSON into the schema-declared type before
                // recursing, so nested alias/type normalization sees the real value.
                // The output-budget field is skipped: it has a dedicated strict
                // validator (`output_limits::max_output_tokens`) that deliberately
                // rejects string/float values, and bypassing it would weaken that
                // guardrail.
                if property_name != vtcode_utility_tool_specs::MAX_OUTPUT_TOKENS_FIELD {
                    changed |= coerce_string_to_schema_type_in_place(property_value, property_schema);
                }
                changed |= normalize_schema_aliases_in_place(property_value, property_schema);
            }
        }
    }

    if let Value::Array(items) = value
        && let Some(items_schema) = schema_object.get("items")
    {
        for item in items {
            changed |= coerce_string_to_schema_type_in_place(item, items_schema);
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

/// Strict, schema-aware compatibility coercion of a string-encoded JSON value.
///
/// Some models occasionally emit a JSON-encoded *string* where the schema
/// expects a structured or primitive value — e.g. `"[\"path\"]"` for an array
/// field, or `"10"` for an integer field. Left unhandled the agent retries the
/// same malformed call for many turns (six preflight failures were observed in
/// a single checkpoint turn), wasting tool budget and context.
///
/// When `value` is a string and `schema` declares a single non-string `type`,
/// this rewrites it in place to the parsed JSON value, but **only** when the
/// parse is unambiguous and the parsed top-level type exactly matches the
/// schema. It deliberately does not:
/// - coerce free-form string fields (schema `type` is `string`, an array of
///   types, or absent),
/// - reinterpret malformed/non-JSON strings (they stay strings and fail strict
///   validation with an actionable message),
/// - coerce floats into integers (`"10.0"` for an integer field stays a string),
/// - relax enum/bounds checks — strict `jsonschema` validation still runs after.
fn coerce_string_to_schema_type_in_place(value: &mut Value, schema: &Value) -> bool {
    let Some(raw) = value.as_str() else {
        return false;
    };
    let Some(schema_type) = schema.get("type").and_then(Value::as_str) else {
        return false;
    };
    let Some(coerced) = parse_string_as_schema_type(raw, schema_type) else {
        return false;
    };
    *value = coerced;
    true
}

/// Parse a JSON-encoded string into the schema-declared primitive/container type.
///
/// Returns `Some` only when `serde_json` accepts the string and the resulting
/// top-level JSON type exactly matches `schema_type`. Anything else returns
/// `None` so the caller leaves the value untouched and lets strict validation
/// produce the error.
fn parse_string_as_schema_type(raw: &str, schema_type: &str) -> Option<Value> {
    let parsed = serde_json::from_str::<Value>(raw).ok()?;
    match schema_type {
        "array" => parsed.is_array().then_some(parsed),
        "object" => parsed.is_object().then_some(parsed),
        "integer" => parsed.as_i64().map(Value::from).or_else(|| parsed.as_u64().map(Value::from)),
        "number" => parsed.is_number().then_some(parsed),
        "boolean" => parsed.as_bool().map(Value::Bool),
        _ => None,
    }
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

fn file_operation_action_for_limit(normalized_tool_name: &str, args: &Value) -> Option<String> {
    if normalized_tool_name == tool_names::UNIFIED_FILE {
        return crate::tools::tool_intent::file_operation_action(args).map(|a| a.to_ascii_lowercase());
    }
    if normalized_tool_name == tool_names::APPLY_PATCH {
        return Some("patch".to_string());
    }
    if normalized_tool_name == tool_names::EDIT_FILE {
        return Some("edit".to_string());
    }
    None
}

/// Shared prefix for action-alias remappers: return the args object only when it
/// is a JSON object that does not already declare an `action`. A caller that
/// needs to reject non-matching tool names does that check before calling.
fn args_object_without_action(args: &Value) -> Option<&Map<String, Value>> {
    let obj = args.as_object()?;
    if obj.contains_key("action") {
        return None;
    }
    Some(obj)
}

/// Clone an args object and inject the resolved `action` field, producing the
/// remapped payload. Single source for the clone-insert-wrap so the two
/// action-alias remappers cannot drift in how the action is attached.
fn with_action_inserted(obj: &Map<String, Value>, action: &str) -> Value {
    let mut mapped = obj.clone();
    mapped.insert("action".to_string(), Value::String(action.to_string()));
    Value::Object(mapped)
}

pub(super) fn remap_public_file_operation_alias_args(
    requested_name: &str,
    normalized_tool_name: &str,
    args: &Value,
) -> Option<Value> {
    if normalized_tool_name != tool_names::UNIFIED_FILE {
        return None;
    }

    let obj = args_object_without_action(args)?;

    let action = super::assembly::public_tool_name_candidates(requested_name)
        .into_iter()
        .find_map(|candidate| match candidate.as_str() {
            tool_names::READ_FILE => Some("read"),
            tool_names::WRITE_FILE => Some("write"),
            tool_names::EDIT_FILE => Some("edit"),
            tool_names::DELETE_FILE => Some("delete"),
            tool_names::MOVE_FILE => Some("move"),
            tool_names::COPY_FILE => Some("copy"),
            tool_names::CREATE_FILE => Some("write"),
            _ => None,
        })?;

    Some(with_action_inserted(obj, action))
}

pub(super) fn remap_consolidated_action_alias_args(
    requested_name: &str,
    normalized_tool_name: &str,
    args: &Value,
) -> Option<Value> {
    let obj = args_object_without_action(args)?;

    let action = super::assembly::public_tool_name_candidates(requested_name)
        .into_iter()
        .find_map(|candidate| match (normalized_tool_name, candidate.as_str()) {
            (tool_names::MCP, tool_names::MCP_SEARCH_TOOLS) => Some("search_tools"),
            (tool_names::MCP, tool_names::MCP_GET_TOOL_DETAILS) => Some("get_tool_details"),
            (tool_names::MCP, tool_names::MCP_LIST_SERVERS) => Some("list_servers"),
            (tool_names::MCP, tool_names::MCP_CONNECT_SERVER) => Some("connect"),
            (tool_names::MCP, tool_names::MCP_DISCONNECT_SERVER) => Some("disconnect"),
            (tool_names::CRON, tool_names::CRON_CREATE) => Some("create"),
            (tool_names::CRON, tool_names::CRON_LIST) => Some("list"),
            (tool_names::CRON, tool_names::CRON_DELETE) => Some("delete"),
            (tool_names::AGENT, tool_names::SPAWN_AGENT) => Some("spawn"),
            (tool_names::AGENT, tool_names::SPAWN_BACKGROUND_SUBPROCESS) => Some("spawn_subprocess"),
            (tool_names::AGENT, tool_names::SEND_INPUT) => Some("send_input"),
            (tool_names::AGENT, tool_names::RESUME_AGENT) => Some("resume"),
            (tool_names::AGENT, tool_names::WAIT_AGENT) => Some("wait"),
            (tool_names::AGENT, tool_names::CLOSE_AGENT) => Some("close"),
            _ => None,
        })?;

    Some(with_action_inserted(obj, action))
}

fn enforce_file_operation_payload_limit(
    normalized_tool_name: &str,
    args: &Value,
    max_payload_bytes: usize,
    failures: &mut Vec<String>,
) {
    let Some(action) = file_operation_action_for_limit(normalized_tool_name, args) else {
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
        "Patch/edit payload too large for '{normalized_tool_name}': action='{action}', payload={payload_bytes} bytes exceeds {max_payload_bytes} bytes. \
         Split the change into smaller patch/edit calls, or raise {UNIFIED_FILE_MAX_PAYLOAD_BYTES_ENV} for intentional large edits."
    ));
}

pub(super) fn normalize_tool_args<'a>(
    normalized_tool_name: &str,
    args: &'a Value,
    parameter_schema: Option<&Value>,
) -> Result<std::borrow::Cow<'a, Value>> {
    let mut normalized = std::borrow::Cow::Borrowed(args);

    if normalized_tool_name == tool_names::APPLY_PATCH
        && let Some(raw_patch) = normalized.as_ref().as_str()
    {
        normalized = std::borrow::Cow::Owned(json!({ "input": raw_patch }));
    }

    if matches!(
        normalized_tool_name,
        tool_names::RUN_PTY_CMD | tool_names::CREATE_PTY_SESSION | tool_names::UNIFIED_EXEC | tool_names::SHELL
    ) {
        let shell_args =
            crate::tools::command_args::normalize_shell_args(normalized.as_ref()).map_err(|error| anyhow!(error))?;
        if shell_args != *normalized.as_ref() {
            normalized = std::borrow::Cow::Owned(shell_args);
        }
        // Share truthful-status normalization with gate accounting. Pure
        // truncators are elided; static read-only filters retain their output
        // under fail-closed pipefail. Safety and permissions still validate
        // the resulting command. Preserve the original cmd as typed evidence.
        if let std::borrow::Cow::Owned(executed) =
            crate::tools::tool_intent::shell_args_as_executed(normalized_tool_name, normalized.as_ref())
        {
            normalized = std::borrow::Cow::Owned(executed);
        }
    }

    if let Some(alias_args) = normalize_details_aliases(normalized.as_ref(), parameter_schema) {
        normalized = std::borrow::Cow::Owned(alias_args);
    }

    Ok(normalized)
}

fn public_exec_validation_args(normalized_tool_name: &str, args: &Value) -> Result<Option<Value>> {
    let write_stdin_dispatch = match normalized_tool_name {
        tool_names::WRITE_STDIN => {
            Some(crate::tools::command_args::write_stdin_dispatch(args).map_err(|error| anyhow!(error))?)
        }
        _ => None,
    };
    let action = match normalized_tool_name {
        tool_names::EXEC_COMMAND => "run",
        tool_names::WRITE_STDIN => write_stdin_dispatch
            .map(crate::tools::command_args::WriteStdinDispatch::command_session_action)
            .ok_or_else(|| anyhow!("write_stdin dispatch was not resolved"))?,
        _ => return Ok(None),
    };
    let mut exec_args = crate::tools::command_args::normalize_shell_args(args).map_err(|error| anyhow!(error))?;
    // Same truthful-status rewrite as `normalize_tool_args` (see above):
    // `EXEC_COMMAND` normalizes here rather than there, so the hook must be
    // repeated to keep validation and execution agreed on one command text.
    if let std::borrow::Cow::Owned(executed) =
        crate::tools::tool_intent::shell_args_as_executed(normalized_tool_name, &exec_args)
    {
        exec_args = executed;
    }
    let payload = exec_args
        .as_object_mut()
        .ok_or_else(|| anyhow!("{normalized_tool_name} requires a JSON object"))?;
    if write_stdin_dispatch == Some(crate::tools::command_args::WriteStdinDispatch::Poll) {
        payload.remove("input");
    }
    payload.insert("action".to_string(), Value::String(action.to_string()));
    Ok(Some(exec_args))
}

pub(super) fn preflight_validate_call(
    registry: &ToolRegistry,
    name: &str,
    args: &Value,
) -> Result<ToolPreflightOutcome> {
    preflight_validate_call_with_mode(registry, name, args, DispatchMode::ModelPublic)
}

/// Preflight-validate a tool call under the given [`DispatchMode`].
///
/// Resolution is delegated to [`resolve_dispatch_target`] so the preflight and
/// execution paths agree on exactly which names are dispatchable in each mode.
pub(super) fn preflight_validate_call_with_mode(
    registry: &ToolRegistry,
    name: &str,
    args: &Value,
    mode: DispatchMode,
) -> Result<ToolPreflightOutcome> {
    let normalized_tool_name = resolve_dispatch_target(registry, name, mode)?;

    if let Some(remapped_args) = remap_public_file_operation_alias_args(name, &normalized_tool_name, args)
        .or_else(|| remap_consolidated_action_alias_args(name, &normalized_tool_name, args))
    {
        preflight_validate_resolved_call(registry, &normalized_tool_name, &remapped_args)
    } else {
        preflight_validate_resolved_call(registry, &normalized_tool_name, args)
    }
}

/// Resolve a requested tool name to its registration name for the given
/// [`DispatchMode`].
///
/// This is the single source of truth shared by both the preflight
/// ([`preflight_validate_call_with_mode`]) and execution
/// (`execute_public_tool_ref_internal_with_mode`) paths, so they can never
/// disagree on whether a tool is dispatchable.
///
/// - [`DispatchMode::ModelPublic`]: only the public route table resolves; any
///   miss returns "Unknown tool".
/// - [`DispatchMode::Harness`]: on a public-route miss, fall back to the
///   internal builtin registration, but only for names in
///   [`HARNESS_DISPATCHABLE_INTERNAL_TOOLS`].
pub(super) fn resolve_dispatch_target(registry: &ToolRegistry, name: &str, mode: DispatchMode) -> Result<String> {
    match registry.resolve_public_tool(name) {
        Ok(resolution) => Ok(resolution.registration_name().to_string()),
        Err(public_err) => {
            if mode.allows_internal_dispatch() {
                if let Some(internal_target) = resolve_internal_dispatch_tool(registry, name) {
                    return Ok(internal_target);
                }
                if name.eq_ignore_ascii_case("mcp_proxy")
                    && let Some(mcp_target) = resolve_mcp_proxy_dispatch_target(registry)
                {
                    return mcp_target;
                }
            }
            Err(anyhow!("Unknown tool: {}: {public_err}", canonical_tool_name(name)))
        }
    }
}

/// Resolve the legacy `mcp_proxy` tool name used by some persisted model
/// histories to a registered MCP proxy.
///
/// Returns `Ok(None)` when no MCP registration exists, and an explicit error
/// naming the model-visible alternatives when the request is ambiguous. This
/// keeps the fallback scoped to MCP tools instead of silently widening the
/// harness dispatch surface.
fn resolve_mcp_proxy_dispatch_target(registry: &ToolRegistry) -> Option<Result<String>> {
    let mcp_registrations = registry
        .inventory
        .registrations_snapshot()
        .into_iter()
        .filter(|registration| registration.catalog_source() == ToolCatalogSource::Mcp)
        .collect::<Vec<_>>();
    if mcp_registrations.is_empty() {
        return None;
    }

    if mcp_registrations.len() == 1 {
        return Some(Ok(mcp_registrations[0].name().to_string()));
    }

    let visible_names = mcp_registrations
        .iter()
        .flat_map(|registration| {
            registration
                .metadata()
                .aliases()
                .iter()
                .find(|alias| alias.starts_with(MCP_QUALIFIED_TOOL_PREFIX))
                .map(|alias| alias.to_string())
                .or_else(|| Some(registration.name().to_string()))
        })
        .collect::<Vec<_>>()
        .join(", ");
    Some(Err(anyhow!("Ambiguous MCP proxy call: specify one of the available MCP tools: {visible_names}")))
}

/// Resolve a requested name to a registered harness-dispatchable internal
/// (model-hidden) tool.
///
/// Returns the registration name only when the canonicalized name is in
/// [`HARNESS_DISPATCHABLE_INTERNAL_TOOLS`] AND actually registered in the
/// inventory. This never exposes the tool publicly; it merely lets the harness
/// dispatch the intentionally model-hidden file helpers. Any other
/// `with_llm_visibility(false)` helper (e.g. diagnostics-only tools) is NOT
/// reachable through this path, so registering a new hidden tool cannot silently
/// widen the harness-dispatchable surface.
fn resolve_internal_dispatch_tool(registry: &ToolRegistry, name: &str) -> Option<String> {
    super::assembly::public_tool_name_candidates(name)
        .iter()
        .map(|candidate| candidate.trim())
        .filter(|candidate| !candidate.is_empty())
        .find(|candidate| {
            HARNESS_DISPATCHABLE_INTERNAL_TOOLS
                .iter()
                .any(|allowed| allowed.eq_ignore_ascii_case(candidate))
        })
        .and_then(|candidate| {
            let candidate = candidate.to_ascii_lowercase();
            registry
                .inventory
                .registration_for(&candidate)
                .map(|registration| registration.name().to_string())
        })
}

pub(super) fn preflight_validate_resolved_call(
    registry: &ToolRegistry,
    normalized_tool_name: &str,
    args: &Value,
) -> Result<ToolPreflightOutcome> {
    let mut routed_tool_name = normalized_tool_name.to_string();
    let mut validation_tool_name = routed_tool_name.clone();
    let parameter_schema = registry
        .inventory
        .registration_for(normalized_tool_name)
        .and_then(|registration| registration.parameter_schema().cloned());
    let mut validation_args = normalize_tool_args(normalized_tool_name, args, parameter_schema.as_ref())?;
    let mut effective_args = None;
    // Schema for the tool we ultimately validate against. Defaults to the
    // originally-resolved tool's schema; each remap branch overwrites it with
    // the exec schema it already fetches, so we never re-look-up the same
    // registration (the previous form fetched the same schema up to 3x).
    let mut effective_parameter_schema = parameter_schema;

    crate::tools::output_limits::max_output_tokens(validation_args.as_ref())
        .map_err(|error| anyhow!("Invalid arguments for tool '{routed_tool_name}': {error}"))?;

    if let Some(exec_args) = public_exec_validation_args(normalized_tool_name, validation_args.as_ref())? {
        validation_tool_name = tool_names::UNIFIED_EXEC.to_string();
        validation_args = std::borrow::Cow::Owned(exec_args);
        effective_args = Some(validation_args.as_ref().clone());
        effective_parameter_schema = registry
            .inventory
            .registration_for(&validation_tool_name)
            .and_then(|registration| registration.parameter_schema().cloned());
    } else if normalized_tool_name == tool_names::UNIFIED_FILE
        && let Some(remapped_args) =
            crate::tools::tool_intent::remap_file_operation_command_args_to_command_session(validation_args.as_ref())
    {
        routed_tool_name = tool_names::UNIFIED_EXEC.to_string();
        validation_tool_name = tool_names::UNIFIED_EXEC.to_string();
        effective_parameter_schema = registry
            .inventory
            .registration_for(&validation_tool_name)
            .and_then(|registration| registration.parameter_schema().cloned());
        validation_args = std::borrow::Cow::Owned(
            normalize_tool_args(&validation_tool_name, &remapped_args, effective_parameter_schema.as_ref())?
                .into_owned(),
        );
        effective_args = Some(validation_args.as_ref().clone());
    }

    if validation_tool_name == tool_names::TASK_TRACKER {
        effective_parameter_schema =
            Some(crate::tools::handlers::task_tracker::task_tracker_parameter_schema_for_workflow(
                registry.is_planning_active(),
            ));
        validation_args = std::borrow::Cow::Owned(
            normalize_tool_args(&validation_tool_name, validation_args.as_ref(), effective_parameter_schema.as_ref())?
                .into_owned(),
        );
        if effective_args.is_some() {
            effective_args = Some(validation_args.as_ref().clone());
        }
    }

    let required = required_args_for_tool(&validation_tool_name);
    let mut failures = Vec::with_capacity(required.len());
    for key in required {
        if is_missing_required_arg(&validation_tool_name, validation_args.as_ref(), key) {
            failures.push(if validation_tool_name == tool_names::APPLY_PATCH {
                crate::tools::apply_patch::APPLY_PATCH_ARGUMENT_CORRECTION.to_string()
            } else {
                missing_required_arg_failure(key)
            });
        }
    }
    if validation_tool_name == tool_names::UNIFIED_EXEC {
        failures.extend(
            crate::tools::command_args::command_session_missing_required_args(validation_args.as_ref())
                .into_iter()
                .map(missing_required_arg_failure),
        );
    }

    if let Some(path) = PATH_ALIAS_KEYS
        .iter()
        .find_map(|key| validation_args.as_ref().get(*key).and_then(Value::as_str))
        && let Err(err) = paths::validate_path_safety(path)
    {
        failures.push(format!("Path security check failed: {err}"));
    }

    let should_validate_command = matches!(
        validation_tool_name.as_str(),
        tool_names::RUN_PTY_CMD | tool_names::CREATE_PTY_SESSION | tool_names::SHELL
    ) || (validation_tool_name == tool_names::UNIFIED_EXEC
        && crate::tools::command_args::command_session_requires_command_safety(validation_args.as_ref()));
    if should_validate_command {
        let command_value = crate::tools::command_args::normalized_command_value(validation_args.as_ref())
            .ok()
            .flatten();
        let collision_command = match &command_value {
            Some(Value::String(command)) => Some(command.as_str()),
            Some(Value::Array(command)) => command.first().and_then(Value::as_str),
            _ => None,
        };
        if collision_command.is_some_and(crate::tools::names::is_apply_patch_shell_collision_command) {
            failures.push(format!(
                "apply_patch is a tool, not a shell executable. {}",
                crate::tools::apply_patch::APPLY_PATCH_ARGUMENT_CORRECTION
            ));
        }
        let validation_result = match command_value {
            Some(Value::Array(_)) => crate::tools::command_args::command_words(validation_args.as_ref())
                .ok()
                .flatten()
                .map_or(Ok(()), |command| commands::validate_command_argv(&command)),
            Some(Value::String(command)) => commands::validate_shell_script(&command),
            _ => Ok(()),
        };
        if let Err(err) = validation_result {
            failures.push(format!("Command security check failed: {err}"));
        }
    }
    enforce_file_operation_payload_limit(
        &validation_tool_name,
        validation_args.as_ref(),
        configured_file_operation_max_payload_bytes(),
        &mut failures,
    );

    if !failures.is_empty() {
        return Err(anyhow!("Tool preflight validation failed for '{}': {}", routed_tool_name, failures.join("; ")));
    }

    if validation_tool_name == tool_names::UNIFIED_EXEC
        && crate::tools::tool_intent::command_session_action(validation_args.as_ref()).is_none()
    {
        return Err(anyhow!(
            "Invalid arguments for tool '{routed_tool_name}': missing action; provide `action` or inferable exec arguments"
        ));
    }
    let schema_validation_args = crate::tools::output_limits::args_without_output_metadata(validation_args.as_ref());
    if let Some(schema) = effective_parameter_schema.as_ref() {
        let error_msg = match jsonschema::validator_for(schema) {
            Ok(validator) => validator
                .iter_errors(&schema_validation_args)
                .map(|error| crate::tools::validation::describe_jsonschema_error(&error))
                .collect::<Vec<_>>()
                .join("; "),
            Err(schema_error) => crate::tools::validation::describe_jsonschema_error(&schema_error),
        };
        if !error_msg.is_empty() {
            let hint_msg = condensed_schema_hint(schema)
                .map(|hint| format!("\nExpected schema (required fields and types): {hint}"))
                .unwrap_or_default();
            let patch_hint = if validation_tool_name == tool_names::APPLY_PATCH {
                crate::tools::apply_patch::APPLY_PATCH_ARGUMENT_CORRECTION
            } else if validation_tool_name == tool_names::TASK_TRACKER
                && crate::tools::handlers::task_tracker::is_task_tracker_shape_error(&error_msg)
            {
                crate::tools::handlers::task_tracker::TASK_TRACKER_ARGUMENT_CORRECTION
            } else {
                ""
            };
            return Err(anyhow!("Invalid arguments for tool '{routed_tool_name}': {error_msg}{hint_msg} {patch_hint}"));
        }
    }
    if validation_tool_name == tool_names::CODE_SEARCH {
        crate::tools::code_search::validate_args(&schema_validation_args)
            .map_err(|error| anyhow!("Invalid arguments for tool '{routed_tool_name}': {error}"))?;
    }

    let intent = crate::tools::tool_intent::classify_tool_intent(&validation_tool_name, validation_args.as_ref());
    let readonly_classification = !intent.mutating;
    if registry.is_planning_active()
        && !registry.is_planning_active_allowed_with_intent(&validation_tool_name, validation_args.as_ref(), &intent)
    {
        let msg = agent_execution::planning_workflow_denial_message(&routed_tool_name);
        return Err(anyhow!(msg).context(agent_execution::PLANNING_DENIED_CONTEXT));
    }

    Ok(ToolPreflightOutcome {
        normalized_tool_name: routed_tool_name.clone(),
        readonly_classification,
        parallel_safe_after_preflight: crate::tools::tool_intent::is_parallel_safe_call_with_intent(
            &validation_tool_name,
            validation_args.as_ref(),
            &intent,
        ),
        effective_args: effective_args.unwrap_or_else(|| validation_args.into_owned()),
        intent,
    })
}

#[cfg(test)]
mod tests {
    use super::super::ToolExecutionRequest;
    use super::super::assembly::public_tool_name_candidates;
    use super::{
        DispatchMode, ToolRegistry, coerce_string_to_schema_type_in_place, configured_file_operation_max_payload_bytes,
        enforce_file_operation_payload_limit, is_missing_required_arg, normalize_tool_args,
        parse_file_operation_max_payload_bytes, parse_string_as_schema_type, preflight_validate_call,
        preflight_validate_resolved_call, public_exec_validation_args, resolve_dispatch_target,
        resolve_internal_dispatch_tool,
    };
    use crate::config::constants::tools as tool_names;
    use crate::tools::command_args::parse_indexed_command_parts;
    use crate::tools::request_user_input::RequestUserInputTool;
    use crate::tools::traits::Tool;
    use anyhow::Result;
    use serde_json::{Value, json};

    async fn new_test_registry() -> (tempfile::TempDir, ToolRegistry) {
        let temp = tempfile::tempdir().expect("temp workspace");
        let registry = ToolRegistry::new(temp.path().to_path_buf()).await;
        (temp, registry)
    }

    #[tokio::test]
    async fn internal_dispatch_preserves_normalization_and_candidate_order() {
        let (_temp, registry) = new_test_registry().await;
        for name in [
            "read_file",
            "  ReAd_FiLe  ",
            "functions.ReAd_FiLe",
            "tools.read-file",
            "READ FILE",
            "unknown:READ_FILE",
        ] {
            assert_eq!(
                resolve_internal_dispatch_tool(&registry, name).as_deref(),
                Some(tool_names::READ_FILE),
                "{name}"
            );
            assert_eq!(resolve_dispatch_target(&registry, name, DispatchMode::Harness).unwrap(), tool_names::READ_FILE);
            assert!(resolve_dispatch_target(&registry, name, DispatchMode::ModelPublic).is_err(), "{name}");
        }
        for name in [tool_names::WRITE_FILE, tool_names::EDIT_FILE, tool_names::LIST_FILES] {
            assert_eq!(resolve_internal_dispatch_tool(&registry, name).as_deref(), Some(name));
        }
        assert_eq!(
            resolve_internal_dispatch_tool(&registry, "read_file<|channel|>WRITE_FILE").as_deref(),
            Some(tool_names::WRITE_FILE)
        );
        assert_eq!(
            resolve_internal_dispatch_tool(&registry, "read_file<|channel|>write_file:list_files").as_deref(),
            Some(tool_names::READ_FILE)
        );
    }

    #[tokio::test]
    async fn internal_dispatch_rejects_hidden_tools_outside_allowlist_and_near_misses() {
        let (_temp, registry) = new_test_registry().await;
        assert!(registry.inventory.registration_for(tool_names::GET_ERRORS).is_some());
        for name in [
            "",
            " \t\u{2003}",
            "GET_ERRORS",
            "functions.get_errors",
            "read_files",
            "repo_browser.read_file",
            "READ_FİLE",
        ] {
            assert!(resolve_internal_dispatch_tool(&registry, name).is_none(), "{name:?}");
            assert!(resolve_dispatch_target(&registry, name, DispatchMode::Harness).is_err(), "{name:?}");
            assert!(resolve_dispatch_target(&registry, name, DispatchMode::ModelPublic).is_err(), "{name:?}");
        }
    }

    #[test]
    fn normalize_elides_truncation_only_piped_verifier() {
        // The pipeline exit status belongs to the truncator, so the piped
        // form must never reach validation or execution: the standalone
        // verifier runs instead and its status is truthful.
        let args = json!({"action": "run", "command": "cargo check --locked 2>&1 | head -c 4000"});
        let normalized =
            normalize_tool_args(tool_names::UNIFIED_EXEC, &args, None).expect("rewrite must not fail normalization");
        assert_eq!(normalized.as_ref()["command"], "cargo check --locked 2>&1");
        // The caller's `cmd` spelling is left intact as the typed record.
        assert_eq!(normalized.as_ref()["cmd"], Value::Null);

        let via_cmd = json!({"action": "run", "cmd": "cargo nextest run 2>&1 | tail -15"});
        let via_cmd =
            normalize_tool_args(tool_names::UNIFIED_EXEC, &via_cmd, None).expect("rewrite must not fail normalization");
        assert_eq!(via_cmd.as_ref()["command"], "cargo nextest run 2>&1");
        assert_eq!(via_cmd.as_ref()["cmd"], "cargo nextest run 2>&1 | tail -15");
    }

    #[test]
    fn normalize_leaves_non_rewritable_pipelines_untouched() {
        for command in [
            "cargo check | grep error; true",
            "cargo check && rm -rf target | tail -5",
            "cargo check | tail -5; rm foo.txt",
            "rg -n 'pattern' src | head -20",
            "cargo check --locked",
        ] {
            let args = json!({"action": "run", "command": command});
            let normalized =
                normalize_tool_args(tool_names::UNIFIED_EXEC, &args, None).expect("normalization must succeed");
            assert_eq!(
                normalized.as_ref()["command"],
                Value::String(command.to_string()),
                "must run as typed: {command}"
            );
        }
    }

    #[test]
    fn exec_command_validation_path_rewrites_piped_verifier() {
        let args = json!({"cmd": "cargo check --locked | tail -8"});
        let Some(exec_args) =
            public_exec_validation_args(tool_names::EXEC_COMMAND, &args).expect("validation args must build")
        else {
            panic!("EXEC_COMMAND run must produce exec validation args");
        };
        assert_eq!(exec_args["command"], "cargo check --locked");
    }

    #[tokio::test]
    async fn preflight_accepts_output_limit_for_legacy_strict_schemas() {
        let (_temp, registry) = new_test_registry().await;
        let outcome = preflight_validate_call(
            &registry,
            tool_names::CODE_SEARCH,
            &json!({"query": "ToolRegistry", "max_output_tokens": 37}),
        )
        .expect("a valid integer output limit should pass preflight");
        assert_eq!(outcome.effective_args["max_output_tokens"], 37);
    }

    #[tokio::test]
    async fn preflight_uses_default_output_limit_when_omitted() {
        let (_temp, registry) = new_test_registry().await;
        let outcome = preflight_validate_call(&registry, tool_names::CODE_SEARCH, &json!({"query": "ToolRegistry"}))
            .expect("omitted output limits should remain dispatchable");

        assert_eq!(
            crate::tools::output_limits::max_output_tokens(&outcome.effective_args)
                .expect("default output limit should be valid"),
            vtcode_utility_tool_specs::DEFAULT_MAX_OUTPUT_TOKENS
        );
    }

    #[tokio::test]
    async fn preflight_rejects_invalid_output_limits_before_dispatch() {
        let (_temp, registry) = new_test_registry().await;
        let error = preflight_validate_call(
            &registry,
            tool_names::CODE_SEARCH,
            &json!({"query": "ToolRegistry", "max_output_tokens": "37"}),
        )
        .expect_err("string output limits must be rejected");
        assert!(error.to_string().contains("max_output_tokens must be an integer"));
    }

    // --- schema-aware string-type coercion -------------------------------------
    //
    // Regression coverage for the checkpoint-observed defect where the model
    // emitted JSON-encoded strings (`"[\"path\"]"`, `"10"`) for array/integer
    // fields and retried the same malformed call six times in one turn.

    #[test]
    fn parse_string_as_schema_type_coerces_array_and_integer() {
        assert_eq!(parse_string_as_schema_type(r#"["path"]"#, "array"), Some(json!(["path"])));
        assert_eq!(parse_string_as_schema_type("10", "integer"), Some(json!(10)));
    }

    #[test]
    fn parse_string_as_schema_type_coerces_boolean_object_and_number() {
        assert_eq!(parse_string_as_schema_type("true", "boolean"), Some(json!(true)));
        assert_eq!(parse_string_as_schema_type("false", "boolean"), Some(json!(false)));
        assert_eq!(parse_string_as_schema_type(r#"{"a": 1}"#, "object"), Some(json!({"a": 1})));
        assert_eq!(parse_string_as_schema_type("3.5", "number"), Some(json!(3.5)));
        // An integer literal is also a valid JSON number.
        assert_eq!(parse_string_as_schema_type("3", "number"), Some(json!(3)));
    }

    #[test]
    fn parse_string_as_schema_type_rejects_float_for_integer() {
        // "10.0" must not be silently coerced to an integer: that would be a
        // lossy reinterpretation. Let strict validation surface the type error.
        assert_eq!(parse_string_as_schema_type("10.0", "integer"), None);
    }

    #[test]
    fn parse_string_as_schema_type_rejects_mismatched_and_malformed() {
        // Parsed type does not match schema type.
        assert_eq!(parse_string_as_schema_type(r#"{"a": 1}"#, "array"), None);
        assert_eq!(parse_string_as_schema_type(r#"["path"]"#, "object"), None);
        assert_eq!(parse_string_as_schema_type("10", "boolean"), None);
        // Malformed JSON stays a string and is left for strict validation.
        assert_eq!(parse_string_as_schema_type("not json", "array"), None);
        assert_eq!(parse_string_as_schema_type("[\"path\"", "array"), None);
        // Unknown schema type is never coerced.
        assert_eq!(parse_string_as_schema_type("anything", "string"), None);
    }

    #[test]
    fn coerce_string_to_schema_type_in_place_only_touches_strings() {
        let mut already_array = json!(["path"]);
        let schema = json!({"type": "array"});
        assert!(!coerce_string_to_schema_type_in_place(&mut already_array, &schema));
        assert_eq!(already_array, json!(["path"]));

        let mut no_type = json!("keep me");
        let schema = json!({}); // no declared type → free-form string
        assert!(!coerce_string_to_schema_type_in_place(&mut no_type, &schema));
        assert_eq!(no_type, json!("keep me"));
    }

    #[test]
    fn normalize_tool_args_coerces_code_search_stringified_args() {
        // Mirrors the exact checkpoint defect: result_types and max_results
        // arrive as JSON-encoded strings.
        let schema = json!({
            "type": "object",
            "properties": {
                "query": {"type": "string"},
                "result_types": {"type": "array", "items": {"type": "string", "enum": ["definition", "usage", "text", "path"]}},
                "max_results": {"type": "integer", "minimum": 1, "maximum": 100}
            }
        });
        let args = json!({
            "query": "turn_loop",
            "result_types": "[\"path\"]",
            "max_results": "10"
        });
        let normalized = normalize_tool_args(tool_names::CODE_SEARCH, &args, Some(&schema))
            .expect("code_search stringified args normalize");
        assert_eq!(normalized["result_types"], json!(["path"]));
        assert_eq!(normalized["max_results"], json!(10));
        assert_eq!(normalized["query"], json!("turn_loop"));
    }

    #[tokio::test]
    async fn preflight_coerces_stringified_code_search_args_then_validates() {
        let (_temp, registry) = new_test_registry().await;
        // The exact malformed shape from the checkpoint should now pass preflight.
        let outcome = preflight_validate_call(
            &registry,
            tool_names::CODE_SEARCH,
            &json!({"query": "turn_loop", "result_types": "[\"path\"]", "max_results": "10"}),
        )
        .expect("stringified args should be coerced and pass preflight");
        assert_eq!(outcome.effective_args["result_types"], json!(["path"]));
        assert_eq!(outcome.effective_args["max_results"], json!(10));
    }

    #[tokio::test]
    async fn preflight_coercion_still_enforces_bounds_and_enum() {
        let (_temp, registry) = new_test_registry().await;
        // Coerced to integer 999, then the max=100 bound must still reject it.
        let over_max = preflight_validate_call(
            &registry,
            tool_names::CODE_SEARCH,
            &json!({"query": "turn_loop", "max_results": "999"}),
        )
        .expect_err("coerced integer must still be bounds-checked");
        assert!(over_max.to_string().contains("max_results"), "msg: {over_max}");

        // Coerced to array ["bogus"], then the item enum must still reject it.
        let bad_enum = preflight_validate_call(
            &registry,
            tool_names::CODE_SEARCH,
            &json!({"query": "turn_loop", "result_types": "[\"bogus\"]"}),
        )
        .expect_err("coerced array items must still be enum-checked");
        assert!(bad_enum.to_string().contains("result_types"), "msg: {bad_enum}");
    }

    #[tokio::test]
    async fn preflight_does_not_coerce_max_output_tokens_string() {
        // The output-budget field has a dedicated strict validator that must keep
        // rejecting strings even though schema-aware coercion exists for other
        // integer fields.
        let (_temp, registry) = new_test_registry().await;
        let error = preflight_validate_call(
            &registry,
            tool_names::CODE_SEARCH,
            &json!({"query": "x", "max_output_tokens": "100"}),
        )
        .expect_err("max_output_tokens string must remain rejected");
        assert!(error.to_string().contains("max_output_tokens must be an integer"));
    }
    #[test]
    fn patch_action_within_limit_is_allowed() {
        let mut failures = Vec::new();
        let args = json!({
            "action": "patch",
            "patch": "*** Begin Patch\n*** End Patch\n"
        });

        enforce_file_operation_payload_limit(tool_names::UNIFIED_FILE, &args, 1024, &mut failures);
        assert!(failures.is_empty());
    }
    #[test]
    fn patch_action_over_limit_is_rejected() {
        let mut failures = Vec::new();
        let args = json!({
            "action": "patch",
            "patch": "x".repeat(512)
        });

        enforce_file_operation_payload_limit(tool_names::UNIFIED_FILE, &args, 128, &mut failures);
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

        enforce_file_operation_payload_limit(tool_names::EDIT_FILE, &args, 128, &mut failures);
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

        enforce_file_operation_payload_limit(tool_names::UNIFIED_FILE, &args, 1, &mut failures);
        assert!(failures.is_empty());
    }

    #[test]
    fn edit_file_required_args_accept_legacy_key_names() {
        let args = json!({
            "path": "file.txt",
            "old_string": "old",
            "new_string": "new"
        });

        assert!(!is_missing_required_arg(tool_names::EDIT_FILE, &args, "path"));
        assert!(!is_missing_required_arg(tool_names::EDIT_FILE, &args, "old_str"));
        assert!(!is_missing_required_arg(tool_names::EDIT_FILE, &args, "new_str"));
    }

    #[test]
    fn parse_payload_limit_accepts_safe_override() {
        let parsed = parse_file_operation_max_payload_bytes(Some("2048"));
        assert_eq!(parsed, Some(2048));
    }

    #[test]
    fn parse_payload_limit_rejects_too_small_values() {
        let parsed = parse_file_operation_max_payload_bytes(Some("512"));
        assert_eq!(parsed, None);
    }

    #[test]
    fn parse_payload_limit_rejects_invalid_values() {
        let parsed = parse_file_operation_max_payload_bytes(Some("not-a-number"));
        assert_eq!(parsed, None);
    }

    #[test]
    fn configured_payload_limit_is_always_safe() {
        let configured = configured_file_operation_max_payload_bytes();
        assert!(configured >= 1024);
    }

    #[test]
    fn apply_patch_required_arg_accepts_input_alias() {
        assert!(!is_missing_required_arg(tool_names::APPLY_PATCH, &json!({"input": ""}), "input"));
    }

    #[test]
    fn apply_patch_required_arg_accepts_raw_string_payload() {
        assert!(!is_missing_required_arg(tool_names::APPLY_PATCH, &json!(""), "input"));
    }

    #[test]
    fn run_pty_cmd_required_arg_accepts_zero_based_indexed_command() -> Result<()> {
        let input = json!({
            "command.0": "ls",
            "command.1": "-a"
        });
        let args = normalize_tool_args(tool_names::RUN_PTY_CMD, &input, None)?;

        assert!(!is_missing_required_arg(tool_names::RUN_PTY_CMD, args.as_ref(), "command"));
        assert_eq!(args.get("command").and_then(|value| value.as_str()), Some("ls -a"));
        Ok(())
    }

    #[test]
    fn run_pty_cmd_required_arg_accepts_one_based_indexed_command() -> Result<()> {
        let input = json!({
            "command.1": "ls",
            "command.2": "-a"
        });
        let args = normalize_tool_args(tool_names::RUN_PTY_CMD, &input, None)?;

        assert!(!is_missing_required_arg(tool_names::RUN_PTY_CMD, args.as_ref(), "command"));
        assert_eq!(args.get("command").and_then(|value| value.as_str()), Some("ls -a"));
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
    fn request_user_input_args_accept_details_alias() -> Result<()> {
        let schema = RequestUserInputTool.parameter_schema().expect("request_user_input schema");
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
        assert_eq!(option.get("description").and_then(Value::as_str), Some("Ship the smallest viable slice."));
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
        assert_eq!(normalized.get("description").and_then(Value::as_str), Some("Add regression coverage"));
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
        assert_eq!(normalized.get("details").and_then(Value::as_str), Some("Keep the real details field."));
        Ok(())
    }

    #[tokio::test]
    async fn command_session_preflight_rejects_run_without_command() {
        let (_temp, registry) = new_test_registry().await;

        let err = preflight_validate_resolved_call(&registry, tool_names::UNIFIED_EXEC, &json!({"action": "run"}))
            .expect_err("missing command should fail preflight");

        assert!(err.to_string().contains("Missing required argument: command"));
    }

    #[tokio::test]
    async fn exec_command_preflight_preserves_public_name_and_validates_as_run() -> Result<()> {
        let (_temp, registry) = new_test_registry().await;

        let result = preflight_validate_call(
            &registry,
            tool_names::EXEC_COMMAND,
            &json!({"cmd": "rg --files", "workdir": ".", "tty": true}),
        )?;

        assert_eq!(result.normalized_tool_name, tool_names::EXEC_COMMAND);
        assert_eq!(result.effective_args["action"], "run");
        assert_eq!(result.effective_args["command"], "rg --files");
        assert_eq!(result.effective_args["workdir"], ".");
        assert_eq!(result.effective_args["tty"], true);
        assert!(result.readonly_classification);
        Ok(())
    }

    #[tokio::test]
    async fn planning_preflight_rejects_mutating_suffixes_and_conflicting_raw_aliases() {
        let (_temp, registry) = new_test_registry().await;
        registry.enable_planning();
        for args in [
            json!({"cmd":"cargo check | sort", "args":["-o", "changed.txt"]}),
            json!({"cmd":"cat README.md", "raw_command":"printf changed > changed.txt"}),
        ] {
            let error = preflight_validate_call(&registry, tool_names::EXEC_COMMAND, &args)
                .expect_err("mutating invocation cannot acquire planning admission");
            assert_eq!(error.to_string(), super::agent_execution::PLANNING_DENIED_CONTEXT);
        }
        let safe =
            preflight_validate_call(&registry, tool_names::EXEC_COMMAND, &json!({"cmd":"sort", "args":["README.md"]}))
                .expect("safe appended operands remain readable in planning");
        assert!(safe.readonly_classification);
        assert!(safe.parallel_safe_after_preflight);
    }

    #[tokio::test]
    async fn planning_preflight_allows_checkpoint_style_readonly_inspection() -> Result<()> {
        let (_temp, registry) = new_test_registry().await;
        registry.enable_planning();
        let command = r#"sed -n '180,285p' src/main.rs; sed -n '60,285p' src/startup/mod.rs; sed -n '1,220p' src/main_helpers/bootstrap.rs; rg -n "\[profile|lto|codegen-units|strip" Cargo.toml"#;

        let result = preflight_validate_call(
            &registry,
            tool_names::EXEC_COMMAND,
            &json!({
                "cmd": command,
                "workdir": ".",
                "yield_time_ms": 10000,
                "max_output_tokens": 30000
            }),
        )?;

        assert!(result.readonly_classification);
        assert_eq!(result.effective_args["action"], "run");
        Ok(())
    }

    #[tokio::test]
    async fn exec_command_preflight_rejects_dangerous_command() {
        let (_temp, registry) = new_test_registry().await;

        let err =
            preflight_validate_call(&registry, tool_names::EXEC_COMMAND, &json!({"cmd": "git reset --hard HEAD~1"}))
                .expect_err("dangerous exec_command should fail preflight");

        let text = err.to_string();
        assert!(text.contains("Tool preflight validation failed for 'exec_command'"));
        assert!(text.contains("Command security check failed"));
    }

    #[tokio::test]
    async fn exec_command_approval_required_payload_is_seen_as_shell_run() -> Result<()> {
        let (_temp, registry) = new_test_registry().await;
        let args = json!({
            "cmd": "cargo check",
            "sandbox_permissions": "require_escalated",
            "justification": "Need unsandboxed access for this check."
        });

        let reason = registry
            .shell_run_approval_reason(tool_names::EXEC_COMMAND, Some(&args))
            .await?;

        assert!(
            reason
                .as_deref()
                .is_some_and(|text| text.contains("without sandbox restrictions"))
        );
        Ok(())
    }

    #[tokio::test]
    async fn direct_unsandboxed_exec_requires_operator_preapproval() {
        let (_temp, registry) = new_test_registry().await;
        let args = json!({
            "cmd": "printf guarded",
            "sandbox_permissions": "require_escalated",
            "justification": "Need unsandboxed access for this check."
        });

        let outcome = registry
            .execute_public_tool_request(ToolExecutionRequest::new(tool_names::EXEC_COMMAND, args))
            .await;
        let error = outcome.error.expect("direct escalation must be rejected");
        assert!(error.message.contains("requires an enforced operator approval decision"));
    }

    #[tokio::test]
    async fn write_stdin_preflight_uses_session_write_validation() -> Result<()> {
        let (_temp, registry) = new_test_registry().await;

        let result = preflight_validate_call(
            &registry,
            tool_names::WRITE_STDIN,
            &json!({
                "session_id": "run-1",
                "chars": "git reset --hard HEAD~1\n"
            }),
        )?;

        assert_eq!(result.normalized_tool_name, tool_names::WRITE_STDIN);
        assert_eq!(result.effective_args["action"], "write");
        assert_eq!(result.effective_args["input"], "git reset --hard HEAD~1\n");
        assert!(!result.readonly_classification);
        Ok(())
    }

    #[tokio::test]
    async fn write_stdin_preflight_uses_session_poll_validation() -> Result<()> {
        let (_temp, registry) = new_test_registry().await;

        let result = preflight_validate_call(
            &registry,
            tool_names::WRITE_STDIN,
            &json!({
                "session_id": "run-1",
                "chars": "",
                "yield_time_ms": 25,
                "max_output_tokens": 7
            }),
        )?;

        assert_eq!(result.normalized_tool_name, tool_names::WRITE_STDIN);
        assert_eq!(result.effective_args["action"], "poll");
        assert!(result.effective_args.get("input").is_none());
        assert_eq!(result.effective_args["yield_time_ms"], 25);
        assert_eq!(result.effective_args["max_output_tokens"], 7);
        assert!(result.readonly_classification);
        Ok(())
    }

    #[tokio::test]
    async fn write_stdin_poll_preflight_rejects_non_string_session_id() {
        let (_temp, registry) = new_test_registry().await;

        let error = preflight_validate_call(&registry, tool_names::WRITE_STDIN, &json!({"session_id": 1, "chars": ""}))
            .expect_err("non-string session id should fail preflight");

        assert!(error.to_string().contains("write_stdin"));
        assert!(error.to_string().contains("Missing required argument: session_id"));
    }

    #[tokio::test]
    async fn command_session_preflight_rejects_missing_action_without_inferable_args() {
        let (_temp, registry) = new_test_registry().await;

        let err = preflight_validate_resolved_call(&registry, tool_names::UNIFIED_EXEC, &json!({}))
            .expect_err("missing action should fail preflight");

        assert!(err.to_string().contains(&format!(
            "Invalid arguments for tool '{}': missing action; provide `action` or inferable exec arguments",
            tool_names::UNIFIED_EXEC
        )));
    }

    #[tokio::test]
    async fn command_session_preflight_rejects_write_without_input() {
        let (_temp, registry) = new_test_registry().await;

        let err = preflight_validate_resolved_call(
            &registry,
            tool_names::UNIFIED_EXEC,
            &json!({"action": "write", "session_id": "run-1"}),
        )
        .expect_err("missing input should fail preflight");

        assert!(err.to_string().contains("Missing required argument: input or chars or text"));
    }

    #[tokio::test]
    async fn command_session_preflight_rejects_poll_without_session_id() {
        let (_temp, registry) = new_test_registry().await;

        let err = preflight_validate_resolved_call(&registry, tool_names::UNIFIED_EXEC, &json!({"action": "poll"}))
            .expect_err("missing session_id should fail preflight");

        assert!(err.to_string().contains("Missing required argument: session_id"));
    }

    #[tokio::test]
    async fn command_session_preflight_accepts_list_without_extra_args() -> Result<()> {
        let (_temp, registry) = new_test_registry().await;

        let result = preflight_validate_resolved_call(&registry, tool_names::UNIFIED_EXEC, &json!({"action": "list"}))?;

        assert_eq!(result.normalized_tool_name, tool_names::UNIFIED_EXEC);
        Ok(())
    }

    #[tokio::test]
    async fn command_session_preflight_accepts_inspect_with_spool_path() -> Result<()> {
        let (_temp, registry) = new_test_registry().await;

        let result = preflight_validate_resolved_call(
            &registry,
            tool_names::UNIFIED_EXEC,
            &json!({"action": "inspect", "spool_path": ".vtcode/context/tool_outputs/out.log"}),
        )?;

        assert_eq!(result.normalized_tool_name, tool_names::UNIFIED_EXEC);
        Ok(())
    }

    #[tokio::test]
    async fn file_operation_command_payload_preflight_remaps_to_command_session() -> Result<()> {
        let (_temp, registry) = new_test_registry().await;

        let result = preflight_validate_resolved_call(
            &registry,
            tool_names::UNIFIED_FILE,
            &json!({
                "command": "echo vtcode",
                "cwd": ".",
            }),
        )?;

        assert_eq!(result.normalized_tool_name, tool_names::UNIFIED_EXEC);
        assert_eq!(result.effective_args["action"], "run");
        assert_eq!(result.effective_args["command"], "echo vtcode");
        assert_eq!(result.effective_args["cwd"], ".");
        Ok(())
    }
}
