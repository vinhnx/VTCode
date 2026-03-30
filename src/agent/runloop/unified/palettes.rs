use std::time::Duration;

use anyhow::Result;
use chrono::Local;

use vtcode_core::config::loader::{ConfigManager, VTCodeConfig};
use vtcode_core::llm::{
    LightweightFeature, LightweightRouteSource, auto_lightweight_model, lightweight_model_choices,
    resolve_lightweight_route,
};
use vtcode_core::ui::theme;
use vtcode_core::ui::{inline_theme_from_core_styles, to_tui_appearance};
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};
use vtcode_core::utils::session_archive::SessionListing;
use vtcode_tui::app::{InlineHandle, InlineListItem, InlineListSearchConfig, InlineListSelection};
use vtcode_tui::core::convert_style;

use crate::agent::runloop::slash_commands::{SessionPaletteMode, ThemePaletteMode};
use crate::agent::runloop::ui::build_inline_header_context;
use crate::agent::runloop::unified::model_selection::finalize_lightweight_model_selection;
use crate::agent::runloop::unified::settings_interactive::{
    SettingsPaletteState, apply_settings_action, parent_view_path, show_settings_palette,
};
use crate::agent::runloop::unified::state::ModelPickerTarget;
use crate::agent::runloop::welcome::SessionBootstrap;

use super::display::{persist_theme_preference, sync_runtime_theme_selection};

const THEME_PALETTE_TITLE: &str = "Theme";
const THEME_ACTIVE_BADGE: &str = "Active";
const THEME_SELECT_HINT: &str = "↑/↓ choose • Enter apply • Esc cancel";
const THEME_SEARCH_LABEL: &str = "Search themes";
const THEME_SEARCH_PLACEHOLDER: &str = "name, id, or appearance";
const SESSION_FORK_PALETTE_TITLE: &str = "Fork session";
const SESSION_FORK_MODE_PALETTE_TITLE: &str = "Fork mode";
const SESSION_RESUME_PALETTE_TITLE: &str = "Resume session";
const SESSIONS_HINT_PRIMARY: &str = "Use ↑/↓ to browse sessions.";
const SESSIONS_FORK_HINT_SECONDARY: &str = "Enter to fork session • Esc to close.";
const SESSIONS_RESUME_HINT_SECONDARY: &str = "Enter to resume session • Esc to close.";
const SESSIONS_LATEST_BADGE: &str = "Latest";
const SESSIONS_SEARCH_LABEL: &str = "Search sessions";
const SESSIONS_SEARCH_PLACEHOLDER: &str = "workspace, provider, model, date";
const FORK_MODE_HINT_SECONDARY: &str = "Enter to confirm • Esc to go back.";
const MODEL_TARGET_PALETTE_TITLE: &str = "Model";
const LIGHTWEIGHT_MODEL_PALETTE_TITLE: &str = "Lightweight model";
pub(crate) const MODEL_TARGET_ACTION_MAIN: &str = "model_target:main";
pub(crate) const MODEL_TARGET_ACTION_LIGHTWEIGHT: &str = "model_target:lightweight";
pub(crate) const LIGHTWEIGHT_MODEL_ACTION_PREFIX: &str = "lightweight_model:";

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
    ModelTarget,
    LightweightModel,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum ConfiguredLightweightSetting {
    Disabled,
    Automatic,
    Main,
    Explicit(String),
}

struct LightweightPaletteView {
    lines: Vec<String>,
    items: Vec<InlineListItem>,
    selected: Option<InlineListSelection>,
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
            search_value: Some(session_search_value(
                listing,
                &ended_local.to_string(),
                &duration_label,
                tool_count,
            )),
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
    renderer.show_list_modal(
        title,
        lines,
        items,
        selected,
        Some(InlineListSearchConfig {
            label: SESSIONS_SEARCH_LABEL.to_string(),
            placeholder: Some(SESSIONS_SEARCH_PLACEHOLDER.to_string()),
        }),
    );
    Ok(true)
}

pub(crate) fn show_fork_mode_palette(
    renderer: &mut AnsiRenderer,
    session_id: &str,
) -> Result<bool> {
    let items = vec![
        InlineListItem {
            title: "Copy full history".to_string(),
            subtitle: Some("Start the fork with the full archived transcript.".to_string()),
            badge: Some("Default".to_string()),
            indent: 0,
            selection: Some(InlineListSelection::SessionForkMode {
                session_id: session_id.to_string(),
                summarize: false,
            }),
            search_value: Some("copy full history fork transcript".to_string()),
        },
        InlineListItem {
            title: "Start summarized fork".to_string(),
            subtitle: Some(
                "Compact the source session into summary plus retained user prompts.".to_string(),
            ),
            badge: Some("Summary".to_string()),
            indent: 0,
            selection: Some(InlineListSelection::SessionForkMode {
                session_id: session_id.to_string(),
                summarize: true,
            }),
            search_value: Some("summary summarized compact fork handoff".to_string()),
        },
    ];

    let lines = vec![
        format!("Selected session: {session_id}"),
        "Choose how the forked session should start.".to_string(),
        FORK_MODE_HINT_SECONDARY.to_string(),
    ];

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

pub(crate) fn show_model_target_palette(renderer: &mut AnsiRenderer) -> Result<bool> {
    let items = [ModelPickerTarget::Main, ModelPickerTarget::Lightweight]
        .into_iter()
        .map(|target| match target {
            ModelPickerTarget::Main => InlineListItem {
                title: "Main model".to_string(),
                subtitle: Some(
                    "Change the active provider/model for the current conversation session."
                        .to_string(),
                ),
                badge: Some("Active".to_string()),
                indent: 0,
                selection: Some(InlineListSelection::ConfigAction(
                    MODEL_TARGET_ACTION_MAIN.to_string(),
                )),
                search_value: Some("model main active conversation provider default".to_string()),
            },
            ModelPickerTarget::Lightweight => InlineListItem {
                title: "Lightweight model".to_string(),
                subtitle: Some(
                    "Configure the shared lower-cost route for memory, prompt suggestions, and smaller delegated tasks."
                        .to_string(),
                ),
                badge: Some("Shared".to_string()),
                indent: 0,
                selection: Some(InlineListSelection::ConfigAction(
                    MODEL_TARGET_ACTION_LIGHTWEIGHT.to_string(),
                )),
                search_value: Some(
                    "model lightweight memory prompt suggestions subagent".to_string(),
                ),
            },
        })
        .collect();

    renderer.show_list_modal(
        MODEL_TARGET_PALETTE_TITLE,
        vec![
            "Choose which model target to edit.".to_string(),
            "Main model changes the active conversation model. Lightweight model updates shared side-task routing only.".to_string(),
        ],
        items,
        Some(InlineListSelection::ConfigAction(
            MODEL_TARGET_ACTION_MAIN.to_string(),
        )),
        None,
    );

    Ok(true)
}

pub(crate) fn show_lightweight_model_palette(
    renderer: &mut AnsiRenderer,
    config: &vtcode_core::config::types::AgentConfig,
    vt_cfg: Option<&VTCodeConfig>,
) -> Result<bool> {
    let view = build_lightweight_model_palette(config, vt_cfg);
    renderer.show_list_modal(
        LIGHTWEIGHT_MODEL_PALETTE_TITLE,
        view.lines,
        view.items,
        view.selected,
        None,
    );
    Ok(true)
}

fn build_lightweight_model_palette(
    config: &vtcode_core::config::types::AgentConfig,
    vt_cfg: Option<&VTCodeConfig>,
) -> LightweightPaletteView {
    let current_setting = configured_lightweight_setting(config, vt_cfg);
    let configured_label = current_setting.label();
    let resolution =
        resolve_lightweight_route(config, vt_cfg, LightweightFeature::PromptSuggestions, None);
    let effective_route = match resolution.source {
        LightweightRouteSource::MainModel => config.model.clone(),
        _ => match resolution.fallback_to_main_model() {
            Some(fallback) => format!(
                "{} -> fallback {}",
                resolution.primary.model, fallback.model
            ),
            None => resolution.primary.model.clone(),
        },
    };

    let auto_model = auto_lightweight_model(&config.provider, &config.model);
    let mut explicit_choices = lightweight_model_choices(&config.provider, &config.model);
    explicit_choices.retain(|model| !model.eq_ignore_ascii_case(config.model.as_str()));
    explicit_choices.retain(|model| !model.eq_ignore_ascii_case(auto_model.as_str()));

    let mut items = vec![
        InlineListItem {
            title: "Automatic (recommended)".to_string(),
            subtitle: Some(format!(
                "Use {} for lower-cost side tasks and fall back to {}.",
                auto_model, config.model
            )),
            badge: Some(
                if matches!(current_setting, ConfiguredLightweightSetting::Automatic) {
                    "Current".to_string()
                } else {
                    "Recommended".to_string()
                },
            ),
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction(format!(
                "{}auto",
                LIGHTWEIGHT_MODEL_ACTION_PREFIX
            ))),
            search_value: Some("lightweight model automatic recommended".to_string()),
        },
        InlineListItem {
            title: "Use main model".to_string(),
            subtitle: Some(format!(
                "Keep lightweight work on {} for accuracy-first behavior.",
                config.model
            )),
            badge: Some(
                if matches!(current_setting, ConfiguredLightweightSetting::Main) {
                    "Current".to_string()
                } else {
                    "Accuracy".to_string()
                },
            ),
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction(format!(
                "{}main",
                LIGHTWEIGHT_MODEL_ACTION_PREFIX
            ))),
            search_value: Some("lightweight model main accuracy".to_string()),
        },
    ];

    items.extend(explicit_choices.iter().map(|model| InlineListItem {
        title: model.clone(),
        subtitle: Some("Explicit same-provider lightweight model.".to_string()),
        badge: Some(if matches!(
            &current_setting,
            ConfiguredLightweightSetting::Explicit(current) if current.eq_ignore_ascii_case(model)
        ) {
            "Current".to_string()
        } else {
            "Model".to_string()
        }),
        indent: 0,
        selection: Some(InlineListSelection::ConfigAction(format!(
            "{}{}",
            LIGHTWEIGHT_MODEL_ACTION_PREFIX, model
        ))),
        search_value: Some(format!("lightweight model {}", model)),
    }));

    let selected = Some(InlineListSelection::ConfigAction(match &current_setting {
        ConfiguredLightweightSetting::Main => format!("{}main", LIGHTWEIGHT_MODEL_ACTION_PREFIX),
        ConfiguredLightweightSetting::Explicit(model) => {
            format!("{}{}", LIGHTWEIGHT_MODEL_ACTION_PREFIX, model)
        }
        ConfiguredLightweightSetting::Disabled | ConfiguredLightweightSetting::Automatic => {
            format!("{}auto", LIGHTWEIGHT_MODEL_ACTION_PREFIX)
        }
    }));

    let mut lines = vec![
        "Choose the shared lightweight model VT Code should prefer for memory triage, prompt suggestions, and smaller delegated tasks.".to_string(),
        format!("Current setting: {}", configured_label),
        format!("Effective route: {}", effective_route),
        "Selecting any option enables the shared lightweight route without changing the active main conversation model.".to_string(),
    ];
    if let Some(warning) = resolution.warning {
        lines.push(format!("Route warning: {}", warning));
    }

    LightweightPaletteView {
        lines,
        items,
        selected,
    }
}

fn configured_lightweight_setting(
    config: &vtcode_core::config::types::AgentConfig,
    vt_cfg: Option<&VTCodeConfig>,
) -> ConfiguredLightweightSetting {
    let Some(vt_cfg) = vt_cfg else {
        return ConfiguredLightweightSetting::Automatic;
    };
    if !vt_cfg.agent.small_model.enabled {
        return ConfiguredLightweightSetting::Disabled;
    }

    let configured_model = vt_cfg.agent.small_model.model.trim();
    if configured_model.is_empty() {
        return ConfiguredLightweightSetting::Automatic;
    }
    if configured_model.eq_ignore_ascii_case(config.model.as_str()) {
        return ConfiguredLightweightSetting::Main;
    }

    ConfiguredLightweightSetting::Explicit(configured_model.to_string())
}

impl ConfiguredLightweightSetting {
    fn label(&self) -> String {
        match self {
            ConfiguredLightweightSetting::Disabled => "Disabled".to_string(),
            ConfiguredLightweightSetting::Automatic => "Automatic".to_string(),
            ConfiguredLightweightSetting::Main => "Use main model".to_string(),
            ConfiguredLightweightSetting::Explicit(model) => model.clone(),
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn refresh_runtime_config_from_manager(
    renderer: &mut AnsiRenderer,
    handle: &InlineHandle,
    config: &mut vtcode_core::config::types::AgentConfig,
    vt_cfg: &mut Option<VTCodeConfig>,
    provider_client: &dyn vtcode_core::llm::provider::LLMProvider,
    session_bootstrap: &SessionBootstrap,
    full_auto: bool,
) -> Result<()> {
    if let Ok(runtime_manager) = ConfigManager::load_from_workspace(&config.workspace) {
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

        let provider_label = {
            let label = crate::agent::runloop::unified::session_setup::resolve_provider_label(
                config,
                Some(&runtime_config),
            );
            if label.is_empty() {
                provider_client.name().to_string()
            } else {
                label
            }
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
            provider_client.effective_context_size(&config.model),
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
                            sync_runtime_theme_selection(config, vt_cfg.as_mut(), &theme_id);
                            persist_theme_preference(renderer, &config.workspace, &theme_id)
                                .await?;
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
        ActivePalette::ForkMode {
            session_id,
            listings,
            limit,
            show_all,
        } => {
            if show_fork_mode_palette(renderer, &session_id)? {
                Ok(Some(ActivePalette::ForkMode {
                    session_id,
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
        ActivePalette::ModelTarget => {
            if show_model_target_palette(renderer)? {
                Ok(Some(ActivePalette::ModelTarget))
            } else {
                Ok(None)
            }
        }
        ActivePalette::LightweightModel => {
            let Some(action) = (match selection {
                InlineListSelection::ConfigAction(action) => Some(action),
                _ => None,
            }) else {
                if show_lightweight_model_palette(renderer, config, vt_cfg.as_ref())? {
                    return Ok(Some(ActivePalette::LightweightModel));
                }
                return Ok(None);
            };

            let Some(choice) = action.strip_prefix(LIGHTWEIGHT_MODEL_ACTION_PREFIX) else {
                if show_lightweight_model_palette(renderer, config, vt_cfg.as_ref())? {
                    return Ok(Some(ActivePalette::LightweightModel));
                }
                return Ok(None);
            };

            let selected_model = match choice {
                "auto" => String::new(),
                "main" => config.model.clone(),
                explicit => explicit.to_string(),
            };
            finalize_lightweight_model_selection(renderer, config, vt_cfg, selected_model).await?;
            Ok(None)
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
        ActivePalette::ModelTarget => Ok(Some(ActivePalette::ModelTarget)),
        ActivePalette::LightweightModel => Ok(Some(ActivePalette::LightweightModel)),
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
        ActivePalette::ForkMode {
            listings,
            limit,
            show_all,
            ..
        } => {
            if show_sessions_palette(
                renderer,
                SessionPaletteMode::Fork,
                &listings,
                limit,
                show_all,
            )? {
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
        ActivePalette::ModelTarget | ActivePalette::LightweightModel => Ok(None),
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
    use chrono::{TimeZone, Utc};
    use vtcode_core::config::constants::models;
    use vtcode_core::config::core::PromptCachingConfig;
    use vtcode_core::config::models::Provider;
    use vtcode_core::config::types::{
        AgentConfig as CoreAgentConfig, ModelSelectionSource, ReasoningEffortLevel,
        UiSurfacePreference,
    };
    use vtcode_core::core::agent::snapshots::{
        DEFAULT_CHECKPOINTS_ENABLED, DEFAULT_MAX_AGE_DAYS, DEFAULT_MAX_SNAPSHOTS,
    };
    use vtcode_core::utils::session_archive::{SessionArchiveMetadata, SessionSnapshot};

    fn runtime_config(provider: &str, model: &str) -> CoreAgentConfig {
        CoreAgentConfig {
            model: model.to_string(),
            api_key: "test-key".to_string(),
            provider: provider.to_string(),
            api_key_env: Provider::OpenAI.default_api_key_env().to_string(),
            workspace: std::env::current_dir().expect("current_dir"),
            verbose: false,
            quiet: false,
            theme: vtcode_core::ui::theme::DEFAULT_THEME_ID.to_string(),
            reasoning_effort: ReasoningEffortLevel::default(),
            ui_surface: UiSurfacePreference::default(),
            prompt_cache: PromptCachingConfig::default(),
            model_source: ModelSelectionSource::WorkspaceConfig,
            custom_api_keys: std::collections::BTreeMap::new(),
            checkpointing_enabled: DEFAULT_CHECKPOINTS_ENABLED,
            checkpointing_storage_dir: None,
            checkpointing_max_snapshots: DEFAULT_MAX_SNAPSHOTS,
            checkpointing_max_age_days: Some(DEFAULT_MAX_AGE_DAYS),
            max_conversation_turns: 1000,
            model_behavior: None,
            openai_chatgpt_auth: None,
        }
    }

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

    #[test]
    fn session_search_value_includes_workspace_model_and_counts() {
        let listing = SessionListing {
            path: "/tmp/session.json".into(),
            snapshot: SessionSnapshot {
                metadata: SessionArchiveMetadata::new(
                    "vtcode",
                    "/workspace/vtcode",
                    "gpt-5.4",
                    "openai",
                    "sunrise",
                    "medium",
                ),
                started_at: Utc.with_ymd_and_hms(2026, 3, 11, 9, 0, 0).unwrap(),
                ended_at: Utc.with_ymd_and_hms(2026, 3, 11, 9, 5, 0).unwrap(),
                total_messages: 12,
                distinct_tools: vec!["unified_exec".to_string(), "unified_search".to_string()],
                transcript: Vec::new(),
                messages: Vec::new(),
                progress: None,
                error_logs: Vec::new(),
            },
        };

        let value = session_search_value(&listing, "2026-03-11 16:05", "5m 0s", 2);
        assert!(value.contains("vtcode"));
        assert!(value.contains("gpt-5.4"));
        assert!(value.contains("2026-03-11 16:05"));
        assert!(value.contains("5m 0s"));
        assert!(value.contains("12 messages"));
        assert!(value.contains("2 tools"));
    }

    #[test]
    fn lightweight_model_palette_includes_automatic_main_and_explicit_choices() {
        let config = runtime_config("openai", models::openai::GPT_5_4);

        let mut vt_cfg = VTCodeConfig::default();
        vt_cfg.agent.small_model.model.clear();

        let view = build_lightweight_model_palette(&config, Some(&vt_cfg));
        assert!(
            view.items
                .iter()
                .any(|item| item.title == "Automatic (recommended)")
        );
        assert!(view.items.iter().any(|item| item.title == "Use main model"));
        assert_eq!(
            view.selected,
            Some(InlineListSelection::ConfigAction(format!(
                "{}auto",
                LIGHTWEIGHT_MODEL_ACTION_PREFIX
            )))
        );
        assert!(
            view.lines
                .iter()
                .any(|line| line.contains("gpt-5.4-mini -> fallback gpt-5.4"))
        );
    }

    #[test]
    fn lightweight_model_palette_marks_main_model_choice_when_configured() {
        let config = runtime_config("openai", models::openai::GPT_5_4);

        let mut vt_cfg = VTCodeConfig::default();
        vt_cfg.agent.small_model.model = "gpt-5.4".to_string();

        let view = build_lightweight_model_palette(&config, Some(&vt_cfg));
        assert_eq!(
            view.selected,
            Some(InlineListSelection::ConfigAction(format!(
                "{}main",
                LIGHTWEIGHT_MODEL_ACTION_PREFIX
            )))
        );
    }
}
