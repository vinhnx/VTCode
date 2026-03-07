use std::time::Duration;

use anyhow::Result;
use chrono::Local;

use vtcode_core::config::loader::{ConfigManager, VTCodeConfig};
use vtcode_core::ui::theme;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};
use vtcode_core::utils::session_archive::SessionListing;
use vtcode_tui::{
    InlineHandle, InlineListItem, InlineListSearchConfig, InlineListSelection, convert_style,
};

use crate::agent::runloop::slash_commands::{SessionPaletteMode, ThemePaletteMode};
use crate::agent::runloop::tui_compat::{inline_theme_from_core_styles, to_tui_appearance};
use crate::agent::runloop::ui::build_inline_header_context;
use crate::agent::runloop::unified::settings_interactive::{
    SettingsPaletteState, apply_settings_action, parent_view_path, show_settings_palette,
};
use crate::agent::runloop::welcome::SessionBootstrap;

use super::display::persist_theme_preference;

const THEME_PALETTE_TITLE: &str = "Theme";
const THEME_ACTIVE_BADGE: &str = "Active";
const THEME_SELECT_HINT: &str =
    "↑/↓ choose • Enter apply • Esc cancel • Type to filter by name or id";
const THEME_SEARCH_LABEL: &str = "Search themes";
const THEME_SEARCH_PLACEHOLDER: &str = "Type theme name or id";
const SESSION_FORK_PALETTE_TITLE: &str = "Fork session";
const SESSION_RESUME_PALETTE_TITLE: &str = "Resume session";
const SESSIONS_HINT_PRIMARY: &str = "Use ↑/↓ to browse sessions.";
const SESSIONS_FORK_HINT_SECONDARY: &str = "Enter to fork session • Esc to close.";
const SESSIONS_RESUME_HINT_SECONDARY: &str = "Enter to resume session • Esc to close.";
const SESSIONS_LATEST_BADGE: &str = "Latest";

#[derive(Clone)]
pub(crate) enum ActivePalette {
    Theme {
        mode: ThemePaletteMode,
        original_theme_id: String,
    },
    Sessions {
        mode: SessionPaletteMode,
        listings: Vec<SessionListing>,
        limit: usize,
        show_all: bool,
    },
    Settings {
        state: Box<SettingsPaletteState>,
        esc_armed: bool,
    },
}

pub(crate) fn show_theme_palette(
    renderer: &mut AnsiRenderer,
    mode: ThemePaletteMode,
) -> Result<bool> {
    let title = match mode {
        ThemePaletteMode::Select => THEME_PALETTE_TITLE,
    };
    let hint = match mode {
        ThemePaletteMode::Select => THEME_SELECT_HINT,
    };

    let current_id = theme::active_theme_id();
    let current_label = theme::active_theme_label().to_string();
    let mut items = Vec::new();

    for id in theme::available_themes() {
        let label = theme::theme_label(id).unwrap_or(id);
        let badge = if id == current_id {
            Some(THEME_ACTIVE_BADGE.to_string())
        } else {
            None
        };
        let scheme_hint = if theme::is_light_theme(id) {
            "light"
        } else {
            "dark"
        };
        items.push(InlineListItem {
            title: label.to_string(),
            subtitle: Some(format!("id: {} • {}", id, scheme_hint)),
            badge,
            indent: 0,
            selection: Some(InlineListSelection::Theme(id.to_string())),
            search_value: Some(theme_search_value(id, label)),
        });
    }

    if items.is_empty() {
        renderer.line(MessageStyle::Info, "No themes available.")?;
        return Ok(false);
    }

    let lines = vec![format!("Active theme: {}", current_label), hint.to_string()];
    renderer.show_list_modal(
        title,
        lines,
        items,
        Some(InlineListSelection::Theme(current_id)),
        Some(InlineListSearchConfig {
            label: THEME_SEARCH_LABEL.to_string(),
            placeholder: Some(THEME_SEARCH_PLACEHOLDER.to_string()),
        }),
    );

    Ok(true)
}

fn theme_search_value(theme_id: &str, theme_label: &str) -> String {
    format!("{theme_label} {theme_id} theme appearance colors")
}

pub(crate) fn show_sessions_palette(
    renderer: &mut AnsiRenderer,
    mode: SessionPaletteMode,
    listings: &[SessionListing],
    limit: usize,
    show_all: bool,
) -> Result<bool> {
    if listings.is_empty() {
        renderer.line(MessageStyle::Info, "No archived sessions found.")?;
        return Ok(false);
    }

    let mut items = Vec::with_capacity(listings.len());
    for (index, listing) in listings.iter().enumerate() {
        let ended_local = listing
            .snapshot
            .ended_at
            .with_timezone(&Local)
            .format("%Y-%m-%d %H:%M");
        let duration = listing
            .snapshot
            .ended_at
            .signed_duration_since(listing.snapshot.started_at);
        let duration_std = duration.to_std().unwrap_or_else(|_| Duration::from_secs(0));
        let detail = format!(
            "Duration: {} · {} msgs · {} tools",
            format_duration_label(duration_std),
            listing.snapshot.total_messages,
            listing.snapshot.distinct_tools.len(),
        );
        let badge = (index == 0).then(|| SESSIONS_LATEST_BADGE.to_string());
        items.push(InlineListItem {
            title: format!(
                "{} · {} · {}",
                ended_local,
                listing.snapshot.metadata.model,
                listing.snapshot.metadata.workspace_label,
            ),
            subtitle: Some(detail),
            badge,
            indent: 0,
            selection: Some(InlineListSelection::Session(listing.identifier())),
            search_value: None,
        });
    }

    let scope_label = if show_all {
        "across all workspaces"
    } else {
        "in the current workspace"
    };
    let hint_secondary = match mode {
        SessionPaletteMode::Resume => SESSIONS_RESUME_HINT_SECONDARY,
        SessionPaletteMode::Fork => SESSIONS_FORK_HINT_SECONDARY,
    };
    let title = match mode {
        SessionPaletteMode::Resume => SESSION_RESUME_PALETTE_TITLE,
        SessionPaletteMode::Fork => SESSION_FORK_PALETTE_TITLE,
    };

    let lines = vec![
        format!(
            "Showing {} of {} archived sessions {}",
            listings.len(),
            limit,
            scope_label
        ),
        SESSIONS_HINT_PRIMARY.to_string(),
        hint_secondary.to_string(),
    ];
    let selected = listings
        .first()
        .map(|listing| InlineListSelection::Session(listing.identifier()));
    renderer.show_list_modal(title, lines, items, selected, None);
    Ok(true)
}

#[allow(clippy::too_many_arguments)]
async fn refresh_runtime_config_from_manager(
    renderer: &mut AnsiRenderer,
    handle: &InlineHandle,
    config: &mut vtcode_core::config::types::AgentConfig,
    vt_cfg: &mut Option<VTCodeConfig>,
    provider_client: &dyn vtcode_core::llm::provider::LLMProvider,
    session_bootstrap: &SessionBootstrap,
    full_auto: bool,
) -> Result<()> {
    if let Ok(runtime_manager) = ConfigManager::load() {
        let runtime_config = runtime_manager.config().clone();
        *vt_cfg = Some(runtime_config.clone());
        config.reasoning_effort = runtime_config.agent.reasoning_effort;
        renderer
            .set_show_diagnostics_in_transcript(runtime_config.ui.show_diagnostics_in_transcript);
        vtcode_tui::panic_hook::set_show_diagnostics(
            runtime_config.ui.show_diagnostics_in_transcript,
        );

        let _ = theme::set_active_theme(&runtime_config.agent.theme);
        let styles = theme::active_styles();
        handle.set_theme(inline_theme_from_core_styles(&styles));
        handle.set_appearance(to_tui_appearance(&runtime_config));

        let provider_label = if config.provider.trim().is_empty() {
            provider_client.name().to_string()
        } else {
            config.provider.clone()
        };
        let reasoning_label = config.reasoning_effort.as_str().to_string();
        let mode_label = match (config.ui_surface, full_auto) {
            (vtcode_core::config::types::UiSurfacePreference::Inline, true) => "auto".to_string(),
            (vtcode_core::config::types::UiSurfacePreference::Inline, false) => {
                "inline".to_string()
            }
            (vtcode_core::config::types::UiSurfacePreference::Alternate, _) => "alt".to_string(),
            (vtcode_core::config::types::UiSurfacePreference::Auto, true) => "auto".to_string(),
            (vtcode_core::config::types::UiSurfacePreference::Auto, false) => "std".to_string(),
        };
        if let Ok(header_context) = build_inline_header_context(
            config,
            session_bootstrap,
            provider_label,
            config.model.clone(),
            mode_label,
            reasoning_label,
        )
        .await
        {
            handle.set_header_context(header_context);
        }

        apply_prompt_style(handle);
        handle.force_redraw();
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn handle_palette_selection(
    palette: ActivePalette,
    selection: InlineListSelection,
    renderer: &mut AnsiRenderer,
    handle: &InlineHandle,
    config: &mut vtcode_core::config::types::AgentConfig,
    vt_cfg: &mut Option<VTCodeConfig>,
    provider_client: &dyn vtcode_core::llm::provider::LLMProvider,
    session_bootstrap: &SessionBootstrap,
    full_auto: bool,
) -> Result<Option<ActivePalette>> {
    match palette {
        ActivePalette::Theme {
            mode,
            original_theme_id,
        } => match selection {
            InlineListSelection::Theme(theme_id) => match mode {
                ThemePaletteMode::Select => {
                    match theme::set_active_theme(&theme_id) {
                        Ok(()) => {
                            let label = theme::active_theme_label();
                            renderer.line(
                                MessageStyle::Info,
                                &format!("Theme switched to {}", label),
                            )?;
                            persist_theme_preference(renderer, &theme_id).await?;
                            let styles = theme::active_styles();
                            handle.set_theme(inline_theme_from_core_styles(&styles));
                            apply_prompt_style(handle);
                            handle.force_redraw();
                        }
                        Err(err) => {
                            renderer.line(
                                MessageStyle::Error,
                                &format!("Theme '{}' not available: {}", theme_id, err),
                            )?;
                        }
                    }
                    Ok(None)
                }
            },
            _ => Ok(Some(ActivePalette::Theme {
                mode,
                original_theme_id,
            })),
        },
        ActivePalette::Sessions {
            mode,
            listings,
            limit,
            show_all,
        } => {
            if show_sessions_palette(renderer, mode, &listings, limit, show_all)? {
                Ok(Some(ActivePalette::Sessions {
                    mode,
                    listings,
                    limit,
                    show_all,
                }))
            } else {
                Ok(None)
            }
        }
        ActivePalette::Settings {
            mut state,
            esc_armed: _,
        } => {
            let normalized_selection = normalize_config_selection(&selection);

            if let InlineListSelection::ConfigAction(action) = &selection {
                let outcome = apply_settings_action(state.as_mut(), action)?;
                if let Some(message) = outcome.message {
                    renderer.line(MessageStyle::Info, &message)?;
                }
                if outcome.saved {
                    refresh_runtime_config_from_manager(
                        renderer,
                        handle,
                        config,
                        vt_cfg,
                        provider_client,
                        session_bootstrap,
                        full_auto,
                    )
                    .await?;
                }
            }

            if show_settings_palette(renderer, state.as_ref(), Some(normalized_selection))? {
                Ok(Some(ActivePalette::Settings {
                    state,
                    esc_armed: false,
                }))
            } else {
                Ok(None)
            }
        }
    }
}

pub(crate) fn handle_palette_preview(
    palette: ActivePalette,
    selection: InlineListSelection,
    renderer: &mut AnsiRenderer,
    handle: &InlineHandle,
) -> Result<Option<ActivePalette>> {
    match palette {
        ActivePalette::Theme {
            mode,
            original_theme_id,
        } => {
            if let InlineListSelection::Theme(theme_id) = selection {
                match mode {
                    ThemePaletteMode::Select => {
                        if let Err(err) = theme::set_active_theme(&theme_id) {
                            renderer.line(
                                MessageStyle::Error,
                                &format!("Theme '{}' not available: {}", theme_id, err),
                            )?;
                        } else {
                            let styles = theme::active_styles();
                            handle.set_theme(inline_theme_from_core_styles(&styles));
                            apply_prompt_style(handle);
                            handle.force_redraw();
                        }
                    }
                }
            }
            Ok(Some(ActivePalette::Theme {
                mode,
                original_theme_id,
            }))
        }
        ActivePalette::Settings { state, .. } => Ok(Some(ActivePalette::Settings {
            state,
            esc_armed: false,
        })),
        other => Ok(Some(other)),
    }
}

fn normalize_config_selection(selection: &InlineListSelection) -> InlineListSelection {
    match selection {
        InlineListSelection::ConfigAction(action) if action.ends_with(":cycle_prev") => {
            let normalized = action.trim_end_matches(":cycle_prev");
            InlineListSelection::ConfigAction(format!("{normalized}:cycle"))
        }
        InlineListSelection::ConfigAction(action) if action.ends_with(":dec") => {
            let normalized = action.trim_end_matches(":dec");
            InlineListSelection::ConfigAction(format!("{normalized}:inc"))
        }
        value => value.clone(),
    }
}

pub(crate) fn handle_palette_cancel(
    palette: ActivePalette,
    renderer: &mut AnsiRenderer,
    handle: &InlineHandle,
) -> Result<Option<ActivePalette>> {
    match palette {
        ActivePalette::Theme {
            mode,
            original_theme_id,
        } => {
            if theme::active_theme_id() != original_theme_id
                && theme::set_active_theme(&original_theme_id).is_ok()
            {
                let styles = theme::active_styles();
                handle.set_theme(inline_theme_from_core_styles(&styles));
                apply_prompt_style(handle);
                handle.force_redraw();
            }
            let message = match mode {
                ThemePaletteMode::Select => "Theme selection cancelled.",
            };
            if !renderer.supports_inline_ui() {
                renderer.line(MessageStyle::Info, message)?;
            }
            Ok(None)
        }
        ActivePalette::Sessions { .. } => {
            if !renderer.supports_inline_ui() {
                renderer.line(MessageStyle::Info, "Closed session browser.")?;
            }
            Ok(None)
        }
        ActivePalette::Settings {
            mut state,
            esc_armed,
        } => {
            if esc_armed {
                return Ok(None);
            }

            let Some(current_path) = state.view_path.clone() else {
                if !renderer.supports_inline_ui() {
                    renderer.line(MessageStyle::Info, "Closed interactive settings.")?;
                }
                return Ok(None);
            };

            state.view_path = parent_view_path(&current_path);
            if show_settings_palette(renderer, state.as_ref(), None)? {
                Ok(Some(ActivePalette::Settings {
                    state,
                    esc_armed: true,
                }))
            } else {
                Ok(None)
            }
        }
    }
}

pub(crate) fn format_duration_label(duration: Duration) -> String {
    let total_seconds = duration.as_secs();
    let hours = total_seconds / 3600;
    let minutes = (total_seconds % 3600) / 60;
    let seconds = total_seconds % 60;

    let mut parts = Vec::new();
    if hours > 0 {
        parts.push(format!("{}h", hours));
    }
    if minutes > 0 || hours > 0 {
        parts.push(format!("{}m", minutes));
    }
    parts.push(format!("{}s", seconds));
    parts.join(" ")
}

pub(crate) fn apply_prompt_style(handle: &InlineHandle) {
    let styles = theme::active_styles();
    let style = convert_style(styles.primary);
    handle.set_prompt("".to_string(), style);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_config_selection_maps_cycle_prev_to_cycle() {
        let selection = InlineListSelection::ConfigAction("ui.display_mode:cycle_prev".to_string());
        let normalized = normalize_config_selection(&selection);
        assert_eq!(
            normalized,
            InlineListSelection::ConfigAction("ui.display_mode:cycle".to_string())
        );
    }

    #[test]
    fn normalize_config_selection_maps_dec_to_inc() {
        let selection =
            InlineListSelection::ConfigAction("context.max_context_tokens:dec".to_string());
        let normalized = normalize_config_selection(&selection);
        assert_eq!(
            normalized,
            InlineListSelection::ConfigAction("context.max_context_tokens:inc".to_string())
        );
    }
}
