//! Style and theming types that depend on `anstyle`.

use std::sync::Arc;

use anstyle::{Color as AnsiColorEnum, Effects, Style as AnsiStyle};

use crate::ui_protocol::types::EditingMode;

/// Inline text styling with foreground/background color and text effects.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct InlineTextStyle {
    pub color: Option<AnsiColorEnum>,
    pub bg_color: Option<AnsiColorEnum>,
    pub effects: Effects,
}

impl InlineTextStyle {
    #[must_use]
    pub fn with_color(mut self, color: Option<AnsiColorEnum>) -> Self {
        self.color = color;
        self
    }

    #[must_use]
    pub fn with_bg_color(mut self, color: Option<AnsiColorEnum>) -> Self {
        self.bg_color = color;
        self
    }

    #[must_use]
    pub fn merge_color(mut self, fallback: Option<AnsiColorEnum>) -> Self {
        if self.color.is_none() {
            self.color = fallback;
        }
        self
    }

    #[must_use]
    pub fn merge_bg_color(mut self, fallback: Option<AnsiColorEnum>) -> Self {
        if self.bg_color.is_none() {
            self.bg_color = fallback;
        }
        self
    }

    #[must_use]
    pub fn bold(mut self) -> Self {
        self.effects |= Effects::BOLD;
        self
    }

    #[must_use]
    pub fn italic(mut self) -> Self {
        self.effects |= Effects::ITALIC;
        self
    }

    #[must_use]
    pub fn underline(mut self) -> Self {
        self.effects |= Effects::UNDERLINE;
        self
    }

    #[must_use]
    pub fn dim(mut self) -> Self {
        self.effects |= Effects::DIMMED;
        self
    }

    #[must_use]
    pub fn to_ansi_style(&self, fallback: Option<AnsiColorEnum>) -> AnsiStyle {
        let mut style = AnsiStyle::new();
        if let Some(color) = self.color.or(fallback) {
            style = style.fg_color(Some(color));
        }
        if let Some(bg) = self.bg_color {
            style = style.bg_color(Some(bg));
        }
        if self.effects.contains(Effects::BOLD) {
            style = style.bold();
        }
        if self.effects.contains(Effects::ITALIC) {
            style = style.italic();
        }
        if self.effects.contains(Effects::UNDERLINE) {
            style = style.underline();
        }
        if self.effects.contains(Effects::DIMMED) {
            style = style.dimmed();
        }
        style
    }
}

/// A styled text segment with shared style.
#[derive(Clone, Debug, Default)]
pub struct InlineSegment {
    pub text: String,
    pub style: Arc<InlineTextStyle>,
}

/// A clickable link target inside a transcript line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InlineLinkTarget {
    Url(String),
}

/// Byte-range inside a line that is a clickable link.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InlineLinkRange {
    pub start: usize,
    pub end: usize,
    pub target: InlineLinkTarget,
}

/// Resolved theme colors for inline rendering.
#[derive(Clone, Debug, Default)]
pub struct InlineTheme {
    pub foreground: Option<AnsiColorEnum>,
    pub background: Option<AnsiColorEnum>,
    pub primary: Option<AnsiColorEnum>,
    pub secondary: Option<AnsiColorEnum>,
    pub tool_accent: Option<AnsiColorEnum>,
    pub tool_body: Option<AnsiColorEnum>,
    pub pty_body: Option<AnsiColorEnum>,
}

// ---------------------------------------------------------------------------
// Header context types
// ---------------------------------------------------------------------------

/// Status-badge tone used in header status indicators.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum InlineHeaderStatusTone {
    #[default]
    Ready,
    Warning,
    Error,
}

/// A labelled status badge for the header bar.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InlineHeaderStatusBadge {
    pub text: String,
    pub tone: InlineHeaderStatusTone,
}

/// A compact pill badge rendered in the header.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct InlineHeaderBadge {
    pub text: String,
    pub style: InlineTextStyle,
    pub full_background: bool,
}

/// A title + content highlight block in the header.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InlineHeaderHighlight {
    pub title: String,
    pub lines: Vec<String>,
}

/// Session metadata displayed in the inline header.
#[derive(Clone, Debug)]
pub struct InlineHeaderContext {
    pub app_name: String,
    pub provider: String,
    pub model: String,
    pub context_window_size: Option<usize>,
    pub version: String,
    pub search_tools: Option<InlineHeaderStatusBadge>,
    pub persistent_memory: Option<InlineHeaderStatusBadge>,
    pub pr_review: Option<InlineHeaderStatusBadge>,
    pub editor_context: Option<String>,
    pub git: String,
    pub mode: String,
    pub reasoning: String,
    pub reasoning_stage: Option<String>,
    pub workspace_trust: String,
    pub tools: String,
    pub mcp: String,
    pub highlights: Vec<InlineHeaderHighlight>,
    pub subagent_badges: Vec<InlineHeaderBadge>,
    /// Current editing mode for display in header.
    pub editing_mode: EditingMode,
    /// Current autonomous mode status.
    pub autonomous_mode: bool,
}

impl Default for InlineHeaderContext {
    fn default() -> Self {
        let version = env!("CARGO_PKG_VERSION").to_string();
        Self {
            app_name: "App".to_string(),
            provider: "Provider: unavailable".to_string(),
            model: "Model: unavailable".to_string(),
            context_window_size: None,
            version,
            search_tools: None,
            persistent_memory: None,
            pr_review: None,
            editor_context: None,
            git: "git: unavailable".to_string(),
            mode: "Inline session".to_string(),
            reasoning: "Reasoning effort: unavailable".to_string(),
            reasoning_stage: None,
            workspace_trust: "Trust: unavailable".to_string(),
            tools: "Tools: unavailable".to_string(),
            mcp: "MCP: unavailable".to_string(),
            highlights: Vec::new(),
            subagent_badges: Vec::new(),
            editing_mode: EditingMode::default(),
            autonomous_mode: false,
        }
    }
}

// ---------------------------------------------------------------------------
// Conversion helpers
// ---------------------------------------------------------------------------

fn convert_ansi_color(color: AnsiColorEnum) -> Option<AnsiColorEnum> {
    Some(match color {
        AnsiColorEnum::Ansi(ansi) => AnsiColorEnum::Ansi(ansi),
        AnsiColorEnum::Ansi256(value) => AnsiColorEnum::Ansi256(value),
        AnsiColorEnum::Rgb(rgb) => AnsiColorEnum::Rgb(rgb),
    })
}

fn convert_style_color(style: &AnsiStyle) -> Option<AnsiColorEnum> {
    style.get_fg_color().and_then(convert_ansi_color)
}

fn convert_style_bg_color(style: &AnsiStyle) -> Option<AnsiColorEnum> {
    style.get_bg_color().and_then(convert_ansi_color)
}

/// Convert an `anstyle::Style` to an [`InlineTextStyle`].
pub fn convert_style(style: AnsiStyle) -> InlineTextStyle {
    InlineTextStyle {
        color: convert_style_color(&style),
        bg_color: convert_style_bg_color(&style),
        effects: style.get_effects(),
    }
}

/// Build an [`InlineTheme`] from individual theme colour fields.
pub fn theme_from_color_fields(
    foreground: AnsiColorEnum,
    background: AnsiColorEnum,
    primary: AnsiStyle,
    secondary: AnsiStyle,
    tool: AnsiStyle,
    tool_detail: AnsiStyle,
    pty_output: AnsiStyle,
) -> InlineTheme {
    InlineTheme {
        foreground: convert_ansi_color(foreground),
        background: convert_ansi_color(background),
        primary: convert_style_color(&primary),
        secondary: convert_style_color(&secondary),
        tool_accent: convert_style_color(&tool),
        tool_body: convert_style_color(&tool_detail),
        pty_body: convert_style_color(&pty_output),
    }
}
