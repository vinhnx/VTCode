use anyhow::Result;
use vtcode_core::ui::theme;
use vtcode_core::utils::ansi::MessageStyle;
use vtcode_core::utils::session_archive;

use crate::agent::runloop::model_picker::{ModelPickerStart, ModelPickerState};
use crate::agent::runloop::tui_compat::inline_theme_from_core_styles;
use crate::agent::runloop::unified::display::persist_theme_preference;
use crate::agent::runloop::unified::model_selection::finalize_model_selection;
use crate::agent::runloop::unified::palettes::{
    ActivePalette, apply_prompt_style, show_sessions_palette, show_theme_palette,
};
use crate::agent::runloop::unified::state::ModelPickerTarget;

use super::{SlashCommandContext, SlashCommandControl};

pub async fn handle_theme_changed(
    ctx: SlashCommandContext<'_>,
    theme_id: String,
) -> Result<SlashCommandControl> {
    persist_theme_preference(ctx.renderer, &theme_id).await?;
    let styles = theme::active_styles();
    ctx.handle.set_theme(inline_theme_from_core_styles(&styles));
    apply_prompt_style(ctx.handle);
    Ok(SlashCommandControl::Continue)
}

pub async fn handle_start_theme_palette(
    ctx: SlashCommandContext<'_>,
    mode: crate::agent::runloop::slash_commands::ThemePaletteMode,
) -> Result<SlashCommandControl> {
    if ctx.model_picker_state.is_some() {
        ctx.renderer.line(
            MessageStyle::Error,
            "Close the active model picker before selecting a theme.",
        )?;
        return Ok(SlashCommandControl::Continue);
    }
    if ctx.palette_state.is_some() {
        ctx.renderer.line(
            MessageStyle::Error,
            "Another selection modal is already open. Press Esc to dismiss it before starting a new one.",
        )?;
        return Ok(SlashCommandControl::Continue);
    }
    if show_theme_palette(ctx.renderer, mode)? {
        *ctx.palette_state = Some(ActivePalette::Theme { mode });
    }
    Ok(SlashCommandControl::Continue)
}

pub async fn handle_start_sessions_palette(
    ctx: SlashCommandContext<'_>,
    limit: usize,
) -> Result<SlashCommandControl> {
    if ctx.model_picker_state.is_some() {
        ctx.renderer.line(
            MessageStyle::Error,
            "Close the active model picker before browsing sessions.",
        )?;
        return Ok(SlashCommandControl::Continue);
    }
    if ctx.palette_state.is_some() {
        ctx.renderer.line(
            MessageStyle::Error,
            "Another selection modal is already open. Press Esc to close it before continuing.",
        )?;
        return Ok(SlashCommandControl::Continue);
    }
    match session_archive::list_recent_sessions(limit).await {
        Ok(listings) => {
            if show_sessions_palette(ctx.renderer, &listings, limit)? {
                *ctx.palette_state = Some(ActivePalette::Sessions { listings, limit });
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

pub async fn handle_start_file_browser(
    ctx: SlashCommandContext<'_>,
    initial_filter: Option<String>,
) -> Result<SlashCommandControl> {
    if ctx.model_picker_state.is_some() {
        ctx.renderer.line(
            MessageStyle::Error,
            "Close the active model picker before opening file browser.",
        )?;
        return Ok(SlashCommandControl::Continue);
    }
    if ctx.palette_state.is_some() {
        ctx.renderer.line(
            MessageStyle::Error,
            "Another selection modal is already open. Press Esc to dismiss it before starting a new one.",
        )?;
        return Ok(SlashCommandControl::Continue);
    }
    ctx.handle.force_redraw();
    if let Some(filter) = initial_filter {
        ctx.handle.set_input(format!("@{}", filter));
    } else {
        ctx.handle.set_input("@".to_string());
    }
    ctx.renderer.line(
        MessageStyle::Info,
        "File browser activated. Use arrow keys to navigate, Enter to select, Esc to close.",
    )?;
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
    match ModelPickerState::new(ctx.renderer, reasoning, workspace_hint).await {
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
