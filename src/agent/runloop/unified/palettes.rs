use std::time::Duration;

use anyhow::Result;
use chrono::Local;

use vtcode_core::ui::theme;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};
use vtcode_core::utils::session_archive::SessionListing;
use vtcode_tui::{InlineHandle, InlineListItem, InlineListSelection, convert_style};

use crate::agent::runloop::slash_commands::ThemePaletteMode;
use crate::agent::runloop::tui_compat::inline_theme_from_core_styles;

use super::display::persist_theme_preference;

const THEME_PALETTE_TITLE: &str = "Theme picker";
const THEME_ACTIVE_BADGE: &str = "Active";
const THEME_SELECT_HINT: &str = "Use ↑/↓ to choose a theme, Enter to apply, Esc to cancel.";
const SESSIONS_PALETTE_TITLE: &str = "Archived sessions";
const SESSIONS_HINT_PRIMARY: &str = "Use ↑/↓ to browse sessions.";
const SESSIONS_HINT_SECONDARY: &str = "Enter to resume session • Esc to close.";
const SESSIONS_LATEST_BADGE: &str = "Latest";

#[derive(Clone)]
pub(crate) enum ActivePalette {
    Theme {
        mode: ThemePaletteMode,
    },
    Sessions {
        listings: Vec<SessionListing>,
        limit: usize,
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
                            handle.set_theme(inline_theme_from_core_styles(&styles));
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
            },
            _ => Ok(Some(ActivePalette::Theme { mode })),
        },
        ActivePalette::Sessions { listings, limit } => {
            // Session selection is handled earlier in the modal handler
            // This path is for refreshing the palette display
            if show_sessions_palette(renderer, &listings, limit)? {
                Ok(Some(ActivePalette::Sessions { listings, limit }))
            } else {
                Ok(None)
            }
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
            };
            renderer.line(MessageStyle::Info, message)?;
        }
        ActivePalette::Sessions { .. } => {
            renderer.line(MessageStyle::Info, "Closed session browser.")?;
        }
    }
    Ok(())
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
