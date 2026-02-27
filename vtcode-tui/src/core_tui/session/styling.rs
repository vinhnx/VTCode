use anstyle::{AnsiColor, Color as AnsiColorEnum, RgbColor};
use ratatui::prelude::*;

use crate::config::constants::ui;
use crate::ui::tui::{
    style::{ratatui_color_from_ansi, ratatui_style_from_inline},
    types::{InlineMessageKind, InlineTextStyle, InlineTheme},
};

use super::message::MessageLine;

fn mix(color: RgbColor, target: RgbColor, ratio: f64) -> RgbColor {
    let ratio = ratio.clamp(ui::THEME_MIX_RATIO_MIN, ui::THEME_MIX_RATIO_MAX);
    let blend = |c: u8, t: u8| -> u8 {
        let c = c as f64;
        let t = t as f64;
        ((c + (t - c) * ratio).round()).clamp(ui::THEME_BLEND_CLAMP_MIN, ui::THEME_BLEND_CLAMP_MAX)
            as u8
    };

    RgbColor(
        blend(color.0, target.0),
        blend(color.1, target.1),
        blend(color.2, target.2),
    )
}

pub fn normalize_tool_name(tool_name: &str) -> &'static str {
    match tool_name.to_lowercase().as_str() {
        "grep" | "rg" | "ripgrep" | "grep_file" | "search" | "find" | "ag" => "search",
        "list" | "ls" | "dir" | "list_files" => "list",
        "read" | "cat" | "file" | "read_file" => "read",
        "write" | "edit" | "save" | "insert" | "edit_file" => "write",
        "run" | "command" | "bash" | "sh" => "run",
        _ => "other",
    }
}

/// Get the inline style for a tool based on its normalized name.
/// Shared by both `SessionStyles` and standalone rendering contexts.
pub fn tool_inline_style_for(tool_name: &str, theme: &InlineTheme) -> InlineTextStyle {
    let normalized_name = normalize_tool_name(tool_name);
    let mut style = InlineTextStyle::default().bold();

    style.color = match normalized_name {
        "read" => Some(AnsiColor::Cyan.into()),
        "list" => Some(AnsiColor::Green.into()),
        "search" => Some(AnsiColor::Cyan.into()),
        "write" => Some(AnsiColor::Magenta.into()),
        "run" => Some(AnsiColor::Red.into()),
        "git" | "version_control" => Some(AnsiColor::Cyan.into()),
        _ => theme.tool_accent.or(theme.primary).or(theme.foreground),
    };

    style
}

/// Styling utilities for the Session UI
pub struct SessionStyles {
    theme: InlineTheme,
}

impl SessionStyles {
    pub fn new(theme: InlineTheme) -> Self {
        Self { theme }
    }

    #[allow(dead_code)]
    pub fn theme(&self) -> &InlineTheme {
        &self.theme
    }

    pub fn set_theme(&mut self, theme: InlineTheme) {
        self.theme = theme;
    }

    /// Get the modal list highlight style
    pub fn modal_list_highlight_style(&self) -> Style {
        let mut style = Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD);
        if let Some(primary) = self.theme.primary.or(self.theme.foreground) {
            style = style.fg(ratatui_color_from_ansi(primary));
        }
        style
    }

    /// Get the inline style for a tool based on its name
    #[allow(dead_code)]
    pub fn tool_inline_style(&self, tool_name: &str) -> InlineTextStyle {
        tool_inline_style_for(tool_name, &self.theme)
    }

    /// Get the tool border style
    pub fn tool_border_style(&self) -> InlineTextStyle {
        self.border_inline_style()
    }

    /// Get the default style
    pub fn default_style(&self) -> Style {
        let mut style = Style::default();
        if let Some(foreground) = self.theme.foreground.map(ratatui_color_from_ansi) {
            style = style.fg(foreground);
        }
        style
    }

    /// Get the default inline style (for tests and inline conversions)
    #[allow(dead_code)]
    pub fn default_inline_style(&self) -> InlineTextStyle {
        InlineTextStyle {
            color: self.theme.foreground,
            ..InlineTextStyle::default()
        }
    }

    /// Get the accent inline style
    pub fn accent_inline_style(&self) -> InlineTextStyle {
        InlineTextStyle {
            color: self.theme.primary.or(self.theme.foreground),
            ..InlineTextStyle::default()
        }
    }

    /// Get the accent style
    pub fn accent_style(&self) -> Style {
        ratatui_style_from_inline(&self.accent_inline_style(), self.theme.foreground)
    }

    /// Get the border inline style
    pub fn border_inline_style(&self) -> InlineTextStyle {
        InlineTextStyle {
            color: self.theme.secondary.or(self.theme.foreground),
            ..InlineTextStyle::default()
        }
    }

    /// Get the border style (dimmed)
    pub fn border_style(&self) -> Style {
        self.dimmed_border_style(true)
    }

    /// Get a border style with configurable boldness.
    /// When `suppress_bold` is true, the BOLD modifier is removed — useful for
    /// info/error/warning block borders that should appear subtle.
    pub fn dimmed_border_style(&self, suppress_bold: bool) -> Style {
        let mut style =
            ratatui_style_from_inline(&self.border_inline_style(), self.theme.foreground)
                .add_modifier(Modifier::DIM);
        if suppress_bold {
            style = style.remove_modifier(Modifier::BOLD);
        }
        style
    }

    pub fn input_background_style(&self) -> Style {
        let mut style = self.default_style();
        let Some(background) = self.theme.background else {
            return style;
        };

        let resolved = match (background, self.theme.foreground) {
            (AnsiColorEnum::Rgb(bg), Some(AnsiColorEnum::Rgb(fg))) => {
                AnsiColorEnum::Rgb(mix(bg, fg, ui::THEME_INPUT_BACKGROUND_MIX_RATIO))
            }
            (color, _) => color,
        };

        style = style.bg(ratatui_color_from_ansi(resolved));
        style
    }

    /// Get the prefix style for a message line
    pub fn prefix_style(&self, line: &MessageLine) -> InlineTextStyle {
        let fallback = self.text_fallback(line.kind).or(self.theme.foreground);

        let color = line
            .segments
            .iter()
            .find_map(|segment| segment.style.color)
            .or(fallback);

        InlineTextStyle {
            color,
            ..InlineTextStyle::default()
        }
    }

    /// Get the fallback text color for a message kind
    pub fn text_fallback(&self, kind: InlineMessageKind) -> Option<AnsiColorEnum> {
        match kind {
            InlineMessageKind::Agent | InlineMessageKind::Policy => Some(AnsiColor::Magenta.into()),
            InlineMessageKind::User => self.theme.secondary.or(self.theme.foreground),
            InlineMessageKind::Tool | InlineMessageKind::Pty | InlineMessageKind::Error => {
                self.theme.primary.or(self.theme.foreground)
            }
            InlineMessageKind::Info => self.theme.foreground,
            InlineMessageKind::Warning => Some(AnsiColor::Red.into()),
        }
    }

    /// Get the message divider style
    pub fn message_divider_style(&self, kind: InlineMessageKind) -> Style {
        let mut style = InlineTextStyle::default();
        if kind == InlineMessageKind::User {
            style.color = self.theme.primary.or(self.theme.foreground);
        } else {
            style.color = self.text_fallback(kind).or(self.theme.foreground);
        }
        let resolved = ratatui_style_from_inline(&style, self.theme.foreground);
        resolved.add_modifier(Modifier::DIM)
    }
}
