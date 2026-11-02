//! Plan-approval HITL flow: "Ready to code?" overlay + plan rendering.
//!
//! Isolates the plan-confirmation UI from the start-planning entry flow and from
//! the runloop's response handling. The `execute_plan_confirmation` function is
//! the canonical interface; callers map the returned `PlanConfirmationOutcome` to
//! their own transition logic.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Result;
use tokio::sync::Notify;
use vtcode_core::config::EditorToolConfig;
use vtcode_core::exec::events::PlanApprovalDecision;
use vtcode_core::tools::terminal_app::{EditorLaunchConfig, TerminalAppLauncher};
use vtcode_ui::tui::app::{
    InlineHandle, InlineListItem, InlineListSelection, InlineMessageKind, InlineSession, ListOverlayRequest,
    PlanContent, TransientHotkey, TransientHotkeyAction, TransientHotkeyKey, TransientRequest, TransientSubmission,
};

use crate::agent::runloop::unified::external_editor::run_blocking_with_event_loop_suspended;
use crate::agent::runloop::unified::inline_events::harness::HarnessEventEmitter;
use crate::agent::runloop::unified::overlay_prompt::{
    OverlayWaitOutcome, show_overlay_and_wait, wait_for_overlay_submission,
};
use crate::agent::runloop::unified::planning_workflow::{
    PlanArtifactError, PlanExecutionContext, ValidatedPlanArtifact, complete_approved_plan_handoff,
    resolve_plan_execution_target,
};
use crate::agent::runloop::unified::state::CtrlCState;

/// Result of the plan confirmation flow
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PlanConfirmationOutcome {
    /// User approved execution in the current context.
    Execute,
    /// User approved execution after clearing transient context.
    FreshContext,
    /// Legacy selection retained for replay compatibility.
    AutoAccept,
    /// User wants to edit the plan
    EditPlan,
    /// User cancelled
    Cancel,
    /// User chose to hand off execution to the build primary agent.
    SwitchBuild,
    /// User chose to hand off execution to the auto primary agent.
    SwitchAuto,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PlanApprovalRoute {
    Inline,
    Headless,
    Automatic,
}

pub(crate) fn plan_approval_route(
    require_confirmation: bool,
    supports_inline_ui: bool,
    skip_confirmations: bool,
    full_auto: bool,
) -> PlanApprovalRoute {
    if require_confirmation && !skip_confirmations && !full_auto {
        if supports_inline_ui {
            PlanApprovalRoute::Inline
        } else {
            PlanApprovalRoute::Headless
        }
    } else {
        PlanApprovalRoute::Automatic
    }
}

/// Event identity carried through the approval overlay so all outcomes are
/// recorded against the same planning turn without widening the function API.
pub(crate) struct PlanApprovalTelemetryContext<'a> {
    pub(crate) emitter: Option<&'a HarnessEventEmitter>,
    pub(crate) thread_id: &'a str,
    pub(crate) turn_id: &'a str,
}

pub(crate) struct PlanApprovalRequestContext<'a> {
    pub(crate) plan: &'a ValidatedPlanArtifact,
    pub(crate) skip_confirmations: bool,
    pub(crate) full_auto: bool,
    pub(crate) context_usage_percent: u8,
    /// External-editor configuration used by the `Ctrl+G` "edit plan" hotkey.
    pub(crate) editor: EditorToolConfig,
    /// Workspace root used to resolve a workspace-relative `plan.file_path`.
    pub(crate) workspace_root: PathBuf,
}

fn line_count(text: &str) -> usize {
    text.lines().count().max(1)
}

fn append_message(handle: &InlineHandle, kind: InlineMessageKind, text: impl Into<String>) {
    let text = text.into();
    handle.append_pasted_message(kind, text.clone(), line_count(&text));
}

/// Append markdown content as pre-rendered styled lines so the TUI transcript
/// shows headings, lists, code, and other formatting instead of raw source.
///
/// `append_pasted_message` stores raw text verbatim: the reflow path wraps it
/// as plain text without invoking the markdown parser. Normal assistant output
/// goes through `AnsiRenderer::line(Response, …)` → `write_markdown` →
/// `append_line`, which is why it renders correctly. The approval flow
/// bypassed that pipeline and therefore displayed raw `<proposed_plan>`
/// wrappers and unstyled `Summary:` / `•` prose (see the Sep-04 screenshot).
/// This helper restores the same pipeline for approval transcript content by
/// rendering markdown to `InlineSegment`s and emitting them via `append_line`.
///
/// On rendering failure or empty output the caller receives `false` and should
/// fall back to plain text plus an explicit warning so the user gets clear
/// feedback instead of a silent blank block.
fn append_markdown_message(handle: &InlineHandle, kind: InlineMessageKind, markdown: &str) -> bool {
    use std::sync::Arc;
    use vtcode_core::ui::markdown::{CodeBlockWeight, RenderMarkdownOptions, render_markdown_to_lines_with_options};
    use vtcode_core::ui::theme;
    use vtcode_core::ui::tui::{InlineSegment, convert_style};
    use vtcode_core::utils::ansi::MessageStyle;

    if markdown.trim().is_empty() {
        return false;
    }

    let base_style = match kind {
        InlineMessageKind::Agent => MessageStyle::Response.style(),
        InlineMessageKind::Info => MessageStyle::Info.style(),
        InlineMessageKind::Warning => MessageStyle::Warning.style(),
        InlineMessageKind::Error => MessageStyle::Error.style(),
        _ => MessageStyle::Info.style(),
    };
    let theme_styles = theme::active_styles();
    let rendered = render_markdown_to_lines_with_options(
        markdown,
        base_style,
        &theme_styles,
        None,
        RenderMarkdownOptions {
            preserve_code_indentation: true,
            disable_code_block_table_reparse: false,
            table_max_width: None,
            code_block_weight: CodeBlockWeight::Primary,
        },
    );

    // Collapse consecutive blanks (mirrors AnsiRenderer) and drop a leading
    // failure mode where the parser yields no visible lines.
    let mut lines: Vec<Vec<InlineSegment>> = Vec::with_capacity(rendered.len().max(1));
    let mut fallback = convert_style(base_style);
    if fallback.color.is_none() {
        fallback = fallback.merge_color(Some(theme_styles.foreground));
    }
    let mut last_blank = false;
    for line in rendered {
        let is_blank = line.is_empty();
        if is_blank {
            if last_blank {
                continue;
            }
            last_blank = true;
            lines.push(Vec::new());
            continue;
        }
        last_blank = false;
        let mut segments = Vec::with_capacity(line.segments.len().max(1));
        for seg in line.segments {
            if seg.text.is_empty() {
                continue;
            }
            let converted = convert_style(seg.style);
            let mut inline_style = fallback.clone();
            inline_style.color = None;
            if let Some(color) = converted.color
                && Some(color) != fallback.color
            {
                inline_style.color = Some(color);
            }
            if let Some(bg) = converted.bg_color {
                inline_style.bg_color = Some(bg);
            }
            inline_style.effects = converted.effects | fallback.effects;
            segments.push(InlineSegment { text: seg.text, style: Arc::new(inline_style) });
        }
        lines.push(segments);
    }

    let has_visible = lines.iter().any(|segs| segs.iter().any(|s| !s.text.trim().is_empty()));
    if !has_visible {
        return false;
    }

    for segments in lines {
        handle.append_line(kind, segments);
    }
    true
}

fn render_confirmation_prompt(handle: &InlineHandle, plan: &PlanContent) {
    tracing::info!(
        target: "vtcode.planning_workflow",
        plan_title = %plan.title,
        "render_confirmation_prompt: adding confirmation messages"
    );
    append_message(handle, InlineMessageKind::Info, "Ready to code?");
    append_message(handle, InlineMessageKind::Info, "A plan is ready to execute. Would you like to proceed?");

    if !plan.summary.trim().is_empty() {
        if !append_markdown_message(handle, InlineMessageKind::Agent, plan.summary.trim()) {
            append_message(handle, InlineMessageKind::Agent, plan.summary.clone());
            append_message(
                handle,
                InlineMessageKind::Warning,
                "Plan summary needed a plain-text fallback; markdown rendering produced no visible lines.",
            );
        }
    } else if !plan.title.trim().is_empty() {
        append_message(handle, InlineMessageKind::Info, format!("Plan: {}", plan.title));
    }

    // Keep the approval overlay compact, but put the complete persisted draft
    // in the scrollable transcript so users can review every section before
    // choosing an execution mode. The overlay's `lines` field is intentionally
    // bounded by `render_plan_summary` and must not be used for this content.
    // The harness normalizes plan markdown here so headings, lists, and other
    // formatting render accurately instead of leaking raw `<proposed_plan>`
    // wrappers or collapsing into a wall of text. Content goes through the
    // markdown pipeline (`append_line` with rendered segments) rather than raw
    // `append_pasted_message`, otherwise headings/lists show as source text.
    if !plan.raw_content.trim().is_empty() {
        let (display_markdown, warnings) =
            crate::agent::runloop::unified::plan_blocks::prepare_plan_markdown_for_display(&plan.raw_content);
        append_message(handle, InlineMessageKind::Info, "Implementation plan (full):");
        if display_markdown.trim().is_empty() {
            append_message(
                handle,
                InlineMessageKind::Warning,
                "Plan content could not be rendered (empty after cleanup). See the plan file for details.",
            );
        } else if !append_markdown_message(handle, InlineMessageKind::Agent, &display_markdown) {
            append_message(handle, InlineMessageKind::Agent, display_markdown.clone());
            append_message(
                handle,
                InlineMessageKind::Warning,
                "Plan markdown could not be styled and is shown as plain text; see the plan file for the formatted version.",
            );
        }
        for warning in warnings {
            append_message(handle, InlineMessageKind::Warning, warning);
        }
    }

    if let Some(path) = plan.file_path.as_deref()
        && !path.trim().is_empty()
    {
        append_message(handle, InlineMessageKind::Info, format!("Plan file: {path}"));
    }
    append_message(
        handle,
        InlineMessageKind::Info,
        "Use the confirmation list to continue here, start a fresh thread, or stay in Plan mode.",
    );
}

/// Render a robust, structured summary of the plan for the confirmation overlay.
///
/// Prefers the parsed `phases`/`steps` shape so the plan reads as a clear,
/// scannable checklist. Falls back to the raw content or summary when the
/// structured data is absent, so a malformed or partially synthesized plan
/// still renders something useful instead of a blank panel.
#[cfg(test)]
pub(crate) fn render_structured_plan(plan: &PlanContent) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();

    if !plan.title.trim().is_empty() {
        lines.push(plan.title.trim().to_string());
        lines.push(String::new());
    }

    if !plan.summary.trim().is_empty() {
        for line in plan.summary.trim().lines() {
            lines.push(line.to_string());
        }
        lines.push(String::new());
    }

    let has_phases = plan.phases.iter().any(|phase| !phase.steps.is_empty());
    if has_phases {
        for phase in &plan.phases {
            if phase.steps.is_empty() {
                continue;
            }
            if !phase.name.trim().is_empty() {
                lines.push(format!("## {}", phase.name.trim()));
            }
            for step in &phase.steps {
                let marker = if step.completed { "[x]" } else { "[ ]" };
                lines.push(format!("{} {} {}", marker, step.number, step.description));
                if let Some(details) = step.details.as_ref().filter(|d| !d.trim().is_empty()) {
                    for detail_line in details.lines() {
                        lines.push(format!("      {detail_line}"));
                    }
                }
                if !step.files.is_empty() {
                    lines.push(format!("      files: {}", step.files.join(", ")));
                }
            }
            lines.push(String::new());
        }
    } else if !plan.raw_content.trim().is_empty() {
        for line in plan.raw_content.lines() {
            lines.push(line.to_string());
        }
        lines.push(String::new());
    }

    if !plan.open_questions.is_empty() {
        lines.push("Open questions:".to_string());
        for question in &plan.open_questions {
            lines.push(format!("- {question}"));
        }
        lines.push(String::new());
    }

    if lines.is_empty() {
        lines.push(plan.title.trim().to_string());
    }

    lines
}

// The confirmation prompt header consumes one of the modal's instruction
// rows, so the summary plus steps share the remaining budget. Entries keep
// their full text and wrap to the modal width (never cut mid-sentence with
// an ellipsis); only the step *count* is bounded, with omitted steps
// collapsed into the explicit `… and N more plan steps` overflow row.
// Budgeting uses estimated visual rows at a conservative 76-column content
// width so a long summary may span up to four wrapped rows (per user
// approval) while steps yield room instead of clipping the viewport.
const PLAN_PREVIEW_MAX_LINES: usize = 5;
const PLAN_PREVIEW_VISUAL_BUDGET_ROWS: usize = 7;
const PLAN_SUMMARY_MAX_VISUAL_ROWS: usize = 4;
const PLAN_PREVIEW_ESTIMATED_CONTENT_WIDTH: usize = 76;

/// Collapse internal whitespace so a plan entry stays on one raw modal line.
/// Wrapping (not truncation) handles narrow widths downstream.
fn collapse_plan_line(text: &str) -> String {
    vtcode_commons::formatting::collapse_whitespace(text)
}

/// Estimate wrapped visual rows for a single raw modal line at the
/// conservative preview width. Real terminals are usually wider, so actual
/// wrapped rows are at most the estimate and the budget never over-promises.
fn estimated_plan_visual_rows(text: &str) -> usize {
    text.chars().count().div_ceil(PLAN_PREVIEW_ESTIMATED_CONTENT_WIDTH).max(1)
}

fn numbered_step_description(line: &str) -> Option<String> {
    let trimmed = line.trim();
    let (number, description) = trimmed.split_once('.')?;
    if number.is_empty() || !number.chars().all(|character| character.is_ascii_digit()) {
        return None;
    }

    let description = description.trim();
    (!description.is_empty()).then(|| format!("{number}. {description}"))
}

/// Render a bounded, decision-ready plan synopsis for the inline approval UI.
///
/// The plan file and plan events retain the complete markdown. The approval
/// modal has a small instruction viewport, so it receives the full summary
/// overview paragraph plus as many numbered steps as fit a seven visual-row
/// budget (the header takes one more row). Entries keep their full text and
/// wrap to the modal width; only the step *count* is bounded, with omitted
/// steps collapsed into an explicit `… and N more plan steps` row. The
/// `Summary:` row renders as a label/value overview section (no bullet) so
/// users grasp the change without reading every step.
pub(crate) fn render_plan_summary(plan: &PlanContent) -> Vec<String> {
    let mut lines = Vec::new();
    let mut used_visual_rows = 0usize;
    if !plan.summary.trim().is_empty() {
        let summary_line = format!("Summary: {}", collapse_plan_line(&plan.summary));
        used_visual_rows = estimated_plan_visual_rows(&summary_line).min(PLAN_SUMMARY_MAX_VISUAL_ROWS);
        lines.push(summary_line);
    }

    let structured_steps: Vec<String> = plan
        .phases
        .iter()
        .flat_map(|phase| phase.steps.iter())
        .map(|step| format!("{}. {}", step.number, step.description))
        .collect();
    let step_lines = if structured_steps.is_empty() {
        plan.raw_content.lines().filter_map(numbered_step_description).collect()
    } else {
        structured_steps
    };

    // Fill steps while both the raw-line cap and the visual-row budget hold,
    // reserving one row for the overflow evidence row when steps remain.
    let mut visible_step_count = 0usize;
    while visible_step_count < step_lines.len() {
        let is_last = visible_step_count + 1 == step_lines.len();
        let prospective_raw = lines.len() + visible_step_count + 1 + usize::from(!is_last);
        if prospective_raw > PLAN_PREVIEW_MAX_LINES {
            break;
        }
        let reserve_visual_rows = usize::from(!is_last);
        if used_visual_rows + estimated_plan_visual_rows(&step_lines[visible_step_count]) + reserve_visual_rows
            > PLAN_PREVIEW_VISUAL_BUDGET_ROWS
        {
            break;
        }
        used_visual_rows += estimated_plan_visual_rows(&step_lines[visible_step_count]);
        visible_step_count += 1;
    }
    lines.extend(step_lines.iter().take(visible_step_count).map(|line| collapse_plan_line(line)));
    if visible_step_count < step_lines.len() {
        lines.push(format!("… and {} more plan steps", step_lines.len() - visible_step_count));
    }

    if lines.is_empty() {
        let fallback = if plan.title.trim().is_empty() {
            "Plan details are available in the plan file.".to_string()
        } else {
            format!("Plan: {}", plan.title.trim())
        };
        lines.push(collapse_plan_line(&fallback));
    }

    lines
}

pub(crate) fn build_plan_confirmation_request_with_context(
    plan: &PlanContent,
    draft_incomplete: bool,
    context_usage_percent: u8,
) -> TransientRequest {
    tracing::info!(
        target: "vtcode.planning_workflow",
        draft_incomplete,
        "build_plan_confirmation_request: building overlay request"
    );
    let mut lines = render_plan_summary(plan);
    lines.insert(0, "A plan is ready to execute. Would you like to proceed?".to_string());

    let footer_hint = plan
        .file_path
        .as_ref()
        .map(|path| format!("ctrl-g to edit in VS Code · {path}"));
    let fresh_recommended = context_usage_percent >= 50;
    let items = vec![
        InlineListItem {
            title: "Yes, implement this plan".to_string(),
            subtitle: Some("Continue with the current context and confirmation policy.".to_string()),
            badge: (!fresh_recommended).then(|| "Recommended".to_string()),
            indent: 0,
            selection: Some(InlineListSelection::PlanApprovalExecute),
            search_value: None,
            ..Default::default()
        },
        InlineListItem {
            title: "Yes, clear context and implement".to_string(),
            subtitle: Some(format!("Fresh thread. Context: {context_usage_percent}% used.")),
            badge: fresh_recommended.then(|| "Recommended".to_string()),
            indent: 0,
            selection: Some(InlineListSelection::PlanApprovalFreshContext),
            search_value: None,
            ..Default::default()
        },
        InlineListItem {
            title: "Yes, switch to Auto and implement".to_string(),
            subtitle: Some(
                "Use the Auto agent's unattended confirmation policy; safety gates remain active.".to_string(),
            ),
            badge: None,
            indent: 0,
            selection: Some(InlineListSelection::PlanApprovalSwitchAuto),
            search_value: None,
            ..Default::default()
        },
        InlineListItem {
            title: "No, stay in Plan mode".to_string(),
            subtitle: Some("Return to planning and revise the plan.".to_string()),
            badge: None,
            indent: 0,
            selection: Some(InlineListSelection::PlanApprovalEditPlan),
            search_value: None,
            ..Default::default()
        },
    ];

    let selected = if draft_incomplete {
        InlineListSelection::PlanApprovalEditPlan
    } else if fresh_recommended {
        InlineListSelection::PlanApprovalFreshContext
    } else {
        InlineListSelection::PlanApprovalExecute
    };

    TransientRequest::List(ListOverlayRequest {
        title: "Ready to code?".to_string(),
        lines,
        footer_hint,
        items,
        selected: Some(selected),
        search: None,
        hotkeys: vec![TransientHotkey {
            key: TransientHotkeyKey::CtrlChar('g'),
            action: TransientHotkeyAction::LaunchEditor,
        }],
        status: None,
    })
}

/// Map a plan-confirmation overlay submission to its [`PlanConfirmationOutcome`].
///
/// Kept as a pure function so the selection→outcome mapping (including the
/// `SwitchBuild`/`SwitchAuto` handoff outcomes) is unit-testable without the
/// TUI driver. `execute_plan_confirmation_with_context` intercepts the
/// `LaunchEditor` hotkey before this mapping to open the plan file; the
/// `EditPlan` fallback below is retained so a missed interception still keeps
/// the user in Plan mode rather than silently executing. Any other
/// unrecognized selection cancels.
pub(crate) fn plan_confirmation_submission_to_outcome(
    submission: &TransientSubmission,
) -> Option<PlanConfirmationOutcome> {
    match submission {
        TransientSubmission::Selection(InlineListSelection::PlanApprovalExecute) => {
            Some(PlanConfirmationOutcome::Execute)
        }
        TransientSubmission::Selection(InlineListSelection::PlanApprovalFreshContext) => {
            Some(PlanConfirmationOutcome::FreshContext)
        }
        TransientSubmission::Selection(InlineListSelection::PlanApprovalAutoAccept) => {
            Some(PlanConfirmationOutcome::AutoAccept)
        }
        TransientSubmission::Selection(InlineListSelection::PlanApprovalEditPlan) => {
            Some(PlanConfirmationOutcome::EditPlan)
        }
        TransientSubmission::Selection(InlineListSelection::PlanApprovalSwitchBuild) => {
            Some(PlanConfirmationOutcome::SwitchBuild)
        }
        TransientSubmission::Selection(InlineListSelection::PlanApprovalSwitchAuto) => {
            Some(PlanConfirmationOutcome::SwitchAuto)
        }
        TransientSubmission::Hotkey(TransientHotkeyAction::LaunchEditor) => Some(PlanConfirmationOutcome::EditPlan),
        TransientSubmission::Selection(_) => Some(PlanConfirmationOutcome::Cancel),
        _ => None,
    }
}

/// Internal wait result: a real user decision, or a request to open the plan
/// draft in the configured external editor and then re-show the approval
/// overlay with any saved edits.
enum PlanConfirmationWait {
    Outcome(PlanConfirmationOutcome),
    EditInExternalEditor,
}

/// Map an approval-overlay submission to its internal wait result.
/// Shared by the overlay-first initial wait and the re-show path after an
/// external-editor edit so the two cannot diverge.
fn map_plan_confirmation_submission(submission: TransientSubmission) -> Option<PlanConfirmationWait> {
    match submission {
        TransientSubmission::Hotkey(TransientHotkeyAction::LaunchEditor) => {
            Some(PlanConfirmationWait::EditInExternalEditor)
        }
        other => plan_confirmation_submission_to_outcome(&other).map(PlanConfirmationWait::Outcome),
    }
}

/// Execute the plan confirmation HITL flow.
///
/// The plan is rendered as static transcript markdown plus an inline confirmation list.
#[cfg(test)]
pub(crate) async fn execute_plan_confirmation(
    handle: &InlineHandle,
    session: &mut InlineSession,
    plan_content: PlanContent,
    draft_incomplete: bool,
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
) -> Result<PlanConfirmationOutcome> {
    // Tests never launch a real editor: disable external editing so a stray
    // `Ctrl+G` submission falls back to a warning plus a re-shown overlay.
    let editor = EditorToolConfig { enabled: false, ..EditorToolConfig::default() };
    execute_plan_confirmation_with_context(
        handle,
        session,
        plan_content,
        draft_incomplete,
        0,
        &editor,
        Path::new("."),
        ctrl_c_state,
        ctrl_c_notify,
    )
    .await
    .map(|(outcome, _)| outcome)
}

/// Show the approval overlay, returning the user's decision plus the edited
/// plan artifact when the user changed the draft in their external editor.
///
/// `Ctrl+G` no longer dismisses the modal and seeds a bare `/edit` command.
/// Instead the configured editor opens the persisted plan file and, after the
/// editor closes, the saved markdown is re-validated and the approval overlay
/// is shown again so the user can review and approve seamlessly.
pub(crate) async fn execute_plan_confirmation_with_context(
    handle: &InlineHandle,
    session: &mut InlineSession,
    plan_content: PlanContent,
    draft_incomplete: bool,
    context_usage_percent: u8,
    editor_config: &EditorToolConfig,
    workspace_root: &Path,
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
) -> Result<(PlanConfirmationOutcome, Option<ValidatedPlanArtifact>)> {
    tracing::info!(
        target: "vtcode.planning_workflow",
        "execute_plan_confirmation: rendering confirmation prompt and showing overlay"
    );
    let mut plan = plan_content;
    let mut edited_plan: Option<ValidatedPlanArtifact> = None;
    // Overlay-first: publish the compact approval overlay before the heavy
    // full-markdown transcript render so the user sees a decision gate
    // immediately. The transcript fills in behind the overlay (wheel-scroll
    // passes through) rather than blocking the gate.
    let first_request = build_plan_confirmation_request_with_context(&plan, draft_incomplete, context_usage_percent);
    let _status_wait = handle.program_status_wait(vtcode_commons::program_status::InteractionKind::Permission);
    handle.show_transient(first_request);
    handle.force_redraw();
    tokio::task::yield_now().await;
    render_confirmation_prompt(handle, &plan);
    handle.force_redraw();

    let mut first_wait = true;
    loop {
        let wait = if first_wait {
            first_wait = false;
            wait_for_overlay_submission(handle, session, ctrl_c_state, ctrl_c_notify, map_plan_confirmation_submission)
                .await?
        } else {
            show_overlay_and_wait(
                handle,
                session,
                build_plan_confirmation_request_with_context(&plan, draft_incomplete, context_usage_percent),
                ctrl_c_state,
                ctrl_c_notify,
                map_plan_confirmation_submission,
            )
            .await?
        };

        match wait {
            OverlayWaitOutcome::Submitted(PlanConfirmationWait::Outcome(outcome)) => {
                tracing::info!(
                    target: "vtcode.planning_workflow",
                    overlay_wait_outcome = "done",
                    "execute_plan_confirmation: overlay wait completed"
                );
                return Ok((outcome, edited_plan));
            }
            OverlayWaitOutcome::Submitted(PlanConfirmationWait::EditInExternalEditor) => {
                if let Some(artifact) = open_plan_in_external_editor(handle, &plan, editor_config, workspace_root).await
                {
                    plan = PlanContent::from_markdown(
                        plan.title.clone(),
                        &artifact.text,
                        Some(artifact.plan_file.to_string_lossy().into_owned()),
                    );
                    edited_plan = Some(artifact);
                }
                // Re-show the approval overlay with the (possibly updated) plan.
                continue;
            }
            OverlayWaitOutcome::Cancelled
            | OverlayWaitOutcome::Interrupted
            | OverlayWaitOutcome::Deferred
            | OverlayWaitOutcome::Exit => {
                tracing::info!(
                    target: "vtcode.planning_workflow",
                    overlay_wait_outcome = "cancelled",
                    "execute_plan_confirmation: overlay wait cancelled"
                );
                return Ok((PlanConfirmationOutcome::Cancel, edited_plan));
            }
        }
    }
}

/// Open the persisted plan draft in the configured external editor, wait for
/// it to close, then re-read and re-validate the saved markdown.
///
/// Returns the validated edited artifact when the user saved a valid plan.
/// Editor-disabled, missing-file, launch-failure, read-failure, and
/// validation-failure paths report a transcript message and return `None`, so
/// the caller re-shows the approval overlay instead of losing it.
async fn open_plan_in_external_editor(
    handle: &InlineHandle,
    plan: &PlanContent,
    editor_config: &EditorToolConfig,
    workspace_root: &Path,
) -> Option<ValidatedPlanArtifact> {
    if !editor_config.enabled {
        append_message(
            handle,
            InlineMessageKind::Warning,
            "External editor is disabled (`tools.editor.enabled = false`).",
        );
        return None;
    }

    let Some(raw_path) = plan.file_path.as_deref().map(str::trim).filter(|path| !path.is_empty()) else {
        append_message(
            handle,
            InlineMessageKind::Warning,
            "The plan draft has no file path to open in an external editor.",
        );
        return None;
    };

    // Persisted drafts store a workspace-relative path; resolve absolute paths
    // as-is so an explicit editor target keeps working.
    let plan_file = if Path::new(raw_path).is_absolute() {
        PathBuf::from(raw_path)
    } else {
        workspace_root.join(raw_path)
    };

    // The editor must open with the current proposal loaded. If the persisted
    // file went missing (deleted, moved, or never flushed), materialize it
    // from the in-memory draft before launching so `Ctrl+G` never degrades to
    // a "file not found" warning without opening anything.
    if !plan_file.is_file() {
        if plan.raw_content.trim().is_empty() {
            append_message(handle, InlineMessageKind::Warning, format!("Plan file not found: {}", plan_file.display()));
            return None;
        }
        if let Some(parent) = plan_file.parent()
            && !parent.as_os_str().is_empty()
            && let Err(error) = tokio::fs::create_dir_all(parent).await
        {
            append_message(
                handle,
                InlineMessageKind::Error,
                format!("Failed to create plan directory {}: {error}", parent.display()),
            );
            return None;
        }
        if let Err(error) = tokio::fs::write(&plan_file, &plan.raw_content).await {
            append_message(
                handle,
                InlineMessageKind::Error,
                format!("Failed to restore the plan file {}: {error}", plan_file.display()),
            );
            return None;
        }
    }

    let preferred_editor =
        (!editor_config.preferred_editor.trim().is_empty()).then(|| editor_config.preferred_editor.clone());
    // Only suspend the TUI event loop for terminal editors (nvim/vim/nano/…).
    // GUI editors (VS Code/Zed/…) run in a separate window with `--wait`, so
    // the approval overlay stays live behind them and re-shows seamlessly
    // after save+close. This mirrors the transcript file-open coordinator.
    let suspend_tui =
        editor_config.suspend_tui && TerminalAppLauncher::editor_command_requires_terminal(preferred_editor.as_deref());
    let workspace = workspace_root.to_path_buf();
    let launch_file = plan_file.clone();
    let launch = run_blocking_with_event_loop_suspended(handle, suspend_tui, move || {
        let launcher = TerminalAppLauncher::new(workspace);
        launcher.launch_editor_with_config(
            Some(launch_file),
            EditorLaunchConfig { preferred_editor, wait_for_editor: true },
        )
    })
    .await;
    handle.force_redraw();

    if let Err(error) = launch {
        append_message(
            handle,
            InlineMessageKind::Error,
            format!(
                "Failed to launch editor for {}: {error}. Set tools.editor.preferred_editor (e.g. \"code --wait\"), or set EDITOR/VISUAL, or install an editor in PATH.",
                plan_file.display()
            ),
        );
        return None;
    }

    let text = match tokio::fs::read_to_string(&plan_file).await {
        Ok(text) => text,
        Err(error) => {
            append_message(
                handle,
                InlineMessageKind::Error,
                format!("Failed to read the edited plan {}: {error}", plan_file.display()),
            );
            return None;
        }
    };

    match ValidatedPlanArtifact::from_text(plan_file, text) {
        Ok(artifact) => {
            append_message(
                handle,
                InlineMessageKind::Info,
                "Plan updated from the external editor. Review the changes, then approve or keep revising.",
            );
            Some(artifact)
        }
        Err(error) => {
            append_message(
                handle,
                InlineMessageKind::Warning,
                format!("The edited plan did not validate, so the previous draft is still active: {error}"),
            );
            None
        }
    }
}

/// Load the persisted plan draft for an approval request received on a later
/// turn, such as a textual `approve` message. The initial plan turn passes its
/// draft directly through `execute_plan_approval`; subsequent turns must use
/// the persisted copy so they can present the same popup instead of bypassing
/// confirmation.
pub(crate) async fn load_plan_text_for_approval(
    tool_registry: &ToolRegistry,
) -> Result<ValidatedPlanArtifact, PlanArtifactError> {
    let plan_file = tool_registry
        .planning_workflow_state()
        .get_plan_file()
        .await
        .ok_or(PlanArtifactError::Missing)?;
    let text = tokio::fs::read_to_string(&plan_file)
        .await
        .map_err(|source| PlanArtifactError::Read { path: plan_file.clone(), source })?;
    ValidatedPlanArtifact::from_text(plan_file, text)
}

use crate::agent::runloop::unified::planning_workflow_state::PlanningWorkflowSessionState;
use crate::agent::runloop::unified::turn::context::{TurnHandlerOutcome, TurnLoopResult};
use vtcode_core::tools::registry::ToolRegistry;

/// Execute the plan-approval overlay and return the corresponding turn outcome.
///
/// This is the canonical interface for the inline plan-confirmation UI. It
/// renders the plan, shows the confirmation overlay, and maps the user's
/// choice onto the appropriate `TurnHandlerOutcome` (break/switch/continue).
///
/// Separated from `TurnProcessingContext` so the planning module owns the
/// approval flow and callers only provide the minimal dependencies.
pub(crate) async fn execute_plan_approval(
    tool_registry: &mut ToolRegistry,
    plan_session: &mut PlanningWorkflowSessionState,
    handle: &InlineHandle,
    session: &mut InlineSession,
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
    request: PlanApprovalRequestContext<'_>,
    telemetry: PlanApprovalTelemetryContext<'_>,
) -> Result<TurnHandlerOutcome> {
    tracing::info!(
        target: "vtcode.planning_workflow",
        "execute_plan_approval: showing confirmation dialog"
    );

    let plan_content = PlanContent::from_markdown(
        "Implementation Plan".to_string(),
        &request.plan.text,
        Some(request.plan.plan_file.to_string_lossy().into_owned()),
    );
    let outcome = execute_plan_confirmation_with_context(
        handle,
        session,
        plan_content,
        false,
        request.context_usage_percent,
        &request.editor,
        &request.workspace_root,
        ctrl_c_state,
        ctrl_c_notify,
    )
    .await;

    tracing::info!(
        target: "vtcode.planning_workflow",
        overlay_outcome = ?outcome.as_ref().ok(),
        "execute_plan_approval: dialog closed"
    );

    // Prefer the plan the user saved in their external editor; otherwise fall
    // back to the artifact that produced the overlay.
    let (outcome, approved_plan) = match outcome {
        Ok((outcome, edited_plan)) => (Ok(outcome), edited_plan.unwrap_or_else(|| request.plan.clone())),
        Err(error) => (Err(error), request.plan.clone()),
    };

    match outcome {
        Ok(PlanConfirmationOutcome::EditPlan) => {
            super::resolve_plan_approval(
                plan_session,
                telemetry.emitter,
                telemetry.thread_id,
                telemetry.turn_id,
                PlanApprovalDecision::Revise,
                false,
            );
            tracing::info!(
                target: "vtcode.planning_workflow",
                "User chose to revise the plan via inline overlay; remaining in Planning workflow"
            );
            Ok(TurnHandlerOutcome::Break(TurnLoopResult::Completed { plan_approved_execution_pending: false }))
        }
        Ok(PlanConfirmationOutcome::Cancel) | Err(_) => {
            super::resolve_plan_approval(
                plan_session,
                telemetry.emitter,
                telemetry.thread_id,
                telemetry.turn_id,
                PlanApprovalDecision::Cancel,
                false,
            );
            tracing::info!(
                target: "vtcode.planning_workflow",
                "User dismissed the plan via inline overlay; remaining in Planning workflow"
            );
            Ok(TurnHandlerOutcome::Break(TurnLoopResult::Completed { plan_approved_execution_pending: false }))
        }
        Ok(approval) => {
            let (decision, skip_confirmations, execution_context) = match approval {
                PlanConfirmationOutcome::Execute => {
                    (PlanApprovalDecision::Execute, request.skip_confirmations, PlanExecutionContext::Current)
                }
                PlanConfirmationOutcome::FreshContext => {
                    (PlanApprovalDecision::FreshContext, request.skip_confirmations, PlanExecutionContext::Fresh)
                }
                PlanConfirmationOutcome::AutoAccept => {
                    (PlanApprovalDecision::AutoAccept, true, PlanExecutionContext::Current)
                }
                PlanConfirmationOutcome::SwitchBuild => {
                    (PlanApprovalDecision::SwitchBuild, false, PlanExecutionContext::Current)
                }
                PlanConfirmationOutcome::SwitchAuto => {
                    (PlanApprovalDecision::SwitchAuto, true, PlanExecutionContext::Current)
                }
                PlanConfirmationOutcome::EditPlan | PlanConfirmationOutcome::Cancel => {
                    tracing::debug!(target: "vtcode.planning_workflow", "approval outcome was already handled");
                    return Ok(TurnHandlerOutcome::Break(TurnLoopResult::Completed {
                        plan_approved_execution_pending: false,
                    }));
                }
            };
            let target =
                resolve_plan_execution_target(decision, execution_context, skip_confirmations, request.full_auto);
            let handoff =
                complete_approved_plan_handoff(tool_registry, plan_session, handle, approved_plan, target).await;
            let handoff = match handoff {
                Ok(handoff) => handoff,
                Err(err) => {
                    tracing::warn!(target: "vtcode.planning_workflow", error = %err, "approved-plan handoff blocked");
                    append_message(handle, InlineMessageKind::Error, format!("Plan execution is blocked: {err}"));
                    return Ok(TurnHandlerOutcome::Break(TurnLoopResult::Completed {
                        plan_approved_execution_pending: false,
                    }));
                }
            };
            super::resolve_plan_approval(
                plan_session,
                telemetry.emitter,
                telemetry.thread_id,
                telemetry.turn_id,
                decision,
                false,
            );
            debug_assert_eq!(handoff.target, target);
            Ok(TurnHandlerOutcome::SwitchPrimaryAgentWithPolicy { target })
        }
    }
}

#[cfg(test)]
mod tests;
