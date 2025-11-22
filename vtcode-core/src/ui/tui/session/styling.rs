use anstyle::{AnsiColor, Color as AnsiColorEnum};
use ratatui::style::{Modifier, Style};

use crate::ui::tui::{
    style::{ratatui_color_from_ansi, ratatui_style_from_inline},
    types::{InlineMessageKind, InlineTextStyle, InlineTheme},
};

use super::message::MessageLine;

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

    /// Get the modal list highlight style
    pub fn modal_list_highlight_style(&self) -> Style {
        let mut style = Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD);
        if let Some(primary) = self.theme.primary.or(self.theme.foreground) {
            style = style.fg(ratatui_color_from_ansi(primary));
        }
        style
    }

    /// Normalize tool names to group similar tools together
    #[allow(dead_code)]
    pub fn normalize_tool_name(&self, tool_name: &str) -> String {
        match tool_name.to_lowercase().as_str() {
            "grep" | "rg" | "ripgrep" | "grep_file" | "search" | "find" | "ag" => {
                "search".to_string()
            }
            "list" | "ls" | "dir" | "list_files" => "list".to_string(),
            "read" | "cat" | "file" | "read_file" => "read".to_string(),
            "write" | "edit" | "save" | "insert" | "edit_file" => "write".to_string(),
            "run" | "command" | "bash" | "sh" => "run".to_string(),
            _ => tool_name.to_string(),
        }
    }

    /// Get the inline style for a tool based on its name
    #[allow(dead_code)]
    pub fn tool_inline_style(&self, tool_name: &str) -> InlineTextStyle {
        let normalized_name = self.normalize_tool_name(tool_name);
        let mut style = InlineTextStyle::default().bold();

        // Assign distinctive colors based on normalized tool type
        style.color = match normalized_name.to_lowercase().as_str() {
            "read" => {
                // Blue for file reading operations
                Some(AnsiColor::Blue.into())
            }
            "list" => {
                // Green for listing operations
                Some(AnsiColor::Green.into())
            }
            "search" => {
                // Yellow for search operations
                Some(AnsiColor::Yellow.into())
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
            InlineMessageKind::Agent | InlineMessageKind::Policy => {
                self.theme.primary.or(self.theme.foreground)
            }
            InlineMessageKind::User => self.theme.secondary.or(self.theme.foreground),
            InlineMessageKind::Tool | InlineMessageKind::Pty | InlineMessageKind::Error => {
                self.theme.primary.or(self.theme.foreground)
            }
            InlineMessageKind::Info => self.theme.foreground,
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
        if kind == InlineMessageKind::User {
            resolved
        } else {
            resolved.add_modifier(Modifier::DIM)
        }
    }
}
