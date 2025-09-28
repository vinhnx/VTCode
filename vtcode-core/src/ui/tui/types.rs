use anstyle::{Color as AnsiColorEnum, Style as AnsiStyle};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

#[derive(Clone, Default, PartialEq)]
pub struct InlineTextStyle {
    pub color: Option<AnsiColorEnum>,
    pub bold: bool,
    pub italic: bool,
}

impl InlineTextStyle {
    #[must_use]
    pub fn merge_color(mut self, fallback: Option<AnsiColorEnum>) -> Self {
        if self.color.is_none() {
            self.color = fallback;
        }
        self
    }

    #[must_use]
    pub fn to_ansi_style(&self, fallback: Option<AnsiColorEnum>) -> AnsiStyle {
        let mut style = AnsiStyle::new();
        if let Some(color) = self.color.or(fallback) {
            style = style.fg_color(Some(color));
        }
        if self.bold {
            style = style.bold();
        }
        if self.italic {
            style = style.italic();
        }
        style
    }
}

#[derive(Clone, Default)]
pub struct InlineSegment {
    pub text: String,
    pub style: InlineTextStyle,
}

#[derive(Clone, Default)]
pub struct InlineTheme {
    pub foreground: Option<AnsiColorEnum>,
    pub primary: Option<AnsiColorEnum>,
    pub secondary: Option<AnsiColorEnum>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InlineMessageKind {
    Agent,
    Error,
    Info,
    Policy,
    Pty,
    Tool,
    User,
}

pub enum InlineCommand {
    AppendLine {
        kind: InlineMessageKind,
        segments: Vec<InlineSegment>,
    },
    Inline {
        kind: InlineMessageKind,
        segment: InlineSegment,
    },
    ReplaceLast {
        count: usize,
        kind: InlineMessageKind,
        lines: Vec<Vec<InlineSegment>>,
    },
    SetPrompt {
        prefix: String,
        style: InlineTextStyle,
    },
    SetPlaceholder {
        hint: Option<String>,
        style: Option<InlineTextStyle>,
    },
    SetMessageLabels {
        agent: Option<String>,
        user: Option<String>,
    },
    SetTheme {
        theme: InlineTheme,
    },
    SetCursorVisible(bool),
    SetInputEnabled(bool),
    ClearInput,
    ForceRedraw,
    Shutdown,
}

#[derive(Debug, Clone)]
pub enum InlineEvent {
    Submit(String),
    Cancel,
    Exit,
    Interrupt,
    ScrollLineUp,
    ScrollLineDown,
    ScrollPageUp,
    ScrollPageDown,
}

#[derive(Clone)]
pub struct InlineHandle {
    pub(crate) sender: UnboundedSender<InlineCommand>,
}

impl InlineHandle {
    pub fn append_line(&self, kind: InlineMessageKind, segments: Vec<InlineSegment>) {
        let segments = if segments.is_empty() {
            vec![InlineSegment::default()]
        } else {
            segments
        };
        let _ = self
            .sender
            .send(InlineCommand::AppendLine { kind, segments });
    }

    pub fn inline(&self, kind: InlineMessageKind, segment: InlineSegment) {
        let _ = self.sender.send(InlineCommand::Inline { kind, segment });
    }

    pub fn replace_last(
        &self,
        count: usize,
        kind: InlineMessageKind,
        lines: Vec<Vec<InlineSegment>>,
    ) {
        let _ = self
            .sender
            .send(InlineCommand::ReplaceLast { count, kind, lines });
    }

    pub fn set_prompt(&self, prefix: String, style: InlineTextStyle) {
        let _ = self.sender.send(InlineCommand::SetPrompt { prefix, style });
    }

    pub fn set_placeholder(&self, hint: Option<String>) {
        self.set_placeholder_with_style(hint, None);
    }

    pub fn set_placeholder_with_style(&self, hint: Option<String>, style: Option<InlineTextStyle>) {
        let _ = self
            .sender
            .send(InlineCommand::SetPlaceholder { hint, style });
    }

    pub fn set_message_labels(&self, agent: Option<String>, user: Option<String>) {
        let _ = self
            .sender
            .send(InlineCommand::SetMessageLabels { agent, user });
    }

    pub fn set_theme(&self, theme: InlineTheme) {
        let _ = self.sender.send(InlineCommand::SetTheme { theme });
    }

    pub fn set_cursor_visible(&self, visible: bool) {
        let _ = self.sender.send(InlineCommand::SetCursorVisible(visible));
    }

    pub fn set_input_enabled(&self, enabled: bool) {
        let _ = self.sender.send(InlineCommand::SetInputEnabled(enabled));
    }

    pub fn clear_input(&self) {
        let _ = self.sender.send(InlineCommand::ClearInput);
    }

    pub fn force_redraw(&self) {
        let _ = self.sender.send(InlineCommand::ForceRedraw);
    }

    pub fn shutdown(&self) {
        let _ = self.sender.send(InlineCommand::Shutdown);
    }
}

pub struct InlineSession {
    pub handle: InlineHandle,
    pub events: UnboundedReceiver<InlineEvent>,
}
