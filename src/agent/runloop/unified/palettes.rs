use crate::agent::runloop::ui_list;
use std::time::Duration;

use anyhow::Result;
use chrono::Local;

use vtcode_core::config::loader::{ConfigManager, VTCodeConfig};
use vtcode_core::ui::theme;
use vtcode_core::ui::{inline_theme_from_core_styles, to_tui_appearance, to_tui_fullscreen};
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};
use vtcode_core::utils::session_archive::SessionListing;
use vtcode_ui::tui::app::{InlineHandle, InlineListItem, InlineListSearchConfig, InlineListSelection};
use vtcode_ui::tui::core::convert_style;

use crate::agent::runloop::slash_commands::{SessionPaletteMode, ThemePaletteMode};
use crate::agent::runloop::ui::build_inline_header_context;
use crate::agent::runloop::unified::settings_interactive::{
    ACTION_BACK, ACTION_OPEN_ROOT, ACTION_RELOAD, ACTION_RESET, ACTION_RESET_CANCEL, ACTION_RESET_CONFIRM,
    SettingsPaletteState, apply_settings_action, parent_view_path, show_settings_palette,
};
use crate::agent::runloop::unified::url_guard::UrlGuardPrompt;
use crate::agent::runloop::welcome::SessionBootstrap;

use super::display::{persist_theme_preference, sync_runtime_theme_selection};

const THEME_PALETTE_TITLE: &str = "Theme";
const THEME_ACTIVE_BADGE: &str = "Active";
const THEME_SEARCH_PLACEHOLDER: &str = "name, id, or appearance";
const SESSION_FORK_PALETTE_TITLE: &str = "Fork session";
const SESSION_FORK_MODE_PALETTE_TITLE: &str = "Fork mode";
const SESSION_RESUME_PALETTE_TITLE: &str = "Resume session";
const SESSIONS_LATEST_BADGE: &str = "Latest";
const SESSIONS_SEARCH_PLACEHOLDER: &str = "workspace, provider, model, date";
const MODE_PALETTE_TITLE: &str = "Agent mode";
const MODE_SEARCH_PLACEHOLDER: &str = "name or description";
pub(crate) const MODE_ACTION_PREFIX: &str = "mode:";

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
    ForkMode {
        session_id: String,
        listings: Vec<SessionListing>,
        limit: usize,
        show_all: bool,
    },
    Settings {
        state: Box<SettingsPaletteState>,
        esc_armed: bool,
    },
    Mode,
    UrlGuard {
        prompt: UrlGuardPrompt,
        previous: Option<Box<ActivePalette>>,
    },
}

pub(crate) fn show_theme_palette(renderer: &mut AnsiRenderer, mode: ThemePaletteMode) -> Result<bool> {
    let title = match mode {
        ThemePaletteMode::Select => THEME_PALETTE_TITLE,
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
        let scheme_hint = if theme::is_light_theme(id) { "light" } else { "dark" };
        let mut row = ui_list::choice(
            label.to_string(),
            Some(format!("id: {id} • {scheme_hint}")),
            Some(InlineListSelection::Theme(id.to_string())),
        );
        row.badge = badge;
        row.search_value = Some(theme_search_value(id, label));
        items.push(row);
    }

    if items.is_empty() {
        renderer.line(MessageStyle::Info, "No themes available.")?;
        return Ok(false);
    }

    let lines = vec![format!("The active theme is {current_label}.")];
    renderer.show_list_modal(
        title,
        lines,
        items,
        Some(InlineListSelection::Theme(current_id)),
        Some(InlineListSearchConfig {
            label: String::new(),
            placeholder: Some(THEME_SEARCH_PLACEHOLDER.to_string()),
            fuzzy: false,
        }),
    );

    Ok(true)
}

fn theme_search_value(theme_id: &str, theme_label: &str) -> String {
    format!("{theme_label} {theme_id} theme appearance colors")
}

pub(crate) fn show_mode_palette(
    renderer: &mut AnsiRenderer,
    specs: &[vtcode_config::SubagentSpec],
    current_name: &str,
) -> Result<bool> {
    let primary_specs: Vec<_> = specs.iter().filter(|s| s.is_primary()).collect();

    if primary_specs.is_empty() {
        renderer.line(MessageStyle::Info, "No primary agents available.")?;
        return Ok(false);
    }

    let mut items = Vec::new();
    let mut canonical_current = current_name;
    for spec in &primary_specs {
        let is_current = spec.name.eq_ignore_ascii_case(current_name);
        if is_current {
            canonical_current = spec.name.as_str();
        }
        let badge = if is_current { Some("Active".to_string()) } else { None };
        let subtitle = if spec.permissions.default == vtcode_config::core::permissions::PermissionDefault::Auto {
            Some(format!("{} (autonomous)", spec.description))
        } else {
            Some(spec.description.clone())
        };
        items.push(InlineListItem {
            title: spec.name.clone(),
            subtitle,
            badge,
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction(format!("{}{}", MODE_ACTION_PREFIX, spec.name))),
            search_value: Some(format!("{} {} agent mode", spec.name, spec.description)),
            ..Default::default()
        });
    }

    renderer.show_list_modal(
        MODE_PALETTE_TITLE,
        vec![format!("The current agent is {canonical_current}.")],
        items,
        Some(InlineListSelection::ConfigAction(format!("{MODE_ACTION_PREFIX}{canonical_current}"))),
        Some(InlineListSearchConfig {
            label: String::new(),
            placeholder: Some(MODE_SEARCH_PLACEHOLDER.to_string()),
            fuzzy: false,
        }),
    );

    Ok(true)
}

fn session_search_value(
    listing: &SessionListing,
    ended_local: &str,
    duration_label: &str,
    tool_count: usize,
) -> String {
    let tool_names = if listing.snapshot.distinct_tools.is_empty() {
        String::new()
    } else {
        listing.snapshot.distinct_tools.join(" ")
    };

    format!(
        "{} {} {} {} {} {} {} messages {} msgs {} tools {} {} {}",
        listing.snapshot.metadata.workspace_label,
        listing.snapshot.metadata.workspace_path,
        listing.snapshot.metadata.model,
        listing.snapshot.metadata.provider,
        ended_local,
        duration_label,
        listing.snapshot.total_messages,
        listing.snapshot.total_messages,
        tool_count,
        tool_names,
        listing.snapshot.metadata.theme,
        listing.snapshot.metadata.reasoning_effort,
    )
}

pub(crate) fn show_sessions_palette(
    renderer: &mut AnsiRenderer,
    mode: SessionPaletteMode,
    listings: &[SessionListing],
    _limit: usize,
    _show_all: bool,
) -> Result<bool> {
    if listings.is_empty() {
        renderer.line(MessageStyle::Info, "No archived sessions found.")?;
        return Ok(false);
    }

    let mut items = Vec::with_capacity(listings.len());
    for (index, listing) in listings.iter().enumerate() {
        let ended_local = listing.snapshot.ended_at.with_timezone(&Local).format("%Y-%m-%d %H:%M");
        let duration = listing.snapshot.ended_at.signed_duration_since(listing.snapshot.started_at);
        let duration_std = duration.to_std().unwrap_or_else(|_| Duration::from_secs(0));
        let duration_label = format_duration_label(duration_std);
        let tool_count = listing.snapshot.distinct_tools.len();
        let detail = format!(
            "{} • {} / {} • {} • {} msgs • {} tools",
            ended_local,
            listing.snapshot.metadata.provider,
            listing.snapshot.metadata.model,
            duration_label,
            listing.snapshot.total_messages,
            tool_count,
        );
        let badge = (index == 0).then(|| SESSIONS_LATEST_BADGE.to_string());
        items.push(InlineListItem {
            title: listing.snapshot.metadata.workspace_label.clone(),
            subtitle: Some(detail),
            badge,
            indent: 0,
            selection: Some(InlineListSelection::Session(listing.identifier())),
            search_value: Some(session_search_value(listing, &ended_local.to_string(), &duration_label, tool_count)),
            ..Default::default()
        });
    }

    let title = match mode {
        SessionPaletteMode::Resume => SESSION_RESUME_PALETTE_TITLE,
        SessionPaletteMode::Fork => SESSION_FORK_PALETTE_TITLE,
    };

    let action_noun = match mode {
        SessionPaletteMode::Resume => "resumed",
        SessionPaletteMode::Fork => "forked",
    };
    let lines = vec![format!(
        "{} archived sessions are available. Select one to have it {action_noun}.",
        listings.len()
    )];
    let selected = listings
        .first()
        .map(|listing| InlineListSelection::Session(listing.identifier()));
    renderer.show_list_modal(
        title,
        lines,
        items,
        selected,
        Some(InlineListSearchConfig {
            label: String::new(),
            placeholder: Some(SESSIONS_SEARCH_PLACEHOLDER.to_string()),
            fuzzy: false,
        }),
    );
    Ok(true)
}

pub(crate) fn show_fork_mode_palette(renderer: &mut AnsiRenderer, session_id: &str) -> Result<bool> {
    let items = vec![
        ui_list::choice(
            "Copy full history",
            Some("Start the fork with the full archived transcript.".to_string()),
            Some(InlineListSelection::SessionForkMode {
                session_id: session_id.to_string(),
                summarize: false,
            }),
        )
        .with_search_value("copy full history fork transcript".to_string()),
        ui_list::choice(
            "Start summarized fork",
            Some("Compact the source session into summary plus retained user prompts.".to_string()),
            Some(InlineListSelection::SessionForkMode {
                session_id: session_id.to_string(),
                summarize: true,
            }),
        )
        .with_search_value("summary summarized compact fork handoff".to_string()),
    ];

    let lines = vec![format!("This fork starts from session {session_id}.")];

    renderer.show_list_modal(
        SESSION_FORK_MODE_PALETTE_TITLE,
        lines,
        items,
        Some(InlineListSelection::SessionForkMode {
            session_id: session_id.to_string(),
            summarize: false,
        }),
        None,
    );

    Ok(true)
}

pub(crate) async fn refresh_runtime_config_from_manager(
    renderer: &mut AnsiRenderer,
    handle: &InlineHandle,
    config: &mut vtcode_core::config::types::AgentConfig,
    vt_cfg: &mut Option<VTCodeConfig>,
    provider_client: &dyn vtcode_core::llm::provider::LLMProvider,
    session_bootstrap: &SessionBootstrap,
    _full_auto: bool,
) -> Result<()> {
    ConfigManager::invalidate_workspace_cache(&config.workspace);
    let runtime_manager = ConfigManager::load_from_workspace(&config.workspace)?;
    let mut runtime_config = runtime_manager.config().clone();
    crate::agent::agents::apply_live_reload_overrides(Some(&mut runtime_config), config);
    let custom_providers_unchanged = vt_cfg
        .as_ref()
        .is_some_and(|old| old.custom_providers == runtime_config.custom_providers);
    if !custom_providers_unchanged {
        vtcode_core::llm::factory::register_custom_providers(&runtime_config.custom_providers);
    }
    *vt_cfg = Some(runtime_config.clone());
    config.reasoning_effort = runtime_config.agent.reasoning_effort;
    config.theme.clone_from(&runtime_config.agent.theme);
    renderer.set_show_diagnostics_in_transcript(runtime_config.ui.show_diagnostics_in_transcript);
    renderer.set_tool_display_mode(runtime_config.ui.tool_display_mode);
    vtcode_ui::tui::panic_hook::set_show_diagnostics(runtime_config.ui.show_diagnostics_in_transcript);

    theme::set_color_accessibility_config(theme::ColorAccessibilityConfig {
        minimum_contrast: runtime_config.ui.minimum_contrast,
        bold_is_bright: runtime_config.ui.bold_is_bright,
        safe_colors_only: runtime_config.ui.safe_colors_only,
    });
    if theme::set_active_theme(&runtime_config.agent.theme).is_err() {
        let _ = theme::set_active_theme(theme::DEFAULT_THEME_ID);
    }
    let styles = theme::active_styles();
    handle.set_theme(inline_theme_from_core_styles(&styles));
    handle.set_appearance(to_tui_appearance(&runtime_config));
    handle.program_status(vtcode_commons::program_status::ProgramStatusUpdate::Configure {
        enabled: runtime_config.ui.program_status.enabled,
    });
    handle.set_fullscreen_interaction(to_tui_fullscreen(&runtime_config));
    handle.set_key_bindings(session_bootstrap.effective_key_bindings(&runtime_config));

    let provider_label = {
        let label =
            crate::agent::runloop::unified::session_setup::resolve_provider_label(config, Some(&runtime_config));
        if label.is_empty() {
            provider_client.name().to_string()
        } else {
            label
        }
    };
    let reasoning_label = config.reasoning_effort.as_str().to_string();
    if let Ok(header_context) = build_inline_header_context(
        config,
        Some(&runtime_config),
        session_bootstrap,
        provider_label,
        config.model.clone(),
        provider_client.effective_context_size(&config.model),
        reasoning_label,
    )
    .await
    {
        handle.set_header_context(header_context);
    }

    apply_prompt_style(handle);
    handle.force_redraw();

    Ok(())
}

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
        ActivePalette::Theme { mode, original_theme_id } => match selection {
            InlineListSelection::Theme(theme_id) => match mode {
                ThemePaletteMode::Select => {
                    match theme::set_active_theme(&theme_id) {
                        Ok(()) => {
                            let label = theme::active_theme_label();
                            renderer.line(MessageStyle::Info, &format!("Theme switched to {label}"))?;
                            sync_runtime_theme_selection(config, vt_cfg.as_mut(), &theme_id);
                            persist_theme_preference(renderer, &config.workspace, &theme_id).await?;
                            let styles = theme::active_styles();
                            handle.set_theme(inline_theme_from_core_styles(&styles));
                            apply_prompt_style(handle);
                            handle.force_redraw();
                        }
                        Err(err) => {
                            renderer.line(MessageStyle::Error, &format!("Theme '{theme_id}' not available: {err}"))?;
                        }
                    }
                    Ok(None)
                }
            },
            _ => Ok(Some(ActivePalette::Theme { mode, original_theme_id })),
        },
        ActivePalette::Sessions { mode, listings, limit, show_all } => {
            if show_sessions_palette(renderer, mode, &listings, limit, show_all)? {
                Ok(Some(ActivePalette::Sessions { mode, listings, limit, show_all }))
            } else {
                Ok(None)
            }
        }
        ActivePalette::ForkMode { session_id, listings, limit, show_all } => {
            if show_fork_mode_palette(renderer, &session_id)? {
                Ok(Some(ActivePalette::ForkMode { session_id, listings, limit, show_all }))
            } else {
                Ok(None)
            }
        }
        ActivePalette::Settings { mut state, esc_armed: _ } => {
            let normalized_selection = normalize_config_selection(&selection);
            let current_view = state.view_path.clone();
            if should_remember_settings_selection(&normalized_selection) {
                state.remember_selection(current_view.as_deref(), normalized_selection.clone());
            }

            if let InlineListSelection::ConfigAction(action) = &selection {
                match apply_settings_action(state.as_mut(), action) {
                    Ok(outcome) => {
                        if let Some(message) = outcome.message {
                            let tone = outcome.tone.unwrap_or(if outcome.saved {
                                vtcode_commons::ui_protocol::InlineTone::Success
                            } else {
                                vtcode_commons::ui_protocol::InlineTone::Accent
                            });
                            state.status = Some(vtcode_commons::ui_protocol::InlineStatus::new(tone, message.clone()));
                            renderer.line(MessageStyle::Info, &format!("Settings: {message}"))?;
                        }
                        if outcome.saved
                            && let Err(err) = refresh_runtime_config_from_manager(
                                renderer,
                                handle,
                                config,
                                vt_cfg,
                                provider_client,
                                session_bootstrap,
                                full_auto,
                            )
                            .await
                        {
                            let warning = format!(
                                "Settings saved, but the running session kept its last valid runtime config: {err:#}"
                            );
                            state.status = Some(vtcode_commons::ui_protocol::InlineStatus::warning(warning.clone()));
                            renderer.line(MessageStyle::Warning, &warning)?;
                        }
                    }
                    Err(err) => {
                        let warning =
                            format!("Could not apply settings change; keeping the last valid configuration: {err:#}");
                        state.status = Some(vtcode_commons::ui_protocol::InlineStatus::error(warning.clone()));
                        renderer.line(MessageStyle::Error, &warning)?;
                    }
                }
            }

            if show_settings_palette(renderer, state.as_ref(), Some(normalized_selection))? {
                Ok(Some(ActivePalette::Settings { state, esc_armed: false }))
            } else {
                Ok(None)
            }
        }
        ActivePalette::UrlGuard { prompt, previous } => Ok(Some(ActivePalette::UrlGuard { prompt, previous })),
        ActivePalette::Mode => Ok(None),
    }
}

pub(crate) fn handle_palette_preview(
    palette: ActivePalette,
    selection: InlineListSelection,
    renderer: &mut AnsiRenderer,
    handle: &InlineHandle,
) -> Result<Option<ActivePalette>> {
    match palette {
        ActivePalette::Theme { mode, original_theme_id } => {
            if let InlineListSelection::Theme(theme_id) = selection {
                match mode {
                    ThemePaletteMode::Select => {
                        if let Err(err) = theme::set_active_theme(&theme_id) {
                            renderer.line(MessageStyle::Error, &format!("Theme '{theme_id}' not available: {err}"))?;
                        } else {
                            let styles = theme::active_styles();
                            handle.set_theme(inline_theme_from_core_styles(&styles));
                            apply_prompt_style(handle);
                            handle.force_redraw();
                        }
                    }
                }
            }
            Ok(Some(ActivePalette::Theme { mode, original_theme_id }))
        }
        ActivePalette::UrlGuard { prompt, previous } => Ok(Some(ActivePalette::UrlGuard { prompt, previous })),
        ActivePalette::Settings { state, .. } => Ok(Some(ActivePalette::Settings { state, esc_armed: false })),
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

fn should_remember_settings_selection(selection: &InlineListSelection) -> bool {
    match selection {
        InlineListSelection::ConfigAction(action) => !matches!(
            action.as_str(),
            ACTION_OPEN_ROOT | ACTION_RELOAD | ACTION_RESET | ACTION_RESET_CANCEL | ACTION_RESET_CONFIRM | ACTION_BACK
        ),
        _ => true,
    }
}

pub(crate) fn handle_palette_cancel(
    palette: ActivePalette,
    renderer: &mut AnsiRenderer,
    handle: &InlineHandle,
) -> Result<Option<ActivePalette>> {
    match palette {
        ActivePalette::Theme { mode, original_theme_id } => {
            if theme::active_theme_id() != original_theme_id && theme::set_active_theme(&original_theme_id).is_ok() {
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
        ActivePalette::ForkMode { listings, limit, show_all, .. } => {
            if show_sessions_palette(renderer, SessionPaletteMode::Fork, &listings, limit, show_all)? {
                Ok(Some(ActivePalette::Sessions {
                    mode: SessionPaletteMode::Fork,
                    listings,
                    limit,
                    show_all,
                }))
            } else {
                Ok(None)
            }
        }
        ActivePalette::Settings { mut state, esc_armed } => {
            if esc_armed {
                return Ok(None);
            }

            let Some(current_path) = state.view_path.clone() else {
                if !renderer.supports_inline_ui() {
                    renderer.line(MessageStyle::Info, "Closed interactive settings.")?;
                }
                return Ok(None);
            };

            let parent_path = if current_path == "__settings_reset_confirmation" {
                None
            } else {
                parent_view_path(&current_path)
            };
            let selected = state.selection_for_view(parent_path.as_deref());
            state.view_path = parent_path;
            if show_settings_palette(renderer, state.as_ref(), selected)? {
                Ok(Some(ActivePalette::Settings { state, esc_armed: true }))
            } else {
                Ok(None)
            }
        }
        ActivePalette::Mode => Ok(None),
        ActivePalette::UrlGuard { previous, .. } => Ok(previous.map(|palette| *palette)),
    }
}

pub(crate) fn format_duration_label(duration: Duration) -> String {
    let total_seconds = duration.as_secs();
    let hours = total_seconds / 3600;
    let minutes = (total_seconds % 3600) / 60;
    let seconds = total_seconds % 60;

    let mut parts = Vec::new();
    if hours > 0 {
        parts.push(format!("{hours}h"));
    }
    if minutes > 0 || hours > 0 {
        parts.push(format!("{minutes}m"));
    }
    parts.push(format!("{seconds}s"));
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
    use chrono::{TimeZone, Utc};
    use vtcode_core::utils::session_archive::{SessionArchiveMetadata, SessionSnapshot};

    #[test]
    fn normalize_config_selection_maps_cycle_prev_to_cycle() {
        let selection = InlineListSelection::ConfigAction("ui.display_mode:cycle_prev".to_string());
        let normalized = normalize_config_selection(&selection);
        assert_eq!(normalized, InlineListSelection::ConfigAction("ui.display_mode:cycle".to_string()));
    }

    #[test]
    fn normalize_config_selection_maps_dec_to_inc() {
        let selection = InlineListSelection::ConfigAction("context.max_context_tokens:dec".to_string());
        let normalized = normalize_config_selection(&selection);
        assert_eq!(normalized, InlineListSelection::ConfigAction("context.max_context_tokens:inc".to_string()));
    }

    #[test]
    fn utility_settings_actions_do_not_replace_remembered_entry() {
        for action in [
            ACTION_OPEN_ROOT,
            ACTION_RELOAD,
            ACTION_RESET,
            ACTION_RESET_CANCEL,
            ACTION_RESET_CONFIRM,
            ACTION_BACK,
        ] {
            assert!(!should_remember_settings_selection(&InlineListSelection::ConfigAction(action.to_string(),)));
        }
    }

    #[test]
    fn session_search_value_includes_workspace_model_and_counts() {
        let listing = SessionListing {
            path: "/tmp/session.json".into(),
            snapshot: SessionSnapshot {
                metadata: SessionArchiveMetadata::new(
                    "vtcode",
                    "/workspace/vtcode",
                    "gpt-5.6-sol",
                    "openai",
                    "sunrise",
                    "medium",
                ),
                started_at: Utc.with_ymd_and_hms(2026, 3, 11, 9, 0, 0).unwrap(),
                ended_at: Utc.with_ymd_and_hms(2026, 3, 11, 9, 5, 0).unwrap(),
                total_messages: 12,
                distinct_tools: vec!["exec_command".to_string(), "code_search".to_string()],
                transcript: Vec::new(),
                messages: Vec::new(),
                progress: None,
                error_logs: Vec::new(),
            },
        };

        let value = session_search_value(&listing, "2026-03-11 16:05", "5m 0s", 2);
        assert!(value.contains("vtcode"));
        assert!(value.contains("gpt-5.6-sol"));
        assert!(value.contains("2026-03-11 16:05"));
        assert!(value.contains("5m 0s"));
        assert!(value.contains("12 messages"));
        assert!(value.contains("2 tools"));
    }
}
