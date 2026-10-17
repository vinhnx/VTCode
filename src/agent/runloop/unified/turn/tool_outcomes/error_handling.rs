use vtcode_commons::ErrorCategory;
use vtcode_core::config::constants::tools as tool_names;
use vtcode_core::tools::error_messages::agent_execution;
use vtcode_core::tools::registry::ToolExecutionError;

use super::helpers::check_is_argument_error;
use super::response_content::compact_model_tool_payload;

const MAX_ERROR_MESSAGE_CHARS: usize = 420;
const MAX_FALLBACK_ARGS_PREVIEW_CHARS: usize = 140;
const MAX_FALLBACK_ARGS_INLINE_CHARS: usize = 240;

#[cold]
pub(super) fn is_blocked_or_denied_failure(error: &str) -> bool {
    if agent_execution::is_planning_active_denial(error) {
        return true;
    }

    let lowered = error.to_ascii_lowercase();
    [
        "tool permission denied",
        "policy violation:",
        "safety validation failed",
        "tool argument validation failed",
        "not allowed in planning workflow",
        "only available when planning workflow state is active",
    ]
    .iter()
    .any(|marker| lowered.contains(marker))
}

/// Returns tool-specific diagnostic information when a critical tool is denied
/// by policy. This helps the LLM agent explain the issue to the user and
/// suggest concrete fixes instead of just reporting a generic denial.
pub(crate) fn tool_denial_diagnostic(tool_name: &str) -> Option<serde_json::Value> {
    match tool_name {
        tool_names::RUN_PTY_CMD | tool_names::UNIFIED_EXEC | tool_names::EXEC_COMMAND | "unified_exec" => {
            Some(serde_json::json!({
                "cause": format!("The '{tool_name}' tool (shell command execution) is denied by tool policy configuration."),
                "impact": "No shell commands can be executed, including: cargo build, cargo check, cargo test, cargo clippy, cargo fmt, and any other command-line tools.",
                "fix": {
                    "action": "Change the tool policy from 'deny' to 'allow' (or 'prompt') in the configuration files.",
                    "files": [
                    format!("vtcode.toml: [tools.policies] section - set '{tool_name}' to \"allow\""),
                    format!(".vtcode/tool-policy.json: policies object - set '{tool_name}' to \"allow\""),
                    "Recognized shell aliases: run_pty_cmd, unified_exec, exec_command; update the exact alias shown in this denial.".to_string()
                    ],
                "note": format!("Update that exact tool alias '{tool_name}' (recognized shell aliases: run_pty_cmd, unified_exec, exec_command), then restart the session so the policy snapshot is rebuilt. Configuration changes do not alter the current session policy snapshot.")
                }
            }))
        }
        // `request_user_input` is permanently unavailable in non-interactive
        // runtimes (e.g. headless sessions). Without this directive, the model
        // sees a generic "execution denied by policy" and retries the call
        // across turns — checkpoint turn_724 shows 7 retries. Stating that the
        // denial is permanent tells the model to proceed without asking the user.
        // The `present_plan` directive tells the model to surface the plan it
        // has gathered and offer a simple yes/no/edit HITL choice in plain text
        // — the user replies in the next turn and `detect_planning_intent`
        // routes `yes`/`implement` to implementation, `keep planning`/`edit` to
        // revision. This is the text-mode equivalent of the interactive modal.
        "request_user_input" => Some(serde_json::json!({
            "cause": "The 'request_user_input' tool is permanently unavailable in this runtime (non-interactive session or the inline UI is not supported).",
            "impact": "You cannot ask the user clarifying questions via a modal in this session.",
            "directive": "request_user_input is unavailable for the rest of this session; every further call returns the same denial.",
            "present_plan": "If you have a clarifying question, present it to the user in plain text and end your turn — the user's next message will answer it. Otherwise, finalize the plan from the evidence already gathered and present it to the user in plain text. End your message by offering a simple choice: type `yes` (or `implement`) to start implementation, `no` to abandon, or `edit` (or `keep planning`) to refine — if the user picks edit, they will describe what to revise in their next message."
        })),
        _ => None,
    }
}

pub(super) fn truncate_text_for_model(value: &str, max_chars: usize) -> (String, bool) {
    const MARKER: &str = " ... [truncated] ... ";
    vtcode_commons::formatting::head_tail_truncate(value, max_chars, MARKER)
}

fn compact_json_preview(serialized: &str, max_chars: usize) -> String {
    let (preview, _) = truncate_text_for_model(serialized, max_chars);
    preview
}

pub(super) fn serialize_json_for_model(value: &serde_json::Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "<invalid-json>".to_string())
}

fn should_inline_fallback_args(serialized_args: &str) -> bool {
    serialized_args.chars().count() <= MAX_FALLBACK_ARGS_INLINE_CHARS
}

fn push_fallback_args(
    payload: &mut serde_json::Value,
    args: serde_json::Value,
    args_preview: &str,
    inline_full_args: bool,
) {
    if let Some(obj) = payload.as_object_mut() {
        if inline_full_args {
            obj.insert("fallback_tool_args".to_string(), args);
        } else {
            obj.insert("fallback_tool_args_preview".to_string(), serde_json::Value::String(args_preview.to_string()));
            obj.insert("fallback_tool_args_truncated".to_string(), serde_json::Value::Bool(true));
        }
    }
}

fn push_error_truncation_flag(payload: &mut serde_json::Value, error_truncated: bool) {
    if !error_truncated {
        return;
    }
    if let Some(obj) = payload.as_object_mut() {
        obj.insert("error_truncated".to_string(), serde_json::Value::Bool(true));
    }
}

fn fallback_args_preview_and_inline(fallback_tool_args: &Option<serde_json::Value>) -> (Option<String>, bool) {
    let Some(args) = fallback_tool_args else {
        return (None, false);
    };
    let serialized = serialize_json_for_model(args);
    let preview = compact_json_preview(&serialized, MAX_FALLBACK_ARGS_PREVIEW_CHARS);
    (Some(preview), should_inline_fallback_args(&serialized))
}

/// Shared by the unstructured and structured `preview_exhaustion_gate` arms.
const PREVIEW_EXHAUSTION_NEXT_ACTION: &str = "Inspections return hidden stubs for the rest of this turn, so a retry at any scope returns another stub. \
Edit from visible evidence, run one verifier, page a spool in small ranges, or synthesize.";

fn failure_guidance(error_msg: &str, failure_kind: &'static str) -> (&'static str, bool, &'static str) {
    if failure_kind == "timeout" {
        return ("timeout", true, "Retry with smaller scope or higher timeout.");
    }

    if failure_kind == "repeated_read_family" {
        return (
            "repeated_read_family",
            false,
            "Further reads of this content are blocked for the rest of this turn; it is already in the conversation history. Continue from that output: edit, run a verifier, or answer.",
        );
    }

    if failure_kind == "repeated_read_path" {
        return (
            "repeated_read_path",
            false,
            "Further reads of this path are blocked for the rest of this turn. Reads of other paths, edits, and other useful actions remain available; continue from the evidence already gathered.",
        );
    }

    // Preview-budget exhaustion is budget-based, not scope-based: a narrower
    // retry returns another contentless stub, so the generic "narrower scope"
    // default only invites wasted retries that feed the blocked-call fuse
    // (observed: three consecutive gate rejections on distinct reads in one
    // execution turn). Keep the failure terminal for inspections; productive
    // channels (edits from visible evidence, one verifier, spool paging,
    // synthesis) are named instead. Error class and recoverability stay at
    // the execution defaults so no control-flow behavior changes.
    if failure_kind == "preview_exhaustion_gate" {
        return ("execution_failure", true, PREVIEW_EXHAUSTION_NEXT_ACTION);
    }

    if check_is_argument_error(error_msg) {
        return ("invalid_arguments", true, "Fix tool arguments to match the schema.");
    }

    if is_blocked_or_denied_failure(error_msg) {
        return ("policy_blocked", false, "Switch to an allowed tool or mode.");
    }

    ("execution_failure", true, "Try an alternative tool or narrower scope.")
}

fn structured_failure_guidance(
    error: &ToolExecutionError,
    failure_kind: &'static str,
) -> (&'static str, bool, &'static str) {
    if failure_kind == "timeout" || matches!(error.category, ErrorCategory::Timeout) {
        return ("timeout", true, "Retry with smaller scope or a higher timeout.");
    }

    // Mirror the unstructured `preview_exhaustion_gate` branch: budget-based,
    // so a narrower retry returns another stub. Keep `error.is_recoverable`
    // (the structured execution default) so control flow is unchanged; only
    // the retry-inviting wording is replaced. Currently unreachable via the
    // read-guard path (which uses `build_error_content`), kept for
    // defense-in-depth if a structured error ever carries this kind.
    if failure_kind == "preview_exhaustion_gate" {
        return ("execution_failure", error.is_recoverable, PREVIEW_EXHAUSTION_NEXT_ACTION);
    }

    if matches!(error.category, ErrorCategory::InvalidParameters) || check_is_argument_error(&error.message) {
        return ("invalid_arguments", true, "Fix the tool arguments to match the schema.");
    }

    if matches!(error.category, ErrorCategory::Authentication) {
        return ("authentication_failed", false, "Verify your credentials or choose a different provider.");
    }

    if matches!(
        error.category,
        ErrorCategory::PermissionDenied | ErrorCategory::PolicyViolation | ErrorCategory::PlanningPolicyViolation
    ) || is_blocked_or_denied_failure(&error.message)
    {
        return ("policy_blocked", false, "Switch to an allowed tool or mode.");
    }

    if matches!(error.category, ErrorCategory::CircuitOpen) {
        return ("service_temporarily_unavailable", true, "Wait briefly, then retry or use a different tool.");
    }

    ("execution_failure", error.is_recoverable, "Try an alternative tool or narrower scope.")
}

#[cold]
pub(super) fn format_structured_tool_error_for_user(
    tool_name: &str,
    error: &ToolExecutionError,
) -> (String, Option<String>) {
    let sanitized = crate::agent::runloop::unified::turn::turn_helpers::sanitize_error_for_display(&error.message);
    let mut primary = format!("Tool '{}' failed ({}): {}", tool_name, error.category.user_label(), sanitized);

    if error.rollback_performed {
        primary.push_str(" Any partial changes were rolled back.");
    } else if error.partial_state_possible {
        primary.push_str(" Partial changes may still exist.");
    }

    if let Some(retry_summary) = error.retry_summary() {
        primary.push(' ');
        primary.push_str(&retry_summary);
    }

    let hint = if error.recovery_suggestions.is_empty() {
        None
    } else {
        Some(format!(
            "Hint: {}",
            error
                .recovery_suggestions
                .iter()
                .map(|suggestion| suggestion.as_ref())
                .collect::<Vec<_>>()
                .join("; ")
        ))
    };

    (primary, hint)
}

#[cold]
pub(crate) fn build_error_content(
    error_msg: String,
    fallback_tool: Option<String>,
    fallback_tool_args: Option<serde_json::Value>,
    failure_kind: &'static str,
) -> serde_json::Value {
    let (error_text, error_truncated) = truncate_text_for_model(&error_msg, MAX_ERROR_MESSAGE_CHARS);
    let (error_class, is_recoverable, next_action) = failure_guidance(&error_msg, failure_kind);
    let (args_preview, inline_full_args) = fallback_args_preview_and_inline(&fallback_tool_args);

    if let Some(tool) = fallback_tool {
        let mut payload = serde_json::json!({
            "error": error_text,
            "failure_kind": failure_kind,
            "error_class": error_class,
            "is_recoverable": is_recoverable,
            "next_action": next_action,
            "fallback_tool": tool,
        });
        push_error_truncation_flag(&mut payload, error_truncated);
        if let Some(args) = fallback_tool_args {
            push_fallback_args(
                &mut payload,
                args,
                args_preview.as_deref().unwrap_or("<invalid-json>"),
                inline_full_args,
            );
        }
        compact_model_tool_payload(payload)
    } else {
        let mut payload = serde_json::json!({
            "error": error_text,
            "failure_kind": failure_kind,
            "error_class": error_class,
            "is_recoverable": is_recoverable,
            "next_action": next_action,
        });
        push_error_truncation_flag(&mut payload, error_truncated);
        compact_model_tool_payload(payload)
    }
}

#[cold]
pub(super) fn build_structured_error_content(
    error: &ToolExecutionError,
    fallback_tool: Option<String>,
    fallback_tool_args: Option<serde_json::Value>,
    failure_kind: &'static str,
) -> serde_json::Value {
    let (error_summary, error_truncated) = truncate_text_for_model(&error.user_message(), MAX_ERROR_MESSAGE_CHARS);
    let (error_class, is_recoverable, default_next_action) = structured_failure_guidance(error, failure_kind);
    let (args_preview, inline_full_args) = fallback_args_preview_and_inline(&fallback_tool_args);
    let retry_summary = error.retry_summary();
    let next_action = error
        .recovery_suggestions
        .first()
        .map(|suggestion| suggestion.as_ref())
        .unwrap_or(default_next_action);
    let mut payload = error.to_json_value();

    if let Some(obj) = payload.as_object_mut() {
        obj.insert("error_summary".to_string(), serde_json::Value::String(error_summary));
        obj.insert("failure_kind".to_string(), serde_json::Value::String(failure_kind.to_string()));
        obj.insert("error_class".to_string(), serde_json::Value::String(error_class.to_string()));
        obj.insert("category".to_string(), serde_json::json!(error.category));
        obj.insert("is_recoverable".to_string(), serde_json::Value::Bool(is_recoverable));
        obj.insert("retryable".to_string(), serde_json::Value::Bool(error.retryable));
        obj.insert("partial_state_possible".to_string(), serde_json::Value::Bool(error.partial_state_possible));
        obj.insert("rollback_performed".to_string(), serde_json::Value::Bool(error.rollback_performed));
        obj.insert("circuit_breaker_impact".to_string(), serde_json::Value::Bool(error.circuit_breaker_impact));
        if is_recoverable {
            obj.insert("next_action".to_string(), serde_json::Value::String(next_action.to_string()));
        }
        if !error.recovery_suggestions.is_empty() {
            obj.insert(
                "recovery_suggestions".to_string(),
                serde_json::json!(
                    error
                        .recovery_suggestions
                        .iter()
                        .map(|suggestion| suggestion.as_ref())
                        .collect::<Vec<_>>()
                ),
            );
        }
        if let Some(summary) = retry_summary {
            obj.insert("retry_summary".to_string(), serde_json::Value::String(summary));
        }
        if let Some(retry_delay_ms) = error.retry_delay_ms {
            obj.insert("retry_delay_ms".to_string(), serde_json::Value::Number(retry_delay_ms.into()));
        }
        if let Some(retry_after_ms) = error.retry_after_ms {
            obj.insert("retry_after_ms".to_string(), serde_json::Value::Number(retry_after_ms.into()));
        }
        if let Some(debug_context) = &error.debug_context {
            obj.insert(
                "debug_context".to_string(),
                serde_json::to_value(debug_context).unwrap_or(serde_json::Value::Null),
            );
        }
        // Policy/permission denials collapse to one generic sentence, which
        // leaves the model guessing between retrying, switching tools, or
        // ending the turn (checkpoint turn_724: 7 retries). Attach the
        // tool-specific denial diagnostic — the same actionable fix the
        // user-facing preflight prompt receives — so the model can act on
        // the denial instead of guessing.
        if error_class == "policy_blocked"
            && let Some(diagnostic) = tool_denial_diagnostic(&error.tool_name)
        {
            obj.insert("diagnostic".to_string(), diagnostic);
        }
    }

    if let Some(tool) = fallback_tool {
        if let Some(obj) = payload.as_object_mut() {
            obj.insert("fallback_tool".to_string(), serde_json::Value::String(tool));
        }
        if let Some(args) = fallback_tool_args {
            push_fallback_args(
                &mut payload,
                args,
                args_preview.as_deref().unwrap_or("<invalid-json>"),
                inline_full_args,
            );
        }
    }

    // Typed patch evidence is already bounded. Keep a single model-facing
    // message and next action; canonical execution events retain the full error.
    if error.patch_context_mismatch_path().is_some()
        && let Some(obj) = payload.as_object_mut()
    {
        obj.remove("error_summary");
        obj.remove("recovery_suggestions");
        if let Some(details) = obj.get_mut("error").and_then(serde_json::Value::as_object_mut) {
            details.remove("original_error");
            details.remove("recovery_suggestions");
        }
    }
    push_error_truncation_flag(&mut payload, error_truncated);
    compact_model_tool_payload(payload)
}

fn is_valid_pty_session_id(session_id: &str) -> bool {
    !session_id.is_empty()
        && session_id.len() <= 128
        && session_id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn extract_pty_session_id_from_error(error_msg: &str) -> Option<String> {
    let marker = "session_id=\"";
    let start = error_msg.find(marker)? + marker.len();
    let rest = &error_msg[start..];
    let end = rest.find('"')?;
    let session_id = &rest[..end];
    if is_valid_pty_session_id(session_id) {
        Some(session_id.to_string())
    } else {
        None
    }
}

fn extract_patch_target_path_from_error(error_msg: &str) -> Option<String> {
    let markers = [
        "failed to locate expected lines in ",
        "failed to locate expected text in ",
    ];
    for marker in markers {
        let Some(start) = error_msg.find(marker) else {
            continue;
        };
        let rest = &error_msg[start + marker.len()..];
        for quote in ['\'', '"'] {
            let quote_s = quote.to_string();
            let Some(stripped) = rest.strip_prefix(&quote_s) else {
                continue;
            };
            let Some(end_idx) = stripped.find(quote) else {
                continue;
            };
            let path = stripped[..end_idx].trim();
            if !path.is_empty() {
                return Some(path.to_string());
            }
        }
    }
    None
}

#[cold]
pub(super) fn fallback_from_error(
    tool_name: &str,
    error_msg: &str,
    args_val: Option<&serde_json::Value>,
) -> Option<(String, serde_json::Value)> {
    if matches!(tool_name, tool_names::UNIFIED_FILE | tool_names::READ_FILE | "read file" | "repo_browser.read_file")
        && let Some(session_id) = extract_pty_session_id_from_error(error_msg)
    {
        return Some((
            tool_names::UNIFIED_EXEC.to_string(),
            serde_json::json!({
                "action": "poll",
                "session_id": session_id
            }),
        ));
    }

    if matches!(tool_name, tool_names::APPLY_PATCH | tool_names::UNIFIED_FILE | "apply patch")
        && let Some(path) = extract_patch_target_path_from_error(error_msg)
    {
        return Some((
            tool_names::READ_FILE.to_string(),
            serde_json::json!({
                "path": path,
                "offset": 1,
                "limit": 120
            }),
        ));
    }

    if matches!(tool_name, tool_names::TASK_TRACKER) {
        let lower = error_msg.to_ascii_lowercase();
        if lower.contains("required for 'update'")
            || lower.contains("required for \"update\"")
            || lower.contains("invalid task_tracker arguments")
        {
            return Some((
                tool_name.to_string(),
                serde_json::json!({
                    "action": "list"
                }),
            ));
        }
    }

    if tool_name == tool_names::SPAWN_AGENT && error_msg.contains("Use spawn_background_subprocess for agent '") {
        let mut fallback_args = args_val.and_then(serde_json::Value::as_object).cloned().unwrap_or_default();
        fallback_args.remove("background");
        fallback_args.remove("fork_context");

        if let Some(agent_name) = extract_background_subagent_name_from_error(error_msg) {
            fallback_args
                .entry("agent_type".to_string())
                .or_insert_with(|| serde_json::Value::String(agent_name));
        }

        return Some((tool_names::SPAWN_BACKGROUND_SUBPROCESS.to_string(), serde_json::Value::Object(fallback_args)));
    }

    None
}

fn extract_background_subagent_name_from_error(error_msg: &str) -> Option<String> {
    let marker = "Use spawn_background_subprocess for agent '";
    let start = error_msg.find(marker)? + marker.len();
    let remaining = &error_msg[start..];
    let end = remaining.find('\'')?;
    Some(remaining[..end].to_string())
}

#[cfg(test)]
mod tests {
    use super::{ToolExecutionError, build_structured_error_content, tool_denial_diagnostic};

    #[test]
    fn patch_recovery_message_is_compact_and_retains_typed_evidence() {
        let source = anyhow::Error::new(vtcode_core::tools::editing::patch::PatchError::SegmentNotFound {
            path: "notes.md".to_string(),
            snippet: "X   Y".to_string(),
        });
        let error = ToolExecutionError::from_anyhow("apply_patch", &source, 0, false, false, None);
        let payload = build_structured_error_content(&error, None, None, "execution");
        assert!(payload["error"]["message"].as_str().unwrap().contains("notes.md"));
        assert_eq!(payload["error"]["patch_failure"]["ContextMismatch"]["evidence"], "X   Y");
        assert!(payload["next_action"].as_str().unwrap().contains("Read the affected path once"));
        assert!(payload.get("error_summary").is_none());
        assert!(payload.get("recovery_suggestions").is_none());
        assert!(payload["error"].get("original_error").is_none());
        assert!(
            error.to_json_value()["error"]["original_error"].is_string(),
            "canonical diagnostics must retain the full error"
        );
    }

    #[test]
    fn request_user_input_denial_directive_tells_model_to_stop() {
        let diagnostic =
            tool_denial_diagnostic("request_user_input").expect("request_user_input should have a diagnostic");
        let directive = diagnostic
            .get("directive")
            .and_then(serde_json::Value::as_str)
            .expect("diagnostic should have a directive field");
        assert!(
            directive.contains("rest of this session") && directive.contains("same denial"),
            "directive must state that the denial is permanent: {directive}"
        );
        let present_plan = diagnostic
            .get("present_plan")
            .and_then(serde_json::Value::as_str)
            .expect("diagnostic should have a present_plan field");
        assert!(
            present_plan.contains("yes") && present_plan.contains("no") && present_plan.contains("edit"),
            "present_plan must offer the user a yes/no/edit choice: {present_plan}"
        );
        assert!(
            present_plan.contains("finalize the plan"),
            "present_plan must tell the model to finalize the plan from gathered evidence: {present_plan}"
        );
    }

    #[test]
    fn exec_command_denial_still_returns_config_fix() {
        let diagnostic = tool_denial_diagnostic("exec_command").expect("exec_command should have a diagnostic");
        assert!(diagnostic.get("fix").is_some(), "exec_command diagnostic should still have a fix field");
        let files = diagnostic["fix"]["files"].to_string();
        assert!(files.contains("run_pty_cmd"), "diagnostic must name the configured shell policy alias");
    }

    #[test]
    fn shell_denial_diagnostic_names_the_exact_active_alias() {
        for tool_name in ["run_pty_cmd", "unified_exec", "exec_command"] {
            let diagnostic = tool_denial_diagnostic(tool_name).expect("shell aliases need actionable diagnostics");
            assert!(diagnostic["cause"].as_str().unwrap_or("").contains(tool_name));
            assert!(diagnostic["fix"]["files"].to_string().contains(tool_name));
            assert!(diagnostic["fix"]["note"].as_str().unwrap_or("").contains("policy snapshot"));
        }
    }

    #[test]
    fn unknown_tool_returns_no_diagnostic() {
        assert!(tool_denial_diagnostic("some_unknown_tool").is_none());
    }

    #[test]
    fn structured_error_content_attaches_diagnostic_on_policy_denial() {
        let error = ToolExecutionError::policy_violation("exec_command", "Tool permission denied");
        let payload = build_structured_error_content(&error, None, None, "execution");

        assert_eq!(
            payload["error_class"].as_str(),
            Some("policy_blocked"),
            "a policy denial must classify as policy_blocked"
        );
        let diagnostic = payload
            .get("diagnostic")
            .expect("policy denial for a covered tool must carry the denial diagnostic");
        assert!(diagnostic["fix"]["action"].is_string(), "diagnostic must carry a concrete fix");

        // A non-denied execution failure gets no diagnostic noise.
        let runtime_error =
            ToolExecutionError::from_anyhow("exec_command", &anyhow::anyhow!("process failed"), 0, false, false, None);
        let payload = build_structured_error_content(&runtime_error, None, None, "execution");
        assert!(payload.get("diagnostic").is_none(), "non-denial failures must not carry a denial diagnostic");
    }
}
