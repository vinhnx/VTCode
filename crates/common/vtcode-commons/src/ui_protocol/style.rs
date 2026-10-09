//! Style and theming types that depend on `anstyle`.

use std::sync::Arc;

use anstyle::{Color as AnsiColorEnum, Effects, Style as AnsiStyle};

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
    pub error: Option<AnsiColorEnum>,
    pub warning: Option<AnsiColorEnum>,
}

// ---------------------------------------------------------------------------
// List / modal presentation tones
// ---------------------------------------------------------------------------

/// Semantic tone for list badges, values, and modal status strips.
///
/// Renderers map each tone onto theme styles; callers pick the tone, never a
/// raw color, so every surface stays theme-consistent and WCAG-checked.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum InlineTone {
    #[default]
    Neutral,
    Accent,
    Success,
    Warning,
    Danger,
    /// Live/current selection marker (the active model, the current value).
    Current,
}

/// A short-lived status message shown inside a list modal (footer strip).
///
/// One status is kept at a time; a new action overwrites the previous status.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InlineStatus {
    pub tone: InlineTone,
    pub message: String,
}

impl InlineStatus {
    #[must_use]
    pub fn new(tone: InlineTone, message: impl Into<String>) -> Self {
        Self { tone, message: message.into() }
    }

    #[must_use]
    pub fn success(message: impl Into<String>) -> Self {
        Self::new(InlineTone::Success, message)
    }

    #[must_use]
    pub fn warning(message: impl Into<String>) -> Self {
        Self::new(InlineTone::Warning, message)
    }

    #[must_use]
    pub fn error(message: impl Into<String>) -> Self {
        Self::new(InlineTone::Danger, message)
    }

    #[must_use]
    pub fn info(message: impl Into<String>) -> Self {
        Self::new(InlineTone::Accent, message)
    }
}

// ---------------------------------------------------------------------------
// Header context types
// ---------------------------------------------------------------------------

/// Status-badge tone used in header status indicators.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
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
    pub git: String,
    pub reasoning: String,
    pub reasoning_stage: Option<String>,
    /// Configured native OpenAI `service_tier` (`Tier: <name>`), when set.
    /// Rendered in the header summary only when present; `None` hides it.
    pub service_tier: Option<String>,
    pub workspace_trust: String,
    pub tools: String,
    pub mcp: String,
    pub primary_agent: Option<String>,
    pub primary_agent_color: Option<String>,
    pub highlights: Vec<InlineHeaderHighlight>,
    pub subagent_badges: Vec<InlineHeaderBadge>,
}

impl Default for InlineHeaderContext {
    fn default() -> Self {
        let version = env!("CARGO_PKG_VERSION").to_string();
        Self {
            // Keep in sync with `vtcode-config::constants::app::DISPLAY_NAME`.
            // `vtcode-commons` cannot depend on `vtcode-config` (config depends
            // on commons), so the product name is duplicated here. Covered by
            // the `header_placeholder_app_name_matches_product` ratchet in
            // `vtcode-ui`.
            app_name: "VT Code".to_string(),
            provider: "Provider: unavailable".to_string(),
            model: "Model: unavailable".to_string(),
            context_window_size: None,
            version,
            search_tools: None,
            persistent_memory: None,
            pr_review: None,
            git: "git: unavailable".to_string(),
            reasoning: "unavailable".to_string(),
            reasoning_stage: None,
            service_tier: None,
            workspace_trust: "Trust: unavailable".to_string(),
            tools: "Tools: unavailable".to_string(),
            mcp: "MCP: unavailable".to_string(),
            primary_agent: None,
            primary_agent_color: None,
            highlights: Vec::new(),
            subagent_badges: Vec::new(),
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
    error: AnsiStyle,
    warning: AnsiStyle,
) -> InlineTheme {
    InlineTheme {
        foreground: convert_ansi_color(foreground),
        background: convert_ansi_color(background),
        primary: convert_style_color(&primary),
        secondary: convert_style_color(&secondary),
        tool_accent: convert_style_color(&tool),
        tool_body: convert_style_color(&tool_detail),
        pty_body: convert_style_color(&pty_output),
        error: convert_style_color(&error),
        warning: convert_style_color(&warning),
    }
}
