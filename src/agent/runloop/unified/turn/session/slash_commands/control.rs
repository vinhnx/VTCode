use anyhow::{Context, Result};
use std::path::PathBuf;
use vtcode_core::config::loader::VTCodeConfig;
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
use crate::agent::runloop::unified::turn::session::slash_commands::{SlashCommandContext, SlashCommandControl};

use super::apps::handle_configure_editor;
use super::ui;
use crate::agent::runloop::ui_list::{self, Tone};
use crate::agent::runloop::unified::overlay_prompt::{OverlayWaitOutcome, show_overlay_and_wait};
use vtcode_ui::tui::app::{InlineListSelection, ListOverlayRequest, TransientRequest, TransientSubmission};

pub(crate) async fn handle_notify(ctx: SlashCommandContext<'_>, message: String) -> Result<SlashCommandControl> {
    let app_name = vtcode_core::config::constants::app::DISPLAY_NAME;
    send_global_notification_force(NotificationEvent::Custom {
        title: app_name.to_string(),
        message: message.clone(),
    })
    .await?;
    ctx.renderer
        .line(MessageStyle::Info, &format!("Sent {app_name} notification: {message}"))?;
    Ok(SlashCommandControl::Continue)
}

pub(crate) async fn handle_show_settings(ctx: SlashCommandContext<'_>) -> Result<SlashCommandControl> {
    let mut ctx = ctx;
    show_settings_at_path_from_context(&mut ctx, None).await
}

pub(crate) async fn handle_show_settings_reset(ctx: SlashCommandContext<'_>) -> Result<SlashCommandControl> {
    let mut ctx = ctx;
    show_settings_at_path_from_context_with_reset(&mut ctx, None, true).await
}

pub(crate) async fn handle_show_permissions(ctx: SlashCommandContext<'_>) -> Result<SlashCommandControl> {
    let mut ctx = ctx;
    if !ui::ensure_selection_ui_available(&mut ctx, "changing permissions")? {
        return Ok(SlashCommandControl::Continue);
    }
    // The mode switcher is a confirmation modal; without inline UI it would
    // wedge the turn until cancel. Report the effective mode as text instead.
    if !ctx.renderer.supports_inline_ui() {
        let mode = if ctx.vt_cfg.as_ref().is_some_and(|config| config.sandbox.is_full_access()) {
            "Full access (this session)"
        } else {
            "Configured"
        };
        ctx.renderer.line(MessageStyle::Info, &format!("Permission mode: {mode}"))?;
        ctx.renderer
            .line(MessageStyle::Info, "Change it from the permissions picker in your client.")?;
        return Ok(SlashCommandControl::Continue);
    }
    let active = ctx.vt_cfg.as_ref().is_some_and(|config| config.sandbox.is_full_access());
    let request = permission_choices(active, false);
    let choice = show_overlay_and_wait(
        ctx.handle,
        ctx.session,
        request,
        ctx.ctrl_c_state,
        ctx.ctrl_c_notify,
        config_action_from_submission,
    )
    .await?;
    ctx.handle.close_modal();
    match choice {
        OverlayWaitOutcome::Submitted(action) if action == "settings" => {
            return show_settings_at_path_from_context(&mut ctx, Some("permissions")).await;
        }
        OverlayWaitOutcome::Submitted(action) if action == "configured" => {
            if let Some(config) = ctx.vt_cfg.as_mut() {
                config.sandbox.session_permission_mode =
                    vtcode_config::core::sandbox::SessionPermissionMode::Configured;
                ctx.tool_registry.apply_sandbox_config(&config.sandbox);
            }
            ctx.renderer.line(MessageStyle::Info, "Permission mode: configured")?;
        }
        OverlayWaitOutcome::Submitted(action) if action == "full" && !active => {
            if ctx.full_auto {
                ctx.renderer
                    .line(MessageStyle::Warning, "Full access cannot be combined with Full auto.")?;
                return Ok(SlashCommandControl::Continue);
            }
            let outcome = show_overlay_and_wait(
                ctx.handle,
                ctx.session,
                permission_choices(active, true),
                ctx.ctrl_c_state,
                ctx.ctrl_c_notify,
                config_action_from_submission,
            )
            .await?;
            ctx.handle.close_modal();
            if matches!(outcome, OverlayWaitOutcome::Submitted(ref action) if action == "confirm") {
                let config = ctx.vt_cfg.get_or_insert_with(VTCodeConfig::default);
                config.sandbox.enable_full_access();
                ctx.tool_registry.apply_sandbox_config(&config.sandbox);
                ctx.renderer
                    .line(MessageStyle::Warning, "Permission mode: Full access (this session)")?;
            } else if matches!(outcome, OverlayWaitOutcome::Exit) {
                return Ok(SlashCommandControl::BreakWithReason(SessionEndReason::Exit));
            }
        }
        OverlayWaitOutcome::Exit => return Ok(SlashCommandControl::BreakWithReason(SessionEndReason::Exit)),
        _ => {}
    }
    ctx.handle.force_redraw();
    Ok(SlashCommandControl::Continue)
}

fn config_action_from_submission(submission: TransientSubmission) -> Option<String> {
    if let TransientSubmission::Selection(InlineListSelection::ConfigAction(action)) = submission {
        Some(action)
    } else {
        None
    }
}

fn permission_choices(active: bool, confirm: bool) -> TransientRequest {
    if confirm {
        return TransientRequest::List(ListOverlayRequest {
            title: "Enable Full access?".into(),
            lines: vec![
                "Tasks can perform the following actions without asking for your approval each time:".into(),
                "Files and folders: read, create, change, and delete accessible files.".into(),
                "Terminal commands: run commands, install software, or change system settings.".into(),
                "Network and apps: access the network, send data, and use connected apps.".into(),
                "These actions may cause data loss or disclosure. Enable only for tasks you trust. Explicit denials and hard safety blocks remain active.".into(),
                "You can switch to another permission mode at any time.".into(),
            ],
            footer_hint: Some("Esc cancels".into()),
            items: vec![
                ui_list::action(
                    "Enable Full access",
                    "I understand the risks and take responsibility for my data security",
                    None,
                    Tone::Warning,
                    Some(InlineListSelection::ConfigAction("confirm".into())),
                ),
                ui_list::action(
                    "Cancel",
                    "Go back without enabling Full access",
                    None,
                    Tone::Accent,
                    Some(InlineListSelection::ConfigAction("cancel".into())),
                ),
            ],
            // The safe option stays pre-selected so an accidental Enter cancels.
            selected: Some(InlineListSelection::ConfigAction("cancel".into())),
            search: None,
            hotkeys: Vec::new(),
            status: None,
        });
    }
    let current_badge = |is_current: bool| is_current.then_some("Current".to_string());
    // The warning tone only renders alongside a badge, so the Full access row
    // keeps a badge in both states: the warning affordance stays visible even
    // when the mode is inactive.
    let full_access_badge = Some(if active { "Current" } else { "Caution" }.to_string());
    TransientRequest::List(ListOverlayRequest {
        title: "Session permissions".into(),
        lines: vec![format!(
            "Current mode: {}",
            if active { "Full access" } else { "Configured" }
        )],
        footer_hint: Some("Esc cancels".into()),
        items: vec![
            ui_list::action(
                "Use configured permissions",
                "Follow your configured rules and approval prompts",
                current_badge(!active),
                Tone::Accent,
                Some(InlineListSelection::ConfigAction("configured".into())),
            ),
            ui_list::action(
                "Full access",
                "Unrestricted access to the internet and any file on your computer",
                full_access_badge,
                Tone::Warning,
                Some(InlineListSelection::ConfigAction("full".into())),
            ),
            ui_list::action(
                "Configure permission rules",
                "Edit allow and deny rules",
                None,
                Tone::Accent,
                Some(InlineListSelection::ConfigAction("settings".into())),
            ),
        ],
        selected: Some(InlineListSelection::ConfigAction(if active { "full" } else { "configured" }.into())),
        search: None,
        hotkeys: Vec::new(),
        status: None,
    })
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
    show_settings_at_path_from_context_with_reset(ctx, view_path, false).await
}

async fn show_settings_at_path_from_context_with_reset(
    ctx: &mut SlashCommandContext<'_>,
    view_path: Option<&str>,
    reset_confirmation: bool,
) -> Result<SlashCommandControl> {
    if !ui::ensure_selection_ui_available(ctx, "configuring settings")? {
        return Ok(SlashCommandControl::Continue);
    }

    if !ctx.renderer.supports_inline_ui() {
        // Reset is destructive and has no text-driven confirm path; the
        // desktop config page documents the same terminal command instead of
        // offering the call.
        let guidance = if reset_confirmation {
            "Run `vtcode config reset` in a terminal to clear one configuration layer."
        } else {
            "Interactive settings need the terminal UI; run vtcode in a terminal to browse and edit them."
        };
        ctx.renderer.line(MessageStyle::Info, guidance)?;
        return Ok(SlashCommandControl::Continue);
    }

    let workspace_path = ctx.config.workspace.clone();
    let vt_snapshot = ctx.vt_cfg.clone();
    let mut settings_state = create_settings_palette_state(&workspace_path, vt_snapshot.as_ref())?;
    settings_state.view_path = view_path.map(resolve_settings_view_path);
    if reset_confirmation {
        settings_state.view_path = Some("__settings_reset_confirmation".to_string());
    }
    if settings_state.view_path.as_deref() == Some("tools.editor") {
        return handle_configure_editor(ctx).await;
    }

    if show_settings_palette(ctx.renderer, &settings_state, None)? {
        *ctx.palette_state = Some(ActivePalette::Settings { state: Box::new(settings_state), esc_armed: false });
    }

    Ok(SlashCommandControl::Continue)
}

pub(crate) async fn handle_stop_agent(ctx: SlashCommandContext<'_>) -> Result<SlashCommandControl> {
    if ctx.tool_registry.active_pty_sessions_count() == 0
        && !ctx.ctrl_c_state.is_cancel_requested()
        && !ctx.ctrl_c_state.is_exit_requested()
    {
        ctx.renderer.line(MessageStyle::Info, "No active run to stop.")?;
        return Ok(SlashCommandControl::Continue);
    }

    match request_local_stop(ctx.ctrl_c_state, ctx.ctrl_c_notify) {
        CtrlCSignal::Cancel => {
            ctx.renderer
                .line(MessageStyle::Info, "Stop requested. VT Code is cancelling the current turn.")?;
            Ok(SlashCommandControl::Continue)
        }
        CtrlCSignal::Exit => Ok(SlashCommandControl::BreakWithReason(SessionEndReason::Exit)),
    }
}

pub(crate) async fn handle_clear_conversation(ctx: SlashCommandContext<'_>) -> Result<SlashCommandControl> {
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
    ctx.handle.clear_screen();
    ctx.handle.clear_input();
    ctx.renderer.clear_screen();
    ctx.renderer.line(MessageStyle::Info, "Cleared conversation history.")?;
    ctx.renderer.line_if_not_empty(MessageStyle::Output)?;
    Ok(SlashCommandControl::Continue)
}

pub(crate) async fn handle_clear_screen(ctx: SlashCommandContext<'_>) -> Result<SlashCommandControl> {
    if !ctx.renderer.supports_inline_ui() {
        ctx.renderer.line(
            MessageStyle::Info,
            "This surface keeps the full transcript; nothing was cleared. Conversation context is preserved.",
        )?;
        return Ok(SlashCommandControl::Continue);
    }
    ctx.renderer.clear_screen();
    ctx.renderer
        .line(MessageStyle::Info, "Cleared screen. Conversation context is preserved.")?;
    ctx.renderer.line_if_not_empty(MessageStyle::Output)?;
    Ok(SlashCommandControl::Continue)
}

pub(crate) async fn handle_show_transcript_stats(ctx: SlashCommandContext<'_>) -> Result<SlashCommandControl> {
    let disk_event_count = transcript::len();

    if !ctx.renderer.supports_inline_ui() {
        ctx.renderer
            .line(MessageStyle::Info, &format!("Transcript: {disk_event_count} disk events."))?;
        return Ok(SlashCommandControl::Continue);
    }
    ctx.renderer.line(
        MessageStyle::Info,
        &format!("Transcript: {disk_event_count} disk events (TUI-only memory is bounded and evicts oldest lines)."),
    )?;
    ctx.renderer.line_if_not_empty(MessageStyle::Output)?;
    Ok(SlashCommandControl::Continue)
}

pub(crate) async fn handle_export_transcript(
    ctx: SlashCommandContext<'_>,
    path: Option<String>,
) -> Result<SlashCommandControl> {
    let target = match path {
        Some(value) if !value.trim().is_empty() => PathBuf::from(value.trim()),
        _ => PathBuf::from("transcript-export.txt"),
    };
    if let Some(parent) = target.parent()
        && !parent.as_os_str().is_empty()
    {
        tokio::fs::create_dir_all(parent)
            .await
            .with_context(|| format!("Failed to create directory {}", parent.display()))?;
    }
    let text = transcript::snapshot().join("\n");
    tokio::fs::write(&target, text)
        .await
        .with_context(|| format!("Failed to export transcript to {}", target.display()))?;
    ctx.renderer
        .line(MessageStyle::Info, &format!("Transcript exported to {}.", target.display()))?;
    ctx.renderer.line_if_not_empty(MessageStyle::Output)?;
    Ok(SlashCommandControl::Continue)
}

pub(crate) async fn handle_copy_latest_assistant_reply(ctx: SlashCommandContext<'_>) -> Result<SlashCommandControl> {
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
        let char_count = reply.chars().count();
        let unit = if char_count == 1 { "char" } else { "chars" };
        if vtcode_ui::tui::core::MouseSelectionState::copy_to_clipboard(&reply) {
            ctx.renderer.line(
                MessageStyle::Info,
                &format!("Copied latest assistant reply ({char_count} {unit}) to clipboard."),
            )?;
        } else {
            ctx.renderer.line(MessageStyle::Warning, "Failed to copy to clipboard.")?;
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use vtcode_commons::ui_protocol::InlineListItem;

    fn list_request(active: bool, confirm: bool) -> ListOverlayRequest {
        match permission_choices(active, confirm) {
            TransientRequest::List(request) => request,
            other => panic!("expected list overlay, got {other:?}"),
        }
    }

    fn item_by_title<'a>(items: &'a [InlineListItem], title: &str) -> &'a InlineListItem {
        items
            .iter()
            .find(|item| item.title == title)
            .unwrap_or_else(|| panic!("missing row {title:?}"))
    }

    fn selected_action(selected: &Option<InlineListSelection>) -> &str {
        match selected {
            Some(InlineListSelection::ConfigAction(action)) => action,
            other => panic!("expected config action selection, got {other:?}"),
        }
    }

    #[test]
    fn permission_mode_list_warns_on_full_access_and_marks_current() {
        for active in [false, true] {
            let request = list_request(active, false);
            assert_eq!(request.title, "Session permissions");
            assert_eq!(request.items.len(), 3);

            let full = item_by_title(&request.items, "Full access");
            assert_eq!(full.badge_tone, Tone::Warning);
            assert_eq!(
                full.subtitle.as_deref(),
                Some("Unrestricted access to the internet and any file on your computer")
            );
            assert_eq!(full.badge.as_deref(), Some(if active { "Current" } else { "Caution" }));

            let configured = item_by_title(&request.items, "Use configured permissions");
            assert_eq!(configured.badge.as_deref(), (!active).then_some("Current"));

            assert_eq!(selected_action(&request.selected), if active { "full" } else { "configured" });
            assert!(
                request
                    .lines
                    .iter()
                    .any(|line| line.contains(if active { "Full access" } else { "Configured" })),
                "header must name the current mode"
            );
        }
    }

    #[test]
    fn permission_mode_list_subtitles_never_leak_action_ids() {
        let request = list_request(false, false);
        for item in &request.items {
            let subtitle = item.subtitle.as_deref().unwrap_or_default();
            assert!(!["configured", "full", "settings"].contains(&subtitle));
            assert!(!subtitle.is_empty());
        }
    }

    #[test]
    fn config_action_mapper_only_accepts_config_actions() {
        assert_eq!(
            config_action_from_submission(TransientSubmission::Selection(InlineListSelection::ConfigAction(
                "full".into()
            ))),
            Some("full".to_string())
        );
        assert_eq!(config_action_from_submission(TransientSubmission::DiffApply), None);
    }

    #[test]
    fn permission_confirm_defaults_to_cancel_with_caution_copy() {
        let request = list_request(false, true);
        assert_eq!(request.title, "Enable Full access?");
        assert_eq!(request.items.len(), 2);
        assert_eq!(selected_action(&request.selected), "cancel");

        let confirm = item_by_title(&request.items, "Enable Full access");
        assert_eq!(confirm.badge_tone, Tone::Warning);
        assert_eq!(
            confirm.subtitle.as_deref(),
            Some("I understand the risks and take responsibility for my data security")
        );
        for expected in [
            "without asking for your approval",
            "Files and folders:",
            "Terminal commands:",
            "Network and apps:",
            "data loss",
            "Explicit denials and hard safety blocks remain active",
            "switch to another permission mode at any time",
        ] {
            assert!(
                request.lines.iter().any(|line| line.contains(expected)),
                "confirm dialog must disclose: {expected}"
            );
        }
    }
}
