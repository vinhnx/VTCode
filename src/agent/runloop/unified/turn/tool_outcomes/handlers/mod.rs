//! Tool outcome handling helpers for turn execution.

use anyhow::Result;
use vtcode_core::config::constants::tools as tool_names;
use vtcode_core::exec_policy::AskForApproval;
use vtcode_core::primary_agent::primary_agent_allows_tool;
use vtcode_core::tools::registry::ToolExecutionError;
use vtcode_core::tools::registry::labels::tool_action_label;
use vtcode_core::utils::ansi::MessageStyle;

use super::error_handling::tool_denial_diagnostic;
use super::helpers::check_is_argument_error;
use crate::agent::runloop::unified::async_mcp_manager::approval_policy_from_human_in_the_loop;
use crate::agent::runloop::unified::run_loop_context::full_auto_loop_grants_enabled;
use crate::agent::runloop::unified::run_loop_context::{
    BudgetExhaustedMetrics, budget_kind, emit_budget_exhausted_metric,
};
use crate::agent::runloop::unified::tool_call_safety::invocation_id_from_call_id;
use crate::agent::runloop::unified::tool_pipeline::validation::{
    SafetyValidationFailure, validate_tool_call_with_limit_prompt,
};
use crate::agent::runloop::unified::tool_routing::{
    PreToolHookPhaseResult, ToolPermissionFlow, ensure_tool_permission_with_call_id,
};
use crate::agent::runloop::unified::turn::context::{
    PreparedAssistantToolCall, TOOL_NAME_NOT_CLEAN_IDENTIFIER_ERROR, TurnHandlerOutcome, TurnLoopResult,
    TurnProcessingContext,
};
pub(crate) use looping::low_signal_family_key;
use looping::maybe_apply_spool_read_offset_hint;
pub(crate) use looping::{shell_failure_error_signature, shell_run_signature};
mod budget;
mod fallbacks;
mod guards;
#[path = "../handlers_batch.rs"]
mod handlers_batch;
mod looping;
mod rate_limit;
mod recovery;
#[cfg(test)]
mod tests;
mod types;
use budget::record_tool_call_budget_usage;
use fallbacks::{
    build_validation_error_content_with_fallback, preflight_validation_fallback, recovery_fallback_for_tool,
    try_recover_preflight_with_fallback,
};
#[cfg(test)]
pub(crate) use guards::BlockedToolCallLimits;
#[cfg(test)]
pub(crate) use guards::blocked_tool_guard::BlockedToolCallFuseTrip;

pub(crate) use guards::{
    blocked_tool_call_fuse_trip, blocked_tool_call_limits, blocked_tool_call_limits_for_tool,
    blocked_tool_call_messages_detailed, max_consecutive_blocked_tool_calls_per_turn,
};
use guards::{
    enforce_blocked_tool_call_guard, enforce_duplicate_task_tracker_create_guard, enforce_read_after_write_guard,
    enforce_repeated_read_only_call_guard, enforce_repeated_shell_run_guard, enforce_spool_chunk_read_guard,
};
pub(crate) use handlers_batch::{execute_and_handle_tool_call, handle_tool_call_batch_prepared};
use rate_limit::acquire_adaptive_rate_limit_slot;
use recovery::try_interactive_circuit_recovery;
pub(crate) use types::{PreparedToolCall, ToolOutcomeContext, ValidationResult};

/// Whether model-emitted arguments echo the request-only history stub
/// `{"cleared":"tool_input"}` from `clear_old_tool_results` in vtcode-core.
/// The stub is valid JSON for the wire but carries no operation; executing it
/// always fails preflight with `Missing required argument`. Session
/// `session-vtcode-20261015T083154Z_961005-54191` burned 3 loops echoing it
/// for `exec_command`/`task_tracker` before the circuit tripped.
pub(crate) fn is_cleared_tool_input_echo(arguments: Option<&serde_json::Value>) -> bool {
    arguments.is_some_and(|args| args.get("cleared").and_then(serde_json::Value::as_str) == Some("tool_input"))
}

/// Whether a preflight reject is a tool-name/identity mistake rather than a
/// policy block or a repeated argument-schema failure.
///
/// Name mistakes (prose-blob tool names, unknown tools, empty names) are
/// per-call rejects: the model already gets a schema correction and can retry.
/// Counting them toward the preflight circuit lets one malformed name in an
/// assistant batch skip every remaining valid sibling
/// (`drain_preflight_circuit_responses`). Argument-schema failures still count
/// so a model stuck on bad JSON trips the recovery fuse; policy and security
/// rejects always count.
pub(crate) fn preflight_failure_is_llm_mistake(error: &str) -> bool {
    let lower = error.to_ascii_lowercase();
    // The name-mistake error embeds the raw model-supplied tool name after
    // the constant. Classify that error on its static prefix only: the name
    // is arbitrary model prose, and policy phrases inside it ("not allowed",
    // "command injection", ...) must not flip a name mistake into a policy
    // block, which would advance the preflight circuit and skip valid
    // sibling calls in the same batch.
    let name_mistake_marker = format!("{}:", TOOL_NAME_NOT_CLEAN_IDENTIFIER_ERROR).to_ascii_lowercase();
    if let Some((static_prefix, _)) = lower.split_once(&name_mistake_marker) {
        return !is_policy_reject_phrase(static_prefix);
    }
    if is_policy_reject_phrase(&lower) {
        return false;
    }
    // Name-mistake allow-list. Each phrase names its producer so rewording
    // one side keeps the other discoverable:
    // - `TOOL_NAME_NOT_CLEAN_IDENTIFIER_ERROR` from `turn/context.rs`.
    // - "unknown tool" from vtcode-core `tools/handlers/router.rs`.
    // - "empty tool name" from the empty-name guard in this file below.
    lower.contains(TOOL_NAME_NOT_CLEAN_IDENTIFIER_ERROR)
        || lower.contains("unknown tool")
        || lower.contains("empty tool name")
}

/// Security / policy phrases that always count toward the preflight circuit,
/// even when they mention a tool name. Keep these phrases tight: a bare
/// "sandbox" substring would misclassify `Unknown tool: sandbox_helper` as a
/// policy block.
fn is_policy_reject_phrase(lower: &str) -> bool {
    lower.contains("command security check failed")
        || lower.contains("command injection")
        || lower.contains("sandbox denied")
        || lower.contains("sandbox policy")
        || lower.contains("policy violation")
        || lower.contains("not allowed")
}

/// Record a malformed or preflight-invalid tool call. When the independent
/// preflight circuit breaker reaches its cap, arm a bounded tool-free
/// recovery pass (mirroring budget-exhaustion and interview-denial) so the
/// model can synthesize a plain-text response instead of the turn
/// hard-blocking. A hard `Blocked` outcome silently dropped approved-plan
/// builds: `plan_approved_execution_pending` only derives from `Completed`,
/// so a blocked build turn was never re-queued and the agent could not
/// continue (checkpoint turn_874). Policy denials intentionally do not use
/// this path; they remain governed by the existing blocked-call fuse.
///
/// LLM-mistake rejects do not advance the circuit streak and never trip it;
/// valid sibling calls in the same batch continue to execute.
pub(crate) fn handle_preflight_failure(
    ctx: &mut TurnProcessingContext<'_>,
    tool_call_id: &str,
    tool_name: &str,
    error: &str,
    arguments: Option<&serde_json::Value>,
    fallback: Option<(String, serde_json::Value)>,
) -> Option<TurnHandlerOutcome> {
    if let Some(stop) = crate::agent::runloop::unified::stop_requests::stop_outcome(ctx.ctrl_c_state) {
        ctx.push_rejected_tool_response(tool_call_id, Some(tool_name), arguments,
            serde_json::json!({"error":"Tool call cancelled before validation", "not_executed":true, "interrupted":true}).to_string());
        return Some(TurnHandlerOutcome::Break(stop));
    }
    let is_cleared_echo = is_cleared_tool_input_echo(arguments);
    let is_llm_mistake = !is_cleared_echo && preflight_failure_is_llm_mistake(error);
    let failure_count = if is_llm_mistake {
        // Report the current streak without advancing it so the model still
        // sees failure_count telemetry, but a prose blob cannot poison the
        // batch circuit.
        ctx.harness_state.consecutive_preflight_failures
    } else {
        ctx.record_preflight_failure()
    };
    let max_failures = max_consecutive_blocked_tool_calls_per_turn(ctx);
    // LLM mistakes never trip the circuit, even if a prior policy streak is
    // already at the cap: one prose blob must not skip valid sibling calls.
    let policy_correction = vtcode_core::tools::error_messages::agent_execution::preflight_policy_correction(error);
    let recoverable_validation = !is_llm_mistake && policy_correction.is_none();
    let repeated = recoverable_validation
        && ctx.session_stats.preflight_recovery.reject(
            &ctx.tool_registry.resolve_tool_name_with_display(tool_name).canonical,
            if arguments.is_some() {
                crate::agent::runloop::unified::state::ValidationCategory::Arguments
            } else {
                crate::agent::runloop::unified::state::ValidationCategory::MalformedJson
            },
            arguments,
            max_failures,
            ctx.is_planning_active(),
        );
    let exhausted = ctx.session_stats.preflight_recovery.exhausted();
    let circuit_tripped = !is_llm_mistake && (failure_count >= max_failures || exhausted);
    let schema_correction = if is_cleared_echo && policy_correction.is_none() {
        if matches!(tool_name, tool_names::EXEC_COMMAND | tool_names::UNIFIED_EXEC | "command_session") {
            format!(
                "Do not echo the history stub '{{\"cleared\":\"tool_input\"}}'. Provide real arguments matching the declared schema for '{tool_name}' (exec_command requires \"cmd\"; \"command\" is accepted as an alias)."
            )
        } else {
            format!(
                "Do not echo the history stub '{{\"cleared\":\"tool_input\"}}'. Provide real arguments matching the declared schema for '{tool_name}'."
            )
        }
    } else {
        preflight_schema_correction(tool_name, error)
    };
    let next_action = if let Some(correction) = policy_correction {
        super::execution_result::policy_denial_next_action(correction, circuit_tripped, ctx.is_planning_active())
    } else if exhausted {
        "Automatic validation recovery stopped. Resume with corrected arguments on fresh user input.".to_owned()
    } else if circuit_tripped && ctx.is_planning_active() {
        "Stop retrying this malformed call. Tools are disabled for the next pass — synthesize exactly one complete <proposed_plan> from the evidence already gathered."
            .to_owned()
    } else if circuit_tripped {
        "Stop retrying this malformed call. Tools are disabled for the next pass — synthesize a plain-text response reporting the failure to the user."
            .to_owned()
    } else {
        "Correct the arguments using schema_correction, then retry this tool once.".to_owned()
    };
    let failure_kind = if circuit_tripped {
        "preflight_circuit_breaker"
    } else {
        "preflight_validation"
    };
    let mut payload = serde_json::json!({
        "error": if repeated {
            format!("Repeated preflight rejection for '{tool_name}'; use schema_correction.")
        } else {
            // The kernel message already carries the
            // "Tool preflight validation failed for '<tool>': " prefix, and
            // nested hop rejections re-wrap it; strip every layer down to the
            // root cause so the payload adds the prefix exactly once.
            let tool_prefix = format!("Tool preflight validation failed for '{tool_name}': ");
            let mut inner = error.trim_start();
            while let Some(stripped) = inner
                .strip_prefix(&tool_prefix)
                .or_else(|| inner.strip_prefix("Tool preflight validation failed: "))
            {
                inner = stripped;
            }
            format!("Tool preflight validation failed for '{tool_name}': {inner}")
        },
        "failure_kind": failure_kind,
        "tool_name": tool_name,
        "failure_count": failure_count,
        "schema_correction": schema_correction,
        "next_action": next_action,
        "retryable": policy_correction.is_none() && !circuit_tripped,
    });
    if let Some((fallback_tool, fallback_args)) = fallback {
        if let Some(object) = payload.as_object_mut() {
            object.insert("fallback_tool".to_string(), serde_json::Value::String(fallback_tool));
            object.insert("fallback_tool_args".to_string(), fallback_args);
        }
    }
    let diagnosis = super::execution_result::deterministic_preflight_diagnosis(
        tool_name,
        if repeated { "Repeated invalid arguments" } else { error },
        circuit_tripped,
        ctx.is_planning_active(),
    );
    if let Some(object) = payload.as_object_mut() {
        object.insert("diagnosis".to_string(), diagnosis.to_value());
    }
    ctx.push_rejected_tool_response(tool_call_id, Some(tool_name), arguments, payload.to_string());
    super::execution_result::render_and_emit(ctx, Some(tool_call_id), tool_name, &diagnosis);

    if exhausted {
        return Some(TurnHandlerOutcome::Break(TurnLoopResult::Blocked {
            reason: Some("Preflight corrective turn repeated an unchanged rejected call; automatic validation recovery stopped. Type continue to resume with corrected arguments.".to_string()),
        }));
    }
    if circuit_tripped && recoverable_validation {
        ctx.session_stats.preflight_recovery.synthesis();
    }
    circuit_tripped.then(|| {
        // Arm a tool-free recovery pass instead of hard-blocking. The flush
        // (called by the tool batch after all responses land) arms the
        // request-only synthesis directive and disables tool choice. Returning
        // `Continue` lets the turn loop proceed to that pass rather than
        // terminating as `Blocked`.
        ctx.harness_state.arm_preflight_circuit_recovery();
        tracing::warn!(
            target: "vtcode.turn.metrics",
            tool = tool_name,
            failure_count,
            max_failures,
            "Preflight validation circuit breaker tripped; arming tool-free recovery"
        );
        TurnHandlerOutcome::Continue
    })
}

fn preflight_schema_correction(tool_name: &str, error: &str) -> String {
    if let Some(correction) = vtcode_core::tools::error_messages::agent_execution::preflight_policy_correction(error) {
        return correction.to_owned();
    }
    if tool_name == tool_names::TASK_TRACKER {
        return vtcode_core::tools::handlers::task_tracker::TASK_TRACKER_ARGUMENT_CORRECTION.to_string();
    }
    if tool_name == tool_names::APPLY_PATCH || error.contains("apply_patch is a tool") {
        return vtcode_core::tools::apply_patch::APPLY_PATCH_ARGUMENT_CORRECTION.to_string();
    }
    if matches!(tool_name, tool_names::EXEC_COMMAND | tool_names::UNIFIED_EXEC | "command_session")
        && error.contains("dynamic shell expansion in find commands")
    {
        return "Do not retry the rejected dynamic find command. Use a literal path and quoted pattern, for example `find src -name '*.rs'`, or use `rg --files src | rg '\\.rs$'`. Do not use `$()`, backticks, variable/brace expansion, or dynamically spliced find options.".to_string();
    }
    if matches!(tool_name, tool_names::EXEC_COMMAND | tool_names::UNIFIED_EXEC | "command_session")
        && error.contains("Missing required argument: command")
    {
        return "Provide `{\"cmd\": \"<shell command>\"}` for exec_command (`command` is accepted as an alias). Parse the arguments as JSON before retrying.".to_string();
    }

    format!(
        "Provide a JSON object matching the declared schema for '{tool_name}'. Parse the arguments as JSON before retrying."
    )
}

/// Emit matching responses for calls that remain in an assistant batch after
/// the preflight circuit trips. These calls are intentionally not admitted or
/// executed, but providers still require one tool response per assistant call.
pub(crate) fn drain_preflight_circuit_responses(
    ctx: &mut TurnProcessingContext<'_>,
    remaining_tool_calls: &[PreparedAssistantToolCall],
) {
    let failure_count = ctx.harness_state.consecutive_preflight_failures;
    for tool_call in remaining_tool_calls {
        let tool_name = tool_call.tool_name();
        if crate::agent::runloop::unified::stop_requests::stop_outcome(ctx.ctrl_c_state).is_some() {
            ctx.push_rejected_tool_response(tool_call.call_id(), Some(tool_name), tool_call.args(),
                serde_json::json!({"error":"Tool call cancelled before execution", "not_executed":true, "interrupted":true}).to_string());
            continue;
        }
        let error = tool_call.args_error().unwrap_or(
            "Tool call skipped because another call in this assistant batch tripped the preflight circuit breaker.",
        );
        let payload = serde_json::json!({
            "error": format!("Tool call '{tool_name}' was not executed: {error}"),
            "failure_kind": "preflight_circuit_breaker",
            "tool_name": tool_name,
            "failure_count": failure_count,
            "schema_correction": preflight_schema_correction(tool_name, error),
            "next_action": if ctx.is_planning_active() {
                "Stop retrying this batch. Tools are disabled for the next pass — synthesize exactly one complete <proposed_plan> from the evidence already gathered."
            } else {
                "Stop retrying this batch. Tools are disabled for the next pass — synthesize a plain-text response reporting the failure to the user."
            },
            "retryable": false,
        });
        let diagnosis = super::execution_result::deterministic_preflight_diagnosis(
            tool_name,
            error,
            true,
            ctx.is_planning_active(),
        );
        let mut payload = payload;
        if let Some(object) = payload.as_object_mut() {
            object.insert("diagnosis".to_string(), diagnosis.to_value());
        }
        ctx.push_rejected_tool_response(tool_call.call_id(), Some(tool_name), tool_call.args(), payload.to_string());
        super::execution_result::render_and_emit(ctx, Some(tool_call.call_id()), tool_name, &diagnosis);
    }
}

fn build_failure_error_content(error: String, failure_kind: &'static str) -> String {
    super::execution_result::build_error_content(error, None, None, failure_kind).to_string()
}

fn build_turn_budget_error_content(error: String) -> String {
    let mut payload = super::execution_result::build_error_content(error, None, None, "policy");
    if let Some(next_action) = payload.get_mut("next_action") {
        *next_action = crate::agent::runloop::unified::run_loop_context::BUDGET_EXHAUSTED_SYNTHESIS_NOTE.into();
    }
    payload.to_string()
}

const INTERVIEW_DENIAL_RECOVERY_DIRECTIVE: &str = "Planning recovery: the interactive interview is unavailable in this runtime. Tools are disabled for the next pass. If you have a clarifying question, present it to the user in plain text and end your turn — the user's next message will answer it and you can continue planning. Otherwise, synthesize exactly one completed `<proposed_plan>` from the research already gathered. Do not emit tool calls or request approval until the plan is present.";

pub(crate) const PREFLIGHT_CIRCUIT_RECOVERY_DIRECTIVE: &str = "Recovery: repeated tool preflight validation failures tripped the circuit breaker, so tools are disabled for this pass. Do not emit tool calls. Summarize what you were trying to do and the validation errors above, then tell the user in plain text what you need to proceed (e.g. re-state the request so the next turn retries with correct arguments). End your turn after this response.";
pub(crate) const PLANNING_TOOL_FREE_RECOVERY_DIRECTIVE: &str = "Planning recovery: tools are disabled for this pass. Emit exactly one complete `<proposed_plan>` from the repository evidence already gathered. Include `## Summary`, `## Implementation Steps`, `## Test Cases and Validation`, and `## Assumptions and Defaults`; every numbered step must use `Action -> files: [concrete/path] -> verify: [command or observable check]`. Do not emit tool calls or approval language.";

const BLOCKED_TOOL_RECOVERY_DIRECTIVE: &str = "Recovery: repeated tool calls were blocked by the active safety or permission policy, so tools are disabled for one bounded pass. Do not retry or re-emit blocked commands. Synthesize a plain-text response from the tool responses above, explain the blocked action and the safe next step, and end your turn.";

/// Convert a permanent interview denial into a bounded, tool-free planning
/// pass. This is flushed after the current tool batch so the directive follows
/// every tool response and remains valid for providers that require strict
/// assistant/tool ordering.
pub(crate) fn flush_interview_denial_recovery(ctx: &mut TurnProcessingContext<'_>) {
    if !ctx.harness_state.take_interview_denial_recovery() {
        return;
    }

    ctx.push_system_message(INTERVIEW_DENIAL_RECOVERY_DIRECTIVE);
    if ctx.harness_state.recovery_reason.is_none() {
        ctx.harness_state.recovery_reason = Some("planning interview unavailable".to_string());
    }
    ctx.harness_state.switch_to_tool_free_recovery();
    tracing::info!(
        target: "vtcode.planning_workflow",
        "interactive interview denied; scheduling bounded tool-free plan synthesis"
    );
}

/// Convert a preflight circuit-breaker trip into a bounded, tool-free
/// synthesis pass. Flushed after the current tool batch (including drained
/// skipped-call responses) so the directive never lands between tool
/// responses. Without this, the breaker hard-blocked the turn as `Blocked`,
/// silently dropping approved-plan builds that the user had already approved.
pub(crate) fn flush_preflight_circuit_recovery(ctx: &mut TurnProcessingContext<'_>) {
    if !ctx.harness_state.take_preflight_circuit_recovery()
        || crate::agent::runloop::unified::stop_requests::stop_outcome(ctx.ctrl_c_state).is_some()
    {
        return;
    }

    if ctx.harness_state.recovery_reason.is_none() {
        ctx.harness_state.recovery_reason = Some("preflight validation circuit breaker".to_string());
    }
    ctx.harness_state.switch_to_tool_free_recovery();
    tracing::info!(
        target: "vtcode.turn.metrics",
        "preflight circuit breaker tripped; scheduling bounded tool-free synthesis"
    );
}

/// Convert a blocked-call fuse trip into one bounded, tool-free synthesis pass.
/// This is flushed after all responses from the current assistant batch so a
/// recovery directive is never interleaved with tool responses.
pub(crate) fn flush_blocked_tool_recovery(ctx: &mut TurnProcessingContext<'_>) {
    if !ctx.harness_state.take_blocked_tool_recovery() {
        return;
    }

    let directive = ctx
        .harness_state
        .take_blocked_tool_recovery_reason()
        .map(|reason| format!("{BLOCKED_TOOL_RECOVERY_DIRECTIVE} Trigger: {reason}"))
        .unwrap_or_else(|| BLOCKED_TOOL_RECOVERY_DIRECTIVE.to_string());
    let directive = if ctx.is_planning_active() {
        format!("{PLANNING_TOOL_FREE_RECOVERY_DIRECTIVE} Trigger: {directive}")
    } else {
        directive
    };
    ctx.push_system_message(directive);
    if ctx.harness_state.recovery_reason.is_none() {
        ctx.harness_state.recovery_reason = Some("blocked tool-call fuse tripped".to_string());
    }
    ctx.harness_state.switch_to_tool_free_recovery();
    tracing::info!(
        target: "vtcode.turn.metrics",
        "blocked tool-call fuse tripped; scheduling bounded tool-free synthesis"
    );
}

/// Emit matching responses for calls that remain in an assistant batch after
/// the blocked-call fuse trips. These calls are not admitted or executed, but
/// providers still require one response per assistant tool call.
pub(crate) fn drain_blocked_tool_recovery_responses(
    ctx: &mut TurnProcessingContext<'_>,
    remaining_tool_calls: &[PreparedAssistantToolCall],
) {
    for tool_call in remaining_tool_calls {
        push_blocked_tool_recovery_response(ctx, tool_call);
    }
}

pub(crate) fn push_blocked_tool_recovery_response(
    ctx: &mut TurnProcessingContext<'_>,
    tool_call: &PreparedAssistantToolCall,
) {
    let tool_name = tool_call.tool_name();
    let payload = serde_json::json!({
        "error": format!("Tool call '{tool_name}' was not executed because blocked-call recovery started."),
        "failure_kind": "blocked_tool_call_recovery",
        "tool_name": tool_name,
        "next_action": "Do not retry this call. Tools are disabled for the next pass; synthesize a plain-text response from the available context.",
        "retryable": false,
    });
    ctx.push_rejected_tool_response(tool_call.call_id(), Some(tool_name), tool_call.args(), payload.to_string());
}

/// Push the one-time budget-exhaustion synthesis directive (wall-clock or
/// tool-call budget) if a rejection armed it during validation. Called after
/// the tool batch (single or grouped) completes so the system message lands
/// *after* all tool responses of the current assistant message, never
/// interleaved between them (which some provider adapters reject). No-op
/// unless a budget tripped this turn.
pub(super) fn flush_budget_synthesis_directives(ctx: &mut TurnProcessingContext<'_>) {
    if ctx.harness_state.take_wall_clock_directive_pending()
        && let Some(exhaustion) = ctx.harness_state.wall_clock_budget_exhaustion()
    {
        ctx.push_system_message(exhaustion.synthesis_directive_message());
        emit_budget_exhausted_metric(
            ctx.traj,
            BudgetExhaustedMetrics {
                budget: budget_kind::WALL_CLOCK,
                used: usize::try_from(exhaustion.max_secs).unwrap_or(usize::MAX),
                max: usize::try_from(exhaustion.max_secs).unwrap_or(usize::MAX),
                step_count: None,
                planning_active: ctx.is_planning_active(),
                tool_calls: ctx.harness_state.tool_calls,
            },
        );
        // Arm the tool-free recovery pass so the next request strips tool
        // definitions at the API level (`tools: None` + `ToolChoice::none`).
        // The directive alone is advisory: models kept emitting tool calls
        // after it, burning requests on rejected calls instead of
        // synthesizing (observed in checkpoints turn_637 and turn_647).
        if ctx.harness_state.recovery_reason.is_none() {
            ctx.harness_state.recovery_reason = Some("tool wall-clock budget exhausted".to_string());
        }
        ctx.harness_state.switch_to_tool_free_recovery();
    }
    if ctx.harness_state.take_tool_budget_directive_pending()
        && let Some(exhaustion) = ctx.harness_state.tool_budget_exhaustion()
    {
        ctx.push_system_message(exhaustion.synthesis_directive_message());
        emit_budget_exhausted_metric(
            ctx.traj,
            BudgetExhaustedMetrics {
                budget: budget_kind::TOOL_CALLS,
                used: exhaustion.used,
                max: exhaustion.max,
                step_count: None,
                planning_active: ctx.is_planning_active(),
                tool_calls: ctx.harness_state.tool_calls,
            },
        );
        if ctx.harness_state.recovery_reason.is_none() {
            ctx.harness_state.recovery_reason = Some("tool-call budget exhausted".to_string());
        }
        ctx.harness_state.switch_to_tool_free_recovery();
    }
}

/// Push model-facing guidance after a user grants more session tool calls.
/// This is deliberately flushed after the tool batch, alongside the existing
/// budget directives, so provider message ordering remains valid.
pub(super) fn flush_session_limit_grant_directive(ctx: &mut TurnProcessingContext<'_>) {
    if ctx.harness_state.take_session_limit_grant_directive_pending() {
        ctx.push_system_message(crate::agent::runloop::unified::run_loop_context::SESSION_LIMIT_GRANT_DIRECTIVE);
    }
}

pub(super) fn apply_reused_read_only_loop_metadata(obj: &mut serde_json::Map<String, serde_json::Value>) {
    // Keep output/content/stdout/stderr intact — stripping them was causing
    // false loop detection to leave the model with no data (issue #680). The
    // cached result may have useful content the model needs.
    obj.insert("reused_recent_result".to_string(), serde_json::Value::Bool(true));
    obj.insert("result_ref_only".to_string(), serde_json::Value::Bool(true));
    obj.insert("loop_detected".to_string(), serde_json::Value::Bool(true));

    // Check for *actual* non-empty content, not just key presence.  An empty
    // string in `output` means the command produced no captured stdout.
    // Claiming "content is in the result above" when it is empty causes the
    // model to hallucinate or spin.
    let has_meaningful_content = has_non_empty_text_content(obj);
    let has_spool_path = obj
        .get("spool_path")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|p| !p.trim().is_empty());

    let (note, next_action) = if has_meaningful_content {
        (
            "Loop detected: same result returned. The content is in the result above \u{2014} use it directly.",
            "The tool result content is already in this response. Continue from the available data.",
        )
    } else if has_spool_path {
        (
            "Loop detected: same result returned. The full output was previously spooled to disk; repeating the call returns it again.",
            "Read the spool_path file for the full output, or use data already in the conversation history.",
        )
    } else {
        (
            "Loop detected: same result returned. The previous execution produced no output, and repeating it will not change that.",
            "Use data already in the conversation history, or change the approach.",
        )
    };

    obj.insert("loop_detected_note".to_string(), serde_json::Value::String(note.to_string()));
    obj.insert("next_action".to_string(), serde_json::Value::String(next_action.to_string()));
}

fn has_non_empty_text_content(obj: &serde_json::Map<String, serde_json::Value>) -> bool {
    let check = |key: &str| {
        obj.get(key)
            .and_then(serde_json::Value::as_str)
            .is_some_and(|s| !s.trim().is_empty())
    };
    check("output") || check("content") || check("stdout")
}

pub(super) enum ValidationTransition {
    Proceed(PreparedToolCall),
    Return(Option<TurnHandlerOutcome>),
}

pub(super) fn finalize_validation_result(
    ctx: &mut TurnProcessingContext<'_>,
    tool_call_id: &str,
    tool_name: &str,
    args_val: &serde_json::Value,
    validation_result: ValidationResult,
) -> ValidationTransition {
    match validation_result {
        ValidationResult::Outcome(outcome) => ValidationTransition::Return(Some(outcome)),
        ValidationResult::Handled => {
            ctx.reset_blocked_tool_call_streak();
            ValidationTransition::Return(None)
        }
        ValidationResult::Blocked => {
            let outcome = enforce_blocked_tool_call_guard(ctx, tool_call_id, tool_name, args_val);
            // A permanent interview denial arms a tool-free planning pass.
            // Keep the generic blocked-call fuse from converting that
            // recovery transition into a terminal turn outcome.
            if outcome.is_some() && ctx.harness_state.interview_denial_recovery_pending() {
                ValidationTransition::Return(None)
            } else {
                ValidationTransition::Return(outcome)
            }
        }
        ValidationResult::ReadCapBlocked => {
            ValidationTransition::Return(guards::blocked_tool_guard::enforce_read_cap_blocked_tool_call_guard(
                ctx,
                tool_call_id,
                tool_name,
                args_val,
            ))
        }
        ValidationResult::Proceed(prepared) => {
            ctx.reset_blocked_tool_call_streak();
            ValidationTransition::Proceed(prepared)
        }
    }
}

async fn run_safety_validation_loop(
    ctx: &mut TurnProcessingContext<'_>,
    tool_call_id: &str,
    canonical_tool_name: &str,
    effective_args: &serde_json::Value,
) -> Result<Option<(ValidationResult, Option<String>)>> {
    let invocation_id = invocation_id_from_call_id(tool_call_id);
    // Hoisted: `is_planning_active` borrows all of `ctx`, which would
    // conflict with the `harness_state` reborrow in the call below.
    let planning_active = ctx.is_planning_active();
    match validate_tool_call_with_limit_prompt(
        ctx.safety_validator,
        ctx.handle,
        ctx.session,
        ctx.ctrl_c_state,
        ctx.ctrl_c_notify,
        canonical_tool_name,
        effective_args,
        invocation_id,
        Some(ctx.harness_state),
        ctx.harness_emitter,
        Some(ctx.active_primary_agent.active().name()),
        full_auto_loop_grants_enabled(ctx.full_auto, ctx.vt_cfg),
        ctx.traj,
        planning_active,
    )
    .await
    {
        Ok(()) => Ok(None),
        Err(SafetyValidationFailure::SessionLimitNotIncreased)
        | Err(SafetyValidationFailure::SessionLimitPromptFailed(_)) => {
            ctx.push_rejected_tool_response(
                tool_call_id,
                Some(canonical_tool_name),
                Some(effective_args),
                build_failure_error_content(
                    "Session tool limit reached and not increased by user".to_string(),
                    "safety_limit",
                ),
            );
            Ok(Some((ValidationResult::Blocked, None)))
        }
        Err(SafetyValidationFailure::NeedsApproval(justification)) => {
            Ok(Some((ValidationResult::Handled, Some(justification))))
        }
        Err(SafetyValidationFailure::Validation(err)) => {
            ctx.renderer
                .line(MessageStyle::Error, &format!("Safety validation failed: {err}"))?;
            ctx.push_rejected_tool_response(
                tool_call_id,
                Some(canonical_tool_name),
                Some(effective_args),
                build_failure_error_content(format!("Safety validation failed: {err}"), "safety_validation"),
            );
            Ok(Some((ValidationResult::Blocked, None)))
        }
    }
}

#[cfg(test)]
fn build_tool_permissions_context<'ctx, 'a>(
    ctx: &'ctx mut TurnProcessingContext<'a>,
) -> crate::agent::runloop::unified::tool_routing::ToolPermissionsContext<'ctx, vtcode_ui::tui::app::InlineSession> {
    build_tool_permissions_context_with_safety(ctx, None)
}

fn build_tool_permissions_context_with_safety<'ctx, 'a>(
    ctx: &'ctx mut TurnProcessingContext<'a>,
    safety_approval_justification: Option<&str>,
) -> crate::agent::runloop::unified::tool_routing::ToolPermissionsContext<'ctx, vtcode_ui::tui::app::InlineSession> {
    crate::agent::runloop::unified::tool_routing::ToolPermissionsContext {
        tool_registry: ctx.tool_registry,
        renderer: ctx.renderer,
        handle: ctx.handle,
        session: ctx.session,
        active_thread_label: Some(ctx.active_thread_label),
        default_placeholder: ctx.default_placeholder.clone(),
        ctrl_c_state: ctx.ctrl_c_state,
        ctrl_c_notify: ctx.ctrl_c_notify,
        hooks: ctx.lifecycle_hooks,
        justification: None,
        approval_recorder: Some(ctx.approval_recorder.as_ref()),
        decision_ledger: Some(ctx.decision_ledger),
        tool_permission_cache: Some(ctx.tool_permission_cache),
        permissions_state: Some(ctx.permissions_state),
        active_agent_permissions: ctx
            .vt_cfg
            .and_then(|cfg| cfg.runtime_agent_permissions.as_ref())
            .or(Some(&ctx.active_primary_agent.active().permissions)),
        hitl_notification_bell: ctx.vt_cfg.map(|cfg| cfg.security.hitl_notification_bell).unwrap_or(true),
        approval_policy: ctx
            .vt_cfg
            .map(|cfg| cfg.security.human_in_the_loop)
            .map(approval_policy_from_human_in_the_loop)
            .unwrap_or(AskForApproval::OnRequest),
        skip_confirmations: ctx.skip_confirmations,
        permissions_config: ctx.vt_cfg.map(|cfg| &cfg.permissions),
        auto_permission_runtime: Some(crate::agent::runloop::unified::run_loop_context::AutoPermissionRuntimeContext {
            config: ctx.config,
            vt_cfg: ctx.vt_cfg,
            provider_client: ctx.provider_client.as_mut(),
            working_history: ctx.working_history.as_slice(),
        }),
        session_stats: Some(ctx.session_stats),
        safety_approval_justification: safety_approval_justification.map(String::from),
        harness_emitter: ctx.harness_emitter,
    }
}

/// Unified handler for a single tool call (whether native or textual).
///
/// This handler applies the full pipeline of checks:
/// 1. Circuit Breaker
/// 2. Rate Limiting
/// 3. Loop Detection
/// 4. Safety Validation (with potential user interaction for limits)
/// 5. Permission Checking (Allow/Deny/Ask)
/// 6. Execution (with progress spinners and PTY streaming)
/// 7. Result Handling (recording metrics, history, UI output)
pub(crate) async fn handle_prepared_tool_call<'a, 'b>(
    t_ctx: &mut ToolOutcomeContext<'a, 'b>,
    tool_call: &PreparedAssistantToolCall,
) -> Result<Option<TurnHandlerOutcome>> {
    let Some(args_val) = tool_call.args() else {
        return Ok(None);
    };
    handle_tool_call_inner(t_ctx, tool_call.call_id(), tool_call.tool_name(), args_val).await
}

pub(crate) async fn handle_single_tool_call<'a, 'b, 'tool>(
    t_ctx: &mut ToolOutcomeContext<'a, 'b>,
    tool_call_id: &str,
    tool_name: &'tool str,
    args_val: serde_json::Value,
) -> Result<Option<TurnHandlerOutcome>> {
    let outcome = handle_tool_call_inner(t_ctx, tool_call_id, tool_name, &args_val).await?;
    flush_budget_synthesis_directives(t_ctx.ctx);
    flush_session_limit_grant_directive(t_ctx.ctx);
    flush_blocked_tool_recovery(t_ctx.ctx);
    Ok(outcome)
}

async fn handle_tool_call_inner<'a, 'b, 'tool>(
    t_ctx: &mut ToolOutcomeContext<'a, 'b>,
    tool_call_id: &str,
    tool_name: &'tool str,
    args_val: &serde_json::Value,
) -> Result<Option<TurnHandlerOutcome>> {
    use crate::agent::runloop::unified::run_loop_context::TurnPhase;
    t_ctx.ctx.set_phase(TurnPhase::ExecutingTools);

    // 1. Validate (Circuit Breaker, Rate Limit, Loop Detection, Safety, Permission)
    let validation_result = validate_tool_call(t_ctx.ctx, tool_call_id, tool_name, args_val).await?;
    let prepared = match finalize_validation_result(t_ctx.ctx, tool_call_id, tool_name, args_val, validation_result) {
        ValidationTransition::Proceed(prepared) => prepared,
        ValidationTransition::Return(outcome) => {
            return Ok(outcome);
        }
    };

    if let Some(signature) = shell_run_signature(&prepared.canonical_name, &prepared.effective_args) {
        t_ctx.ctx.harness_state.record_admitted_shell_command(signature);
    }

    // 3. Execute and Handle Result
    if let Some(outcome) = execute_and_handle_tool_call(
        t_ctx.ctx,
        t_ctx.repeated_tool_attempts,
        t_ctx.turn_modified_files,
        tool_call_id.to_string(),
        &prepared.canonical_name,
        prepared.effective_args,
        None,
        None,
    )
    .await?
    {
        return Ok(Some(outcome));
    }

    Ok(None)
}

/// Validates a tool call against all safety and permission checks.
/// Returns Some(TurnHandlerOutcome) if the turn loop should break/exit/cancel.
/// Returns None if execution should proceed (or if a local error was already handled/pushed).
pub(crate) async fn validate_tool_call<'a>(
    ctx: &mut TurnProcessingContext<'a>,
    tool_call_id: &str,
    tool_name: &str,
    args_val: &serde_json::Value,
) -> Result<ValidationResult> {
    ctx.handle
        .set_progress_phase(vtcode_commons::ui_protocol::ProgressPhase::CheckingPermissions);
    // Early guard: reject empty tool names with a clear error message.
    // This handles malformed LLM responses where tool name is missing.
    // The "empty tool name" phrase is matched by `preflight_failure_is_llm_mistake`.
    if tool_name.trim().is_empty() {
        let outcome = handle_preflight_failure(
            ctx,
            tool_call_id,
            "<empty tool name>",
            "Tool call has an empty tool name. Provide a valid tool name.",
            Some(args_val),
            None,
        );
        return Ok(outcome.map_or(ValidationResult::Handled, ValidationResult::Outcome));
    }

    if !vtcode_core::tools::tool_intent::is_turn_budget_exempt_call(tool_name, args_val)
        && let Some(notice) = ctx.harness_state.record_tool_budget_exhaustion_notice()
    {
        // Mirror the wall-clock exhaustion contract: reject the call with a
        // policy error (full message once, compact stub for later calls in
        // the batch) and let `flush_budget_synthesis_directives` push a single
        // "synthesize now" directive after the batch and arm the tool-free
        // recovery pass. The old behavior broke the turn as `Blocked` with no
        // synthesis pass, so plan mode ended with research but no plan and the
        // model looped on "I'll synthesize the plan" across continue-turns.
        //
        // Control-plane exec calls (blocking `wait`, bounded `inspect`) are
        // checked first: they must neither consume the one-shot first-notice
        // nor arm the synthesis directive, since a long build stays observable
        // through them even after the work budget is exhausted.
        let error_msg = if notice.first_notice {
            notice.exhaustion.policy_violation_message()
        } else {
            notice.exhaustion.skipped_call_message()
        };
        ctx.push_rejected_tool_response(
            tool_call_id,
            Some(tool_name),
            Some(args_val),
            build_turn_budget_error_content(error_msg),
        );
        return Ok(ValidationResult::Blocked);
    }

    if let Some(notice) = ctx.harness_state.record_wall_clock_exhaustion_notice() {
        // Emit the full policy message once (first notice); subsequent rejected
        // calls in the same batch get a compact stub to avoid repeating it N
        // times. The "synthesize now" system directive is pushed after the whole
        // batch by the caller (see `take_wall_clock_directive_pending`) so it is
        // never interleaved between tool responses of the same assistant message.
        let error_msg = if notice.first_notice {
            notice.exhaustion.policy_violation_message()
        } else {
            notice.exhaustion.skipped_call_message()
        };
        ctx.push_rejected_tool_response(
            tool_call_id,
            Some(tool_name),
            Some(args_val),
            build_turn_budget_error_content(error_msg),
        );
        return Ok(ValidationResult::Blocked);
    }

    let mut prepared = match ctx.tool_registry.admit_public_tool_call(tool_name, args_val) {
        Ok(prepared) => prepared,
        Err(err) => {
            if let Some(recovered_prepared) = try_recover_preflight_with_fallback(ctx, tool_name, args_val, &err) {
                tracing::info!(
                    tool = tool_name,
                    recovered_tool = %recovered_prepared.canonical_name,
                    "Recovered tool preflight by applying fallback arguments"
                );
                recovered_prepared
            } else {
                let fallback = preflight_validation_fallback(tool_name, args_val, &err);
                let (fallback_tool, fallback_tool_args) = fallback
                    .as_ref()
                    .map(|(tool, args)| (Some(tool.clone()), Some(args.clone())))
                    .unwrap_or((None, None));
                let error_text = format!("{err:#}");
                if check_is_argument_error(&error_text)
                    || error_text.to_ascii_lowercase().contains("tool preflight validation failed")
                {
                    let outcome =
                        handle_preflight_failure(ctx, tool_call_id, tool_name, &error_text, Some(args_val), fallback);
                    return Ok(outcome.map_or(ValidationResult::Handled, ValidationResult::Outcome));
                }
                ctx.push_rejected_tool_response(
                    tool_call_id,
                    Some(tool_name),
                    Some(args_val),
                    build_validation_error_content_with_fallback(
                        format!("Tool preflight validation failed: {err:#}"),
                        "preflight",
                        fallback_tool,
                        fallback_tool_args,
                    ),
                );
                return Ok(ValidationResult::Blocked);
            }
        }
    };

    // Check correction only after admission against the live schema. Unrelated
    // operations retain the validation episode and its independent failure cap.
    if ctx.session_stats.preflight_recovery.exhausted() {
        ctx.push_rejected_tool_response(
            tool_call_id,
            Some(tool_name),
            Some(args_val),
            build_failure_error_content(
                "Automatic preflight recovery is exhausted; resume with corrected arguments.".to_string(),
                "preflight_circuit_breaker",
            ),
        );
        return Ok(ValidationResult::Outcome(TurnHandlerOutcome::Break(TurnLoopResult::Blocked {
            reason: Some("Automatic preflight recovery is exhausted; type continue to resume.".to_string()),
        })));
    }

    let canonical_tool_name = prepared.canonical_name.clone();
    if !primary_agent_allows_tool(ctx.active_primary_agent.active(), &canonical_tool_name) {
        ctx.push_rejected_tool_response(
            tool_call_id,
            Some(&canonical_tool_name),
            Some(args_val),
            serde_json::to_string(
                &ToolExecutionError::policy_violation(
                    canonical_tool_name.clone(),
                    format!("Tool '{canonical_tool_name}' execution denied by active primary agent policy"),
                )
                .to_json_value(),
            )
            .unwrap_or_else(|_| "{}".to_string()),
        );
        return Ok(ValidationResult::Blocked);
    }

    prepared.effective_args =
        maybe_apply_spool_read_offset_hint(ctx.tool_registry, &canonical_tool_name, &prepared.effective_args);
    prepared.parallel_safe_after_preflight =
        vtcode_core::tools::tool_intent::is_parallel_safe_call(&canonical_tool_name, &prepared.effective_args);
    let effective_args = &prepared.effective_args;

    // PreToolUse hooks run before the argument-dependent guards, the safety
    // gateway, and permission evaluation so rewritten arguments are what every
    // downstream check sees (read-after-write, repeated-read-only, mutation
    // bookkeeping, parallel grouping). Mirrors the pipeline path.
    let hook_phase = match crate::agent::runloop::unified::tool_routing::pipeline_pre_tool_hooks(
        ctx.lifecycle_hooks,
        ctx.renderer,
        &canonical_tool_name,
        effective_args,
        tool_call_id,
    )
    .await
    {
        Ok(Some(PreToolHookPhaseResult::Deny)) => {
            ctx.harness_state.record_denied_tool_call();
            // Surface the denial to the model so it does not silently retry
            // the same call; the reason is also rendered for the user.
            ctx.push_rejected_tool_response(
                tool_call_id,
                Some(&canonical_tool_name),
                Some(effective_args),
                build_failure_error_content(
                    format!("Tool '{canonical_tool_name}' denied by PreToolUse hook"),
                    "policy",
                ),
            );
            return Ok(ValidationResult::Blocked);
        }
        Ok(Some(PreToolHookPhaseResult::Proceed { rewritten_args: Some(rewritten), requires_prompt })) => {
            // Re-validate the rewritten arguments: preflight ran on the
            // original payload, so a rewrite could otherwise bypass schema
            // checks. A hook-produced invalid payload is a hook error — block
            // with a clear message instead of executing malformed input.
            if let Err(err) = ctx
                .tool_registry
                .preflight_validate_harness_call(&canonical_tool_name, &rewritten)
            {
                ctx.harness_state.record_denied_tool_call();
                ctx.push_rejected_tool_response(
                    tool_call_id,
                    Some(&canonical_tool_name),
                    Some(effective_args),
                    build_failure_error_content(
                        format!("PreToolUse hook produced invalid arguments for '{canonical_tool_name}': {err}"),
                        "policy",
                    ),
                );
                return Ok(ValidationResult::Blocked);
            }
            prepared.effective_args = rewritten;
            // Re-derive the intent classification for the rewritten arguments:
            // the parallel-readonly grouping and the mutation bookkeeping below
            // must see what will actually execute, not the pre-rewrite form.
            prepared.readonly_classification =
                !vtcode_core::tools::tool_intent::classify_tool_intent(&canonical_tool_name, &prepared.effective_args)
                    .mutating;
            prepared.parallel_safe_after_preflight =
                vtcode_core::tools::tool_intent::is_parallel_safe_call(&canonical_tool_name, &prepared.effective_args);
            Some(PreToolHookPhaseResult::Proceed { rewritten_args: None, requires_prompt })
        }
        Ok(phase) => phase,
        Err(err) => {
            ctx.harness_state.record_denied_tool_call();
            ctx.push_system_message(format!("Pre-tool hook phase failed: {err}"));
            ctx.push_rejected_tool_response(
                tool_call_id,
                Some(&canonical_tool_name),
                Some(effective_args),
                build_failure_error_content(format!("Pre-tool hook phase failed: {err}"), "policy"),
            );
            return Ok(ValidationResult::Blocked);
        }
    };

    // Only after the hook phase (and possible rewrite) do the argument- and
    // classification-dependent bookkeeping and guards run, so they observe the
    // arguments that will actually execute.
    if !prepared.readonly_classification {
        ctx.harness_state.reset_file_read_family_streak();
    }
    let fallback_recommendation =
        recovery_fallback_for_tool(&canonical_tool_name, &prepared.effective_args).map(|(tool_name, args)| {
            vtcode_core::core::agent::harness_kernel::FallbackRecommendation { tool_name, args, chain: Vec::new() }
        });
    prepared = prepared.with_fallback_recommendation(fallback_recommendation);
    let effective_args = &prepared.effective_args;

    if let Some(outcome) =
        enforce_duplicate_task_tracker_create_guard(ctx, tool_call_id, &canonical_tool_name, effective_args)
    {
        return Ok(outcome);
    }

    if let Some(outcome) = enforce_read_after_write_guard(ctx, tool_call_id, &canonical_tool_name, effective_args) {
        return Ok(outcome);
    }

    if let Some(outcome) = enforce_repeated_read_only_call_guard(
        ctx,
        tool_call_id,
        &canonical_tool_name,
        effective_args,
        prepared.readonly_classification,
    ) {
        return Ok(outcome);
    }

    if let Some(outcome) = enforce_repeated_shell_run_guard(ctx, tool_call_id, &canonical_tool_name, effective_args) {
        return Ok(outcome);
    }

    if let Some(outcome) = enforce_spool_chunk_read_guard(ctx, tool_call_id, &canonical_tool_name, effective_args).await
    {
        return Ok(outcome);
    }

    // Phase 4 Check: Per-tool Circuit Breaker
    let circuit_breaker_blocked = !ctx.circuit_breaker.allow_request_for_tool(&canonical_tool_name);
    if circuit_breaker_blocked {
        let display_tool = tool_action_label(&canonical_tool_name, args_val);
        let (fallback_tool, fallback_tool_args) = prepared
            .fallback_recommendation
            .as_ref()
            .map(|fallback| (Some(fallback.tool_name.clone()), Some(fallback.args.clone())))
            .unwrap_or((None, None));
        let block_reason = format!(
            "Circuit breaker blocked '{display_tool}' due to high failure rate. Switching to autonomous fallback strategy."
        );
        tracing::warn!(tool = %canonical_tool_name, "Circuit breaker open, tool disabled");

        // In interactive mode, attempt recovery prompt; None = user chose to proceed.
        if let Some(result) =
            try_interactive_circuit_recovery(ctx, tool_call_id, &canonical_tool_name, fallback_tool, fallback_tool_args)
                .await?
        {
            ctx.push_system_message(block_reason);
            return Ok(result);
        }
    }

    // Phase 4 Check: Adaptive Rate Limiter
    if let Some(outcome) = acquire_adaptive_rate_limit_slot(ctx, tool_call_id, &canonical_tool_name).await? {
        return Ok(outcome);
    }

    // Unified interactive turns own loop/recovery policy via turn-local guards and
    // the turn balancer. The legacy core loop detector remains available for
    // non-unified autonomous execution paths only.

    let effective_args = &prepared.effective_args;

    let mut safety_approval_justification = None;
    if let Some((outcome, justification)) =
        run_safety_validation_loop(ctx, tool_call_id, &canonical_tool_name, effective_args).await?
    {
        safety_approval_justification = justification;
        if matches!(outcome, ValidationResult::Blocked) {
            return Ok(outcome);
        }
    }

    // Ensure tool permission. The PreToolUse hook phase already ran above;
    // forwarding it here prevents a second hook invocation.
    let permission_result = ensure_tool_permission_with_call_id(
        build_tool_permissions_context_with_safety(ctx, safety_approval_justification.as_deref()),
        &canonical_tool_name,
        Some(effective_args),
        Some(tool_call_id),
        hook_phase,
    )
    .await;

    match permission_result {
        Ok(ToolPermissionFlow::Approved { updated_args }) => {
            if let Some(updated_args) = updated_args {
                // A PermissionRequest hook may supply its own rewrite via
                // `updated_input`; it replaces the arguments the safety
                // gateway and argument-dependent guards evaluated. Validate
                // the schema and re-run the safety gateway against the final
                // arguments so the replacement does not execute under
                // decisions made for earlier arguments.
                if let Err(err) = ctx
                    .tool_registry
                    .preflight_validate_harness_call(&canonical_tool_name, &updated_args)
                {
                    ctx.harness_state.record_denied_tool_call();
                    ctx.push_rejected_tool_response(
                        tool_call_id,
                        Some(&canonical_tool_name),
                        Some(effective_args),
                        build_failure_error_content(
                            format!(
                                "PermissionRequest hook produced invalid arguments for '{canonical_tool_name}': {err}"
                            ),
                            "policy",
                        ),
                    );
                    return Ok(ValidationResult::Blocked);
                }
                let rewritten_differ = *effective_args != updated_args;
                prepared.effective_args = updated_args;
                if rewritten_differ
                    && let Some((outcome, _)) =
                        run_safety_validation_loop(ctx, tool_call_id, &canonical_tool_name, &prepared.effective_args)
                            .await?
                    && matches!(outcome, ValidationResult::Blocked)
                {
                    ctx.harness_state.record_denied_tool_call();
                    return Ok(outcome);
                }
            }
            if canonical_tool_name == tool_names::START_PLANNING {
                ctx.harness_state.clear_task_tracker_create_signatures();
            }
            // Count budget only for calls that pass all validation/permission gates.
            record_tool_call_budget_usage(ctx, &canonical_tool_name, &prepared.effective_args);
            Ok(ValidationResult::Proceed(prepared))
        }
        Ok(ToolPermissionFlow::Denied) => {
            ctx.harness_state.record_denied_tool_call();
            // A permanent `request_user_input` denial (non-interactive runtime,
            // inline UI unavailable, etc.) must be recorded so the planning
            // workflow stops re-forcing the interview AND the tool is suppressed
            // from subsequent catalogs. Without this, the model sees a generic
            // "execution denied by policy" and retries across turns —
            // checkpoint turn_724 shows 7 retries. `handle_failure` is NOT
            // reached on this path (permission denial returns Blocked, not
            // Failure), so we must call `mark_interview_denied()` here too.
            if canonical_tool_name == tool_names::REQUEST_USER_INPUT {
                ctx.plan_session.mark_interview_denied();
                if ctx.is_planning_active() {
                    ctx.harness_state.arm_interview_denial_recovery();
                }
            }
            let denial = if let Some(denial) = ctx.session_stats.last_auto_permission_denial() {
                serde_json::json!({
                    "error": format!("Auto permission review blocked tool '{}': {}", prepared.canonical_name, denial.reason),
                    "reason": denial.reason,
                    "matched_rule": denial.matched_rule,
                    "matched_exception": denial.matched_exception,
                    "review_stage": denial.stage,
                    "next_action": "Choose a safer tool or narrower action that stays within the user's explicit request."
                })
            } else {
                let mut error_json = ToolExecutionError::policy_violation(
                    canonical_tool_name.as_str(),
                    format!("Tool '{}' execution denied by policy", prepared.canonical_name),
                )
                .to_json_value();
                if let Some(diagnostic) = tool_denial_diagnostic(&canonical_tool_name)
                    && let Some(obj) = error_json.as_object_mut()
                {
                    obj.insert("diagnostic".to_string(), diagnostic);
                }
                error_json
            };
            ctx.push_rejected_tool_response(
                tool_call_id,
                Some(&canonical_tool_name),
                Some(effective_args),
                serde_json::to_string(&denial).unwrap_or_else(|_| "{}".to_string()),
            );
            Ok(ValidationResult::Blocked)
        }
        Ok(ToolPermissionFlow::Blocked { reason }) => {
            // A terminal permission flow is still a blocked tool call. Keep
            // its exact denial reason for the first attempts, while routing
            // accounting and fuse handling through the same dispatch guard as
            // every other blocked validation result.
            let guard_response_expected = !ctx.is_recovery_active() || ctx.recovery_pass_used();
            if let Some(outcome) =
                enforce_blocked_tool_call_guard(ctx, tool_call_id, &canonical_tool_name, effective_args)
            {
                if !guard_response_expected {
                    ctx.push_rejected_tool_response(
                        tool_call_id,
                        Some(&canonical_tool_name),
                        Some(effective_args),
                        build_failure_error_content(reason.clone(), "policy"),
                    );
                }
                Ok(ValidationResult::Outcome(outcome))
            } else {
                ctx.push_rejected_tool_response(
                    tool_call_id,
                    Some(&canonical_tool_name),
                    Some(effective_args),
                    build_failure_error_content(reason.clone(), "policy"),
                );
                Ok(ValidationResult::Outcome(TurnHandlerOutcome::Break(TurnLoopResult::Blocked {
                    reason: Some(reason),
                })))
            }
        }
        Ok(ToolPermissionFlow::Exit) => Ok(ValidationResult::Outcome(TurnHandlerOutcome::Break(TurnLoopResult::Exit))),
        Ok(ToolPermissionFlow::Interrupted) => {
            Ok(ValidationResult::Outcome(TurnHandlerOutcome::Break(TurnLoopResult::Cancelled)))
        }
        Err(err) => {
            let err_json = serde_json::json!({
                "error": format!("Failed to evaluate policy for tool '{}': {}", tool_name, err)
            });
            ctx.push_rejected_tool_response(tool_call_id, Some(tool_name), Some(args_val), err_json.to_string());
            Ok(ValidationResult::Blocked)
        }
    }
}
