use anyhow::Result;
use vtcode_core::core::decision_tracker::DecisionTracker;
use vtcode_core::hooks::SessionEndReason;
use vtcode_core::llm::provider::MessageRole;
use vtcode_core::notifications::{NotificationEvent, send_global_notification_force};
use vtcode_core::utils::ansi::MessageStyle;
use vtcode_core::utils::transcript;

use crate::agent::runloop::unified::palettes::ActivePalette;
use crate::agent::runloop::unified::settings_interactive::{
    create_settings_palette_state, resolve_settings_view_path, show_settings_palette,
};
use crate::agent::runloop::unified::state::{CtrlCSignal, SessionStats};
use crate::agent::runloop::unified::stop_requests::request_local_stop;
use crate::agent::runloop::unified::turn::session::slash_commands::{
    SlashCommandContext, SlashCommandControl,
};

use super::apps::handle_configure_editor;
use super::ui;

pub(crate) async fn handle_notify(
    ctx: SlashCommandContext<'_>,
    message: String,
) -> Result<SlashCommandControl> {
    send_global_notification_force(NotificationEvent::Custom {
        title: "VT Code".to_string(),
        message: message.clone(),
    })
    .await?;
    ctx.renderer
        .line(MessageStyle::Info, &format!("Sent VT Code notification: {message}"))?;
    Ok(SlashCommandControl::Continue)
}

pub(crate) async fn handle_show_settings(
    ctx: SlashCommandContext<'_>,
) -> Result<SlashCommandControl> {
    let mut ctx = ctx;
    show_settings_at_path_from_context(&mut ctx, None).await
}

pub(crate) async fn handle_show_permissions(
    ctx: SlashCommandContext<'_>,
) -> Result<SlashCommandControl> {
    let mut ctx = ctx;
    show_settings_at_path_from_context(&mut ctx, Some("permissions")).await
}

pub(crate) async fn handle_show_settings_at_path(
    ctx: SlashCommandContext<'_>,
    view_path: Option<&str>,
) -> Result<SlashCommandControl> {
    let mut ctx = ctx;
    show_settings_at_path_from_context(&mut ctx, view_path).await
}

pub(crate) async fn show_settings_at_path_from_context(
    ctx: &mut SlashCommandContext<'_>,
    view_path: Option<&str>,
) -> Result<SlashCommandControl> {
    if !ui::ensure_selection_ui_available(ctx, "configuring settings")? {
        return Ok(SlashCommandControl::Continue);
    }

    if !ctx.renderer.supports_inline_ui() {
        ctx.renderer.line(
            MessageStyle::Info,
            "Interactive settings require inline UI; use /config to inspect effective values.",
        )?;
        return Ok(SlashCommandControl::Continue);
    }

    let workspace_path = ctx.config.workspace.clone();
    let vt_snapshot = ctx.vt_cfg.clone();
    let mut settings_state = create_settings_palette_state(&workspace_path, &vt_snapshot)?;
    settings_state.view_path = view_path.map(resolve_settings_view_path);
    if settings_state.view_path.as_deref() == Some("tools.editor") {
        return handle_configure_editor(ctx).await;
    }

    if show_settings_palette(ctx.renderer, &settings_state, None)? {
        *ctx.palette_state =
            Some(ActivePalette::Settings { state: Box::new(settings_state), esc_armed: false });
    }

    Ok(SlashCommandControl::Continue)
}

pub(crate) async fn handle_stop_agent(ctx: SlashCommandContext<'_>) -> Result<SlashCommandControl> {
    if ctx.tool_registry.active_pty_sessions() == 0
        && !ctx.ctrl_c_state.is_cancel_requested()
        && !ctx.ctrl_c_state.is_exit_requested()
    {
        ctx.renderer.line(MessageStyle::Info, "No active run to stop.")?;
        return Ok(SlashCommandControl::Continue);
    }

    match request_local_stop(ctx.ctrl_c_state, ctx.ctrl_c_notify) {
        CtrlCSignal::Cancel => {
            ctx.renderer.line(
                MessageStyle::Info,
                "Stop requested. VT Code is cancelling the current turn.",
            )?;
            Ok(SlashCommandControl::Continue)
        }
        CtrlCSignal::Exit => Ok(SlashCommandControl::BreakWithReason(SessionEndReason::Exit)),
    }
}

pub(crate) async fn handle_clear_conversation(
    ctx: SlashCommandContext<'_>,
) -> Result<SlashCommandControl> {
    let vim_mode_enabled = ctx.session_stats.vim_mode_enabled;
    ctx.conversation_history.clear();
    *ctx.session_stats = SessionStats::default();
    ctx.session_stats.vim_mode_enabled = vim_mode_enabled;
    ctx.handle.hide_task_panel();
    ctx.handle.update_task_panel(Vec::new());
    {
        let mut ledger = ctx.decision_ledger.write().await;
        *ledger = DecisionTracker::new();
    }
    transcript::clear();
    ctx.renderer.clear_screen();
    ctx.renderer.line(MessageStyle::Info, "Cleared conversation history.")?;
    ctx.renderer.line_if_not_empty(MessageStyle::Output)?;
    Ok(SlashCommandControl::Continue)
}

pub(crate) async fn handle_clear_screen(
    ctx: SlashCommandContext<'_>,
) -> Result<SlashCommandControl> {
    ctx.renderer.clear_screen();
    ctx.renderer
        .line(MessageStyle::Info, "Cleared screen. Conversation context is preserved.")?;
    ctx.renderer.line_if_not_empty(MessageStyle::Output)?;
    Ok(SlashCommandControl::Continue)
}

pub(crate) async fn handle_copy_latest_assistant_reply(
    ctx: SlashCommandContext<'_>,
) -> Result<SlashCommandControl> {
    let latest_reply = ctx.conversation_history.iter().rev().find_map(|message| {
        if message.role != MessageRole::Assistant {
            return None;
        }
        if message.tool_calls.as_ref().is_some_and(|calls| !calls.is_empty()) {
            return None;
        }
        let text = message.content.as_text();
        let trimmed = text.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    });

    if let Some(reply) = latest_reply {
        vtcode_ui::tui::core::MouseSelectionState::copy_to_clipboard(&reply);
        ctx.renderer
            .line(MessageStyle::Info, "Copied latest assistant reply to clipboard.")?;
    } else {
        ctx.renderer
            .line(MessageStyle::Warning, "No complete assistant reply found to copy yet.")?;
    }

    Ok(SlashCommandControl::Continue)
}

/// Handle the /exit slash command.
///
/// # Priority Guarantee
///
/// The /exit command is always the highest priority and cannot be blocked.
/// It immediately returns `SlashCommandControl::BreakWithReason(SessionEndReason::Exit)`
/// which propagates to `InteractionOutcome::Exit` and is checked at the top
/// of every interaction loop iteration.
///
/// This is a user safety guarantee - users must always be able to exit the
/// program without being trapped in an unresponsive state.
pub(crate) async fn handle_exit(ctx: SlashCommandContext<'_>) -> Result<SlashCommandControl> {
    ctx.renderer.line(MessageStyle::Info, "✓")?;
    Ok(SlashCommandControl::BreakWithReason(SessionEndReason::Exit))
}
