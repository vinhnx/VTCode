//! Unified message styles and their logical mappings

use crate::ui::theme;
use crate::ui::tui::InlineMessageKind;
use anstyle::Style;

/// Styles available for rendering messages
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MessageStyle {
    Info,
    Error,
    Output,
    Response,
    Tool,
    ToolDetail,
    ToolOutput,
    ToolError,
    Status,
    McpStatus,
    User,
    Reasoning,
    Warning,
}

impl MessageStyle {
    /// Get the ANSI style for this message style
    pub fn style(self) -> Style {
        let styles = theme::active_styles();
        match self {
            Self::Info => styles.info,
            Self::Error => styles.error,
            Self::Output => styles.output,
            Self::Response => styles.response,
            Self::Tool => styles.tool,
            Self::ToolDetail => styles.tool_detail,
            Self::ToolOutput => styles.tool_output,
            Self::ToolError => styles.error,
            Self::Status => styles.status,
            Self::McpStatus => styles.mcp,
            Self::User => styles.user,
            Self::Reasoning => styles.reasoning,
            Self::Warning => styles.error,
        }
    }

    /// Get the indentation string for this message style
    pub fn indent(self) -> &'static str {
        match self {
            Self::Response | Self::Tool | Self::Reasoning => "  ",
            Self::ToolDetail | Self::ToolOutput | Self::ToolError => "    ",
            _ => "",
        }
    }

    /// Map MessageStyle to InlineMessageKind for TUI integration
    pub fn message_kind(self) -> InlineMessageKind {
        match self {
            Self::Info => InlineMessageKind::Info,
            Self::Error => InlineMessageKind::Error,
            Self::Output | Self::ToolOutput => InlineMessageKind::Pty,
            Self::Response => InlineMessageKind::Agent,
            Self::Tool | Self::ToolDetail | Self::ToolError => InlineMessageKind::Tool,
            Self::Status | Self::McpStatus => InlineMessageKind::Info,
            Self::User => InlineMessageKind::User,
            Self::Reasoning => InlineMessageKind::Policy,
            Self::Warning => InlineMessageKind::Warning,
        }
    }
}
