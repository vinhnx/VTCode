use std::time::Duration;

use anyhow::Result;
use chrono::Local;

use vtcode_core::ui::slash::SlashCommandInfo;
use vtcode_core::ui::theme;
use vtcode_core::ui::tui::{
    InlineHandle, InlineListItem, InlineListSelection, convert_style, theme_from_styles,
};
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};
use vtcode_core::utils::session_archive::SessionListing;

use crate::agent::runloop::slash_commands::ThemePaletteMode;

use super::display::persist_theme_preference;

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

#[derive(Clone)]
pub(crate) enum ActivePalette {
    Theme {
        mode: ThemePaletteMode,
    },
    Sessions {
        listings: Vec<SessionListing>,
        limit: usize,
    },
    Help,
}

pub(crate) fn show_theme_palette(
    renderer: &mut AnsiRenderer,
    mode: ThemePaletteMode,
) -> Result<bool> {
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

pub(crate) fn show_sessions_palette(
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

pub(crate) fn show_help_palette(
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

pub(crate) fn render_session_details(
    renderer: &mut AnsiRenderer,
    listing: &SessionListing,
) -> Result<()> {
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

pub(crate) async fn handle_palette_selection(
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
                            persist_theme_preference(renderer, &theme_id).await?;
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
                    let label = theme::theme_label(&theme_id).unwrap_or(theme_id.as_str());
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
            if let InlineListSelection::Session(selected_id) = &selection
                && let Some(listing) = listings
                    .iter()
                    .find(|entry| entry.identifier() == *selected_id)
                    .cloned()
            {
                render_session_details(renderer, &listing)?;
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
            renderer.close_modal();
            Ok(None)
        }
    }
}

pub(crate) fn handle_palette_cancel(
    palette: ActivePalette,
    renderer: &mut AnsiRenderer,
) -> Result<()> {
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

pub(crate) fn apply_prompt_style(handle: &InlineHandle) {
    let styles = theme::active_styles();
    let style = convert_style(styles.primary);
    handle.set_prompt("".to_string(), style);
}
