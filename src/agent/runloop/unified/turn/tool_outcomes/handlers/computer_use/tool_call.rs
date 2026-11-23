//! Model-initiated computer turns through the `computer_use` tool.
//!
//! The turn loop intercepts validated `computer_use` calls here instead of
//! registry execution: the dispatcher, the permission-flow approval, and the
//! isolated target all live on the binary side of the core/binary boundary,
//! where registry executors cannot reach. The registered executor stays a
//! fail-closed stub for defense in depth.
//!
//! Every outcome maps onto the generic [`ToolPipelineOutcome`] contract so
//! batch tracking, circuit diagnostics, harness events, and transcript
//! rendering behave exactly like any other tool: completed turns report
//! redacted lines, denials and cancellations are `Cancelled`, and every
//! fail-closed refusal is a `Failure` carrying its guidance text.

use std::sync::Arc;

use anyhow::{Context, Result, anyhow};
use serde_json::Value;
use vtcode_core::core::agent::events::{
    tool_invocation_completed_event, tool_invocation_updated_event, tool_output_completed_event,
    tool_output_started_event,
};
use vtcode_core::exec::events::{AppPreviewItem, ToolCallStatus, tool_outcome_from_status};
use vtcode_core::llm::provider::{LLMRequest, Message, MessageContent, MessageRole};

use super::execution::{
    ComputerTurnOutcome, ComputerTurnOutcomeKind, ComputerTurnProgress, ComputerTurnProposal, ComputerTurnRuntime,
    execute_proposed_turn, propose_computer_turn,
};
use super::report::turn_report_lines;
use super::{ApprovalRequest, NATIVE_SANDBOX_BRIEFING, TOOL_NAME, approval_args, request_target_approval};
use crate::agent::runloop::unified::inline_events::harness::HarnessEventEmitter;
use crate::agent::runloop::unified::tool_pipeline::{
    emit_tool_start_if_needed, resolve_harness_item_identity,
    status::{ToolExecutionStatus, ToolPipelineOutcome},
};
use crate::agent::runloop::unified::turn::context::TurnProcessingContext;

/// Common public suffixes plus RFC-reserved names, for bare-domain
/// detection. Deliberately small: unknown suffixes escape (documented)
/// rather than risk false refuses on filenames and versions.
const EXTERNAL_ADDRESS_SUFFIXES: &[&str] = &[
    "com",
    "net",
    "org",
    "io",
    "dev",
    "app",
    "ai",
    "co",
    "edu",
    "gov",
    "info",
    "biz",
    "us",
    "uk",
    "de",
    "fr",
    "jp",
    "local",
    "localhost",
    "invalid",
    "test",
    "example",
];

/// Whether a host part names an external address: IP literal, localhost,
/// or a dotted name under a known suffix. Strict on shape (valid hostname
/// characters, no leading/trailing dots or dashes) so versions, filenames,
/// and times fail open instead of refusing legitimate preview tasks.
fn host_is_external_address(hostport: &str) -> bool {
    if let Some(rest) = hostport.strip_prefix('[') {
        return rest
            .split_once(']')
            .is_some_and(|(inner, _)| inner.parse::<std::net::IpAddr>().is_ok());
    }
    if hostport.parse::<std::net::IpAddr>().is_ok() {
        return true;
    }
    let host = match hostport.rsplit_once(':') {
        Some((head, port)) if !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()) => head,
        _ => hostport,
    };
    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }
    match host.rsplit_once('.') {
        Some((head, tld)) => {
            !head.is_empty()
                && head.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
                && !head.starts_with(['.', '-'])
                && !head.ends_with(['.', '-'])
                && EXTERNAL_ADDRESS_SUFFIXES.contains(&tld.to_ascii_lowercase().as_str())
        }
        None => false,
    }
}

/// Whether the task names an address outside the offline preview: URL
/// schemes, IP literals, localhost, or bare domains. Guards the
/// fail-closed refusal above. Known limits, all fail-open: unknown
/// suffixes, scheme-less brand names ("screenshot google"), and single
/// labels escape to the system briefing instead; preview tasks that
/// merely mention a domain ("type example.com into the field") over-match.
fn task_requests_external_address(task: &str) -> bool {
    task.split_whitespace().any(|raw| {
        let token = raw.trim_matches(['\'', '"', '(', ')', ',', ';', '!', '?', '.']);
        let lower = token.to_ascii_lowercase();
        if lower.starts_with("http://") || lower.starts_with("https://") {
            return true;
        }
        let hostport = token.split(['/', '?', '#']).next().unwrap_or_default();
        host_is_external_address(hostport)
    })
}

/// Run one model-requested computer turn for `task`, approving through the
/// permission flow. Borrows never overlap: correlate, propose, approve, and
/// execute each take the turn context in turn.
///
/// The wrapper owns the outer tool row end to end: it reuses the streamed
/// invocation id (so progress and completion land on the row the model call
/// opened instead of orphaning it), streams dispatcher progress against it,
/// and completes it exactly once for every outcome. The dispatcher itself
/// only ever emits progress.
pub(crate) async fn handle_computer_use_tool_call(
    ctx: &mut TurnProcessingContext<'_>,
    args_val: &Value,
    tool_call_id: &str,
) -> Result<ToolPipelineOutcome> {
    // Correlate before validating: even refused turns must complete the
    // streamed row, or it dangles `in progress` until turn teardown fails it.
    let harness_item_id = correlate_outer_row(ctx, args_val, tool_call_id);
    let progress = ComputerTurnProgress {
        emitter: ctx.harness_emitter,
        item_id: &harness_item_id,
        call_id: tool_call_id,
    };
    let complete = |ctx: &TurnProcessingContext<'_>,
                    args: &Value,
                    status: ToolCallStatus,
                    output: &str,
                    previews: Vec<AppPreviewItem>| {
        complete_outer_row(ctx.harness_emitter, &harness_item_id, tool_call_id, args, status, output, previews);
    };

    let Some(task) = args_val
        .get("task")
        .and_then(Value::as_str)
        .filter(|task| !task.trim().is_empty())
    else {
        let message = "computer_use needs a non-empty task describing one screen-sized goal.";
        complete(ctx, args_val, ToolCallStatus::Failed, message, Vec::new());
        return Ok(failure_outcome(message));
    };
    // Fail closed on tasks the offline sandbox cannot serve: without this,
    // an external-URL task would boot the placeholder preview, screenshot
    // it, and report success — a silent wrong answer the redacted outer
    // transcript cannot catch. Refuse before config, provider, proposal,
    // and approval; the check needs only the task text.
    if task_requests_external_address(task) {
        let outcome = ComputerTurnOutcome {
            kind: ComputerTurnOutcomeKind::Disabled {
                reason: "external address outside the offline preview",
            },
            model: ctx.config.model.clone(),
            usage: None,
        };
        let report = turn_report_lines(&outcome).join("\n");
        complete(ctx, args_val, ToolCallStatus::Failed, &report, Vec::new());
        return Ok(map_computer_outcome(&outcome));
    }
    // Footer status for the whole turn (fire-and-forget like the pipeline's
    // own tool status; the turn boundary restores it). The TUI input reads
    // the shared prefix back to paint the running accent.
    ctx.handle.set_input_status(Some(running_status_line(args_val)), None);

    let computer_use = match ctx.vt_cfg.as_ref() {
        Some(config) => config.computer_use.clone(),
        None => match crate::main_helpers::load_workspace_config(&ctx.config.workspace)
            .context("Failed to load VT Code configuration")
        {
            Ok(loaded) => loaded.config().computer_use.clone(),
            Err(err) => {
                let message = format!("Computer turn failed to load configuration: {err:#}");
                complete(ctx, args_val, ToolCallStatus::Failed, &message, Vec::new());
                return Ok(failure_outcome(&message));
            }
        },
    };
    let model = ctx.config.model.clone();
    let provider_name = ctx.provider_client.name().to_string();
    let request = LLMRequest {
        model,
        messages: Arc::new(vec![
            Message {
                role: MessageRole::System,
                content: MessageContent::Text(NATIVE_SANDBOX_BRIEFING.to_string()),
                ..Default::default()
            },
            Message {
                role: MessageRole::User,
                content: MessageContent::Text(task.to_string()),
                ..Default::default()
            },
        ]),
        ..Default::default()
    };
    // The dispatcher is infallible today (every failure is an outcome, not an
    // `Err`), but the row must close even if that changes: a bare `?` here
    // would orphan it.
    let proposal = match propose_computer_turn(
        &**ctx.provider_client,
        request,
        &computer_use,
        &mut ComputerTurnRuntime {
            stop: ctx.ctrl_c_state,
            notify: ctx.ctrl_c_notify,
            stats: ctx.session_stats,
        },
        progress,
    )
    .await
    {
        Ok(proposal) => proposal,
        Err(err) => {
            let message = format!("Computer turn failed: {err:#}");
            complete(ctx, args_val, ToolCallStatus::Failed, &message, Vec::new());
            return Ok(failure_outcome(&message));
        }
    };
    let (outcome, event_args) = match proposal {
        ComputerTurnProposal::Done(outcome) => (outcome, args_val.clone()),
        ComputerTurnProposal::Proceed(proposed) => {
            // Approval infrastructure failures are tool data, not turn
            // aborts (matching sibling tools); usage still records. The row
            // still closes with the merged args, like every approved path.
            let granted = match request_target_approval(ctx, &proposed.approval).await {
                Ok(granted) => granted,
                Err(err) => {
                    ctx.session_stats.record_usage(&provider_name, proposed.usage.as_ref());
                    let merged = merged_event_args(task, &proposed.approval);
                    let message = format!("Computer turn approval failed: {err:#}");
                    complete(ctx, &merged, ToolCallStatus::Failed, &message, Vec::new());
                    return Ok(failure_outcome(&message));
                }
            };
            // Merge the task with the approved batch: the row keeps its task
            // headline while the details gain target, kinds, and recipient.
            let merged = merged_event_args(task, &proposed.approval);
            update_outer_row(ctx.harness_emitter, &harness_item_id, tool_call_id, &merged);
            let outcome = match execute_proposed_turn(
                proposed,
                &computer_use,
                &mut ComputerTurnRuntime {
                    stop: ctx.ctrl_c_state,
                    notify: ctx.ctrl_c_notify,
                    stats: ctx.session_stats,
                },
                granted,
                progress,
            )
            .await
            {
                Ok(outcome) => outcome,
                Err(err) => {
                    let message = format!("Computer turn failed: {err:#}");
                    complete(ctx, &merged, ToolCallStatus::Failed, &message, Vec::new());
                    return Ok(failure_outcome(&message));
                }
            };
            (outcome, merged)
        }
    };
    ctx.session_stats.record_usage(&provider_name, outcome.usage.as_ref());
    let report = turn_report_lines(&outcome).join("\n");
    let (status, previews) = match &outcome.kind {
        ComputerTurnOutcomeKind::Completed { preview, .. } => {
            (ToolCallStatus::Completed, preview.as_deref().cloned().into_iter().collect())
        }
        _ => (ToolCallStatus::Failed, Vec::new()),
    };
    complete(ctx, &event_args, status, &report, previews);
    Ok(map_computer_outcome(&outcome))
}

/// Footer status line for the whole computer turn. Built from the shared
/// headline (never a literal) so the producer phrases identically to the
/// transcript row; the TUI input detects it through the shared prefix
/// predicate (pinned by `computer_use_footer_status_marks_running_turns`).
fn running_status_line(args_val: &Value) -> String {
    let (headline, _) = vtcode_commons::tool_headlines::describe_computer_use(args_val);
    format!("Running {headline}")
}

/// Reuse the streamed invocation id for the outer row (the pipeline's own
/// correlation), starting the invocation and its output shell when the
/// streamer did not. Returns the harness item id progress and completion
/// must address.
fn correlate_outer_row(ctx: &mut TurnProcessingContext<'_>, args_val: &Value, tool_call_id: &str) -> String {
    let streamed = ctx
        .harness_state
        .take_streamed_tool_call_item_id(tool_call_id)
        .map(|streamed| streamed.item_id);
    let already_started = streamed.is_some();
    let harness_item_id = streamed.unwrap_or_else(|| resolve_harness_item_identity(tool_call_id).1);
    emit_tool_start_if_needed(
        ctx.harness_emitter,
        already_started,
        &harness_item_id,
        tool_call_id,
        TOOL_NAME,
        args_val,
    );
    if let Some(emitter) = ctx.harness_emitter {
        let _ = emitter.emit(tool_output_started_event(harness_item_id.clone(), Some(tool_call_id)));
    }
    harness_item_id
}

/// Task plus approved batch in one args object: the headline phrases the
/// task while the detail lines phrase target, kinds, and recipient.
fn merged_event_args(task: &str, approval: &ApprovalRequest) -> Value {
    let mut merged = serde_json::Map::new();
    merged.insert("task".to_string(), Value::String(task.to_string()));
    if let Value::Object(obj) = approval_args(approval) {
        merged.extend(obj);
    }
    Value::Object(merged)
}

/// Refresh the outer invocation with the merged args after approval.
fn update_outer_row(emitter: Option<&HarnessEventEmitter>, harness_item_id: &str, tool_call_id: &str, args: &Value) {
    let Some(emitter) = emitter else {
        return;
    };
    let _ = emitter.emit(tool_invocation_updated_event(
        harness_item_id.to_string(),
        TOOL_NAME,
        Some(args),
        Some(tool_call_id),
        ToolCallStatus::InProgress,
    ));
}

/// Complete the outer row exactly once: invocation verdict plus the
/// redacted report text, with the retained preview reference when the turn
/// captured one. Denials and cancellations complete as failed rows
/// (matching the pipeline's own cancelled mapping), never silently.
fn complete_outer_row(
    emitter: Option<&HarnessEventEmitter>,
    harness_item_id: &str,
    tool_call_id: &str,
    args: &Value,
    status: ToolCallStatus,
    output: &str,
    previews: Vec<AppPreviewItem>,
) {
    let Some(emitter) = emitter else {
        return;
    };
    let outcome = tool_outcome_from_status(&status);
    let _ = emitter.emit(tool_invocation_completed_event(
        harness_item_id.to_string(),
        TOOL_NAME,
        Some(args),
        Some(tool_call_id),
        status.clone(),
        outcome,
    ));
    let _ = emitter.emit(tool_output_completed_event(
        harness_item_id.to_string(),
        Some(tool_call_id),
        status,
        None,
        None,
        output.to_string(),
        previews,
    ));
}

fn failure_outcome(message: &str) -> ToolPipelineOutcome {
    ToolPipelineOutcome::from_status(ToolExecutionStatus::Failure {
        error: vtcode_core::tools::registry::ToolExecutionError::from_anyhow(
            TOOL_NAME,
            &anyhow!(message.to_string()),
            0,
            false,
            false,
            Some("unified_runloop"),
        ),
    })
}

/// Map a dispatcher outcome onto the pipeline contract. Denials and user
/// cancellations are `Cancelled` (not failures); refusals carry their
/// guidance as the failure message so the model can relay it.
fn map_computer_outcome(outcome: &ComputerTurnOutcome) -> ToolPipelineOutcome {
    match &outcome.kind {
        ComputerTurnOutcomeKind::ApprovalDenied | ComputerTurnOutcomeKind::Cancelled => {
            ToolPipelineOutcome::from_status(ToolExecutionStatus::Cancelled)
        }
        ComputerTurnOutcomeKind::Completed { .. } => ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
            output: Value::String(turn_report_lines(outcome).join("\n")),
            stdout: None,
            modified_files: Vec::new(),
            command_success: true,
        }),
        _ => failure_outcome(&turn_report_lines(outcome).join("\n")),
    }
}

#[cfg(test)]
mod tests;
