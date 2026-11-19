//! Master-switch and browser-target updates for `/computer-use`.

use std::sync::Arc;

use anyhow::{Context, Result, bail};
use serde_json::Value;
use tokio::sync::Notify;
use vtcode_config::ConfigManager;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::llm::provider::{LLMRequest, Message, MessageContent, MessageRole};
use vtcode_tui_runtime::utils::ansi::MessageStyle;
use vtcode_ui::tui::app::{
    InlineHandle, InlineHeaderContext, InlineListSelection, InlineSession, ListOverlayRequest, TransientRequest,
    TransientSubmission,
};

use super::ui;
use super::{SlashCommandContext, SlashCommandControl};
use crate::agent::runloop::slash_commands::ComputerUseAction;
use crate::agent::runloop::ui_list::{self, Tone};
use crate::agent::runloop::unified::overlay_prompt::{OverlayWaitOutcome, show_overlay_and_wait};
use crate::agent::runloop::unified::state::CtrlCState;
use crate::agent::runloop::unified::turn::tool_outcomes::handlers::computer_use::ApprovalRequest;
use crate::agent::runloop::unified::turn::tool_outcomes::handlers::computer_use::execution::{
    ComputerTurnOutcome, ComputerTurnOutcomeKind, ComputerTurnRuntime, NativeTurnResults, run_computer_turn,
};

pub(crate) async fn handle_computer_use_command(
    ctx: SlashCommandContext<'_>,
    action: ComputerUseAction,
) -> Result<SlashCommandControl> {
    let action = match action {
        ComputerUseAction::Run { task } => return run_computer_task(ctx, task).await,
        other => other,
    };
    let mut manager = crate::main_helpers::load_workspace_config(&ctx.config.workspace)
        .context("Failed to load VT Code configuration")?;
    let mut runtime_cfg = ctx.vt_cfg.as_ref().unwrap_or_else(|| manager.config()).clone();
    let report_target = matches!(action, ComputerUseAction::SetTarget { .. });
    match apply_computer_use_action(&mut runtime_cfg, &mut manager, action) {
        Ok(outcome) => {
            if outcome.changed {
                *ctx.vt_cfg = Some(runtime_cfg.clone());
                sync_computer_use_header(ctx.header_context, ctx.handle, runtime_cfg.computer_use.enabled);
            }
            let lines = if report_target {
                target_report_lines(&runtime_cfg.computer_use)
            } else {
                state_report_lines(&runtime_cfg.computer_use, outcome.changed)
            };
            for line in lines {
                ctx.renderer.line(MessageStyle::Info, &line)?;
            }
        }
        Err(err) => {
            ctx.renderer
                .line(MessageStyle::Error, &format!("Could not change computer use: {err:#}"))?;
        }
    }
    Ok(SlashCommandControl::Continue)
}

#[derive(Debug)]
struct ApplyOutcome {
    changed: bool,
}

/// Mirror the master switch into the header badge: `Some` shows the
/// `COMPUTER USE` pill, `None` hides it. Split out so tests can prove
/// the mapping without building a full slash-command context.
fn sync_computer_use_header(header_context: &mut InlineHeaderContext, handle: &InlineHandle, enabled: bool) {
    header_context.computer_use =
        enabled.then(|| format!("{}on", vtcode_config::constants::ui::HEADER_COMPUTER_USE_PREFIX));
    handle.set_header_context(header_context.clone());
}

fn apply_computer_use_action(
    runtime_cfg: &mut VTCodeConfig,
    manager: &mut ConfigManager,
    action: ComputerUseAction,
) -> Result<ApplyOutcome> {
    let enabled = match action {
        ComputerUseAction::Run { .. } => {
            bail!("internal error: run actions execute through run_computer_task, not apply");
        }
        ComputerUseAction::SetTarget { path } => {
            return set_browser_target(runtime_cfg, manager, &path);
        }
        ComputerUseAction::Status => return Ok(ApplyOutcome { changed: false }),
        ComputerUseAction::On => true,
        ComputerUseAction::Off => false,
        ComputerUseAction::Toggle => !runtime_cfg.computer_use.enabled,
    };
    if enabled && runtime_cfg.computer_use.browser_build.is_none() {
        bail!("Computer use needs a browser target first: run /computer-use target <absolute-path>.");
    }
    runtime_cfg.computer_use.enabled = enabled;
    let mut persisted_cfg = manager.config().clone();
    persisted_cfg.computer_use.enabled = enabled;
    manager
        .save_config(&persisted_cfg)
        .context("Failed to save computer-use state")?;
    Ok(ApplyOutcome { changed: true })
}

fn set_browser_target(runtime_cfg: &mut VTCodeConfig, manager: &mut ConfigManager, path: &str) -> Result<ApplyOutcome> {
    let build_dir = std::path::PathBuf::from(path);
    if !build_dir.is_absolute() {
        bail!("Browser target must be an absolute directory, got '{path}'.");
    }
    if !build_dir.is_dir() {
        bail!("Browser target '{path}' is not a directory.");
    }
    runtime_cfg.computer_use.browser_build = Some(build_dir.clone());
    let mut persisted_cfg = manager.config().clone();
    persisted_cfg.computer_use.browser_build = Some(build_dir);
    manager
        .save_config(&persisted_cfg)
        .context("Failed to save computer-use target")?;
    Ok(ApplyOutcome { changed: true })
}

fn describe_browser_target(computer_use: &vtcode_config::ComputerUseConfig) -> String {
    computer_use
        .browser_build
        .as_ref()
        .map(|build| build.display().to_string())
        .unwrap_or_else(|| "no browser target configured".to_string())
}

fn target_report_lines(computer_use: &vtcode_config::ComputerUseConfig) -> Vec<String> {
    let mut lines = vec![format!("Browser target: {}.", describe_browser_target(computer_use))];
    if !computer_use.enabled {
        lines.push("Run /computer-use on to enable the runtime.".to_string());
    }
    lines
}

fn state_report_lines(computer_use: &vtcode_config::ComputerUseConfig, changed: bool) -> Vec<String> {
    let state = if computer_use.enabled { "on" } else { "off" };
    let verb = if changed {
        "Computer use is now"
    } else {
        "Computer use is"
    };
    let mut lines = vec![format!("{verb} {state}.")];
    if computer_use.enabled {
        lines.push(format!("Browser target: {}.", describe_browser_target(computer_use)));
        lines.push(
            "Each computer turn still asks for approval; tune the target and limits in /config under Computer Use."
                .to_string(),
        );
    }
    lines
}

/// Run one explicit computer turn for a user-supplied task.
///
/// Unlike model-initiated turns (which must go through the permission flow
/// when that path lands), a typed `/computer-use run` command carries its
/// own human intent, so approval is an overlay confirm: no caches, no
/// auto-approval surface, Esc denies. Fails closed outside inline UI.
async fn run_computer_task(ctx: SlashCommandContext<'_>, task: String) -> Result<SlashCommandControl> {
    let mut ctx = ctx;
    if !ui::ensure_selection_ui_available(&mut ctx, "running a computer turn")? {
        return Ok(SlashCommandControl::Continue);
    }
    if !ctx.renderer.supports_inline_ui() {
        ctx.renderer.line(
            MessageStyle::Error,
            "Computer turns need inline UI for approval; run /computer-use run from an interactive session.",
        )?;
        return Ok(SlashCommandControl::Continue);
    }
    let computer_use = match ctx.vt_cfg.as_ref() {
        Some(config) => config.computer_use.clone(),
        None => crate::main_helpers::load_workspace_config(&ctx.config.workspace)
            .context("Failed to load VT Code configuration")?
            .config()
            .computer_use
            .clone(),
    };
    // Fail fast with guidance before spending a model call; the dispatcher
    // re-checks the same gates.
    if computer_use.browser_build.is_none() {
        ctx.renderer
            .line(MessageStyle::Error, "Set a browser target first: /computer-use target <absolute-path>.")?;
        return Ok(SlashCommandControl::Continue);
    }
    if !computer_use.enabled {
        ctx.renderer
            .line(MessageStyle::Error, "Computer use is off — run /computer-use on first.")?;
        return Ok(SlashCommandControl::Continue);
    }
    if !ctx.provider_client.supports_computer_use() {
        ctx.renderer.line(
            MessageStyle::Error,
            &format!("The {} provider does not support computer use.", ctx.provider_client.name()),
        )?;
        return Ok(SlashCommandControl::Continue);
    }
    let request = computer_task_request(&ctx.config.model, &task);
    let provider_name = ctx.provider_client.name().to_string();
    let stop: &CtrlCState = ctx.ctrl_c_state;
    let notify: &Notify = ctx.ctrl_c_notify;
    let mut runtime = ComputerTurnRuntime {
        stop,
        notify,
        stats: ctx.session_stats,
        emitter: ctx.harness_emitter,
    };
    let outcome = run_computer_turn(&**ctx.provider_client, request, &computer_use, &mut runtime, |approval| {
        ask_computer_use_approval(ctx.handle, ctx.session, ctx.ctrl_c_state, ctx.ctrl_c_notify, approval)
    })
    .await?;
    ctx.session_stats.record_usage(&provider_name, outcome.usage.as_ref());
    for line in turn_report_lines(&outcome) {
        ctx.renderer.line(MessageStyle::Info, &line)?;
    }
    Ok(SlashCommandControl::Continue)
}

fn computer_task_request(model: &str, task: &str) -> LLMRequest {
    LLMRequest {
        model: model.to_string(),
        messages: Arc::new(vec![Message {
            role: MessageRole::User,
            content: MessageContent::Text(task.to_string()),
            ..Default::default()
        }]),
        ..Default::default()
    }
}

/// Overlay confirm for a user-initiated turn. Every non-confirm outcome —
/// cancel, interrupt, exit, deferral, or overlay failure — denies.
async fn ask_computer_use_approval(
    handle: &InlineHandle,
    session: &mut InlineSession,
    stop: &Arc<CtrlCState>,
    notify: &Arc<Notify>,
    request: ApprovalRequest,
) -> Result<bool> {
    let outcome = show_overlay_and_wait(
        handle,
        session,
        TransientRequest::List(ListOverlayRequest {
            title: "Run computer actions?".into(),
            lines: vec![
                format!("Target: {} ({})", request.target.display(), request.target.kind()),
                format!("Screenshots go to: {}", request.recipient),
                format!("Proposed actions ({}): {}", request.action_count, request.action_kinds.join(", ")),
                "Deny cancels the turn before anything launches.".into(),
            ],
            footer_hint: Some("Esc denies".into()),
            items: vec![
                ui_list::action(
                    "Run actions",
                    "Execute the proposed batch on the approved target",
                    None,
                    Tone::Warning,
                    Some(InlineListSelection::ConfigAction("run".into())),
                ),
                ui_list::action(
                    "Deny",
                    "Cancel the turn without launching anything",
                    None,
                    Tone::Accent,
                    Some(InlineListSelection::ConfigAction("deny".into())),
                ),
            ],
            // The safe option stays pre-selected so an accidental Enter denies.
            selected: Some(InlineListSelection::ConfigAction("deny".into())),
            search: None,
            hotkeys: Vec::new(),
            status: None,
            plan_markdown: None,
        }),
        stop,
        notify,
        map_computer_use_submission,
    )
    .await?;
    handle.close_modal();
    Ok(matches!(outcome, OverlayWaitOutcome::Submitted(true)))
}

/// Map an overlay submission to an approval decision. Only an explicit
/// "run" selection approves; anything else (including unknown future
/// submission shapes) yields no decision, and every non-confirm wait
/// outcome denies at the call site.
fn map_computer_use_submission(submission: TransientSubmission) -> Option<bool> {
    match submission {
        TransientSubmission::Selection(InlineListSelection::ConfigAction(action)) => Some(action == "run"),
        _ => None,
    }
}

fn turn_report_lines(outcome: &ComputerTurnOutcome) -> Vec<String> {
    match &outcome.kind {
        ComputerTurnOutcomeKind::Disabled { reason } => vec![format!("Computer turn did not run: {reason}.")],
        ComputerTurnOutcomeKind::NoNativeCalls { content, ordinaries } => {
            let mut lines = vec![format!(
                "The model made no computer calls ({} ordinary calls ignored).",
                ordinaries.len()
            )];
            if let Some(text) = content.as_deref().filter(|text| !text.trim().is_empty()) {
                lines.push(format!("Model said: {}", vtcode_commons::formatting::truncate_text(text, 500, "…")));
            }
            lines
        }
        ComputerTurnOutcomeKind::ApprovalDenied => vec!["Computer turn denied.".to_string()],
        ComputerTurnOutcomeKind::BudgetExceeded { proposed, max } => {
            vec![format!(
                "Computer turn refused: {proposed} proposed actions exceed the budget of {max}."
            )]
        }
        ComputerTurnOutcomeKind::ReviewHalted => {
            vec!["Review halted the turn: the batch was judged SUSPECT.".to_string()]
        }
        ComputerTurnOutcomeKind::Cancelled => vec!["Computer turn cancelled.".to_string()],
        ComputerTurnOutcomeKind::Failed { stage, message } => {
            vec![format!("Computer turn failed at the {stage:?} stage: {message}")]
        }
        ComputerTurnOutcomeKind::Completed { results, summary } => {
            let mut lines = vec![format!(
                "Computer turn completed ({}): {} units.",
                outcome.model, summary.executed_units
            )];
            lines.extend(result_report_lines(results));
            lines
        }
    }
}

/// Per-call verdicts without payloads: screenshots never reach the transcript.
fn result_report_lines(results: &NativeTurnResults) -> Vec<String> {
    match results {
        NativeTurnResults::Anthropic(positioned) => positioned
            .iter()
            .map(|(index, block)| {
                let id = block.get("tool_use_id").and_then(Value::as_str).unwrap_or("?");
                let verdict = if block.get("is_error") == Some(&serde_json::json!(true)) {
                    "error"
                } else {
                    "ok"
                };
                format!("call {id} (#{index}): {verdict}")
            })
            .collect(),
        NativeTurnResults::OpenAi(outputs) => outputs
            .iter()
            .map(|output| {
                let id = output.get("call_id").and_then(Value::as_str).unwrap_or("?");
                format!("call {id}: ok")
            })
            .collect(),
    }
}

#[cfg(test)]
#[path = "computer_use/tests.rs"]
mod tests;
