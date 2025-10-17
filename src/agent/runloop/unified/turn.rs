use anyhow::{Context, Result, anyhow};
use chrono::Local;
use futures::StreamExt;
use indicatif::ProgressStyle;
use std::collections::{BTreeSet, HashSet};
use std::fmt::Write as FmtWrite;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use tokio::sync::Notify;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::task;
use tokio::time::sleep;

use serde_json::Value;
use tempfile::Builder;
use toml::Value as TomlValue;
use tracing::warn;
use vtcode_core::SimpleIndexer;
use vtcode_core::config::api_keys::{ApiKeySources, get_api_key};
use vtcode_core::config::constants::tools as tool_names;
use vtcode_core::config::constants::{defaults, ui};
use vtcode_core::config::loader::{ConfigManager, VTCodeConfig};
use vtcode_core::config::models::Provider;
use vtcode_core::config::types::{AgentConfig as CoreAgentConfig, UiSurfacePreference};
use vtcode_core::core::context_curator::{
    ConversationPhase, CuratedContext, Message as CuratorMessage,
    ToolDefinition as CuratorToolDefinition,
};
use vtcode_core::core::decision_tracker::{Action as DTAction, DecisionOutcome};
use vtcode_core::core::router::{Router, TaskClass};
use vtcode_core::core::token_budget::{ContextComponent, TokenBudgetManager};
use vtcode_core::llm::error_display;
use vtcode_core::llm::factory::create_provider_with_config;
use vtcode_core::llm::provider::{self as uni, LLMStreamEvent};
use vtcode_core::llm::rig_adapter::{reasoning_parameters_for, verify_model_with_rig};
use vtcode_core::tool_policy::ToolPolicy;
use vtcode_core::tools::registry::{ToolErrorType, ToolExecutionError, ToolPermissionDecision};
use vtcode_core::ui::slash::{SLASH_COMMANDS, SlashCommandInfo};
use vtcode_core::ui::theme;
use vtcode_core::ui::tui::{
    InlineEvent, InlineHandle, InlineListItem, InlineListSelection, InlineTextStyle,
    convert_style as convert_ui_style, spawn_session, theme_from_styles,
};
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};
use vtcode_core::utils::session_archive::{
    self, SessionArchive, SessionArchiveMetadata, SessionListing, SessionMessage,
};
use vtcode_core::utils::transcript;

use crate::agent::runloop::context::{
    apply_aggressive_trim_unified, enforce_unified_context_window, prune_unified_tool_responses,
};
use crate::agent::runloop::git::{confirm_changes_with_git_diff, git_status_summary};
use crate::agent::runloop::is_context_overflow_error;
use crate::agent::runloop::model_picker::{
    ModelPickerProgress, ModelPickerState, ModelSelectionResult,
};
use crate::agent::runloop::prompt::refine_user_prompt_if_enabled;
use crate::agent::runloop::slash_commands::{
    SlashCommandOutcome, ThemePaletteMode, handle_slash_command,
};
use crate::agent::runloop::text_tools::{detect_textual_tool_call, extract_code_fence_blocks};
use crate::agent::runloop::tool_output::render_code_fence_blocks;
use crate::agent::runloop::tool_output::render_tool_output;
use crate::agent::runloop::ui::{build_inline_header_context, render_session_banner};

use super::display::{display_user_message, ensure_turn_bottom_gap, persist_theme_preference};
use super::session_setup::{SessionState, initialize_session};
use super::shell::{derive_recent_tool_output, should_short_circuit_shell};
use crate::agent::runloop::mcp_events;
use crate::agent::runloop::welcome::SessionBootstrap;

#[derive(Default)]
struct SessionStats {
    tools: BTreeSet<String>,
}

impl SessionStats {
    fn record_tool(&mut self, name: &str) {
        self.tools.insert(name.to_string());
    }

    fn sorted_tools(&self) -> Vec<String> {
        self.tools.iter().cloned().collect()
    }
}

const THEME_PALETTE_TITLE: &str = "Theme picker";
const THEME_LIST_TITLE: &str = "Available themes";
const THEME_ACTIVE_BADGE: &str = "Active";
const THEME_SELECT_HINT: &str = "Use ↑/↓ to choose a theme, Enter to apply, Esc to cancel.";
const THEME_INSPECT_HINT: &str = "Use ↑/↓ to browse themes, Enter to view details, Esc to close.";
const SESSIONS_PALETTE_TITLE: &str = "Archived sessions";
const SESSIONS_HINT_PRIMARY: &str = "Use ↑/↓ to browse sessions.";
const SESSIONS_HINT_SECONDARY: &str = "Enter to print details • Esc to close.";
const SESSIONS_LATEST_BADGE: &str = "Latest";
const HELP_PALETTE_TITLE: &str = "Slash command help";
const HELP_HINT_PRIMARY: &str = "Use ↑/↓ to pick a slash command.";
const HELP_HINT_SECONDARY: &str = "Enter to insert into the input • Esc to dismiss.";

enum ActivePalette {
    Theme {
        mode: ThemePaletteMode,
    },
    Sessions {
        listings: Vec<SessionListing>,
        limit: usize,
    },
    Help,
}

fn show_theme_palette(renderer: &mut AnsiRenderer, mode: ThemePaletteMode) -> Result<bool> {
    let title = match mode {
        ThemePaletteMode::Select => THEME_PALETTE_TITLE,
        ThemePaletteMode::Inspect => THEME_LIST_TITLE,
    };
    let hint = match mode {
        ThemePaletteMode::Select => THEME_SELECT_HINT,
        ThemePaletteMode::Inspect => THEME_INSPECT_HINT,
    };

    let current_id = theme::active_theme_id();
    let current_label = theme::active_theme_label().to_string();
    let mut items = Vec::new();

    for id in theme::available_themes() {
        let label = theme::theme_label(id).unwrap_or(id);
        let badge = (id == current_id).then(|| THEME_ACTIVE_BADGE.to_string());
        items.push(InlineListItem {
            title: label.to_string(),
            subtitle: Some(format!("ID: {}", id)),
            badge,
            indent: 0,
            selection: Some(InlineListSelection::Theme(id.to_string())),
            search_value: None,
        });
    }

    if items.is_empty() {
        renderer.line(MessageStyle::Info, "No themes available.")?;
        return Ok(false);
    }

    let lines = vec![
        format!("Current theme: {}", current_label),
        hint.to_string(),
    ];
    renderer.show_list_modal(
        title,
        lines,
        items,
        Some(InlineListSelection::Theme(current_id)),
        None,
    );

    Ok(true)
}

fn show_sessions_palette(
    renderer: &mut AnsiRenderer,
    listings: &[SessionListing],
    limit: usize,
) -> Result<bool> {
    if listings.is_empty() {
        renderer.line(MessageStyle::Info, "No archived sessions found.")?;
        return Ok(false);
    }

    let mut items = Vec::new();
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

    let lines = vec![
        format!("Showing {} of {} archived sessions", listings.len(), limit),
        SESSIONS_HINT_PRIMARY.to_string(),
        SESSIONS_HINT_SECONDARY.to_string(),
    ];
    let selected = listings
        .first()
        .map(|listing| InlineListSelection::Session(listing.identifier()));
    renderer.show_list_modal(SESSIONS_PALETTE_TITLE, lines, items, selected, None);
    Ok(true)
}

fn show_help_palette(
    renderer: &mut AnsiRenderer,
    commands: &[&'static SlashCommandInfo],
) -> Result<bool> {
    if commands.is_empty() {
        renderer.line(MessageStyle::Info, "No slash commands available.")?;
        return Ok(false);
    }

    let mut items = Vec::new();
    for info in commands {
        let subtitle = if info.description.is_empty() {
            None
        } else {
            Some(info.description.to_string())
        };
        items.push(InlineListItem {
            title: format!("/{}", info.name),
            subtitle,
            badge: None,
            indent: 0,
            selection: Some(InlineListSelection::SlashCommand(info.name.to_string())),
            search_value: None,
        });
    }

    let lines = vec![
        HELP_HINT_PRIMARY.to_string(),
        HELP_HINT_SECONDARY.to_string(),
    ];
    let selected = commands
        .first()
        .map(|info| InlineListSelection::SlashCommand(info.name.to_string()));
    renderer.show_list_modal(HELP_PALETTE_TITLE, lines, items, selected, None);
    Ok(true)
}

fn render_session_details(renderer: &mut AnsiRenderer, listing: &SessionListing) -> Result<()> {
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

    renderer.line(
        MessageStyle::Info,
        &format!(
            "- (ID: {}) {} · Model: {} · Workspace: {}",
            listing.identifier(),
            ended_local,
            listing.snapshot.metadata.model,
            listing.snapshot.metadata.workspace_label,
        ),
    )?;
    renderer.line(
        MessageStyle::Info,
        &format!(
            "    Duration: {} · {} msgs · {} tools",
            duration_label, listing.snapshot.total_messages, tool_count,
        ),
    )?;

    if let Some(prompt) = listing.first_prompt_preview() {
        renderer.line(MessageStyle::Info, &format!("    Prompt: {prompt}"))?;
    }

    if let Some(reply) = listing.first_reply_preview() {
        renderer.line(MessageStyle::Info, &format!("    Reply: {reply}"))?;
    }

    renderer.line(
        MessageStyle::Info,
        &format!("    File: {}", listing.path.display()),
    )?;
    Ok(())
}

fn format_duration_label(duration: Duration) -> String {
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

fn handle_palette_selection(
    palette: ActivePalette,
    selection: InlineListSelection,
    renderer: &mut AnsiRenderer,
    handle: &InlineHandle,
) -> Result<Option<ActivePalette>> {
    match palette {
        ActivePalette::Theme { mode } => match selection {
            InlineListSelection::Theme(theme_id) => match mode {
                ThemePaletteMode::Select => {
                    match theme::set_active_theme(&theme_id) {
                        Ok(()) => {
                            let label = theme::active_theme_label();
                            renderer.line(
                                MessageStyle::Info,
                                &format!("Theme switched to {}", label),
                            )?;
                            persist_theme_preference(renderer, &theme_id)?;
                            let styles = theme::active_styles();
                            handle.set_theme(theme_from_styles(&styles));
                            apply_prompt_style(handle);
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
                ThemePaletteMode::Inspect => {
                    let label = theme::theme_label(&theme_id).unwrap_or_else(|| theme_id.as_str());
                    renderer.line(
                        MessageStyle::Info,
                        &format!("Theme {} ({}) is available.", label, theme_id),
                    )?;
                    if show_theme_palette(renderer, mode)? {
                        Ok(Some(ActivePalette::Theme { mode }))
                    } else {
                        Ok(None)
                    }
                }
            },
            _ => Ok(Some(ActivePalette::Theme { mode })),
        },
        ActivePalette::Sessions { listings, limit } => {
            if let InlineListSelection::Session(selected_id) = &selection {
                if let Some(listing) = listings
                    .iter()
                    .find(|entry| entry.identifier() == *selected_id)
                    .cloned()
                {
                    render_session_details(renderer, &listing)?;
                }
            }
            if show_sessions_palette(renderer, &listings, limit)? {
                Ok(Some(ActivePalette::Sessions { listings, limit }))
            } else {
                Ok(None)
            }
        }
        ActivePalette::Help => {
            if let InlineListSelection::SlashCommand(command) = selection {
                handle.set_input(format!("/{} ", command));
                renderer.line(
                    MessageStyle::Info,
                    &format!("Inserted '/{}' into the input.", command),
                )?;
            }
            Ok(None)
        }
    }
}

fn handle_palette_cancel(palette: ActivePalette, renderer: &mut AnsiRenderer) -> Result<()> {
    match palette {
        ActivePalette::Theme { mode } => {
            let message = match mode {
                ThemePaletteMode::Select => "Theme selection cancelled.",
                ThemePaletteMode::Inspect => "Closed theme list.",
            };
            renderer.line(MessageStyle::Info, message)?;
        }
        ActivePalette::Sessions { .. } => {
            renderer.line(MessageStyle::Info, "Closed session browser.")?;
        }
        ActivePalette::Help => {
            renderer.line(MessageStyle::Info, "Dismissed slash command help.")?;
        }
    }
    Ok(())
}

struct CuratedPromptSection {
    heading: &'static str,
    component: ContextComponent,
    body: String,
}

fn map_role_to_component(role: &uni::MessageRole) -> ContextComponent {
    match role {
        uni::MessageRole::System => ContextComponent::SystemPrompt,
        uni::MessageRole::User => ContextComponent::UserMessage,
        uni::MessageRole::Assistant => ContextComponent::AssistantMessage,
        uni::MessageRole::Tool => ContextComponent::ToolResult,
    }
}

fn describe_phase(phase: ConversationPhase) -> Option<String> {
    match phase {
        ConversationPhase::Exploration => Some("Exploration – gathering context".to_string()),
        ConversationPhase::Implementation => {
            Some("Implementation – applying code changes".to_string())
        }
        ConversationPhase::Validation => {
            Some("Validation – executing tests and checks".to_string())
        }
        ConversationPhase::Debugging => {
            Some("Debugging – addressing failures or regressions".to_string())
        }
        ConversationPhase::Unknown => None,
    }
}

fn resolve_mode_label(preference: UiSurfacePreference, full_auto: bool) -> String {
    let base = match preference {
        UiSurfacePreference::Alternate => ui::HEADER_MODE_ALTERNATE,
        UiSurfacePreference::Inline => ui::HEADER_MODE_INLINE,
        UiSurfacePreference::Auto => ui::HEADER_MODE_AUTO,
    };
    if full_auto {
        format!("{}{}", base, ui::HEADER_MODE_FULL_AUTO_SUFFIX)
    } else {
        base.to_string()
    }
}

fn format_provider_label(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    Provider::from_str(trimmed)
        .map(|provider| provider.label().to_string())
        .unwrap_or_else(|_| {
            let mut chars = trimmed.chars();
            let Some(first) = chars.next() else {
                return String::new();
            };
            let mut formatted: String = first.to_uppercase().collect();
            formatted.push_str(chars.as_str());
            formatted
        })
}

fn build_curated_sections(context: &CuratedContext) -> Vec<CuratedPromptSection> {
    let mut sections = Vec::new();

    if let Some(summary) = &context.ledger_summary {
        if !summary.trim().is_empty() {
            sections.push(CuratedPromptSection {
                heading: "Decision Ledger",
                component: ContextComponent::DecisionLedger,
                body: summary.trim().to_string(),
            });
        }
    }

    if !context.active_files.is_empty() {
        let mut body = String::new();
        for file in &context.active_files {
            let _ = writeln!(body, "{} ({} lines)", file.path, file.size_lines);
            if !file.summary.trim().is_empty() {
                let _ = writeln!(body, "  {}", file.summary.trim());
            }
        }
        sections.push(CuratedPromptSection {
            heading: "Active Files",
            component: ContextComponent::FileContent,
            body,
        });
    }

    if !context.recent_errors.is_empty() {
        let mut body = String::new();
        for error in &context.recent_errors {
            let mut line = error.error_message.trim().to_string();
            if let Some(tool) = &error.tool_name {
                line.push_str(&format!(" (tool: {})", tool));
            }
            let _ = writeln!(body, "{}", line);
            if let Some(resolution) = &error.resolution {
                if !resolution.trim().is_empty() {
                    let _ = writeln!(body, "  resolution: {}", resolution.trim());
                }
            }
        }
        sections.push(CuratedPromptSection {
            heading: "Recent Errors",
            component: ContextComponent::ToolResult,
            body,
        });
    }

    if !context.relevant_tools.is_empty() {
        let mut body = String::new();
        for tool in &context.relevant_tools {
            let description = tool.description.trim();
            if description.is_empty() {
                let _ = writeln!(body, "{}", tool.name);
            } else {
                let _ = writeln!(body, "{} – {}", tool.name, description);
            }
        }
        sections.push(CuratedPromptSection {
            heading: "Relevant Tools",
            component: ContextComponent::ProjectGuidelines,
            body,
        });
    }

    if let Some(phase_text) = describe_phase(context.phase) {
        sections.push(CuratedPromptSection {
            heading: "Conversation Phase",
            component: ContextComponent::ProjectGuidelines,
            body: phase_text,
        });
    }

    sections
}

async fn build_curator_messages(
    history: &[uni::Message],
    token_budget: &TokenBudgetManager,
    token_budget_enabled: bool,
) -> Result<Vec<CuratorMessage>> {
    let mut messages = Vec::with_capacity(history.len());

    for (index, message) in history.iter().enumerate() {
        let mut materialized = message.content.clone();
        if let Some(tool_calls) = &message.tool_calls {
            if !tool_calls.is_empty() {
                let serialized =
                    serde_json::to_string(tool_calls).unwrap_or_else(|_| "[]".to_string());
                if !serialized.is_empty() {
                    if !materialized.is_empty() {
                        materialized.push('\n');
                    }
                    materialized.push_str(&serialized);
                }
            }
        }

        let component = map_role_to_component(&message.role);
        let component_id = format!("msg_{}", index);
        let component_id_ref = Some(component_id.as_str());
        let estimated_tokens = if token_budget_enabled {
            match token_budget
                .count_tokens_for_component(&materialized, component, component_id_ref)
                .await
            {
                Ok(count) => count,
                Err(err) => {
                    warn!(
                        ?err,
                        "Failed to count tokens for conversation message; using rough estimate"
                    );
                    let estimate = materialized.len() / 4;
                    token_budget
                        .record_tokens_for_component(component, estimate, component_id_ref)
                        .await;
                    estimate
                }
            }
        } else {
            materialized.len() / 4
        };

        messages.push(CuratorMessage {
            role: message.role.as_generic_str().to_string(),
            content: materialized,
            estimated_tokens,
        });
    }

    Ok(messages)
}

fn build_curator_tools(tools: &[uni::ToolDefinition]) -> Vec<CuratorToolDefinition> {
    tools
        .iter()
        .map(|tool| {
            let parameters_repr = tool.function.parameters.to_string();
            let estimated_tokens = tool.function.description.len() / 4 + parameters_repr.len() / 4;
            CuratorToolDefinition {
                name: tool.function.name.clone(),
                description: tool.function.description.clone(),
                estimated_tokens,
            }
        })
        .collect()
}

fn finalize_model_selection(
    renderer: &mut AnsiRenderer,
    picker: &ModelPickerState,
    selection: ModelSelectionResult,
    config: &mut CoreAgentConfig,
    vt_cfg: &mut Option<VTCodeConfig>,
    provider_client: &mut Box<dyn uni::LLMProvider>,
    session_bootstrap: &SessionBootstrap,
    handle: &InlineHandle,
    full_auto: bool,
) -> Result<()> {
    let workspace = config.workspace.clone();

    let api_key = if let Some(key) = selection.api_key.as_ref() {
        persist_env_value(&workspace, &selection.env_key, key)?;
        unsafe {
            // SAFETY: we only write ASCII-alphanumeric keys derived from known providers or
            // sanitized user input, and values are supplied directly by the user.
            std::env::set_var(&selection.env_key, key);
        }
        key.clone()
    } else if selection.provider_enum.is_some() {
        let key = get_api_key(&selection.provider, &ApiKeySources::default())
            .with_context(|| format!("API key not found for provider '{}'", selection.provider))?;
        unsafe {
            // SAFETY: see above. Keys are sanitized and values come from configuration sources.
            std::env::set_var(&selection.env_key, &key);
        }
        key
    } else {
        match std::env::var(&selection.env_key) {
            Ok(value) if !value.trim().is_empty() => value,
            _ if selection.requires_api_key => {
                return Err(anyhow!(
                    "API key not found for provider '{}'. Set {} or enter a key to continue.",
                    selection.provider,
                    selection.env_key
                ));
            }
            _ => String::new(),
        }
    };

    if let Some(provider_enum) = selection.provider_enum {
        if let Err(err) = verify_model_with_rig(provider_enum, &selection.model, &api_key) {
            renderer.line(
                MessageStyle::Error,
                &format!(
                    "Rig validation warning: unable to initialise {} via rig-core ({err}).",
                    selection.model_display
                ),
            )?;
        }
    }

    let updated_cfg = picker.persist_selection(&workspace, &selection)?;
    *vt_cfg = Some(updated_cfg);

    if let Some(provider_enum) = selection.provider_enum {
        let provider_name = selection.provider.clone();
        let new_client = create_provider_with_config(
            &provider_name,
            Some(api_key.clone()),
            None,
            Some(selection.model.clone()),
            Some(config.prompt_cache.clone()),
        )
        .context("Failed to initialize provider for the selected model")?;
        *provider_client = new_client;
        config.provider = provider_enum.to_string();
    } else {
        renderer.line(
            MessageStyle::Info,
            "Saved selection, but custom providers require manual configuration before taking effect.",
        )?;
        config.provider = selection.provider.clone();
    }

    config.model = selection.model.clone();
    config.api_key = api_key;
    config.reasoning_effort = selection.reasoning;
    config.api_key_env = selection.env_key.clone();
    if let Some(ref key) = selection.api_key {
        config
            .custom_api_keys
            .insert(selection.provider.clone(), key.clone());
    } else {
        config.custom_api_keys.remove(&selection.provider);
    }

    if let Some(provider_enum) = selection.provider_enum {
        if selection.reasoning_supported {
            if let Some(payload) = reasoning_parameters_for(provider_enum, selection.reasoning) {
                renderer.line(
                    MessageStyle::Info,
                    &format!("Rig reasoning configuration prepared: {}", payload),
                )?;
            }
        }
    }

    let reasoning_label = selection.reasoning.as_str().to_string();
    let mode_label = resolve_mode_label(config.ui_surface, full_auto);
    let header_context = build_inline_header_context(
        config,
        session_bootstrap,
        selection.provider_label.clone(),
        selection.model.clone(),
        mode_label,
        reasoning_label.clone(),
    )?;
    handle.set_header_context(header_context);

    renderer.line(
        MessageStyle::Info,
        &format!(
            "Model set to {} ({}) via {}.",
            selection.model_display, selection.model, selection.provider_label
        ),
    )?;

    if !selection.known_model {
        renderer.line(
            MessageStyle::Info,
            "The selected model is not part of VTCode's curated list; capabilities may vary.",
        )?;
    }

    if selection.reasoning_supported {
        let message = if selection.reasoning_changed {
            format!("Reasoning effort updated to '{}'.", selection.reasoning)
        } else {
            format!("Reasoning effort remains '{}'.", selection.reasoning)
        };
        renderer.line(MessageStyle::Info, &message)?;
    }

    if selection.api_key.is_some() {
        renderer.line(
            MessageStyle::Info,
            &format!(
                "Stored credential under {} and updated the active environment.",
                selection.env_key
            ),
        )?;
    } else if selection.requires_api_key {
        renderer.line(
            MessageStyle::Info,
            &format!(
                "Using environment variable {} for authentication.",
                selection.env_key
            ),
        )?;
    }

    Ok(())
}

fn persist_env_value(workspace: &Path, key: &str, value: &str) -> Result<()> {
    let env_path = workspace.join(".env");
    let mut lines: Vec<String> = if env_path.exists() {
        std::fs::read_to_string(&env_path)
            .with_context(|| format!("Failed to read {}", env_path.display()))?
            .lines()
            .map(|line| line.to_string())
            .collect()
    } else {
        Vec::new()
    };

    let mut replaced = false;
    for line in lines.iter_mut() {
        if let Some((existing_key, _)) = line.split_once('=') {
            if existing_key.trim() == key {
                *line = format!("{key}={value}");
                replaced = true;
            }
        }
    }

    if !replaced {
        lines.push(format!("{key}={value}"));
    }

    let parent = env_path
        .parent()
        .map(std::path::Path::to_path_buf)
        .unwrap_or_else(|| workspace.to_path_buf());

    if !parent.exists() {
        std::fs::create_dir_all(&parent)
            .with_context(|| format!("Failed to create directory {}", parent.display()))?;
    }

    let temp = Builder::new()
        .prefix(".env.")
        .suffix(".tmp")
        .tempfile_in(&parent)
        .with_context(|| format!("Failed to create temporary file in {}", parent.display()))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let permissions = std::fs::Permissions::from_mode(0o600);
        temp.as_file()
            .set_permissions(permissions)
            .with_context(|| format!("Failed to set permissions on {}", temp.path().display()))?;
    }

    {
        let mut writer = BufWriter::new(temp.as_file());
        for line in &lines {
            writeln!(writer, "{line}")
                .with_context(|| format!("Failed to write .env entry for {key}"))?;
        }
        writer
            .flush()
            .with_context(|| format!("Failed to flush temporary .env for {}", key))?;
    }

    temp.as_file()
        .sync_all()
        .with_context(|| format!("Failed to sync temporary .env for {}", key))?;

    let _file = temp
        .persist(&env_path)
        .with_context(|| format!("Failed to persist {}", env_path.display()))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&env_path, std::fs::Permissions::from_mode(0o600))
            .with_context(|| format!("Failed to set permissions on {}", env_path.display()))?;
    }

    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HitlDecision {
    Approved,
    Denied,
    Exit,
    Interrupt,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ToolPermissionFlow {
    Approved,
    Denied,
    Exit,
    Interrupted,
}

#[derive(Default)]
struct CtrlCState {
    cancel_requested: AtomicBool,
    exit_requested: AtomicBool,
    exit_armed: AtomicBool,
}

enum CtrlCSignal {
    Cancel,
    Exit,
}

impl CtrlCState {
    fn new() -> Self {
        Self::default()
    }

    fn register_signal(&self) -> CtrlCSignal {
        if self.cancel_requested.swap(true, Ordering::SeqCst)
            || self.exit_armed.swap(false, Ordering::SeqCst)
        {
            self.exit_requested.store(true, Ordering::SeqCst);
            CtrlCSignal::Exit
        } else {
            CtrlCSignal::Cancel
        }
    }

    fn clear_cancel(&self) {
        self.cancel_requested.store(false, Ordering::SeqCst);
        self.exit_armed.store(true, Ordering::SeqCst);
    }

    fn is_cancel_requested(&self) -> bool {
        self.cancel_requested.load(Ordering::SeqCst)
    }

    fn is_exit_requested(&self) -> bool {
        self.exit_requested.load(Ordering::SeqCst)
    }

    fn disarm_exit(&self) {
        self.exit_armed.store(false, Ordering::SeqCst);
    }
}

struct PlaceholderGuard {
    handle: InlineHandle,
    restore: Option<String>,
}

impl PlaceholderGuard {
    fn new(handle: &InlineHandle, restore: Option<String>) -> Self {
        Self {
            handle: handle.clone(),
            restore,
        }
    }
}

#[derive(Default, Clone)]
struct InputStatusState {
    left: Option<String>,
    right: Option<String>,
    git_left: Option<String>,
    last_git_refresh: Option<Instant>,
}

const GIT_STATUS_REFRESH_INTERVAL: Duration = Duration::from_secs(2);

fn update_input_status_if_changed(
    handle: &InlineHandle,
    workspace: &Path,
    model: &str,
    reasoning: &str,
    state: &mut InputStatusState,
) {
    let should_refresh_git = match state.last_git_refresh {
        Some(last_refresh) => last_refresh.elapsed() >= GIT_STATUS_REFRESH_INTERVAL,
        None => true,
    };

    if should_refresh_git {
        match git_status_summary(workspace) {
            Ok(Some(summary)) => {
                let mut branch = summary.branch;
                if summary.dirty {
                    branch.push('*');
                }
                state.git_left = Some(branch);
            }
            Ok(None) => {
                state.git_left = None;
            }
            Err(error) => {
                warn!(
                    workspace = %workspace.display(),
                    error = ?error,
                    "Failed to resolve git status"
                );
            }
        }

        state.last_git_refresh = Some(Instant::now());
    }

    let left = state.git_left.clone();

    let trimmed_model = model.trim();
    let trimmed_reasoning = reasoning.trim();
    let right = if trimmed_model.is_empty() {
        None
    } else if trimmed_reasoning.is_empty() {
        Some(trimmed_model.to_string())
    } else {
        Some(format!("{} ({})", trimmed_model, trimmed_reasoning))
    };

    if state.left != left || state.right != right {
        handle.set_input_status(left.clone(), right.clone());
        state.left = left;
        state.right = right;
    }
}

impl Drop for PlaceholderGuard {
    fn drop(&mut self) {
        self.handle.set_placeholder(self.restore.clone());
    }
}

fn render_tool_call_summary(
    renderer: &mut AnsiRenderer,
    tool_name: &str,
    args: &Value,
) -> Result<()> {
    let (headline, _) = describe_tool_action(tool_name, args);
    renderer.line(MessageStyle::Info, &format!("→ {}", headline))?;

    Ok(())
}

fn describe_tool_action(tool_name: &str, args: &Value) -> (String, HashSet<String>) {
    match tool_name {
        tool_names::RUN_TERMINAL_CMD | tool_names::BASH => describe_shell_command(args)
            .unwrap_or_else(|| ("Run shell command".to_string(), HashSet::new())),
        tool_names::LIST_FILES => {
            describe_list_files(args).unwrap_or_else(|| ("List files".to_string(), HashSet::new()))
        }
        tool_names::GREP_SEARCH => describe_grep_search(args)
            .unwrap_or_else(|| ("Search with grep".to_string(), HashSet::new())),
        tool_names::READ_FILE => describe_path_action(args, "Read file", &["path"])
            .unwrap_or_else(|| ("Read file".to_string(), HashSet::new())),
        tool_names::WRITE_FILE => describe_path_action(args, "Write file", &["path"])
            .unwrap_or_else(|| ("Write file".to_string(), HashSet::new())),
        tool_names::EDIT_FILE => describe_path_action(args, "Edit file", &["path"])
            .unwrap_or_else(|| ("Edit file".to_string(), HashSet::new())),
        tool_names::CREATE_FILE => describe_path_action(args, "Create file", &["path"])
            .unwrap_or_else(|| ("Create file".to_string(), HashSet::new())),
        tool_names::DELETE_FILE => describe_path_action(args, "Delete file", &["path"])
            .unwrap_or_else(|| ("Delete file".to_string(), HashSet::new())),
        tool_names::CURL => {
            describe_curl(args).unwrap_or_else(|| ("Fetch URL".to_string(), HashSet::new()))
        }
        tool_names::SIMPLE_SEARCH => describe_simple_search(args)
            .unwrap_or_else(|| ("Search workspace".to_string(), HashSet::new())),
        tool_names::SRGN => describe_srgn(args)
            .unwrap_or_else(|| ("Search and replace".to_string(), HashSet::new())),
        tool_names::APPLY_PATCH => ("Apply workspace patch".to_string(), HashSet::new()),
        tool_names::UPDATE_PLAN => ("Update task plan".to_string(), HashSet::new()),
        _ => (
            format!("Use {}", humanize_tool_name(tool_name)),
            HashSet::new(),
        ),
    }
}

fn describe_shell_command(args: &Value) -> Option<(String, HashSet<String>)> {
    let mut used = HashSet::new();
    if let Some(parts) = args
        .get("command")
        .and_then(|value| value.as_array())
        .map(|array| {
            array
                .iter()
                .filter_map(|value| value.as_str().map(str::to_string))
                .collect::<Vec<_>>()
        })
        .filter(|parts: &Vec<String>| !parts.is_empty())
    {
        used.insert("command".to_string());
        let joined = parts.join(" ");
        let summary = truncate_middle(&joined, 60);
        return Some((format!("Run command {}", summary), used));
    }

    if let Some(cmd) = args
        .get("bash_command")
        .and_then(|value| value.as_str())
        .filter(|s| !s.is_empty())
    {
        used.insert("bash_command".to_string());
        let summary = truncate_middle(cmd, 60);
        return Some((format!("Run bash {}", summary), used));
    }

    None
}

fn describe_list_files(args: &Value) -> Option<(String, HashSet<String>)> {
    if let Some(path) = lookup_string(args, "path") {
        let mut used = HashSet::new();
        used.insert("path".to_string());
        let location = if path == "." {
            "workspace root".to_string()
        } else {
            truncate_middle(&path, 60)
        };
        return Some((format!("List files in {}", location), used));
    }
    if let Some(pattern) = lookup_string(args, "name_pattern") {
        let mut used = HashSet::new();
        used.insert("name_pattern".to_string());
        return Some((
            format!("Find files named {}", truncate_middle(&pattern, 40)),
            used,
        ));
    }
    if let Some(pattern) = lookup_string(args, "content_pattern") {
        let mut used = HashSet::new();
        used.insert("content_pattern".to_string());
        return Some((
            format!("Search files for {}", truncate_middle(&pattern, 40)),
            used,
        ));
    }
    None
}

fn describe_grep_search(args: &Value) -> Option<(String, HashSet<String>)> {
    let pattern = lookup_string(args, "pattern");
    let path = lookup_string(args, "path");
    match (pattern, path) {
        (Some(pat), Some(path)) => {
            let mut used = HashSet::new();
            used.insert("pattern".to_string());
            used.insert("path".to_string());
            Some((
                format!(
                    "Grep {} in {}",
                    truncate_middle(&pat, 40),
                    truncate_middle(&path, 40)
                ),
                used,
            ))
        }
        (Some(pat), None) => {
            let mut used = HashSet::new();
            used.insert("pattern".to_string());
            Some((format!("Grep {}", truncate_middle(&pat, 40)), used))
        }
        _ => None,
    }
}

fn describe_simple_search(args: &Value) -> Option<(String, HashSet<String>)> {
    if let Some(query) = lookup_string(args, "query") {
        let mut used = HashSet::new();
        used.insert("query".to_string());
        return Some((format!("Search for {}", truncate_middle(&query, 50)), used));
    }
    None
}

fn describe_srgn(args: &Value) -> Option<(String, HashSet<String>)> {
    let pattern = lookup_string(args, "pattern");
    let replacement = lookup_string(args, "replacement");
    match (pattern, replacement) {
        (Some(pat), Some(rep)) => {
            let mut used = HashSet::new();
            used.insert("pattern".to_string());
            used.insert("replacement".to_string());
            Some((
                format!(
                    "Replace {} → {}",
                    truncate_middle(&pat, 30),
                    truncate_middle(&rep, 30)
                ),
                used,
            ))
        }
        (Some(pat), None) => {
            let mut used = HashSet::new();
            used.insert("pattern".to_string());
            Some((format!("Search for {}", truncate_middle(&pat, 40)), used))
        }
        _ => None,
    }
}

fn describe_path_action(
    args: &Value,
    verb: &str,
    keys: &[&str],
) -> Option<(String, HashSet<String>)> {
    for key in keys {
        if let Some(value) = lookup_string(args, key) {
            let mut used = HashSet::new();
            used.insert((*key).to_string());
            let summary = truncate_middle(&value, 60);
            return Some((format!("{} {}", verb, summary), used));
        }
    }
    None
}

fn describe_curl(args: &Value) -> Option<(String, HashSet<String>)> {
    if let Some(url) = lookup_string(args, "url") {
        let mut used = HashSet::new();
        used.insert("url".to_string());
        return Some((format!("Fetch {}", truncate_middle(&url, 60)), used));
    }
    None
}

fn lookup_string(args: &Value, key: &str) -> Option<String> {
    args.as_object()
        .and_then(|map| map.get(key))
        .and_then(|value| value.as_str())
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty())
}
fn humanize_tool_name(name: &str) -> String {
    humanize_key(name)
}

fn humanize_key(key: &str) -> String {
    let replaced = key.replace('_', " ");
    if replaced.is_empty() {
        return replaced;
    }
    let mut chars = replaced.chars();
    let first = chars.next().unwrap();
    let mut result = first.to_uppercase().collect::<String>();
    result.push_str(&chars.collect::<String>());
    result
}

fn truncate_middle(text: &str, max_len: usize) -> String {
    if max_len == 0 {
        return String::new();
    }
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= max_len {
        return text.to_string();
    }
    if max_len <= 1 {
        return "…".to_string();
    }
    let head_len = max_len / 2;
    let tail_len = max_len.saturating_sub(head_len + 1);
    let mut result: String = chars.iter().take(head_len).collect();
    result.push('…');
    if tail_len > 0 {
        let tail: String = chars
            .iter()
            .rev()
            .take(tail_len)
            .cloned()
            .collect::<Vec<char>>()
            .into_iter()
            .rev()
            .collect();
        result.push_str(&tail);
    }
    result
}

async fn prompt_tool_permission(
    tool_name: &str,
    renderer: &mut AnsiRenderer,
    handle: &InlineHandle,
    events: &mut UnboundedReceiver<InlineEvent>,
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
    default_placeholder: Option<String>,
) -> Result<HitlDecision> {
    // Clear any existing content
    renderer.line_if_not_empty(MessageStyle::Info)?;

    renderer.line(
        MessageStyle::Info,
        &format!(
            "Approve '{}' tool? Respond with 'y' to approve or 'n' to deny. (Esc to cancel)",
            tool_name
        ),
    )?;

    let _placeholder_guard = PlaceholderGuard::new(handle, default_placeholder);
    handle.set_placeholder(Some("y/n (Esc to cancel)".to_string()));

    // Yield once so the UI processes the prompt lines and placeholder update
    // before we start listening for user input. Without this the question would
    // only appear after a subsequent event (like cancel) fired.
    task::yield_now().await;

    loop {
        if ctrl_c_state.is_cancel_requested() {
            return Ok(HitlDecision::Interrupt);
        }

        let notify = ctrl_c_notify.clone();
        let maybe_event = tokio::select! {
            _ = notify.notified(), if !ctrl_c_state.is_cancel_requested() => None,
            event = events.recv() => event,
        };

        let Some(event) = maybe_event else {
            // Clear input before exiting
            handle.clear_input();
            if ctrl_c_state.is_cancel_requested() {
                return Ok(HitlDecision::Interrupt);
            }
            return Ok(HitlDecision::Exit);
        };

        ctrl_c_state.disarm_exit();

        match event {
            InlineEvent::Submit(input) => {
                let normalized = input.trim().to_lowercase();
                if normalized.is_empty() {
                    renderer.line(MessageStyle::Info, "Please respond with 'yes' or 'no'.")?;
                    continue;
                }

                if matches!(normalized.as_str(), "y" | "yes" | "approve" | "allow") {
                    // Clear the input before returning
                    handle.clear_input();
                    return Ok(HitlDecision::Approved);
                }

                if matches!(normalized.as_str(), "n" | "no" | "deny" | "cancel" | "stop") {
                    // Clear the input before returning
                    handle.clear_input();
                    return Ok(HitlDecision::Denied);
                }

                renderer.line(
                    MessageStyle::Info,
                    "Respond with 'yes' to approve or 'no' to deny.",
                )?;
            }
            InlineEvent::ListModalSubmit(_) | InlineEvent::ListModalCancel => {
                continue;
            }
            InlineEvent::Cancel => {
                handle.clear_input();
                return Ok(HitlDecision::Denied);
            }
            InlineEvent::Exit => {
                handle.clear_input();
                return Ok(HitlDecision::Exit);
            }
            InlineEvent::Interrupt => {
                handle.clear_input();
                return Ok(HitlDecision::Interrupt);
            }
            InlineEvent::ScrollLineUp
            | InlineEvent::ScrollLineDown
            | InlineEvent::ScrollPageUp
            | InlineEvent::ScrollPageDown => {
                // Scrolling is handled by the TUI event loop, just continue
            }
        }
    }
}

async fn ensure_tool_permission(
    tool_registry: &mut vtcode_core::tools::registry::ToolRegistry,
    tool_name: &str,
    renderer: &mut AnsiRenderer,
    handle: &InlineHandle,
    events: &mut UnboundedReceiver<InlineEvent>,
    default_placeholder: Option<String>,
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
) -> Result<ToolPermissionFlow> {
    match tool_registry.evaluate_tool_policy(tool_name)? {
        ToolPermissionDecision::Allow => Ok(ToolPermissionFlow::Approved),
        ToolPermissionDecision::Deny => Ok(ToolPermissionFlow::Denied),
        ToolPermissionDecision::Prompt => {
            if tool_name == tool_names::RUN_TERMINAL_CMD {
                tool_registry.mark_tool_preapproved(tool_name);
                if let Ok(manager) = tool_registry.policy_manager_mut() {
                    if let Err(err) = manager.set_policy(tool_name, ToolPolicy::Allow) {
                        warn!(
                            "Failed to persist auto-approval for '{}': {}",
                            tool_name, err
                        );
                    }
                }
                return Ok(ToolPermissionFlow::Approved);
            }
            let decision = prompt_tool_permission(
                tool_name,
                renderer,
                handle,
                events,
                ctrl_c_state,
                ctrl_c_notify,
                default_placeholder,
            )
            .await?;
            match decision {
                HitlDecision::Approved => {
                    tool_registry.mark_tool_preapproved(tool_name);
                    if let Err(err) =
                        tool_registry.persist_mcp_tool_policy(tool_name, ToolPolicy::Allow)
                    {
                        warn!(
                            "Failed to persist MCP approval for tool '{}': {}",
                            tool_name, err
                        );
                    }
                    Ok(ToolPermissionFlow::Approved)
                }
                HitlDecision::Denied => {
                    if let Err(err) =
                        tool_registry.persist_mcp_tool_policy(tool_name, ToolPolicy::Deny)
                    {
                        warn!(
                            "Failed to persist MCP denial for tool '{}': {}",
                            tool_name, err
                        );
                    }
                    Ok(ToolPermissionFlow::Denied)
                }
                HitlDecision::Exit => Ok(ToolPermissionFlow::Exit),
                HitlDecision::Interrupt => Ok(ToolPermissionFlow::Interrupted),
            }
        }
    }
}

fn apply_prompt_style(handle: &InlineHandle) {
    let styles = theme::active_styles();
    let style = convert_ui_style(styles.primary);
    handle.set_prompt("❯ ".to_string(), style);
}

const SPINNER_UPDATE_INTERVAL_MS: u64 = 120;
const REASONING_HEADING: &str = "Thinking";
const REASONING_PREFIX: &str = "Thinking: ";

struct SpinnerFrameGenerator {
    style: ProgressStyle,
    tick: u64,
}

impl SpinnerFrameGenerator {
    fn new() -> Self {
        Self {
            style: ProgressStyle::default_spinner(),
            tick: 0,
        }
    }

    fn next_frame(&mut self) -> &str {
        let frame = self.style.get_tick_str(self.tick);
        self.tick = self.tick.wrapping_add(1);
        frame
    }
}

struct PlaceholderSpinner {
    handle: InlineHandle,
    restore_hint: Option<String>,
    active: Arc<AtomicBool>,
    task: task::JoinHandle<()>,
}

fn spinner_placeholder_style() -> InlineTextStyle {
    let styles = theme::active_styles();
    let mut style = convert_ui_style(styles.secondary);
    if style.color.is_none() {
        let fallback = convert_ui_style(styles.primary);
        style.color = fallback.color;
    }
    style.bold = true;
    style
}

impl PlaceholderSpinner {
    fn new(
        handle: &InlineHandle,
        restore_hint: Option<String>,
        message: impl Into<String>,
    ) -> Self {
        let message = message.into();
        let active = Arc::new(AtomicBool::new(true));
        let spinner_active = active.clone();
        let spinner_handle = handle.clone();
        let restore_on_stop = restore_hint.clone();
        let spinner_style = spinner_placeholder_style();
        let spinner_message = message;

        spinner_handle.set_input_enabled(false);
        spinner_handle.set_cursor_visible(false);
        let task = task::spawn(async move {
            let mut frames = SpinnerFrameGenerator::new();
            while spinner_active.load(Ordering::SeqCst) {
                let frame = frames.next_frame();
                let display = if spinner_message.is_empty() {
                    frame.to_string()
                } else {
                    format!("{frame} {spinner_message}")
                };
                spinner_handle
                    .set_placeholder_with_style(Some(display), Some(spinner_style.clone()));
                sleep(Duration::from_millis(SPINNER_UPDATE_INTERVAL_MS)).await;
            }

            spinner_handle.set_cursor_visible(true);
            spinner_handle.set_input_enabled(true);
            spinner_handle.set_placeholder_with_style(restore_on_stop, None);
        });

        Self {
            handle: handle.clone(),
            restore_hint,
            active,
            task,
        }
    }

    fn finish(&self) {
        if self.active.swap(false, Ordering::SeqCst) {
            self.handle
                .set_placeholder_with_style(self.restore_hint.clone(), None);
            self.handle.set_input_enabled(true);
            self.handle.set_cursor_visible(true);
        }
    }
}

impl Drop for PlaceholderSpinner {
    fn drop(&mut self) {
        self.finish();
        self.task.abort();
    }
}

fn map_render_error(provider_name: &str, err: anyhow::Error) -> uni::LLMError {
    let formatted_error = error_display::format_llm_error(
        provider_name,
        &format!("Failed to render streaming output: {}", err),
    );
    uni::LLMError::Provider(formatted_error)
}

fn stream_plain_response_delta(
    renderer: &mut AnsiRenderer,
    style: MessageStyle,
    indent: &str,
    pending_indent: &mut bool,
    delta: &str,
) -> Result<()> {
    for chunk in delta.split_inclusive('\n') {
        if chunk.is_empty() {
            continue;
        }

        if chunk.ends_with('\n') {
            let text = &chunk[..chunk.len() - 1];
            if !text.is_empty() {
                if *pending_indent && !indent.is_empty() {
                    renderer.inline_with_style(style, indent)?;
                }
                renderer.inline_with_style(style, text)?;
                *pending_indent = false;
            }
            renderer.inline_with_style(style, "\n")?;
            *pending_indent = true;
        } else {
            if *pending_indent && !indent.is_empty() {
                renderer.inline_with_style(style, indent)?;
                *pending_indent = false;
            }
            renderer.inline_with_style(style, chunk)?;
        }
    }

    Ok(())
}

#[derive(Default)]
struct StreamingReasoningState {
    aggregated: String,
    inline_line_count: usize,
    last_rendered: Vec<String>,
    cli_prefix_printed: bool,
    cli_pending_indent: bool,
    inline_enabled: bool,
}

impl StreamingReasoningState {
    fn new(inline_enabled: bool) -> Self {
        Self {
            inline_enabled,
            ..Self::default()
        }
    }

    fn handle_delta(&mut self, renderer: &mut AnsiRenderer, delta: &str) -> Result<()> {
        if delta.trim().is_empty() {
            return Ok(());
        }

        self.append_delta(delta);

        if self.inline_enabled {
            self.render_inline(renderer)
        } else {
            self.render_cli(renderer, delta)
        }
    }

    fn finalize(
        &mut self,
        renderer: &mut AnsiRenderer,
        final_reasoning: Option<&str>,
    ) -> Result<()> {
        if self.inline_enabled {
            if let Some(reasoning) = final_reasoning.map(str::trim) {
                if !reasoning.is_empty() && reasoning != self.aggregated.trim() {
                    self.aggregated = reasoning.to_string();
                    self.render_inline(renderer)?;
                }
            }
            Ok(())
        } else {
            self.finalize_cli(renderer)?;
            if let Some(reasoning) = final_reasoning.map(str::trim) {
                if reasoning.is_empty() {
                    return Ok(());
                }

                if !self.cli_prefix_printed {
                    renderer.line(
                        MessageStyle::Reasoning,
                        &format!("{REASONING_PREFIX}{reasoning}"),
                    )?;
                    self.cli_prefix_printed = true;
                } else if self.aggregated.trim() != reasoning {
                    renderer.line(MessageStyle::Reasoning, reasoning)?;
                }

                self.aggregated = reasoning.to_string();
            }
            Ok(())
        }
    }

    fn handle_stream_failure(&mut self, renderer: &mut AnsiRenderer) -> Result<()> {
        if !self.inline_enabled {
            self.finalize_cli(renderer)?;
        }
        Ok(())
    }

    fn append_delta(&mut self, delta: &str) {
        let delta = if self.aggregated.is_empty() {
            delta.trim_start_matches(['\n', '\r'])
        } else {
            delta
        };

        if delta.is_empty() {
            return;
        }

        self.aggregated.push_str(delta);
    }

    fn render_inline(&mut self, renderer: &mut AnsiRenderer) -> Result<()> {
        let lines = self.display_lines();
        if lines.is_empty() || lines == self.last_rendered {
            return Ok(());
        }

        renderer.render_reasoning_stream(&lines, &mut self.inline_line_count)?;
        self.last_rendered = lines;
        Ok(())
    }

    fn render_cli(&mut self, renderer: &mut AnsiRenderer, delta: &str) -> Result<()> {
        if !self.cli_prefix_printed {
            let indent = MessageStyle::Reasoning.indent();
            if !indent.is_empty() {
                renderer.inline_with_style(MessageStyle::Reasoning, indent)?;
            }
            renderer.inline_with_style(MessageStyle::Reasoning, REASONING_PREFIX)?;
            self.cli_prefix_printed = true;
            self.cli_pending_indent = false;
        }

        stream_plain_response_delta(
            renderer,
            MessageStyle::Reasoning,
            MessageStyle::Reasoning.indent(),
            &mut self.cli_pending_indent,
            delta,
        )
    }

    fn finalize_cli(&mut self, renderer: &mut AnsiRenderer) -> Result<()> {
        if self.cli_prefix_printed && !self.cli_pending_indent {
            renderer.inline_with_style(MessageStyle::Reasoning, "\n")?;
            self.cli_pending_indent = true;
        }
        Ok(())
    }

    fn display_lines(&self) -> Vec<String> {
        let trimmed = self.aggregated.trim_matches(['\r', '\n']);
        if trimmed.is_empty() {
            return Vec::new();
        }

        if trimmed.contains('\n') {
            let mut lines = Vec::new();
            lines.push(format!("{REASONING_HEADING}:"));
            for line in trimmed.lines() {
                lines.push(line.trim_end().to_string());
            }
            lines
        } else {
            vec![format!("{REASONING_PREFIX}{}", trimmed.trim())]
        }
    }
}

async fn stream_and_render_response(
    provider: &dyn uni::LLMProvider,
    request: uni::LLMRequest,
    spinner: &PlaceholderSpinner,
    renderer: &mut AnsiRenderer,
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
) -> Result<(uni::LLMResponse, bool), uni::LLMError> {
    let mut stream = provider.stream(request).await?;
    let provider_name = provider.name();
    let mut final_response: Option<uni::LLMResponse> = None;
    let mut aggregated = String::new();
    let mut spinner_active = true;
    let supports_streaming_markdown = renderer.supports_streaming_markdown();
    let mut rendered_line_count = 0usize;
    let response_style = MessageStyle::Response;
    let response_indent = response_style.indent();
    let mut needs_indent = true;
    let finish_spinner = |active: &mut bool| {
        if *active {
            spinner.finish();
            *active = false;
        }
    };
    let mut emitted_tokens = false;
    let mut reasoning_state = StreamingReasoningState::new(supports_streaming_markdown);

    loop {
        if ctrl_c_state.is_cancel_requested() {
            finish_spinner(&mut spinner_active);
            reasoning_state
                .handle_stream_failure(renderer)
                .map_err(|err| map_render_error(provider_name, err))?;
            return Err(uni::LLMError::Provider(error_display::format_llm_error(
                provider_name,
                "Interrupted by user",
            )));
        }

        let maybe_event = tokio::select! {
            biased;

            _ = ctrl_c_notify.notified(), if ctrl_c_state.is_cancel_requested() => {
                finish_spinner(&mut spinner_active);
                reasoning_state
                    .handle_stream_failure(renderer)
                    .map_err(|err| map_render_error(provider_name, err))?;
                return Err(uni::LLMError::Provider(error_display::format_llm_error(
                    provider_name,
                    "Interrupted by user",
                )));
            }
            event = stream.next() => event,
        };

        let Some(event_result) = maybe_event else {
            break;
        };

        match event_result {
            Ok(LLMStreamEvent::Token { delta }) => {
                finish_spinner(&mut spinner_active);
                aggregated.push_str(&delta);
                if supports_streaming_markdown {
                    rendered_line_count = renderer
                        .stream_markdown_response(&aggregated, rendered_line_count)
                        .map_err(|err| map_render_error(provider_name, err))?;
                } else {
                    stream_plain_response_delta(
                        renderer,
                        response_style,
                        response_indent,
                        &mut needs_indent,
                        &delta,
                    )
                    .map_err(|err| map_render_error(provider_name, err))?;
                }
                emitted_tokens = true;
            }
            Ok(LLMStreamEvent::Reasoning { delta }) => {
                finish_spinner(&mut spinner_active);
                reasoning_state
                    .handle_delta(renderer, &delta)
                    .map_err(|err| map_render_error(provider_name, err))?;
            }
            Ok(LLMStreamEvent::Completed { response }) => {
                final_response = Some(response);
            }
            Err(err) => {
                finish_spinner(&mut spinner_active);
                reasoning_state
                    .handle_stream_failure(renderer)
                    .map_err(|render_err| map_render_error(provider_name, render_err))?;
                return Err(err);
            }
        }
    }

    finish_spinner(&mut spinner_active);

    let response = match final_response {
        Some(response) => response,
        None => {
            reasoning_state
                .handle_stream_failure(renderer)
                .map_err(|err| map_render_error(provider_name, err))?;
            let formatted_error = error_display::format_llm_error(
                provider_name,
                "Stream ended without a completion event",
            );
            return Err(uni::LLMError::Provider(formatted_error));
        }
    };

    reasoning_state
        .finalize(renderer, response.reasoning.as_deref())
        .map_err(|err| map_render_error(provider_name, err))?;

    if aggregated.is_empty() {
        if let Some(content) = response.content.clone() {
            if !content.is_empty() {
                aggregated.push_str(&content);
            }
        }
    }

    if !aggregated.is_empty() {
        if !emitted_tokens {
            if supports_streaming_markdown {
                let _ = renderer
                    .stream_markdown_response(&aggregated, rendered_line_count)
                    .map_err(|err| map_render_error(provider_name, err))?;
            } else {
                renderer
                    .line(MessageStyle::Response, &aggregated)
                    .map_err(|err| map_render_error(provider_name, err))?;
            }
            emitted_tokens = true;
        } else if !supports_streaming_markdown && !aggregated.ends_with('\n') {
            renderer
                .line_if_not_empty(MessageStyle::Response)
                .map_err(|err| map_render_error(provider_name, err))?;
        }
    }

    Ok((response, emitted_tokens))
}

enum TurnLoopResult {
    Completed,
    Aborted,
    Cancelled,
}

const CONFIG_MODAL_TITLE: &str = "VTCode Configuration";
const MODAL_CLOSE_HINT: &str = "Press Esc to close the configuration modal.";
const SENSITIVE_KEYWORDS: [&str; 5] = ["key", "token", "secret", "password", "credential"];

struct ConfigModalContent {
    title: String,
    source_label: String,
    config_lines: Vec<String>,
}

async fn bootstrap_config_files(workspace: PathBuf, force: bool) -> Result<Vec<String>> {
    let label = workspace.display().to_string();
    let result = task::spawn_blocking(move || VTCodeConfig::bootstrap_project(&workspace, force))
        .await
        .map_err(|err| anyhow!("failed to join configuration bootstrap task: {}", err))?;
    result.with_context(|| format!("failed to initialize configuration in {}", label))
}

async fn build_workspace_index(workspace: PathBuf) -> Result<()> {
    let label = workspace.display().to_string();
    let result = task::spawn_blocking(move || -> Result<()> {
        let mut indexer = SimpleIndexer::new(workspace.clone());
        indexer.init()?;
        indexer.index_directory(&workspace)?;
        Ok(())
    })
    .await
    .map_err(|err| anyhow!("failed to join workspace indexing task: {}", err))?;
    result.with_context(|| format!("failed to build workspace index in {}", label))
}

async fn load_config_modal_content(
    workspace: PathBuf,
    vt_cfg: Option<VTCodeConfig>,
) -> Result<ConfigModalContent> {
    task::spawn_blocking(move || prepare_config_modal_content(&workspace, vt_cfg))
        .await
        .map_err(|err| anyhow!("failed to join configuration load task: {}", err))?
}

fn prepare_config_modal_content(
    workspace: &Path,
    vt_cfg: Option<VTCodeConfig>,
) -> Result<ConfigModalContent> {
    let manager = ConfigManager::load_from_workspace(workspace).with_context(|| {
        format!(
            "failed to resolve configuration for workspace {}",
            workspace.display()
        )
    })?;

    let config_path = manager.config_path().map(Path::to_path_buf);
    let config_data = if config_path.is_some() {
        manager.config().clone()
    } else if let Some(snapshot) = vt_cfg {
        snapshot
    } else {
        manager.config().clone()
    };

    let mut value = TomlValue::try_from(config_data)
        .context("failed to serialize configuration for display")?;
    mask_sensitive_config(&mut value);

    let formatted =
        toml::to_string_pretty(&value).context("failed to render configuration to TOML")?;
    let config_lines = formatted.lines().map(|line| line.to_string()).collect();

    let source_label = if let Some(path) = config_path {
        format!("Configuration source: {}", path.display())
    } else {
        "No vtcode.toml file found; showing runtime defaults.".to_string()
    };

    Ok(ConfigModalContent {
        title: CONFIG_MODAL_TITLE.to_string(),
        source_label,
        config_lines,
    })
}

fn mask_sensitive_config(value: &mut TomlValue) {
    match value {
        TomlValue::Table(table) => {
            for (key, entry) in table.iter_mut() {
                if is_sensitive_key(key) {
                    *entry = TomlValue::String("********".to_string());
                } else {
                    mask_sensitive_config(entry);
                }
            }
        }
        TomlValue::Array(items) => {
            for item in items {
                mask_sensitive_config(item);
            }
        }
        _ => {}
    }
}

fn is_sensitive_key(key: &str) -> bool {
    let lowered = key.to_ascii_lowercase();
    SENSITIVE_KEYWORDS
        .iter()
        .any(|keyword| lowered.contains(keyword))
}

pub(crate) async fn run_single_agent_loop_unified(
    config: &CoreAgentConfig,
    mut vt_cfg: Option<VTCodeConfig>,
    skip_confirmations: bool,
    full_auto: bool,
) -> Result<()> {
    // Set up panic handler to ensure MCP cleanup on panic
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        eprintln!("Application panic occurred: {:?}", panic_info);
        // Note: We can't easily access the MCP client here due to move semantics
        // The cleanup will happen in the Drop implementations
        original_hook(panic_info);
    }));
    let mut config = config.clone();
    let SessionState {
        session_bootstrap,
        mut provider_client,
        mut tool_registry,
        tools,
        trim_config,
        mut conversation_history,
        decision_ledger,
        trajectory: traj,
        base_system_prompt,
        full_auto_allowlist,
        #[allow(unused_variables)]
        mcp_client,
        mut mcp_panel_state,
        token_budget,
        token_budget_enabled,
        mut curator,
    } = initialize_session(&config, vt_cfg.as_ref(), full_auto).await?;

    let curator_tool_catalog = build_curator_tools(&tools);

    let active_styles = theme::active_styles();
    let theme_spec = theme_from_styles(&active_styles);
    let mut default_placeholder = session_bootstrap
        .placeholder
        .clone()
        .or_else(|| Some(ui::CHAT_INPUT_PLACEHOLDER_BOOTSTRAP.to_string()));
    let mut follow_up_placeholder = if session_bootstrap.placeholder.is_none() {
        Some(ui::CHAT_INPUT_PLACEHOLDER_FOLLOW_UP.to_string())
    } else {
        None
    };
    let inline_rows = vt_cfg
        .as_ref()
        .map(|cfg| cfg.ui.inline_viewport_rows)
        .unwrap_or(ui::DEFAULT_INLINE_VIEWPORT_ROWS);
    let show_timeline_pane = vt_cfg
        .as_ref()
        .map(|cfg| cfg.ui.show_timeline_pane)
        .unwrap_or(ui::INLINE_SHOW_TIMELINE_PANE);
    let session = spawn_session(
        theme_spec.clone(),
        default_placeholder.clone(),
        config.ui_surface,
        inline_rows,
        show_timeline_pane,
    )
    .context("failed to launch inline session")?;
    let handle = session.handle.clone();
    let highlight_config = vt_cfg
        .as_ref()
        .map(|cfg| cfg.syntax_highlighting.clone())
        .unwrap_or_default();
    let mut renderer = AnsiRenderer::with_inline_ui(handle.clone(), highlight_config);

    transcript::clear();

    let workspace_label = config
        .workspace
        .file_name()
        .and_then(|component| component.to_str())
        .map(|value| value.to_string())
        .unwrap_or_else(|| "workspace".to_string());
    let workspace_path = config.workspace.to_string_lossy().into_owned();
    let provider_label = if config.provider.trim().is_empty() {
        format_provider_label(provider_client.name())
    } else {
        format_provider_label(&config.provider)
    };
    let header_provider_label = provider_label.clone();
    let archive_metadata = SessionArchiveMetadata::new(
        workspace_label,
        workspace_path,
        config.model.clone(),
        provider_label,
        config.theme.clone(),
        config.reasoning_effort.as_str().to_string(),
    );
    let mut session_archive_error: Option<String> = None;
    let mut session_archive = match SessionArchive::new(archive_metadata) {
        Ok(archive) => Some(archive),
        Err(err) => {
            session_archive_error = Some(err.to_string());
            None
        }
    };

    handle.set_theme(theme_spec);
    apply_prompt_style(&handle);
    handle.set_placeholder(default_placeholder.clone());

    let reasoning_label = vt_cfg
        .as_ref()
        .map(|cfg| cfg.agent.reasoning_effort.as_str().to_string())
        .unwrap_or_else(|| config.reasoning_effort.as_str().to_string());

    render_session_banner(
        &mut renderer,
        &config,
        &session_bootstrap,
        &config.model,
        &reasoning_label,
    )?;
    let mode_label = resolve_mode_label(config.ui_surface, full_auto);
    let header_context = build_inline_header_context(
        &config,
        &session_bootstrap,
        header_provider_label,
        config.model.clone(),
        mode_label,
        reasoning_label.clone(),
    )?;
    handle.set_header_context(header_context);
    // MCP events are now rendered as message blocks in the conversation history

    if let Some(message) = session_archive_error.take() {
        renderer.line(
            MessageStyle::Info,
            &format!("Session archiving disabled: {}", message),
        )?;
        renderer.line_if_not_empty(MessageStyle::Output)?;
    }

    if full_auto {
        if let Some(allowlist) = full_auto_allowlist.as_ref() {
            if allowlist.is_empty() {
                renderer.line(
                    MessageStyle::Info,
                    "Full-auto mode enabled with no tool permissions; tool calls will be skipped.",
                )?;
            } else {
                renderer.line(
                    MessageStyle::Info,
                    &format!(
                        "Full-auto mode enabled. Permitted tools: {}",
                        allowlist.join(", ")
                    ),
                )?;
            }
        }
    }

    let ctrl_c_state = Arc::new(CtrlCState::new());
    let ctrl_c_notify = Arc::new(Notify::new());
    let mcp_client_for_signal = mcp_client.clone();
    {
        let state = ctrl_c_state.clone();
        let notify = ctrl_c_notify.clone();
        tokio::spawn(async move {
            loop {
                if tokio::signal::ctrl_c().await.is_err() {
                    break;
                }

                let signal = state.register_signal();
                notify.notify_waiters();

                // Shutdown MCP client on interrupt
                if let Some(mcp_client) = &mcp_client_for_signal {
                    if let Err(e) = mcp_client.shutdown().await {
                        let error_msg = e.to_string();
                        if error_msg.contains("EPIPE")
                            || error_msg.contains("Broken pipe")
                            || error_msg.contains("write EPIPE")
                        {
                            eprintln!(
                                "Info: MCP client shutdown encountered pipe errors during interrupt (normal): {}",
                                e
                            );
                        } else {
                            eprintln!("Warning: Failed to shutdown MCP client on interrupt: {}", e);
                        }
                    }
                }

                if matches!(signal, CtrlCSignal::Exit) {
                    break;
                }
            }
        });
    }

    let mut session_stats = SessionStats::default();
    let mut model_picker_state: Option<ModelPickerState> = None;
    let mut palette_state: Option<ActivePalette> = None;
    let mut events = session.events;
    let mut last_forced_redraw = Instant::now();
    let mut input_status_state = InputStatusState::default();
    loop {
        update_input_status_if_changed(
            &handle,
            &config.workspace,
            &config.model,
            config.reasoning_effort.as_str(),
            &mut input_status_state,
        );
        if ctrl_c_state.is_exit_requested() {
            break;
        }

        let maybe_event = tokio::select! {
            biased;

            _ = ctrl_c_notify.notified(), if ctrl_c_state.is_cancel_requested() => None,
            event = events.recv() => event,
        };

        if ctrl_c_state.is_cancel_requested() {
            if ctrl_c_state.is_exit_requested() {
                break;
            }

            renderer.line_if_not_empty(MessageStyle::Output)?;
            renderer.line(
                MessageStyle::Info,
                "Interrupted current task. Press Ctrl+C again to exit.",
            )?;
            handle.clear_input();
            handle.set_placeholder(default_placeholder.clone());
            ctrl_c_state.clear_cancel();
            continue;
        }

        let Some(event) = maybe_event else {
            break;
        };

        ctrl_c_state.disarm_exit();

        let submitted = match event {
            InlineEvent::Submit(text) => text,
            InlineEvent::ListModalSubmit(selection) => {
                if let Some(picker) = model_picker_state.as_mut() {
                    let progress =
                        picker.handle_list_selection(&mut renderer, selection.clone())?;
                    match progress {
                        ModelPickerProgress::InProgress => {}
                        ModelPickerProgress::Cancelled => {
                            model_picker_state = None;
                            renderer.line(MessageStyle::Info, "Model picker cancelled.")?;
                        }
                        ModelPickerProgress::Completed(selection) => {
                            let picker_state = model_picker_state.take().unwrap();
                            if let Err(err) = finalize_model_selection(
                                &mut renderer,
                                &picker_state,
                                selection,
                                &mut config,
                                &mut vt_cfg,
                                &mut provider_client,
                                &session_bootstrap,
                                &handle,
                                full_auto,
                            ) {
                                renderer.line(
                                    MessageStyle::Error,
                                    &format!("Failed to apply model selection: {}", err),
                                )?;
                            }
                        }
                    }
                }
                if let Some(active) = palette_state.take() {
                    let restore =
                        handle_palette_selection(active, selection, &mut renderer, &handle)?;
                    if let Some(state) = restore {
                        palette_state = Some(state);
                    }
                }
                continue;
            }
            InlineEvent::ListModalCancel => {
                if let Some(_) = model_picker_state.take() {
                    renderer.line(MessageStyle::Info, "Model picker cancelled.")?;
                } else if let Some(active) = palette_state.take() {
                    handle_palette_cancel(active, &mut renderer)?;
                }
                continue;
            }
            InlineEvent::Cancel => {
                renderer.line(
                    MessageStyle::Info,
                    "Cancellation request noted. No active run to stop.",
                )?;
                continue;
            }
            InlineEvent::Exit => {
                renderer.line(MessageStyle::Info, "Goodbye!")?;
                break;
            }
            InlineEvent::Interrupt => {
                break;
            }
            InlineEvent::ScrollLineUp
            | InlineEvent::ScrollLineDown
            | InlineEvent::ScrollPageUp
            | InlineEvent::ScrollPageDown => continue,
        };

        let input_owned = submitted.trim().to_string();

        if input_owned.is_empty() {
            continue;
        }

        if let Some(next_placeholder) = follow_up_placeholder.take() {
            handle.set_placeholder(Some(next_placeholder.clone()));
            default_placeholder = Some(next_placeholder);
        }

        match input_owned.as_str() {
            "" => continue,
            "exit" | "quit" => {
                renderer.line(MessageStyle::Info, "Goodbye!")?;
                break;
            }
            "help" => {
                renderer.line(MessageStyle::Info, "Commands: exit, help")?;
                continue;
            }
            input if input.starts_with('/') => {
                // Handle slash commands
                if let Some(command_input) = input.strip_prefix('/') {
                    match handle_slash_command(command_input, &mut renderer)? {
                        SlashCommandOutcome::Handled => {
                            continue;
                        }
                        SlashCommandOutcome::ThemeChanged(theme_id) => {
                            persist_theme_preference(&mut renderer, &theme_id)?;
                            let styles = theme::active_styles();
                            handle.set_theme(theme_from_styles(&styles));
                            apply_prompt_style(&handle);
                            continue;
                        }
                        SlashCommandOutcome::StartThemePalette { mode } => {
                            if model_picker_state.is_some() {
                                renderer.line(
                                    MessageStyle::Error,
                                    "Close the active model picker before selecting a theme.",
                                )?;
                                continue;
                            }
                            if palette_state.is_some() {
                                renderer.line(
                                    MessageStyle::Error,
                                    "Another selection modal is already open. Press Esc to dismiss it before starting a new one.",
                                )?;
                                continue;
                            }
                            if show_theme_palette(&mut renderer, mode)? {
                                palette_state = Some(ActivePalette::Theme { mode });
                            }
                            continue;
                        }
                        SlashCommandOutcome::StartSessionsPalette { limit } => {
                            if model_picker_state.is_some() {
                                renderer.line(
                                    MessageStyle::Error,
                                    "Close the active model picker before browsing sessions.",
                                )?;
                                continue;
                            }
                            if palette_state.is_some() {
                                renderer.line(
                                    MessageStyle::Error,
                                    "Another selection modal is already open. Press Esc to close it before continuing.",
                                )?;
                                continue;
                            }

                            match session_archive::list_recent_sessions(limit) {
                                Ok(listings) => {
                                    if show_sessions_palette(&mut renderer, &listings, limit)? {
                                        palette_state =
                                            Some(ActivePalette::Sessions { listings, limit });
                                    }
                                }
                                Err(err) => {
                                    renderer.line(
                                        MessageStyle::Error,
                                        &format!("Failed to load session archives: {}", err),
                                    )?;
                                }
                            }
                            continue;
                        }
                        SlashCommandOutcome::StartHelpPalette => {
                            if model_picker_state.is_some() {
                                renderer.line(
                                    MessageStyle::Error,
                                    "Close the active model picker before opening help.",
                                )?;
                                continue;
                            }
                            if palette_state.is_some() {
                                renderer.line(
                                    MessageStyle::Error,
                                    "Another selection modal is already open. Press Esc to dismiss it before starting a new one.",
                                )?;
                                continue;
                            }
                            let commands: Vec<&'static SlashCommandInfo> =
                                SLASH_COMMANDS.iter().collect();
                            if show_help_palette(&mut renderer, &commands)? {
                                palette_state = Some(ActivePalette::Help);
                            }
                            continue;
                        }
                        SlashCommandOutcome::StartModelSelection => {
                            if model_picker_state.is_some() {
                                renderer.line(
                                    MessageStyle::Error,
                                    "A model picker session is already active. Complete or type 'cancel' to exit it before starting another.",
                                )?;
                                continue;
                            }
                            let reasoning = vt_cfg
                                .as_ref()
                                .map(|cfg| cfg.agent.reasoning_effort)
                                .unwrap_or(config.reasoning_effort);
                            let workspace_hint = Some(config.workspace.clone());
                            match ModelPickerState::new(&mut renderer, reasoning, workspace_hint) {
                                Ok(picker) => {
                                    model_picker_state = Some(picker);
                                }
                                Err(err) => {
                                    renderer.line(
                                        MessageStyle::Error,
                                        &format!("Failed to start model picker: {}", err),
                                    )?;
                                }
                            }
                            continue;
                        }
                        SlashCommandOutcome::InitializeWorkspace { force } => {
                            let workspace_path = config.workspace.clone();
                            let workspace_label = workspace_path.display().to_string();
                            renderer.line(
                                MessageStyle::Info,
                                &format!(
                                    "Initializing vtcode configuration in {}...",
                                    workspace_label
                                ),
                            )?;

                            let created_files =
                                match bootstrap_config_files(workspace_path.clone(), force).await {
                                    Ok(files) => files,
                                    Err(err) => {
                                        renderer.line(
                                            MessageStyle::Error,
                                            &format!("Failed to initialize configuration: {}", err),
                                        )?;
                                        continue;
                                    }
                                };

                            if created_files.is_empty() {
                                renderer.line(
                                    MessageStyle::Info,
                                    "Existing configuration detected; no files were changed.",
                                )?;
                            } else {
                                renderer.line(
                                    MessageStyle::Info,
                                    &format!(
                                        "Created {}: {}",
                                        if created_files.len() == 1 {
                                            "file"
                                        } else {
                                            "files"
                                        },
                                        created_files.join(", "),
                                    ),
                                )?;
                            }

                            renderer.line(
                                MessageStyle::Info,
                                "Indexing workspace context (this may take a moment)...",
                            )?;

                            match build_workspace_index(workspace_path.clone()).await {
                                Ok(()) => {
                                    renderer.line(
                                        MessageStyle::Info,
                                        "Workspace indexing complete. Stored under .vtcode/index.",
                                    )?;
                                }
                                Err(err) => {
                                    renderer.line(
                                        MessageStyle::Error,
                                        &format!("Failed to index workspace: {}", err),
                                    )?;
                                }
                            }

                            continue;
                        }
                        SlashCommandOutcome::ShowConfig => {
                            let workspace_path = config.workspace.clone();
                            let vt_snapshot = vt_cfg.clone();
                            match load_config_modal_content(workspace_path, vt_snapshot).await {
                                Ok(content) => {
                                    if renderer.prefers_untruncated_output() {
                                        let mut modal_lines = Vec::new();
                                        modal_lines.push(content.source_label.clone());
                                        modal_lines.push(String::new());
                                        modal_lines.extend(content.config_lines.clone());
                                        modal_lines.push(String::new());
                                        modal_lines.push(MODAL_CLOSE_HINT.to_string());
                                        handle.close_modal();
                                        handle.show_modal(content.title.clone(), modal_lines, None);
                                        renderer.line(
                                            MessageStyle::Info,
                                            &format!(
                                                "Opened {} modal ({}).",
                                                content.title, content.source_label
                                            ),
                                        )?;
                                        renderer.line(MessageStyle::Info, MODAL_CLOSE_HINT)?;
                                    } else {
                                        renderer.line(MessageStyle::Info, &content.source_label)?;
                                        for line in content.config_lines {
                                            renderer.line(MessageStyle::Info, &line)?;
                                        }
                                    }
                                }
                                Err(err) => {
                                    renderer.line(
                                        MessageStyle::Error,
                                        &format!(
                                            "Failed to load configuration for display: {}",
                                            err
                                        ),
                                    )?;
                                }
                            }
                            continue;
                        }
                        #[allow(unused_variables)]
                        SlashCommandOutcome::ExecuteTool { name, args: _ } => {
                            // Handle tool execution from slash command
                            match ensure_tool_permission(
                                &mut tool_registry,
                                &name,
                                &mut renderer,
                                &handle,
                                &mut events,
                                default_placeholder.clone(),
                                &ctrl_c_state,
                                &ctrl_c_notify,
                            )
                            .await
                            {
                                Ok(ToolPermissionFlow::Approved) => {
                                    // Tool execution logic
                                    continue;
                                }
                                Ok(ToolPermissionFlow::Denied) => continue,
                                Ok(ToolPermissionFlow::Exit) => break,
                                Ok(ToolPermissionFlow::Interrupted) => break,
                                Err(err) => {
                                    renderer.line(
                                        MessageStyle::Error,
                                        &format!(
                                            "Failed to evaluate policy for tool '{}': {}",
                                            name, err
                                        ),
                                    )?;
                                    continue;
                                }
                            }
                        }
                        SlashCommandOutcome::Exit => {
                            renderer.line(MessageStyle::Info, "Goodbye!")?;
                            break;
                        }
                    }
                }
                continue;
            }
            _ => {}
        }

        if let Some(picker) = model_picker_state.as_mut() {
            let progress = picker.handle_input(&mut renderer, input_owned.as_str())?;
            match progress {
                ModelPickerProgress::InProgress => continue,
                ModelPickerProgress::Cancelled => {
                    model_picker_state = None;
                    continue;
                }
                ModelPickerProgress::Completed(selection) => {
                    let picker_state = model_picker_state.take().unwrap();
                    if let Err(err) = finalize_model_selection(
                        &mut renderer,
                        &picker_state,
                        selection,
                        &mut config,
                        &mut vt_cfg,
                        &mut provider_client,
                        &session_bootstrap,
                        &handle,
                        full_auto,
                    ) {
                        renderer.line(
                            MessageStyle::Error,
                            &format!("Failed to apply model selection: {}", err),
                        )?;
                    }
                    continue;
                }
            }
        }

        let input = input_owned.as_str();

        let refined_user = refine_user_prompt_if_enabled(input, &config, vt_cfg.as_ref()).await;
        // Display the user message with inline border decoration
        display_user_message(&mut renderer, &refined_user)?;
        conversation_history.push(uni::Message::user(refined_user));
        let _pruned_tools = prune_unified_tool_responses(
            &mut conversation_history,
            trim_config.preserve_recent_turns,
        );
        // Removed: Tool response pruning message
        let trim_result = enforce_unified_context_window(&mut conversation_history, trim_config);
        if trim_result.is_trimmed() {
            renderer.line(
                MessageStyle::Info,
                &format!(
                    "Trimmed {} earlier messages to respect the context window (~{} tokens).",
                    trim_result.removed_messages, trim_config.max_tokens,
                ),
            )?;
        }

        let mut working_history = conversation_history.clone();
        let max_tool_loops = vt_cfg
            .as_ref()
            .map(|cfg| cfg.tools.max_tool_loops)
            .filter(|&value| value > 0)
            .unwrap_or(defaults::DEFAULT_MAX_TOOL_LOOPS);

        let mut loop_guard = 0usize;
        let mut any_write_effect = false;
        let mut last_tool_stdout: Option<String> = None;
        let mut bottom_gap_applied = false;

        let turn_result = 'outer: loop {
            if ctrl_c_state.is_cancel_requested() {
                break TurnLoopResult::Cancelled;
            }
            if loop_guard == 0 {
                renderer.line_if_not_empty(MessageStyle::Output)?;
            }
            loop_guard += 1;
            if loop_guard >= max_tool_loops {
                if !bottom_gap_applied {
                    renderer.line(MessageStyle::Output, "")?;
                }
                let notice = format!(
                    "I reached the configured tool-call limit of {} for this turn and paused further tool execution. Increase `tools.max_tool_loops` in vtcode.toml if you need more, then ask me to continue.",
                    max_tool_loops
                );
                renderer.line(MessageStyle::Error, &notice)?;
                ensure_turn_bottom_gap(&mut renderer, &mut bottom_gap_applied)?;
                working_history.push(uni::Message::assistant(notice));
                break TurnLoopResult::Completed;
            }

            let _ = enforce_unified_context_window(&mut working_history, trim_config);

            let decision = if let Some(cfg) = vt_cfg.as_ref().filter(|cfg| cfg.router.enabled) {
                Router::route_async(cfg, &config, &config.api_key, input).await
            } else {
                Router::route(&VTCodeConfig::default(), &config, input)
            };
            traj.log_route(
                working_history.len(),
                &decision.selected_model,
                match decision.class {
                    TaskClass::Simple => "simple",
                    TaskClass::Standard => "standard",
                    TaskClass::Complex => "complex",
                    TaskClass::CodegenHeavy => "codegen_heavy",
                    TaskClass::RetrievalHeavy => "retrieval_heavy",
                },
                &input.chars().take(120).collect::<String>(),
            );

            let active_model = decision.selected_model;
            let (max_tokens_opt, parallel_cfg_opt) = if let Some(vt) = vt_cfg.as_ref() {
                let key = match decision.class {
                    TaskClass::Simple => "simple",
                    TaskClass::Standard => "standard",
                    TaskClass::Complex => "complex",
                    TaskClass::CodegenHeavy => "codegen_heavy",
                    TaskClass::RetrievalHeavy => "retrieval_heavy",
                };
                let budget = vt.router.budgets.get(key);
                let max_tokens = budget.and_then(|b| b.max_tokens).map(|value| value as u32);
                let parallel = budget.and_then(|b| b.max_parallel_tools).map(|value| {
                    vtcode_core::llm::provider::ParallelToolConfig {
                        disable_parallel_tool_use: value <= 1,
                        max_parallel_tools: Some(value),
                        encourage_parallel: value > 1,
                    }
                });
                (max_tokens, parallel)
            } else {
                (None, None)
            };

            {
                let mut ledger = decision_ledger.write().await;
                ledger.start_turn(
                    working_history.len(),
                    working_history
                        .last()
                        .map(|message| message.content.clone()),
                );
                let tool_names: Vec<String> = tools
                    .iter()
                    .map(|tool| tool.function.name.clone())
                    .collect();
                ledger.update_available_tools(tool_names);
            }

            let mut attempt_history = working_history.clone();
            let mut retry_attempts = 0usize;
            let (response, response_streamed) = loop {
                retry_attempts += 1;
                let _ = enforce_unified_context_window(&mut attempt_history, trim_config);

                if token_budget_enabled {
                    token_budget.reset().await;
                }

                let curator_messages =
                    build_curator_messages(&attempt_history, &*token_budget, token_budget_enabled)
                        .await?;
                let curated_context = curator
                    .curate_context(&curator_messages, &curator_tool_catalog)
                    .await?;
                let curated_sections = build_curated_sections(&curated_context);

                let mut system_prompt = base_system_prompt.clone();
                if token_budget_enabled {
                    token_budget
                        .count_tokens_for_component(
                            &system_prompt,
                            ContextComponent::SystemPrompt,
                            Some(&format!("base_system_{}", retry_attempts)),
                        )
                        .await?;
                }

                if !curated_sections.is_empty() {
                    system_prompt.push_str("\n\n[Curated Context]\n");
                    for (idx, section) in curated_sections.iter().enumerate() {
                        let body = section.body.trim();
                        if body.is_empty() {
                            continue;
                        }
                        system_prompt.push_str(section.heading);
                        system_prompt.push('\n');
                        system_prompt.push_str(section.body.trim_end());
                        system_prompt.push('\n');
                        if token_budget_enabled {
                            token_budget
                                .count_tokens_for_component(
                                    body,
                                    section.component,
                                    Some(&format!("section_{}_{}", retry_attempts, idx)),
                                )
                                .await?;
                        }
                    }
                }

                let use_streaming = provider_client.supports_streaming();
                let reasoning_effort = vt_cfg.as_ref().and_then(|cfg| {
                    if provider_client.supports_reasoning_effort(&active_model) {
                        Some(cfg.agent.reasoning_effort)
                    } else {
                        None
                    }
                });
                let request = uni::LLMRequest {
                    messages: attempt_history.clone(),
                    system_prompt: Some(system_prompt.clone()),
                    tools: Some(tools.clone()),
                    model: active_model.clone(),
                    max_tokens: max_tokens_opt.or(Some(2000)),
                    temperature: Some(0.7),
                    stream: use_streaming,
                    tool_choice: Some(uni::ToolChoice::auto()),
                    parallel_tool_calls: None,
                    parallel_tool_config: parallel_cfg_opt.clone(),
                    reasoning_effort,
                };

                let thinking_spinner =
                    PlaceholderSpinner::new(&handle, default_placeholder.clone(), "Thinking...");
                let mut spinner_active = true;
                task::yield_now().await;
                let result = if use_streaming {
                    let outcome = stream_and_render_response(
                        provider_client.as_ref(),
                        request,
                        &thinking_spinner,
                        &mut renderer,
                        &ctrl_c_state,
                        &ctrl_c_notify,
                    )
                    .await;
                    spinner_active = false;
                    outcome
                } else {
                    provider_client
                        .generate(request)
                        .await
                        .map(|resp| (resp, false))
                };

                if spinner_active {
                    thinking_spinner.finish();
                }

                match result {
                    Ok((result, streamed_tokens)) => {
                        working_history = attempt_history.clone();
                        break (result, streamed_tokens);
                    }
                    Err(error) => {
                        if ctrl_c_state.is_cancel_requested() {
                            break 'outer TurnLoopResult::Cancelled;
                        }
                        let error_text = error.to_string();
                        if is_context_overflow_error(&error_text)
                            && retry_attempts <= vtcode_core::config::constants::context::CONTEXT_ERROR_RETRY_LIMIT
                        {
                            let removed_tool_messages = prune_unified_tool_responses(
                                &mut attempt_history,
                                trim_config.preserve_recent_turns,
                            );
                            let removed_turns =
                                apply_aggressive_trim_unified(&mut attempt_history, trim_config);
                            let total_removed = removed_tool_messages + removed_turns;
                            if total_removed > 0 {
                                renderer.line(
                                    MessageStyle::Info,
                                    &format!(
                                        "Context overflow detected; removed {} older messages (retry {}/{}).",
                                        total_removed,
                                        retry_attempts,
                                        vtcode_core::config::constants::context::CONTEXT_ERROR_RETRY_LIMIT,
                                    ),
                                )?;
                                conversation_history.clone_from(&attempt_history);
                                continue;
                            }
                        }

                        let has_tool = working_history
                            .iter()
                            .any(|msg| msg.role == uni::MessageRole::Tool);

                        if has_tool {
                            eprintln!("Provider error (suppressed): {error_text}");
                            let reply = derive_recent_tool_output(&working_history)
                                .unwrap_or_else(|| "Command completed successfully.".to_string());
                            renderer.line(MessageStyle::Response, &reply)?;
                            ensure_turn_bottom_gap(&mut renderer, &mut bottom_gap_applied)?;
                            working_history.push(uni::Message::assistant(reply));
                            let _ = last_tool_stdout.take();
                            break 'outer TurnLoopResult::Completed;
                        } else {
                            renderer.line(
                                MessageStyle::Error,
                                &format!("Provider error: {error_text}"),
                            )?;
                            ensure_turn_bottom_gap(&mut renderer, &mut bottom_gap_applied)?;
                            break 'outer TurnLoopResult::Aborted;
                        }
                    }
                }
            };

            let mut final_text = response.content.clone();
            let mut tool_calls = response.tool_calls.clone().unwrap_or_default();
            let mut interpreted_textual_call = false;

            if tool_calls.is_empty()
                && let Some(text) = final_text.clone()
                && let Some((name, args)) = detect_textual_tool_call(&text)
            {
                let args_json = serde_json::to_string(&args).unwrap_or_else(|_| "{}".to_string());
                let code_blocks = extract_code_fence_blocks(&text);
                if !code_blocks.is_empty() {
                    render_code_fence_blocks(&mut renderer, &code_blocks)?;
                    renderer.line(MessageStyle::Output, "")?;
                }
                let (headline, _) = describe_tool_action(&name, &args);
                let notice = if headline.is_empty() {
                    format!("Detected {} request", humanize_tool_name(&name))
                } else {
                    format!("Detected {headline}")
                };
                renderer.line(MessageStyle::Info, &notice)?;
                let call_id = format!("call_textual_{}", working_history.len());
                tool_calls.push(uni::ToolCall::function(
                    call_id.clone(),
                    name.clone(),
                    args_json.clone(),
                ));
                interpreted_textual_call = true;
                final_text = None;
            }

            if tool_calls.is_empty()
                && let Some(text) = final_text.clone()
            {
                working_history.push(uni::Message::assistant(text));
            } else {
                let assistant_text = if interpreted_textual_call {
                    String::new()
                } else {
                    final_text.clone().unwrap_or_default()
                };
                working_history.push(uni::Message::assistant_with_tools(
                    assistant_text,
                    tool_calls.clone(),
                ));
                for call in &tool_calls {
                    let name = call.function.name.as_str();
                    let args_val = call
                        .parsed_arguments()
                        .unwrap_or_else(|_| serde_json::json!({}));

                    // Render MCP tool calls as assistant messages instead of user input
                    if name.starts_with("mcp_") {
                        let tool_name = &name[4..]; // Remove "mcp_" prefix
                        let (headline, _) = describe_tool_action(tool_name, &args_val);

                        // Render MCP tool call as a single message block
                        renderer.line(MessageStyle::Info, &format!("→ {}", headline))?;
                        renderer.line(
                            MessageStyle::Info,
                            &format!("MCP: {} → {}", "mcp", tool_name),
                        )?;

                        // Force immediate TUI refresh to ensure proper layout
                        handle.force_redraw();
                        tokio::time::sleep(Duration::from_millis(10)).await;

                        // Also capture for logging
                        {
                            let mut mcp_event = mcp_events::McpEvent::new(
                                "mcp".to_string(),
                                tool_name.to_string(),
                                Some(args_val.to_string()),
                            );
                            mcp_event.success(None);
                            mcp_panel_state.add_event(mcp_event);
                        }
                    } else {
                        render_tool_call_summary(&mut renderer, name, &args_val)?;
                    }
                    let dec_id = {
                        let mut ledger = decision_ledger.write().await;
                        ledger.record_decision(
                            format!("Execute tool '{}' to progress task", name),
                            DTAction::ToolCall {
                                name: name.to_string(),
                                args: args_val.clone(),
                                expected_outcome: "Use tool output to decide next step".to_string(),
                            },
                            None,
                        )
                    };

                    match ensure_tool_permission(
                        &mut tool_registry,
                        name,
                        &mut renderer,
                        &handle,
                        &mut events,
                        default_placeholder.clone(),
                        &ctrl_c_state,
                        &ctrl_c_notify,
                    )
                    .await
                    {
                        Ok(ToolPermissionFlow::Approved) => {
                            let tool_spinner = PlaceholderSpinner::new(
                                &handle,
                                default_placeholder.clone(),
                                format!("Running tool: {}", name),
                            );

                            // Force TUI refresh to ensure display stability
                            safe_force_redraw(&handle, &mut last_forced_redraw);

                            match tokio::time::timeout(
                                tokio::time::Duration::from_secs(300), // 5 minute timeout for long-running tools
                                tool_registry.execute_tool(name, args_val.clone()),
                            )
                            .await
                            {
                                Ok(Ok(tool_output)) => {
                                    tool_spinner.finish();

                                    // Ensure TUI layout is clean after spinner finishes
                                    safe_force_redraw(&handle, &mut last_forced_redraw);
                                    tokio::time::sleep(Duration::from_millis(50)).await;

                                    session_stats.record_tool(name);
                                    traj.log_tool_call(
                                        working_history.len(),
                                        name,
                                        &args_val,
                                        true,
                                    );

                                    // Add MCP success message and capture event for logging (only for MCP tools)
                                    if name.starts_with("mcp_") {
                                        let tool_name = &name[4..];
                                        // Ensure clean message block for completion
                                        renderer.line_if_not_empty(MessageStyle::Output)?;
                                        renderer.line(
                                            MessageStyle::Info,
                                            &format!("✓ MCP: {} completed", tool_name),
                                        )?;

                                        // Force immediate TUI refresh to ensure proper layout
                                        handle.force_redraw();
                                        tokio::time::sleep(Duration::from_millis(10)).await;

                                        {
                                            let mut mcp_event = mcp_events::McpEvent::new(
                                                "mcp".to_string(),
                                                tool_name.to_string(),
                                                Some(args_val.to_string()),
                                            );
                                            mcp_event.success(None);
                                            mcp_panel_state.add_event(mcp_event);
                                        }
                                    }

                                    render_tool_output(
                                        &mut renderer,
                                        Some(name),
                                        &tool_output,
                                        vt_cfg.as_ref(),
                                    )?;
                                    last_tool_stdout = tool_output
                                        .get("stdout")
                                        .and_then(|value| value.as_str())
                                        .map(|s| s.trim().to_string())
                                        .filter(|s| !s.is_empty());
                                    let modified_files: Vec<String> = if let Some(files) =
                                        tool_output
                                            .get("modified_files")
                                            .and_then(|value| value.as_array())
                                    {
                                        files
                                            .iter()
                                            .filter_map(|file| {
                                                file.as_str().map(|value| value.to_string())
                                            })
                                            .collect()
                                    } else {
                                        vec![]
                                    };

                                    if matches!(
                                        name,
                                        "write_file"
                                            | "edit_file"
                                            | "create_file"
                                            | "delete_file"
                                            | "srgn"
                                    ) {
                                        any_write_effect = true;
                                    }

                                    if !modified_files.is_empty()
                                        && confirm_changes_with_git_diff(
                                            &modified_files,
                                            skip_confirmations,
                                        )
                                        .await?
                                    {
                                        renderer.line(
                                            MessageStyle::Info,
                                            "Changes applied successfully.",
                                        )?;
                                    } else if !modified_files.is_empty() {
                                        renderer.line(MessageStyle::Info, "Changes discarded.")?;
                                    }

                                    let content = serde_json::to_string(&tool_output)
                                        .unwrap_or("{}".to_string());
                                    working_history.push(uni::Message::tool_response(
                                        call.id.clone(),
                                        content,
                                    ));
                                    {
                                        let mut ledger = decision_ledger.write().await;
                                        ledger.record_outcome(
                                            &dec_id,
                                            DecisionOutcome::Success {
                                                result: "tool_ok".to_string(),
                                                metrics: Default::default(),
                                            },
                                        );
                                    }

                                    if should_short_circuit_shell(input, name, &args_val) {
                                        let reply = last_tool_stdout.clone().unwrap_or_else(|| {
                                            "Command completed successfully.".to_string()
                                        });
                                        renderer.line(MessageStyle::Response, &reply)?;
                                        ensure_turn_bottom_gap(
                                            &mut renderer,
                                            &mut bottom_gap_applied,
                                        )?;
                                        working_history.push(uni::Message::assistant(reply));
                                        let _ = last_tool_stdout.take();
                                        break 'outer TurnLoopResult::Completed;
                                    }
                                }
                                Ok(Err(error)) => {
                                    tool_spinner.finish();

                                    // Ensure TUI layout is clean after spinner finishes
                                    safe_force_redraw(&handle, &mut last_forced_redraw);
                                    tokio::time::sleep(Duration::from_millis(50)).await;

                                    session_stats.record_tool(name);
                                    renderer.line(
                                        MessageStyle::Tool,
                                        &format!("Tool {} failed.", name),
                                    )?;
                                    traj.log_tool_call(
                                        working_history.len(),
                                        name,
                                        &args_val,
                                        false,
                                    );

                                    // Add MCP failure as assistant message and capture for logging
                                    if name.starts_with("mcp_") {
                                        let tool_name = &name[4..];
                                        // Ensure clean message block for error
                                        renderer.line_if_not_empty(MessageStyle::Output)?;
                                        renderer.line(
                                            MessageStyle::Error,
                                            &format!("❌ MCP: {} failed - {}", tool_name, error),
                                        )?;

                                        // Force immediate TUI refresh to ensure proper layout
                                        handle.force_redraw();
                                        tokio::time::sleep(Duration::from_millis(10)).await;

                                        {
                                            let mut mcp_event = mcp_events::McpEvent::new(
                                                "mcp".to_string(),
                                                tool_name.to_string(),
                                                Some(args_val.to_string()),
                                            );
                                            mcp_event.failure(Some(error.to_string()));
                                            mcp_panel_state.add_event(mcp_event);
                                        }
                                    }

                                    renderer.line(
                                        MessageStyle::Error,
                                        &format!("Tool error: {error}"),
                                    )?;
                                    let err = serde_json::json!({ "error": error.to_string() });
                                    let content = err.to_string();
                                    working_history.push(uni::Message::tool_response(
                                        call.id.clone(),
                                        content,
                                    ));
                                    let _ = last_tool_stdout.take();
                                    {
                                        let mut ledger = decision_ledger.write().await;
                                        ledger.record_outcome(
                                            &dec_id,
                                            DecisionOutcome::Failure {
                                                error: error.to_string(),
                                                recovery_attempts: 0,
                                                context_preserved: true,
                                            },
                                        );
                                    }
                                }
                                Err(_timeout) => {
                                    tool_spinner.finish();

                                    // Ensure TUI layout is clean after spinner finishes
                                    handle.force_redraw();
                                    tokio::time::sleep(Duration::from_millis(10)).await;

                                    session_stats.record_tool(name);
                                    // Ensure clean message block for timeout error
                                    renderer.line_if_not_empty(MessageStyle::Output)?;
                                    renderer.line(
                                        MessageStyle::Error,
                                        &format!("Tool {} timed out after 5 minutes.", name),
                                    )?;
                                    traj.log_tool_call(
                                        working_history.len(),
                                        name,
                                        &args_val,
                                        false,
                                    );

                                    let timeout_error = ToolExecutionError::new(
                                        name.to_string(),
                                        ToolErrorType::ExecutionError,
                                        "Tool execution timed out after 5 minutes".to_string(),
                                    );
                                    let err_json = serde_json::json!({
                                        "error": timeout_error.message
                                    });
                                    working_history.push(uni::Message::tool_response(
                                        call.id.clone(),
                                        err_json.to_string(),
                                    ));
                                    {
                                        let mut ledger = decision_ledger.write().await;
                                        ledger.record_outcome(
                                            &dec_id,
                                            DecisionOutcome::Failure {
                                                error: "Tool execution timed out after 5 minutes"
                                                    .to_string(),
                                                recovery_attempts: 0,
                                                context_preserved: true,
                                            },
                                        );
                                    }

                                    // Force final TUI refresh after timeout
                                    handle.force_redraw();
                                    tokio::time::sleep(Duration::from_millis(10)).await;
                                }
                            }
                        }
                        Ok(ToolPermissionFlow::Denied) => {
                            session_stats.record_tool(name);
                            let denial = ToolExecutionError::new(
                                name.to_string(),
                                ToolErrorType::PolicyViolation,
                                format!("Tool '{}' execution denied by policy", name),
                            )
                            .to_json_value();
                            traj.log_tool_call(working_history.len(), name, &args_val, false);
                            render_tool_output(
                                &mut renderer,
                                Some(name),
                                &denial,
                                vt_cfg.as_ref(),
                            )?;
                            let content =
                                serde_json::to_string(&denial).unwrap_or("{}".to_string());
                            working_history
                                .push(uni::Message::tool_response(call.id.clone(), content));
                            {
                                let mut ledger = decision_ledger.write().await;
                                ledger.record_outcome(
                                    &dec_id,
                                    DecisionOutcome::Failure {
                                        error: format!(
                                            "Tool '{}' execution denied by policy",
                                            name
                                        ),
                                        recovery_attempts: 0,
                                        context_preserved: true,
                                    },
                                );
                            }
                            continue;
                        }
                        Ok(ToolPermissionFlow::Exit) => {
                            renderer.line(MessageStyle::Info, "Goodbye!")?;
                            break 'outer TurnLoopResult::Cancelled;
                        }
                        Ok(ToolPermissionFlow::Interrupted) => {
                            break 'outer TurnLoopResult::Cancelled;
                        }
                        Err(err) => {
                            traj.log_tool_call(working_history.len(), name, &args_val, false);
                            renderer.line(
                                MessageStyle::Error,
                                &format!("Failed to evaluate policy for tool '{}': {}", name, err),
                            )?;
                            let err_json = serde_json::json!({
                                "error": format!(
                                    "Policy evaluation error for '{}' : {}",
                                    name, err
                                )
                            });
                            working_history.push(uni::Message::tool_response(
                                call.id.clone(),
                                err_json.to_string(),
                            ));
                            let _ = last_tool_stdout.take();
                            {
                                let mut ledger = decision_ledger.write().await;
                                ledger.record_outcome(
                                    &dec_id,
                                    DecisionOutcome::Failure {
                                        error: format!(
                                            "Failed to evaluate policy for tool '{}': {}",
                                            name, err
                                        ),
                                        recovery_attempts: 0,
                                        context_preserved: true,
                                    },
                                );
                            }
                            continue;
                        }
                    }
                }
                continue;
            }

            if let Some(mut text) = final_text.clone() {
                let do_review = vt_cfg
                    .as_ref()
                    .map(|cfg| cfg.agent.enable_self_review)
                    .unwrap_or(false);
                let review_passes = vt_cfg
                    .as_ref()
                    .map(|cfg| cfg.agent.max_review_passes)
                    .unwrap_or(1)
                    .max(1);
                if do_review {
                    let review_system = "You are the agent's critical code reviewer. Improve clarity, correctness, and add missing test or validation guidance. Return only the improved final answer (no meta commentary).".to_string();
                    for _ in 0..review_passes {
                        let review_req = uni::LLMRequest {
                            messages: vec![uni::Message::user(format!(
                                "Please review and refine the following response. Return only the improved response.\n\n{}",
                                text
                            ))],
                            system_prompt: Some(review_system.clone()),
                            tools: None,
                            model: config.model.clone(),
                            max_tokens: Some(2000),
                            temperature: Some(0.5),
                            stream: false,
                            tool_choice: Some(uni::ToolChoice::none()),
                            parallel_tool_calls: None,
                            parallel_tool_config: None,
                            reasoning_effort: vt_cfg.as_ref().and_then(|cfg| {
                                if provider_client.supports_reasoning_effort(&active_model) {
                                    Some(cfg.agent.reasoning_effort)
                                } else {
                                    None
                                }
                            }),
                        };
                        let rr = provider_client.generate(review_req).await.ok();
                        if let Some(r) = rr.and_then(|result| result.content)
                            && !r.trim().is_empty()
                        {
                            text = r;
                        }
                    }
                }
                let trimmed = text.trim();
                let suppress_response = trimmed.is_empty()
                    || last_tool_stdout
                        .as_ref()
                        .map(|stdout| stdout == trimmed)
                        .unwrap_or(false);

                let streamed_matches_output = response_streamed
                    && response
                        .content
                        .as_ref()
                        .map(|original| original == &text)
                        .unwrap_or(false);

                if !suppress_response && !streamed_matches_output {
                    renderer.line(MessageStyle::Response, &text)?;
                }
                ensure_turn_bottom_gap(&mut renderer, &mut bottom_gap_applied)?;
                working_history.push(uni::Message::assistant(text));
                let _ = last_tool_stdout.take();
            } else {
                ensure_turn_bottom_gap(&mut renderer, &mut bottom_gap_applied)?;
            }
            break TurnLoopResult::Completed;
        };

        match turn_result {
            TurnLoopResult::Cancelled => {
                if ctrl_c_state.is_exit_requested() {
                    break;
                }

                renderer.line_if_not_empty(MessageStyle::Output)?;
                renderer.line(
                    MessageStyle::Info,
                    "Interrupted current task. Press Ctrl+C again to exit.",
                )?;
                handle.clear_input();
                handle.set_placeholder(default_placeholder.clone());
                ctrl_c_state.clear_cancel();
                continue;
            }
            TurnLoopResult::Aborted => {
                let _ = conversation_history.pop();
                continue;
            }
            TurnLoopResult::Completed => {
                conversation_history = working_history;

                let _pruned_after_turn = prune_unified_tool_responses(
                    &mut conversation_history,
                    trim_config.preserve_recent_turns,
                );
                // Removed: Tool response pruning message after completion
                let post_trim =
                    enforce_unified_context_window(&mut conversation_history, trim_config);
                if post_trim.is_trimmed() {
                    renderer.line(
                        MessageStyle::Info,
                        &format!(
                            "Trimmed {} earlier messages to respect the context window (~{} tokens).",
                            post_trim.removed_messages, trim_config.max_tokens,
                        ),
                    )?;
                }

                if let Some(last) = conversation_history.last()
                    && last.role == uni::MessageRole::Assistant
                {
                    let text = &last.content;
                    let claims_write = text.contains("I've updated")
                        || text.contains("I have updated")
                        || text.contains("updated the `");
                    if claims_write && !any_write_effect {
                        renderer.line_if_not_empty(MessageStyle::Output)?;
                        renderer.line(
                            MessageStyle::Info,
                            "Note: The assistant mentioned edits but no write tool ran.",
                        )?;
                    }
                }
            }
        }
    }

    let transcript_lines = transcript::snapshot();
    if let Some(archive) = session_archive.take() {
        let distinct_tools = session_stats.sorted_tools();
        let total_messages = conversation_history.len();
        let session_messages: Vec<SessionMessage> = conversation_history
            .iter()
            .map(SessionMessage::from)
            .collect();
        match archive.finalize(
            transcript_lines,
            total_messages,
            distinct_tools,
            session_messages,
        ) {
            Ok(path) => {
                renderer.line(
                    MessageStyle::Info,
                    &format!("Session saved to {}", path.display()),
                )?;
                renderer.line_if_not_empty(MessageStyle::Output)?;
            }
            Err(err) => {
                renderer.line(
                    MessageStyle::Error,
                    &format!("Failed to save session: {}", err),
                )?;
                renderer.line_if_not_empty(MessageStyle::Output)?;
            }
        }
    }

    // Shutdown MCP client properly before TUI shutdown
    if let Some(mcp_client) = &mcp_client {
        if let Err(e) = mcp_client.shutdown().await {
            let error_msg = e.to_string();
            if error_msg.contains("EPIPE")
                || error_msg.contains("Broken pipe")
                || error_msg.contains("write EPIPE")
            {
                eprintln!(
                    "Info: MCP client shutdown encountered pipe errors (normal): {}",
                    e
                );
            } else {
                eprintln!("Warning: Failed to shutdown MCP client cleanly: {}", e);
            }
        }
    }

    handle.shutdown();
    Ok(())
}

fn safe_force_redraw(handle: &InlineHandle, last_forced_redraw: &mut Instant) {
    // Rate limit force_redraw calls to prevent TUI corruption
    if last_forced_redraw.elapsed() > std::time::Duration::from_millis(100) {
        handle.force_redraw();
        *last_forced_redraw = Instant::now();
    }
}
