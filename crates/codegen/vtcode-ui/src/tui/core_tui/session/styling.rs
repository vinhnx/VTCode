use anstyle::{AnsiColor, Color as AnsiColorEnum, RgbColor};
use ratatui::prelude::*;
use vtcode_config::constants::tools;

use crate::tui::config::constants::ui;
use crate::tui::ui::tui::{
    style::{ratatui_color_from_ansi, ratatui_style_from_inline},
    types::{InlineMessageKind, InlineTextStyle, InlineTheme},
};

use super::message::MessageLine;

fn mix(color: RgbColor, target: RgbColor, ratio: f32) -> RgbColor {
    let ratio = ratio.clamp(ui::THEME_MIX_RATIO_MIN, ui::THEME_MIX_RATIO_MAX);
    let blend = |c: u8, t: u8| -> u8 {
        let c = c as f32;
        let t = t as f32;
        ((c + (t - c) * ratio).round()).clamp(ui::THEME_BLEND_CLAMP_MIN, ui::THEME_BLEND_CLAMP_MAX) as u8
    };

    RgbColor(blend(color.0, target.0), blend(color.1, target.1), blend(color.2, target.2))
}

fn normalize_tool_name(tool_name: &str) -> &'static str {
    match tool_name.to_lowercase().as_str() {
        "grep" | "rg" | "ripgrep" | "search" | "find" | "ag" | tools::GREP_FILE => "search",
        "list" | "ls" | "dir" | tools::LIST_FILES => "list",
        "read" | "cat" | "file" | tools::READ_FILE => "read",
        "write" | "edit" | "save" | "insert" | tools::EDIT_FILE => "write",
        "git" | "version_control" => "git",
        "run" | "command" | "bash" | "sh" | "ran" => "run",
        _ => "other",
    }
}

/// Get the inline style for a tool based on its normalized name.
/// Shared by both `SessionStyles` and standalone rendering contexts.
pub(crate) fn tool_inline_style_for(tool_name: &str, theme: &InlineTheme) -> InlineTextStyle {
    let normalized_name = normalize_tool_name(tool_name);
    let mut style = InlineTextStyle::default().bold();

    style.color = match normalized_name {
        "read" | "list" | "search" | "git" => theme.primary.or(theme.tool_accent).or(theme.foreground),
        _ => theme.tool_accent.or(theme.primary).or(theme.foreground),
    };

    style
}

/// Styling utilities for the Session UI
pub struct SessionStyles {
    theme: InlineTheme,
}

impl SessionStyles {
    pub(crate) fn new(theme: InlineTheme) -> Self {
        Self { theme }
    }

    pub fn theme(&self) -> &InlineTheme {
        &self.theme
    }

    pub(crate) fn set_theme(&mut self, theme: InlineTheme) {
        self.theme = theme;
    }

    /// Get the modal list highlight style (Select-style: primary fg, no bg change)
    pub(crate) fn modal_list_highlight_style(&self) -> Style {
        let accent = self.theme.primary.or(self.theme.tool_accent).or(self.theme.foreground);
        let mut style = self.default_style().add_modifier(Modifier::BOLD);
        if let Some(accent) = accent {
            style = style.fg(ratatui_color_from_ansi(accent));
        }
        style
    }

    /// Get the inline style for a tool based on its name
    pub fn tool_inline_style(&self, tool_name: &str) -> InlineTextStyle {
        tool_inline_style_for(tool_name, &self.theme)
    }

    /// Get the tool border style
    pub fn tool_border_style(&self) -> InlineTextStyle {
        self.border_inline_style()
    }

    /// Get the default style with both foreground and background from the theme.
    /// Painting the theme background ensures readability regardless of terminal
    /// color scheme (e.g. a light theme on a dark terminal no longer appears blank).
    pub(crate) fn default_style(&self) -> Style {
        let mut style = Style::default();
        if let Some(background) = self.theme.background.map(ratatui_color_from_ansi) {
            style = style.bg(background);
        }
        if let Some(foreground) = self.theme.foreground.map(ratatui_color_from_ansi) {
            style = style.fg(foreground);
        }
        style
    }

    /// Get the default inline style (for tests and inline conversions)
    pub(crate) fn default_inline_style(&self) -> InlineTextStyle {
        InlineTextStyle {
            color: self.theme.foreground,
            ..InlineTextStyle::default()
        }
    }

    /// Get the accent inline style
    pub(crate) fn accent_inline_style(&self) -> InlineTextStyle {
        InlineTextStyle {
            color: self.theme.primary.or(self.theme.foreground),
            ..InlineTextStyle::default()
        }
    }

    /// Get the accent style
    pub(crate) fn accent_style(&self) -> Style {
        ratatui_style_from_inline(&self.accent_inline_style(), self.theme.foreground)
    }

    /// Get the warning style (amber token, falling back to the theme foreground).
    ///
    /// Reuses the canonical [`Self::text_fallback`] chain for `Warning` so this
    /// style cannot drift from the semantic warning color.
    pub(crate) fn warning_style(&self) -> Style {
        let color = self.text_fallback(InlineMessageKind::Warning);
        ratatui_style_from_inline(&InlineTextStyle { color, ..InlineTextStyle::default() }, self.theme.foreground)
    }

    pub(crate) fn transcript_link_style(&self) -> Style {
        let style = InlineTextStyle {
            color: self.theme.tool_accent.or(self.theme.primary).or(self.theme.foreground),
            ..InlineTextStyle::default()
        };
        ratatui_style_from_inline(&style, self.theme.foreground)
    }

    /// Get the border inline style
    fn border_inline_style(&self) -> InlineTextStyle {
        InlineTextStyle {
            color: self.theme.secondary.or(self.theme.foreground),
            ..InlineTextStyle::default()
        }
    }

    /// Get the border style (dimmed)
    pub(crate) fn border_style(&self) -> Style {
        self.dimmed_border_style(true)
    }

    /// Muted foreground for secondary text (context labels, subtitles, hints).
    ///
    /// Deliberately an explicit color — theme `secondary` with a `Gray`
    /// fallback, mirroring the `muted` slot of `input_styles_from_theme` —
    /// instead of `Modifier::DIM`: DIM renders as SGR 2, which several
    /// terminals attenuate to near-invisible, and ratatui's `Cell::set_style`
    /// only ever *inserts* modifiers, so a DIM painted as an area background
    /// sticks to every glyph drawn on top of it and mutes otherwise-bright
    /// text. Same no-DIM rule the diff gutter styles follow.
    pub(crate) fn muted_text_style(&self) -> Style {
        let color = self
            .theme
            .secondary
            .or(self.theme.foreground)
            .map(ratatui_color_from_ansi)
            .unwrap_or(Color::Gray);
        self.default_style().fg(color)
    }

    /// Get a border style with configurable boldness.
    /// When `suppress_bold` is true, the BOLD modifier is removed — useful for
    /// subtle block borders that should appear dimmed.
    pub(crate) fn dimmed_border_style(&self, suppress_bold: bool) -> Style {
        let mut style =
            ratatui_style_from_inline(&self.border_inline_style(), self.theme.foreground).add_modifier(Modifier::DIM);
        if suppress_bold {
            style = style.remove_modifier(Modifier::BOLD);
        }
        style
    }

    pub(crate) fn input_background_style(&self) -> Style {
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

    /// Preserve theme foreground contrast while using the composer tint where
    /// it remains readable. Some light themes sit close to the AA floor.
    pub(crate) fn sticky_prompt_style(&self) -> Style {
        let style = self.input_background_style();
        if let (Some(Color::Rgb(fr, fg, fb)), Some(Color::Rgb(br, bg, bb))) = (style.fg, style.bg)
            && crate::theme::contrast_ratio(RgbColor(fr, fg, fb), RgbColor(br, bg, bb)) < ui::THEME_MIN_CONTRAST_RATIO
        {
            return self.default_style();
        }
        style
    }

    /// Get the prefix style for a message line
    pub(crate) fn prefix_style(&self, line: &MessageLine) -> InlineTextStyle {
        let fallback = self.text_fallback(line.kind).or(self.theme.foreground);

        let color = line.segments.iter().find_map(|segment| segment.style.color).or(fallback);

        InlineTextStyle { color, ..InlineTextStyle::default() }
    }

    /// Get the fallback text color for a message kind
    pub(crate) fn text_fallback(&self, kind: InlineMessageKind) -> Option<AnsiColorEnum> {
        match kind {
            // Assistant content should be legible and clearly distinct from subdued PTY output.
            InlineMessageKind::Agent => self.theme.foreground.or(self.theme.primary),
            InlineMessageKind::Policy => self.theme.primary.or(self.theme.foreground),
            InlineMessageKind::User => self.theme.secondary.or(self.theme.foreground),
            InlineMessageKind::Tool => self.theme.primary.or(self.theme.foreground),
            InlineMessageKind::Error => self.theme.error.or(Some(AnsiColor::Red.into())).or(self.theme.foreground),
            InlineMessageKind::Warning => {
                self.theme.warning.or(Some(AnsiColor::Yellow.into())).or(self.theme.foreground)
            }
            InlineMessageKind::Pty => self.theme.pty_body.or(self.theme.tool_body).or(self.theme.foreground),
            InlineMessageKind::Info => self.theme.foreground,
        }
    }

    /// Get the message divider style
    ///
    /// Section dividers (`User` turn breaks and `Agent` synthesis after tool
    /// work) share one quiet prose language: muted `secondary` border hue +
    /// `DIM`, never bold or background. Full-width shape keeps the break
    /// glanceable while the muted tone avoids clutter.
    pub(crate) fn message_divider_style(&self, _kind: InlineMessageKind) -> Style {
        self.dimmed_border_style(true)
    }
}

#[cfg(test)]
mod tests {
    use anstyle::Color as AnsiColorEnum;
    use ratatui::style::Color;

    use super::*;

    #[test]
    fn warning_style_prefers_explicit_theme_warning() {
        let theme = InlineTheme {
            warning: Some(AnsiColorEnum::Rgb(RgbColor(0xAB, 0xCD, 0xEF))),
            foreground: Some(AnsiColorEnum::Rgb(RgbColor(0x11, 0x22, 0x33))),
            ..InlineTheme::default()
        };

        assert_eq!(
            SessionStyles::new(theme).warning_style().fg,
            Some(Color::Rgb(0xAB, 0xCD, 0xEF)),
            "explicit warning token must win"
        );
    }

    #[test]
    fn warning_style_falls_back_to_amber_not_foreground() {
        // With no warning token, the canonical `text_fallback(Warning)` chain
        // supplies amber — not the theme foreground.
        let theme = InlineTheme {
            foreground: Some(AnsiColorEnum::Rgb(RgbColor(0x11, 0x22, 0x33))),
            ..InlineTheme::default()
        };

        assert_eq!(
            SessionStyles::new(theme).warning_style().fg,
            Some(Color::Yellow),
            "missing warning token must fall back to amber"
        );
    }
}
