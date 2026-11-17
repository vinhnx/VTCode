use anyhow::Result;
use vtcode_core::subagents::{BackgroundSubprocessStatus, SubagentStatus};
use vtcode_core::tools::types::{VTCodeExecSession, VTCodeSessionLifecycleState};
use vtcode_tui_runtime::utils::ansi::MessageStyle;

use super::ui::ensure_selection_ui_available;
use super::{SlashCommandContext, SlashCommandControl};
use crate::agent::runloop::unified::session_setup::refresh_local_agents;

pub(crate) async fn handle_toggle_tasks_panel(ctx: SlashCommandContext<'_>) -> Result<SlashCommandControl> {
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

pub(crate) async fn handle_show_jobs_panel(mut ctx: SlashCommandContext<'_>) -> Result<SlashCommandControl> {
    if !ensure_selection_ui_available(&mut ctx, "opening jobs")? {
        return Ok(SlashCommandControl::Continue);
    }

    if !ctx.renderer.supports_inline_ui() {
        return render_jobs_text(&mut ctx).await;
    }

    let controller = ctx.tool_registry.subagent_controller();
    let exec_sessions = ctx.tool_registry.exec_session_manager();
    if let Err(error) = refresh_local_agents(ctx.handle, controller.as_ref(), exec_sessions).await {
        tracing::warn!(%error, "Failed to refresh local agents before opening jobs");
    }
    ctx.handle.show_local_agents();
    Ok(SlashCommandControl::Continue)
}

async fn render_jobs_text(ctx: &mut SlashCommandContext<'_>) -> Result<SlashCommandControl> {
    let mut rendered = false;

    if let Some(controller) = ctx.tool_registry.subagent_controller() {
        for entry in controller.status_entries().await {
            if matches!(entry.status, SubagentStatus::Completed | SubagentStatus::Closed) {
                continue;
            }
            rendered = true;
            ctx.renderer
                .line(MessageStyle::Info, &format!("{} {} (delegated)", entry.display_label, entry.status.as_str()))?;
            if let Some(summary) = entry.summary.as_deref().or(entry.error.as_deref()) {
                ctx.renderer.line(MessageStyle::Output, &format!("Summary: {summary}"))?;
            }
        }

        let entries = match controller.refresh_background_processes().await {
            Ok(entries) => entries,
            Err(error) => {
                tracing::warn!(%error, "Failed to refresh managed background jobs for text output");
                controller.background_status_entries().await
            }
        };
        for entry in entries {
            if !(matches!(entry.status, BackgroundSubprocessStatus::Starting | BackgroundSubprocessStatus::Running)
                || entry.desired_enabled && matches!(entry.status, BackgroundSubprocessStatus::Error))
            {
                continue;
            }
            rendered = true;
            ctx.renderer.line(
                MessageStyle::Info,
                &format!(
                    "{} {} pid {} (managed)",
                    entry.display_label,
                    entry.status.as_str(),
                    entry.pid.map_or_else(|| "-".to_string(), |pid| pid.to_string())
                ),
            )?;
            if let Some(summary) = entry.summary.as_deref().or(entry.error.as_deref()) {
                ctx.renderer.line(MessageStyle::Output, &format!("Summary: {summary}"))?;
            }
        }
    }

    for snapshot in ctx.tool_registry.exec_session_manager().background_session_snapshots().await {
        rendered = true;
        let metadata = &snapshot.metadata;
        ctx.renderer.line(
            MessageStyle::Info,
            &format!("{} {} (exec-session)", exec_session_label(metadata), exec_session_status(metadata)),
        )?;
        ctx.renderer.line(
            MessageStyle::Output,
            &format!(
                "Summary: cwd {} · pid {}",
                metadata.working_dir.as_deref().unwrap_or("unknown"),
                metadata.child_pid.map_or_else(|| "-".to_string(), |pid| pid.to_string())
            ),
        )?;
    }

    if !rendered {
        ctx.renderer.line(MessageStyle::Info, "No active background jobs.")?;
    }
    Ok(SlashCommandControl::Continue)
}

fn exec_session_label(metadata: &VTCodeExecSession) -> String {
    std::iter::once(metadata.command.as_str())
        .chain(metadata.args.iter().map(String::as_str))
        .collect::<Vec<_>>()
        .join(" ")
}

fn exec_session_status(metadata: &VTCodeExecSession) -> String {
    match metadata.lifecycle_state {
        Some(VTCodeSessionLifecycleState::Running) => "running".to_string(),
        Some(VTCodeSessionLifecycleState::Exited) => metadata
            .exit_code
            .map_or_else(|| "exited".to_string(), |code| format!("exited ({code})")),
        None => "unknown".to_string(),
    }
}
