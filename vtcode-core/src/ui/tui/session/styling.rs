use anstyle::{AnsiColor, Color as AnsiColorEnum, RgbColor};
use ratatui::prelude::*;

use crate::config::constants::ui;
use crate::ui::tui::{
    style::{ratatui_color_from_ansi, ratatui_style_from_inline},
    types::{InlineMessageKind, InlineTextStyle, InlineTheme},
};

use super::message::MessageLine;

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
        let normalized_name = normalize_tool_name(tool_name);
        let mut style = InlineTextStyle::default().bold();

        // Assign distinctive colors based on normalized tool type
        style.color = match normalized_name {
            "read" => {
                // Cyan for file reading operations
                Some(AnsiColor::Cyan.into())
            }
            "list" => {
                // Green for listing operations
                Some(AnsiColor::Green.into())
            }
            "search" => {
                // Cyan for search operations
                Some(AnsiColor::Cyan.into())
            }
            "write" => {
                // Magenta for write/edit operations
                Some(AnsiColor::Magenta.into())
            }
            "run" => {
                // Red for execution operations
                Some(AnsiColor::Red.into())
            }
            "git" | "version_control" => {
                // Cyan for version control
                Some(AnsiColor::Cyan.into())
            }
            _ => {
                // Use the default tool accent color for other tools
                self.theme
                    .tool_accent
                    .or(self.theme.primary)
                    .or(self.theme.foreground)
            }
        };

        style
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

    /// Get the border style
    pub fn border_style(&self) -> Style {
        ratatui_style_from_inline(&self.border_inline_style(), self.theme.foreground)
            .add_modifier(Modifier::DIM)
    }

    pub fn input_background_style(&self) -> Style {
        let mut style = self.default_style();
        let Some(background) = self.theme.background else {
            return style;
        };

        let resolved = match (background, self.theme.foreground) {
            (AnsiColorEnum::Rgb(bg), Some(AnsiColorEnum::Rgb(fg))) => {
                AnsiColorEnum::Rgb(mix_rgb(bg, fg, ui::THEME_INPUT_BACKGROUND_MIX_RATIO))
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

fn mix_rgb(color: RgbColor, target: RgbColor, ratio: f64) -> RgbColor {
    let ratio = ratio.clamp(0.0, 1.0);
    let blend = |channel: u8, target_channel: u8| -> u8 {
        let channel = f64::from(channel);
        let target_channel = f64::from(target_channel);
        ((channel + (target_channel - channel) * ratio).round()).clamp(0.0, 255.0) as u8
    };

    RgbColor(
        blend(color.0, target.0),
        blend(color.1, target.1),
        blend(color.2, target.2),
    )
}
