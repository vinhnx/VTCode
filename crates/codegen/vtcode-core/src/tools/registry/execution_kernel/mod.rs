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
mod tests;
