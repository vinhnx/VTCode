use anstyle::{Color as AnsiColorEnum, Effects, Style as AnsiStyle};

use crate::config::constants::ui;

/// Editing mode for the agent session
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EditingMode {
    /// Full tool access - can edit files and run commands
    #[default]
    Edit,
    /// Read-only mode - produces implementation plans without executing
    Plan,
}

impl EditingMode {
    /// Cycle to the next mode: Edit -> Plan -> Edit
    pub fn next(self) -> Self {
        match self {
            Self::Edit => Self::Plan,
            Self::Plan => Self::Edit,
        }
    }

    /// Get display name for the mode
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Edit => "Edit",
            Self::Plan => "Plan",
        }
    }
}

#[derive(Clone, Debug)]
pub struct InlineHeaderContext {
    pub app_name: String,
    pub provider: String,
    pub model: String,
    pub version: String,
    pub git: String,
    pub mode: String,
    pub reasoning: String,
    pub reasoning_stage: Option<String>,
    pub workspace_trust: String,
    pub tools: String,
    pub mcp: String,
    pub highlights: Vec<InlineHeaderHighlight>,
    /// Current editing mode for display in header
    pub editing_mode: EditingMode,
    /// Current autonomous mode status
    pub autonomous_mode: bool,
}

impl Default for InlineHeaderContext {
    fn default() -> Self {
        let version = env!("CARGO_PKG_VERSION").to_string();
        let git = format!(
            "{}{}",
            ui::HEADER_GIT_PREFIX,
            ui::HEADER_UNKNOWN_PLACEHOLDER
        );
        let reasoning = format!(
            "{}{}",
            ui::HEADER_REASONING_PREFIX,
            ui::HEADER_UNKNOWN_PLACEHOLDER
        );
        let trust = format!(
            "{}{}",
            ui::HEADER_TRUST_PREFIX,
            ui::HEADER_UNKNOWN_PLACEHOLDER
        );
        let tools = format!(
            "{}{}",
            ui::HEADER_TOOLS_PREFIX,
            ui::HEADER_UNKNOWN_PLACEHOLDER
        );
        let mcp = format!(
            "{}{}",
            ui::HEADER_MCP_PREFIX,
            ui::HEADER_UNKNOWN_PLACEHOLDER
        );

        Self {
            app_name: ui::HEADER_VERSION_PREFIX.to_string(),
            provider: format!(
                "{}{}",
                ui::HEADER_PROVIDER_PREFIX,
                ui::HEADER_UNKNOWN_PLACEHOLDER
            ),
            model: format!(
                "{}{}",
                ui::HEADER_MODEL_PREFIX,
                ui::HEADER_UNKNOWN_PLACEHOLDER
            ),
            version,
            git,
            mode: ui::HEADER_MODE_INLINE.to_string(),
            reasoning,
            reasoning_stage: None,
            workspace_trust: trust,
            tools,
            mcp,
            highlights: Vec::new(),
            editing_mode: EditingMode::default(),
            autonomous_mode: false,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct InlineHeaderHighlight {
    pub title: String,
    pub lines: Vec<String>,
}

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
        // Apply effects
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

#[derive(Clone, Debug, Default)]
pub struct InlineSegment {
    pub text: String,
    pub style: std::sync::Arc<InlineTextStyle>,
}

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
