use anyhow::Result;
use vtcode_core::core::threads::{SessionQueryScope, list_recent_sessions_in_scope};
use vtcode_core::ui::theme;
use vtcode_core::utils::ansi::MessageStyle;
use vtcode_tui::{InlineListSelection, OverlaySubmission};

use crate::agent::runloop::model_picker::{ModelPickerStart, ModelPickerState};
use crate::agent::runloop::slash_commands::SessionPaletteMode;
use crate::agent::runloop::tui_compat::inline_theme_from_core_styles;
use crate::agent::runloop::unified::display::persist_theme_preference;
use crate::agent::runloop::unified::model_selection::finalize_model_selection;
use crate::agent::runloop::unified::overlay_prompt::{
    OverlayWaitOutcome, wait_for_overlay_submission,
};
use crate::agent::runloop::unified::palettes::{
    ActivePalette, apply_prompt_style, show_sessions_palette, show_theme_palette,
};
use crate::agent::runloop::unified::state::ModelPickerTarget;

use super::{SlashCommandContext, SlashCommandControl};

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
            OverlaySubmission::Selection(selection) => Some(selection),
            _ => None,
        },
    )
    .await
    .ok()?;

    match outcome {
        OverlayWaitOutcome::Submitted(selection) => Some(selection),
        OverlayWaitOutcome::Cancelled
        | OverlayWaitOutcome::Interrupted
        | OverlayWaitOutcome::Exit => None,
    }
}

pub async fn handle_theme_changed(
    ctx: SlashCommandContext<'_>,
    theme_id: String,
) -> Result<SlashCommandControl> {
    persist_theme_preference(ctx.renderer, &theme_id).await?;
    let styles = theme::active_styles();
    ctx.handle.set_theme(inline_theme_from_core_styles(&styles));
    apply_prompt_style(ctx.handle);
    ctx.handle.force_redraw();
    Ok(SlashCommandControl::Continue)
}

pub async fn handle_start_theme_palette(
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

pub async fn handle_start_session_palette(
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

pub async fn handle_start_history_picker(
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

    ctx.handle.open_history_picker();
    Ok(SlashCommandControl::Continue)
}

pub async fn handle_start_file_browser(
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

pub async fn handle_start_model_selection(
    ctx: SlashCommandContext<'_>,
) -> Result<SlashCommandControl> {
    ctx.session_stats.model_picker_target = ModelPickerTarget::Main;
    start_model_picker(ctx).await
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
    let workspace_hint = Some(ctx.config.workspace.clone());
    match ModelPickerState::new(
        ctx.renderer,
        reasoning,
        workspace_hint,
        ctx.config.provider.clone(),
        ctx.config.model.clone(),
    )
    .await
    {
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
                ctx.full_auto,
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
