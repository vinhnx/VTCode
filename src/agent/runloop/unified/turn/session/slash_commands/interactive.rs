use anyhow::Result;
use vtcode_core::utils::ansi::MessageStyle;
use vtcode_tui::app::{
    InlineListItem, InlineListSearchConfig, InlineListSelection, ListOverlayRequest,
    TransientEvent, TransientHotkey, TransientHotkeyAction, TransientHotkeyKey, TransientRequest,
    TransientSelectionChange, TransientSubmission,
};

use super::ui::{ensure_selection_ui_available, wait_for_list_modal_selection};
use super::{SlashCommandContext, SlashCommandControl};
use crate::agent::runloop::unified::interactive_features::{
    BackgroundJobSummary, PromptSuggestion, collect_background_jobs, generate_prompt_suggestions,
};

const PROMPT_SUGGESTION_ACTION_PREFIX: &str = "suggest:";

pub(crate) async fn handle_trigger_prompt_suggestions(
    mut ctx: SlashCommandContext<'_>,
) -> Result<SlashCommandControl> {
    if !ensure_selection_ui_available(&mut ctx, "opening prompt suggestions")? {
        return Ok(SlashCommandControl::Continue);
    }

    let suggestions = generate_prompt_suggestions(
        ctx.provider_client.as_ref(),
        ctx.config,
        ctx.vt_cfg.as_ref(),
        &ctx.config.workspace,
        ctx.conversation_history,
        ctx.session_stats,
        ctx.tool_registry,
    )
    .await;
    if suggestions.is_empty() {
        ctx.renderer.line(
            MessageStyle::Info,
            "No prompt suggestions are available yet.",
        )?;
        return Ok(SlashCommandControl::Continue);
    }

    let items = suggestions
        .iter()
        .map(prompt_suggestion_item)
        .collect::<Vec<_>>();
    let selected = items.first().and_then(|item| item.selection.clone());
    ctx.handle.show_list_modal(
        "Prompt suggestions".to_string(),
        vec![
            "Suggestions are derived from your recent VT Code session state.".to_string(),
            "Enter inserts the selected prompt into the composer.".to_string(),
        ],
        items,
        selected,
        Some(InlineListSearchConfig {
            label: "Search prompts".to_string(),
            placeholder: Some("prompt, jobs, review, debug".to_string()),
        }),
    );

    let Some(selection) = wait_for_list_modal_selection(&mut ctx).await else {
        return Ok(SlashCommandControl::Continue);
    };
    let InlineListSelection::ConfigAction(action) = selection else {
        return Ok(SlashCommandControl::Continue);
    };
    let Some(id) = action.strip_prefix(PROMPT_SUGGESTION_ACTION_PREFIX) else {
        return Ok(SlashCommandControl::Continue);
    };
    if let Some(suggestion) = suggestions.iter().find(|suggestion| suggestion.id == id) {
        ctx.handle.apply_suggested_prompt(suggestion.prompt.clone());
    }

    Ok(SlashCommandControl::Continue)
}

pub(crate) async fn handle_toggle_tasks_panel(
    ctx: SlashCommandContext<'_>,
) -> Result<SlashCommandControl> {
    let visible = !ctx.session_stats.task_panel_visible;
    ctx.session_stats.task_panel_visible = visible;
    if visible {
        ctx.handle.show_task_panel();
    } else {
        ctx.handle.hide_task_panel();
    }
    let message = if visible {
        "TODO panel enabled."
    } else {
        "TODO panel hidden."
    };
    ctx.renderer.line(MessageStyle::Info, message)?;
    Ok(SlashCommandControl::Continue)
}

pub(crate) async fn handle_show_jobs_panel(
    mut ctx: SlashCommandContext<'_>,
) -> Result<SlashCommandControl> {
    if !ensure_selection_ui_available(&mut ctx, "opening jobs")? {
        return Ok(SlashCommandControl::Continue);
    }

    let jobs = collect_background_jobs(ctx.tool_registry);
    if jobs.is_empty() {
        ctx.renderer
            .line(MessageStyle::Info, "No active background jobs.")?;
        return Ok(SlashCommandControl::Continue);
    }

    let items = jobs.iter().map(background_job_item).collect::<Vec<_>>();
    let selected = items.first().and_then(|item| item.selection.clone());
    ctx.handle
        .show_transient(TransientRequest::List(ListOverlayRequest {
            title: "Jobs".to_string(),
            lines: vec![
            "Active/background command sessions.".to_string(),
            "Enter or Ctrl+R focuses the selected job output. Ctrl+P previews. Ctrl+X interrupts."
                .to_string(),
        ],
            footer_hint: Some(
                "ctrl-r focus output · ctrl-p preview snapshot · ctrl-x interrupt selected job"
                    .to_string(),
            ),
            items,
            selected: selected.clone(),
            search: Some(InlineListSearchConfig {
                label: "Search jobs".to_string(),
                placeholder: Some("command, cwd, status".to_string()),
            }),
            hotkeys: vec![
                TransientHotkey {
                    key: TransientHotkeyKey::CtrlChar('r'),
                    action: TransientHotkeyAction::FocusJobOutput,
                },
                TransientHotkey {
                    key: TransientHotkeyKey::CtrlChar('p'),
                    action: TransientHotkeyAction::PreviewJobSnapshot,
                },
                TransientHotkey {
                    key: TransientHotkeyKey::CtrlChar('x'),
                    action: TransientHotkeyAction::InterruptJob,
                },
            ],
        }));

    let Some(action) = wait_for_jobs_modal_action(
        ctx.handle,
        ctx.session,
        ctx.ctrl_c_state,
        ctx.ctrl_c_notify,
        selected,
    )
    .await
    else {
        return Ok(SlashCommandControl::Continue);
    };
    let job_id = match action.selection {
        Some(InlineListSelection::ConfigAction(action)) => match action.strip_prefix("job:") {
            Some(job_id) => job_id.to_string(),
            None => return Ok(SlashCommandControl::Continue),
        },
        _ => return Ok(SlashCommandControl::Continue),
    };

    match action.kind {
        JobModalActionKind::Focus => focus_job_output(&mut ctx, &job_id)?,
        JobModalActionKind::Preview => preview_job_snapshot(&mut ctx, &job_id)?,
        JobModalActionKind::Interrupt => interrupt_job(&mut ctx, &job_id)?,
    }
    Ok(SlashCommandControl::Continue)
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct JobModalAction {
    kind: JobModalActionKind,
    selection: Option<InlineListSelection>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum JobModalActionKind {
    Focus,
    Interrupt,
    Preview,
}

async fn wait_for_jobs_modal_action(
    handle: &vtcode_tui::app::InlineHandle,
    session: &mut vtcode_tui::app::InlineSession,
    ctrl_c_state: &std::sync::Arc<crate::agent::runloop::unified::state::CtrlCState>,
    ctrl_c_notify: &std::sync::Arc<tokio::sync::Notify>,
    initial_selection: Option<InlineListSelection>,
) -> Option<JobModalAction> {
    let mut current_selection = initial_selection;
    loop {
        if ctrl_c_state.is_cancel_requested() {
            handle.close_transient();
            handle.force_redraw();
            return None;
        }

        let notify = ctrl_c_notify.clone();
        let maybe_event = tokio::select! {
            _ = notify.notified() => None,
            event = session.next_event() => event,
        };

        let Some(event) = maybe_event else {
            handle.close_transient();
            handle.force_redraw();
            return None;
        };

        match event {
            vtcode_tui::app::InlineEvent::Transient(TransientEvent::SelectionChanged(
                TransientSelectionChange::List(selection),
            )) => {
                current_selection = Some(selection);
            }
            vtcode_tui::app::InlineEvent::Transient(TransientEvent::Submitted(
                TransientSubmission::Selection(selection),
            )) => {
                ctrl_c_state.reset();
                return Some(JobModalAction {
                    kind: JobModalActionKind::Focus,
                    selection: Some(selection),
                });
            }
            vtcode_tui::app::InlineEvent::Transient(TransientEvent::Submitted(
                TransientSubmission::Hotkey(action),
            )) => {
                ctrl_c_state.reset();
                let kind = match action {
                    TransientHotkeyAction::FocusJobOutput => JobModalActionKind::Focus,
                    TransientHotkeyAction::PreviewJobSnapshot => JobModalActionKind::Preview,
                    TransientHotkeyAction::InterruptJob => JobModalActionKind::Interrupt,
                    _ => continue,
                };
                return Some(JobModalAction {
                    kind,
                    selection: current_selection.clone(),
                });
            }
            vtcode_tui::app::InlineEvent::Transient(TransientEvent::Cancelled)
            | vtcode_tui::app::InlineEvent::Cancel
            | vtcode_tui::app::InlineEvent::Exit => {
                ctrl_c_state.reset();
                return None;
            }
            vtcode_tui::app::InlineEvent::Interrupt => {
                handle.close_transient();
                handle.force_redraw();
                return None;
            }
            _ => {}
        }
    }
}

fn focus_job_output(ctx: &mut SlashCommandContext<'_>, job_id: &str) -> Result<()> {
    let snapshot = match ctx.tool_registry.pty_manager().snapshot_session(job_id) {
        Ok(snapshot) => snapshot,
        Err(err) => {
            ctx.renderer.line(
                MessageStyle::Error,
                &format!("Failed to inspect job {job_id}: {err}"),
            )?;
            return Ok(());
        }
    };
    let output = read_job_output(ctx, job_id);
    ctx.renderer.line(
        MessageStyle::Info,
        &format!("Focused job {}: {}", snapshot.id, snapshot.command),
    )?;
    ctx.renderer.line(
        MessageStyle::Output,
        &format!(
            "Working dir: {}",
            snapshot
                .working_dir
                .unwrap_or_else(|| "unknown".to_string())
        ),
    )?;
    for line in truncate_job_output(&output).lines() {
        ctx.renderer.line(MessageStyle::Output, line)?;
    }
    Ok(())
}

fn preview_job_snapshot(ctx: &mut SlashCommandContext<'_>, job_id: &str) -> Result<()> {
    let snapshot = match ctx.tool_registry.pty_manager().snapshot_session(job_id) {
        Ok(snapshot) => snapshot,
        Err(err) => {
            ctx.renderer.line(
                MessageStyle::Error,
                &format!("Failed to inspect job {job_id}: {err}"),
            )?;
            return Ok(());
        }
    };
    let output = read_job_output(ctx, job_id);

    ctx.handle.show_modal(
        format!("Job {}", snapshot.id),
        vec![
            format!("Command: {}", snapshot.command),
            format!(
                "Working dir: {}",
                snapshot
                    .working_dir
                    .unwrap_or_else(|| "unknown".to_string())
            ),
            format!("Preview:\n{}", truncate_job_output(&output)),
        ],
        None,
    );
    Ok(())
}

fn interrupt_job(ctx: &mut SlashCommandContext<'_>, job_id: &str) -> Result<()> {
    match ctx
        .tool_registry
        .pty_manager()
        .send_input_to_session(job_id, &[3], false)
    {
        Ok(_) => ctx.renderer.line(
            MessageStyle::Info,
            &format!("Sent interrupt to job {job_id}."),
        )?,
        Err(err) => ctx.renderer.line(
            MessageStyle::Error,
            &format!("Failed to interrupt job {job_id}: {err}"),
        )?,
    }
    Ok(())
}

fn read_job_output(ctx: &SlashCommandContext<'_>, job_id: &str) -> String {
    ctx.tool_registry
        .pty_manager()
        .read_session_output(job_id, false)
        .ok()
        .flatten()
        .unwrap_or_default()
}
fn prompt_suggestion_item(suggestion: &PromptSuggestion) -> InlineListItem {
    InlineListItem {
        title: suggestion.title.clone(),
        subtitle: suggestion.subtitle.clone(),
        badge: suggestion.badge.clone(),
        indent: 0,
        selection: Some(InlineListSelection::ConfigAction(format!(
            "{PROMPT_SUGGESTION_ACTION_PREFIX}{}",
            suggestion.id
        ))),
        search_value: Some(format!(
            "{} {} {}",
            suggestion.title,
            suggestion.prompt,
            suggestion.subtitle.clone().unwrap_or_default()
        )),
    }
}

fn background_job_item(job: &BackgroundJobSummary) -> InlineListItem {
    let subtitle = match &job.working_dir {
        Some(dir) => format!("{} • {}", job.status, dir),
        None => job.status.clone(),
    };
    InlineListItem {
        title: job.command.clone(),
        subtitle: Some(subtitle),
        badge: Some(job.id.clone()),
        indent: 0,
        selection: Some(InlineListSelection::ConfigAction(format!("job:{}", job.id))),
        search_value: Some(format!(
            "{} {} {} {}",
            job.id,
            job.command,
            job.status,
            job.working_dir.clone().unwrap_or_default()
        )),
    }
}

fn truncate_job_output(output: &str) -> String {
    let trimmed = output.trim();
    if trimmed.is_empty() {
        return "(no output yet)".to_string();
    }
    let lines = trimmed.lines().rev().take(20).collect::<Vec<_>>();
    let mut preview = lines.into_iter().rev().collect::<Vec<_>>().join("\n");
    if preview.chars().count() > 1200 {
        preview = preview.chars().take(1199).collect::<String>();
        preview.push('…');
    }
    preview
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tokio::sync::{Notify, mpsc};
    use vtcode_tui::app::{InlineEvent, InlineHandle, InlineSession};

    use crate::agent::runloop::unified::state::CtrlCState;

    #[tokio::test]
    async fn jobs_modal_hotkey_uses_latest_selection() {
        let (command_tx, _command_rx) = mpsc::unbounded_channel();
        let handle = InlineHandle::new_for_tests(command_tx);
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let mut session = InlineSession {
            handle: handle.clone(),
            events: event_rx,
        };
        let ctrl_c_state = Arc::new(CtrlCState::new());
        let ctrl_c_notify = Arc::new(Notify::new());

        event_tx
            .send(InlineEvent::Transient(TransientEvent::SelectionChanged(
                TransientSelectionChange::List(InlineListSelection::ConfigAction(
                    "job:session-2".to_string(),
                )),
            )))
            .expect("selection change");
        event_tx
            .send(InlineEvent::Transient(TransientEvent::Submitted(
                TransientSubmission::Hotkey(TransientHotkeyAction::InterruptJob),
            )))
            .expect("hotkey submission");

        let action = wait_for_jobs_modal_action(
            &handle,
            &mut session,
            &ctrl_c_state,
            &ctrl_c_notify,
            Some(InlineListSelection::ConfigAction(
                "job:session-1".to_string(),
            )),
        )
        .await
        .expect("job action");

        assert_eq!(action.kind, JobModalActionKind::Interrupt);
        assert_eq!(
            action.selection,
            Some(InlineListSelection::ConfigAction(
                "job:session-2".to_string()
            ))
        );
    }
}
