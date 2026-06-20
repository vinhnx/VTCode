use anyhow::Result;
use tokio::task;
use vtcode_core::core::threads::{SessionQueryScope, list_recent_sessions_in_scope};
use vtcode_core::ui::inline_theme_from_core_styles;
use vtcode_core::ui::theme;
use vtcode_core::utils::ansi::MessageStyle;
use vtcode_ui::tui::app::{InlineListItem, InlineListSelection, TransientSubmission};

use super::{SlashCommandContext, SlashCommandControl};
use crate::agent::runloop::model_picker::{ModelPickerStart, ModelPickerState};
use crate::agent::runloop::slash_commands::SessionPaletteMode;
use crate::agent::runloop::unified::display::{
    persist_theme_preference, sync_runtime_theme_selection,
};
use crate::agent::runloop::unified::model_selection::finalize_model_selection;
use crate::agent::runloop::unified::overlay_prompt::{
    OverlayWaitOutcome, wait_for_overlay_submission,
};
use crate::agent::runloop::unified::palettes::{
    ActivePalette, apply_prompt_style, build_lightweight_palette_view,
    show_lightweight_model_palette, show_mode_palette, show_model_target_palette,
    show_sessions_palette, show_theme_palette,
};
use crate::agent::runloop::unified::session_setup::{
    apply_ide_context_snapshot, ide_context_status_label_from_bridge,
};
use crate::agent::runloop::unified::state::ModelPickerTarget;
use crate::agent::runloop::unified::ui_interaction::PlaceholderSpinner;

#[path = "ui/statusline.rs"]
mod statusline;
#[path = "ui/terminal_title.rs"]
mod terminal_title;

pub(crate) use statusline::handle_start_statusline_setup;
pub(crate) use terminal_title::handle_start_terminal_title_setup;

pub(super) fn ensure_selection_ui_available(
    ctx: &mut SlashCommandContext<'_>,
    activity: &str,
) -> Result<bool> {
    if ctx.model_picker_state.is_some() {
        ctx.renderer.line(
            MessageStyle::Error,
            &format!("Close the active model picker before {}.", activity),
        )?;
        return Ok(false);
    }
    if ctx.palette_state.is_some() {
        ctx.renderer.line(
            MessageStyle::Error,
            "Another selection modal is already open. Press Esc to dismiss it before starting a new one.",
        )?;
        return Ok(false);
    }
    Ok(true)
}

pub(super) async fn wait_for_list_modal_selection(
    ctx: &mut SlashCommandContext<'_>,
) -> Option<InlineListSelection> {
    let outcome: OverlayWaitOutcome<InlineListSelection> = wait_for_overlay_submission(
        ctx.handle,
        ctx.session,
        ctx.ctrl_c_state,
        ctx.ctrl_c_notify,
        |submission| match submission {
            TransientSubmission::Selection(selection) => Some(selection),
            _ => None,
        },
    )
    .await
    .ok()?;

    close_list_modal(ctx).await;

    match outcome {
        OverlayWaitOutcome::Submitted(selection) => Some(selection),
        OverlayWaitOutcome::Cancelled
        | OverlayWaitOutcome::Interrupted
        | OverlayWaitOutcome::Exit => None,
    }
}

async fn close_list_modal(ctx: &mut SlashCommandContext<'_>) {
    ctx.handle.close_modal();
    ctx.handle.force_redraw();
    task::yield_now().await;
}

pub(crate) async fn handle_theme_changed(
    ctx: SlashCommandContext<'_>,
    theme_id: String,
) -> Result<SlashCommandControl> {
    sync_runtime_theme_selection(ctx.config, ctx.vt_cfg.as_mut(), &theme_id);
    persist_theme_preference(ctx.renderer, &ctx.config.workspace, &theme_id).await?;
    let styles = theme::active_styles();
    ctx.handle.set_theme(inline_theme_from_core_styles(&styles));
    apply_prompt_style(ctx.handle);
    ctx.handle.force_redraw();
    Ok(SlashCommandControl::Continue)
}

pub(crate) async fn handle_start_theme_palette(
    mut ctx: SlashCommandContext<'_>,
    mode: crate::agent::runloop::slash_commands::ThemePaletteMode,
) -> Result<SlashCommandControl> {
    if !ensure_selection_ui_available(&mut ctx, "selecting a theme")? {
        return Ok(SlashCommandControl::Continue);
    }
    if show_theme_palette(ctx.renderer, mode)? {
        *ctx.palette_state = Some(ActivePalette::Theme {
            mode,
            original_theme_id: theme::active_theme_id(),
        });
    }
    Ok(SlashCommandControl::Continue)
}

pub(crate) async fn handle_start_session_palette(
    mut ctx: SlashCommandContext<'_>,
    mode: SessionPaletteMode,
    limit: usize,
    show_all: bool,
) -> Result<SlashCommandControl> {
    let activity = match mode {
        SessionPaletteMode::Resume => "browsing sessions",
        SessionPaletteMode::Fork => "selecting a session to fork",
    };
    if !ensure_selection_ui_available(&mut ctx, activity)? {
        return Ok(SlashCommandControl::Continue);
    }
    let scope = if show_all {
        SessionQueryScope::All
    } else {
        SessionQueryScope::CurrentWorkspace(ctx.config.workspace.clone())
    };

    match list_recent_sessions_in_scope(limit, &scope).await {
        Ok(listings) => {
            if show_sessions_palette(ctx.renderer, mode, &listings, limit, show_all)? {
                *ctx.palette_state = Some(ActivePalette::Sessions {
                    mode,
                    listings,
                    limit,
                    show_all,
                });
            }
        }
        Err(err) => {
            ctx.renderer.line(
                MessageStyle::Error,
                &format!("Failed to load session archives: {}", err),
            )?;
        }
    }
    Ok(SlashCommandControl::Continue)
}

pub(crate) async fn handle_start_history_picker(
    mut ctx: SlashCommandContext<'_>,
) -> Result<SlashCommandControl> {
    if !ctx.renderer.supports_inline_ui() {
        ctx.renderer.line(
            MessageStyle::Info,
            "Command history picker is available in inline UI only. Use /resume for archived sessions.",
        )?;
        return Ok(SlashCommandControl::Continue);
    }

    if !ensure_selection_ui_available(&mut ctx, "opening command history")? {
        return Ok(SlashCommandControl::Continue);
    }

    ctx.handle.show_history_picker();
    Ok(SlashCommandControl::Continue)
}

pub(crate) async fn handle_start_file_browser(
    mut ctx: SlashCommandContext<'_>,
    initial_filter: Option<String>,
) -> Result<SlashCommandControl> {
    if !ensure_selection_ui_available(&mut ctx, "opening file browser")? {
        return Ok(SlashCommandControl::Continue);
    }
    // Ensure stale inline modal state cannot overlap with the file palette overlay.
    ctx.handle.close_modal();
    ctx.handle.force_redraw();
    if let Some(filter) = initial_filter {
        ctx.handle.set_input(format!("@{}", filter));
    } else {
        ctx.handle.set_input("@".to_string());
    }
    Ok(SlashCommandControl::Continue)
}

fn config_action_item(
    title: &str,
    subtitle: &str,
    badge: &str,
    indent: u8,
    action: impl Into<String>,
    search_value: impl Into<String>,
) -> InlineListItem {
    InlineListItem {
        title: title.to_string(),
        subtitle: Some(subtitle.to_string()),
        badge: Some(badge.to_string()),
        indent,
        selection: Some(InlineListSelection::ConfigAction(action.into())),
        search_value: Some(search_value.into()),
    }
}

pub(crate) async fn handle_start_model_selection(
    mut ctx: SlashCommandContext<'_>,
) -> Result<SlashCommandControl> {
    if !ensure_selection_ui_available(&mut ctx, "selecting a model target")? {
        return Ok(SlashCommandControl::Continue);
    }

    if !ctx.renderer.supports_inline_ui() {
        ctx.renderer.line(
            MessageStyle::Info,
            "Inline UI is unavailable; opening the main model picker directly.",
        )?;
        return start_model_selection_target(ctx, ModelPickerTarget::Main).await;
    }

    if show_model_target_palette(ctx.renderer)? {
        *ctx.palette_state = Some(ActivePalette::ModelTarget);
    }
    Ok(SlashCommandControl::Continue)
}

pub(super) async fn start_model_selection_target(
    ctx: SlashCommandContext<'_>,
    target: ModelPickerTarget,
) -> Result<SlashCommandControl> {
    ctx.session_stats.model_picker_target = target;
    match target {
        ModelPickerTarget::Main => start_model_picker(ctx).await,
        ModelPickerTarget::Lightweight => {
            let vt_cfg = ctx.vt_cfg.clone();
            let restore_status_left = ctx.input_status_state.left.clone();
            let restore_status_right = ctx.input_status_state.right.clone();
            let view = {
                let loading_spinner = if ctx.renderer.supports_inline_ui() {
                    Some(PlaceholderSpinner::new(
                        ctx.handle,
                        restore_status_left,
                        restore_status_right,
                        "Loading lightweight model lists...",
                    ))
                } else {
                    ctx.renderer
                        .line(MessageStyle::Info, "Loading lightweight model lists...")?;
                    None
                };
                let result = build_lightweight_palette_view(ctx.config, vt_cfg.as_ref()).await;
                drop(loading_spinner);
                result
            };
            if show_lightweight_model_palette(ctx.renderer, &view, None)? {
                *ctx.palette_state = Some(ActivePalette::LightweightModel {
                    view: Box::new(view),
                });
            }
            ctx.session_stats.model_picker_target = ModelPickerTarget::Main;
            Ok(SlashCommandControl::Continue)
        }
    }
}

pub(crate) async fn handle_toggle_ide_context(
    ctx: SlashCommandContext<'_>,
) -> Result<SlashCommandControl> {
    let enabled = ctx.context_manager.toggle_session_ide_context();

    let latest_editor_snapshot = if let Some(bridge) = ctx.ide_context_bridge.as_mut() {
        match bridge.refresh() {
            Ok((snapshot, _)) => snapshot,
            Err(err) => {
                tracing::warn!(error = %err, "Failed to refresh IDE context while toggling /ide");
                bridge.snapshot().cloned()
            }
        }
    } else {
        None
    };

    apply_ide_context_snapshot(
        ctx.context_manager,
        ctx.header_context,
        ctx.handle,
        ctx.config.workspace.as_path(),
        ctx.vt_cfg.as_ref(),
        latest_editor_snapshot.clone(),
    );

    crate::agent::runloop::unified::status_line::update_ide_context_source(
        ctx.input_status_state,
        ide_context_status_label_from_bridge(
            ctx.context_manager,
            ctx.config.workspace.as_path(),
            ctx.vt_cfg.as_ref(),
            ctx.ide_context_bridge.as_ref(),
        ),
    );

    let message = match (enabled, latest_editor_snapshot.is_some()) {
        (true, true) => "IDE context enabled for this session.",
        (true, false) => {
            "IDE context enabled for this session. No IDE snapshot is currently available."
        }
        (false, _) => "IDE context disabled for this session.",
    };
    ctx.renderer.line(MessageStyle::Info, message)?;

    Ok(SlashCommandControl::Continue)
}

pub(super) async fn start_model_picker(
    ctx: SlashCommandContext<'_>,
) -> Result<SlashCommandControl> {
    if ctx.model_picker_state.is_some() {
        ctx.renderer.line(
            MessageStyle::Error,
            "A model picker session is already active. Complete or type 'cancel' to exit it before starting another.",
        )?;
        return Ok(SlashCommandControl::Continue);
    }
    let reasoning = ctx
        .vt_cfg
        .as_ref()
        .map(|cfg| cfg.agent.reasoning_effort)
        .unwrap_or(ctx.config.reasoning_effort);
    let service_tier = ctx
        .vt_cfg
        .as_ref()
        .and_then(|cfg| cfg.provider.openai.service_tier);
    let workspace_hint = Some(ctx.config.workspace.clone());
    let restore_status_left = ctx.input_status_state.left.clone();
    let restore_status_right = ctx.input_status_state.right.clone();
    let picker_start = {
        let loading_spinner = if ctx.renderer.supports_inline_ui() {
            Some(PlaceholderSpinner::new(
                ctx.handle,
                restore_status_left.clone(),
                restore_status_right.clone(),
                "Loading model lists...",
            ))
        } else {
            ctx.renderer
                .line(MessageStyle::Info, "Loading model lists...")?;
            None
        };
        let result = ModelPickerState::new(
            ctx.renderer,
            ctx.vt_cfg.clone(),
            reasoning,
            service_tier,
            workspace_hint,
            ctx.config.provider.clone(),
            ctx.config.model.clone(),
            Some(std::sync::Arc::clone(ctx.ctrl_c_state)),
            Some(std::sync::Arc::clone(ctx.ctrl_c_notify)),
        )
        .await;
        drop(loading_spinner);
        result
    };
    match picker_start {
        Ok(ModelPickerStart::InProgress(picker)) => {
            *ctx.model_picker_state = Some(picker);
        }
        Ok(ModelPickerStart::Completed { state, selection }) => {
            if let Err(err) = finalize_model_selection(
                ctx.renderer,
                &state,
                selection,
                ctx.config,
                ctx.vt_cfg,
                ctx.provider_client,
                ctx.session_bootstrap,
                ctx.handle,
                ctx.header_context,
                ctx.full_auto,
                ctx.conversation_history.len(),
            )
            .await
            {
                ctx.renderer.line(
                    MessageStyle::Error,
                    &format!("Failed to apply model selection: {}", err),
                )?;
            }
        }
        Err(err) => {
            ctx.renderer.line(
                MessageStyle::Error,
                &format!("Failed to start model picker: {}", err),
            )?;
        }
    }
    Ok(SlashCommandControl::Continue)
}

pub(crate) async fn handle_start_mode_palette(
    mut ctx: SlashCommandContext<'_>,
) -> Result<SlashCommandControl> {
    if !ensure_selection_ui_available(&mut ctx, "selecting an agent mode")? {
        return Ok(SlashCommandControl::Continue);
    }

    let specs = if let Some(controller) = ctx.tool_registry.subagent_controller() {
        controller.effective_specs().await
    } else {
        match vtcode_config::discover_subagents(&vtcode_config::SubagentDiscoveryInput::new(
            ctx.config.workspace.clone(),
        )) {
            Ok(discovered) => discovered.effective,
            Err(err) => {
                ctx.renderer.line(
                    MessageStyle::Error,
                    &format!("Failed to discover agents: {}", err),
                )?;
                return Ok(SlashCommandControl::Continue);
            }
        }
    };

    let current_name = ctx.active_primary_agent.active().identity.name.clone();
    if show_mode_palette(ctx.renderer, &specs, &current_name)? {
        *ctx.palette_state = Some(ActivePalette::Mode);
    }
    Ok(SlashCommandControl::Continue)
}

pub(crate) async fn handle_select_primary_agent_from_slash(
    _ctx: SlashCommandContext<'_>,
    name: &str,
) -> Result<SlashCommandControl> {
    Ok(SlashCommandControl::SelectAgent(name.to_string()))
}
