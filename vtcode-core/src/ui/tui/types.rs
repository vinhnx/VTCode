use anstyle::{Color as AnsiColorEnum, Style as AnsiStyle};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

use crate::config::{constants::ui, types::ReasoningEffortLevel};

#[derive(Clone)]
pub struct InlineHeaderContext {
    pub provider: String,
    pub model: String,
    pub version: String,
    pub mode: String,
    pub reasoning: String,
    pub workspace_trust: String,
    pub tools: String,
    pub mcp: String,
    pub highlights: Vec<InlineHeaderHighlight>,
}

impl Default for InlineHeaderContext {
    fn default() -> Self {
        let version = env!("CARGO_PKG_VERSION").to_string();
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
            mode: ui::HEADER_MODE_INLINE.to_string(),
            reasoning,
            workspace_trust: trust,
            tools,
            mcp,
            highlights: Vec::new(),
        }
    }
}

#[derive(Clone, Default)]
pub struct InlineHeaderHighlight {
    pub title: String,
    pub lines: Vec<String>,
}

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
    pub tool_accent: Option<AnsiColorEnum>,
    pub tool_body: Option<AnsiColorEnum>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InlineListSelection {
    Model(usize),
    DynamicModel(usize),
    RefreshDynamicModels,
    Reasoning(ReasoningEffortLevel),
    DisableReasoning,
    CustomModel,
    Theme(String),
    Session(String),
    SlashCommand(String),
    ToolApproval(bool),
    ToolApprovalSession,
    ToolApprovalPermanent,
}

#[derive(Clone, Debug)]
pub struct InlineListItem {
    pub title: String,
    pub subtitle: Option<String>,
    pub badge: Option<String>,
    pub indent: u8,
    pub selection: Option<InlineListSelection>,
    pub search_value: Option<String>,
}

#[derive(Clone)]
pub struct InlineListSearchConfig {
    pub label: String,
    pub placeholder: Option<String>,
}

#[derive(Clone, Debug)]
pub struct SecurePromptConfig {
    pub label: String,
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
    SetHeaderContext {
        context: InlineHeaderContext,
    },
    SetInputStatus {
        left: Option<String>,
        right: Option<String>,
    },
    SetTheme {
        theme: InlineTheme,
    },
    SetQueuedInputs {
        entries: Vec<String>,
    },
    SetCursorVisible(bool),
    SetInputEnabled(bool),
    SetInput(String),
    ClearInput,
    ForceRedraw,
    ShowModal {
        title: String,
        lines: Vec<String>,
        secure_prompt: Option<SecurePromptConfig>,
    },
    ShowListModal {
        title: String,
        lines: Vec<String>,
        items: Vec<InlineListItem>,
        selected: Option<InlineListSelection>,
        search: Option<InlineListSearchConfig>,
    },
    CloseModal,
    SetCustomPrompts {
        registry: crate::prompts::CustomPromptRegistry,
    },
    LoadFilePalette {
        files: Vec<String>,
        workspace: std::path::PathBuf,
    },
    Shutdown,
}

#[derive(Debug, Clone)]
pub enum InlineEvent {
    Submit(String),
    QueueSubmit(String),
    ListModalSubmit(InlineListSelection),
    ListModalCancel,
    Cancel,
    Exit,
    Interrupt,
    ScrollLineUp,
    ScrollLineDown,
    ScrollPageUp,
    ScrollPageDown,
    FileSelected(String),
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

    pub fn set_header_context(&self, context: InlineHeaderContext) {
        let _ = self
            .sender
            .send(InlineCommand::SetHeaderContext { context });
    }

    pub fn set_input_status(&self, left: Option<String>, right: Option<String>) {
        let _ = self
            .sender
            .send(InlineCommand::SetInputStatus { left, right });
    }

    pub fn set_theme(&self, theme: InlineTheme) {
        let _ = self.sender.send(InlineCommand::SetTheme { theme });
    }

    pub fn set_queued_inputs(&self, entries: Vec<String>) {
        let _ = self.sender.send(InlineCommand::SetQueuedInputs { entries });
    }

    pub fn set_cursor_visible(&self, visible: bool) {
        let _ = self.sender.send(InlineCommand::SetCursorVisible(visible));
    }

    pub fn set_input_enabled(&self, enabled: bool) {
        let _ = self.sender.send(InlineCommand::SetInputEnabled(enabled));
    }

    pub fn set_input(&self, content: String) {
        let _ = self.sender.send(InlineCommand::SetInput(content));
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

    pub fn show_modal(
        &self,
        title: String,
        lines: Vec<String>,
        secure_prompt: Option<SecurePromptConfig>,
    ) {
        let _ = self.sender.send(InlineCommand::ShowModal {
            title,
            lines,
            secure_prompt,
        });
    }

    pub fn show_list_modal(
        &self,
        title: String,
        lines: Vec<String>,
        items: Vec<InlineListItem>,
        selected: Option<InlineListSelection>,
        search: Option<InlineListSearchConfig>,
    ) {
        let _ = self.sender.send(InlineCommand::ShowListModal {
            title,
            lines,
            items,
            selected,
            search,
        });
    }

    pub fn close_modal(&self) {
        let _ = self.sender.send(InlineCommand::CloseModal);
    }

    pub fn set_custom_prompts(&self, registry: crate::prompts::CustomPromptRegistry) {
        let _ = self
            .sender
            .send(InlineCommand::SetCustomPrompts { registry });
    }

    pub fn load_file_palette(&self, files: Vec<String>, workspace: std::path::PathBuf) {
        let _ = self
            .sender
            .send(InlineCommand::LoadFilePalette { files, workspace });
    }
}

pub struct InlineSession {
    pub handle: InlineHandle,
    pub events: UnboundedReceiver<InlineEvent>,
}

impl InlineSession {
    pub fn clone_inline_handle(&self) -> InlineHandle {
        InlineHandle {
            sender: self.handle.sender.clone(),
        }
    }
}
