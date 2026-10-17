//! TUI protocol types and session interface.
//!
//! When the `tui` feature is enabled, this module re-exports the full app-layer
//! protocol from `vtcode-ui`.  When the feature is disabled (headless build),
//! it re-exports the shared data types from `vtcode-commons` and provides
//! lightweight no-op stubs for `InlineHandle`, `InlineSession`, and
//! `InlineEvent`.

// ── Shared data types (always available from vtcode-commons) ────────────────

pub use vtcode_commons::ui_protocol::{
    CompactActivityMetadata, InlineHeaderContext, InlineHeaderHighlight, InlineHeaderStatusBadge,
    InlineHeaderStatusTone, InlineLinkRange, InlineLinkTarget, InlineListItem, InlineListSearchConfig,
    InlineListSelection, InlineMessageKind, InlineSegment, InlineTextStyle, InlineTheme, LayoutModeOverride,
    PlanContent, PlanPhase, PlanStep, ReasoningDisplayMode, RewindAction, SecurePromptConfig, SessionSurface,
    SlashCommandItem, ThinkingBlockState, ToolOutputId, UiMode, WizardModalMode, WizardStep, convert_style,
    theme_from_color_fields,
};

pub use vtcode_commons::ui_protocol::KeyboardProtocolSettings;

// ── Full TUI re-exports (feature = "tui") ───────────────────────────────────

#[cfg(feature = "tui")]
pub use vtcode_ui::tui::app::*;

// ── Headless stubs (feature = "tui" disabled) ───────────────────────────────

#[cfg(not(feature = "tui"))]
mod headless {
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{Arc, Mutex};

    use hashbrown::HashMap;
    use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

    use super::{
        CompactActivityMetadata, InlineListItem, InlineListSearchConfig, InlineListSelection, InlineMessageKind,
        InlineSegment, SecurePromptConfig, ToolOutputId,
    };

    use crate::ui::theme::ThemeStyles;
    pub use vtcode_ui::tui::app::{ContentPart, SubmittedInput};

    /// Headless `InlineEvent` — all variants present so match arms compile.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub enum InlineEvent {
        Submit(SubmittedInput),
        /// Submit text from the WebMCP bridge without interpreting slash commands.
        WebmcpSubmit(SubmittedInput),
        QueueSubmit(SubmittedInput),
        Steer(SubmittedInput),
        ProcessLatestQueued,
        EditQueue,
        Cancel,
        Exit,
        Interrupt,
        Pause,
        Resume,
        BackgroundOperation,
        ScrollLineUp,
        ScrollLineDown,
        ScrollPageUp,
        ScrollPageDown,
        JumpToLastChange,
        OpenFileInEditor(String),
        OpenUrl(String),
        LaunchEditor,
        ForceCancelPtySession,
        RequestInlinePromptSuggestion(String),
        HistoryPrevious,
        HistoryNext,
    }

    /// Minimal command surface used by tests and headless sinks.
    #[derive(Clone, Debug)]
    pub enum InlineCommand {
        AppendLine {
            kind: InlineMessageKind,
            segments: Vec<InlineSegment>,
        },
        AppendPastedMessage {
            kind: InlineMessageKind,
            text: String,
            line_count: usize,
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
        RecordToolOutput {
            id: ToolOutputId,
            lines: Vec<String>,
        },
        AppendToolOutputLine {
            id: ToolOutputId,
            kind: InlineMessageKind,
            segments: Vec<InlineSegment>,
        },
        AppendCompactActivity(CompactActivityMetadata),
        RecordDiffReview(vtcode_commons::ui_protocol::DiffReviewAnchor),
        ReplaceCompactActivity(CompactActivityMetadata),
        CollapsePtyBlock(CompactActivityMetadata),
        SetKeyBindings {
            bindings: HashMap<String, Vec<String>>,
        },
        SetImageInputEnabled(bool),
        RestoreInputDraft(SubmittedInput),
        ForceRedraw,
        Shutdown,
        ClearScreen,
        CloseModal,
        SetReasoningStage(Option<String>),
    }

    /// Headless handle; commands go to an optional test sink and otherwise discard.
    #[derive(Clone, Debug)]
    pub struct InlineHandle {
        sender: Option<UnboundedSender<InlineCommand>>,
        deferred_events: Arc<Mutex<VecDeque<InlineEvent>>>,
        next_tool_output_id: Arc<AtomicU64>,
    }

    const MAX_DEFERRED_EVENTS: usize = 32;

    impl InlineHandle {
        pub fn new_for_tests(sender: UnboundedSender<InlineCommand>) -> Self {
            Self {
                sender: Some(sender),
                deferred_events: Arc::new(Mutex::new(VecDeque::new())),
                next_tool_output_id: Arc::new(AtomicU64::new(0)),
            }
        }

        pub fn defer_event(&self, event: InlineEvent) -> anyhow::Result<()> {
            let mut deferred_events = self
                .deferred_events
                .lock()
                .map_err(|error| anyhow::anyhow!("deferred input queue poisoned: {error}"))?;
            if deferred_events.len() >= MAX_DEFERRED_EVENTS {
                return Err(anyhow::anyhow!("deferred input queue is full"));
            }
            deferred_events.push_back(event);
            Ok(())
        }

        fn take_deferred_event(&self) -> Option<InlineEvent> {
            self.deferred_events.lock().ok()?.pop_front()
        }

        pub fn has_deferred_event(&self) -> bool {
            self.deferred_events.lock().is_ok_and(|events| !events.is_empty())
        }

        fn send_command(&self, command: InlineCommand) {
            if let Some(sender) = &self.sender {
                let _ = sender.send(command);
            }
        }

        pub fn append_line(&self, kind: InlineMessageKind, segments: Vec<InlineSegment>) {
            self.send_command(InlineCommand::AppendLine { kind, segments });
        }
        pub fn append_pasted_message(&self, kind: InlineMessageKind, text: String, line_count: usize) {
            self.send_command(InlineCommand::AppendPastedMessage { kind, text, line_count });
        }
        pub fn inline(&self, kind: InlineMessageKind, segment: InlineSegment) {
            self.send_command(InlineCommand::Inline { kind, segment });
        }
        pub fn replace_last(&self, count: usize, kind: InlineMessageKind, lines: Vec<Vec<InlineSegment>>) {
            self.send_command(InlineCommand::ReplaceLast { count, kind, lines });
        }
        pub fn record_tool_output(&self, lines: Vec<String>) -> ToolOutputId {
            let id = self.next_tool_output_id.fetch_add(1, Ordering::Relaxed);
            self.send_command(InlineCommand::RecordToolOutput { id, lines });
            id
        }
        pub fn append_tool_output_line(&self, id: ToolOutputId, kind: InlineMessageKind, segments: Vec<InlineSegment>) {
            self.send_command(InlineCommand::AppendToolOutputLine { id, kind, segments });
        }
        pub fn append_compact_activity(&self, activity: CompactActivityMetadata) {
            self.send_command(InlineCommand::AppendCompactActivity(activity));
        }
        pub fn record_diff_review(&self, anchor: vtcode_commons::ui_protocol::DiffReviewAnchor) {
            // Headless sinks have no transcript expand affordance, but tests
            // still need to observe that an anchor was recorded.
            self.send_command(InlineCommand::RecordDiffReview(anchor));
        }
        pub fn replace_compact_activity(&self, activity: CompactActivityMetadata) {
            self.send_command(InlineCommand::ReplaceCompactActivity(activity));
        }
        pub fn collapse_pty_block(&self, activity: CompactActivityMetadata) {
            self.send_command(InlineCommand::CollapsePtyBlock(activity));
        }
        pub fn set_key_bindings(&self, bindings: HashMap<String, Vec<String>>) {
            self.send_command(InlineCommand::SetKeyBindings { bindings });
        }
        pub fn force_redraw(&self) {
            self.send_command(InlineCommand::ForceRedraw);
        }
        pub fn shutdown(&self) {
            self.send_command(InlineCommand::Shutdown);
        }
        pub fn clear_screen(&self) {
            self.send_command(InlineCommand::ClearScreen);
        }
        pub fn show_modal(&self, _title: String, _lines: Vec<String>, _secure_prompt: Option<SecurePromptConfig>) {}
        pub fn show_list_modal(
            &self,
            _title: String,
            _lines: Vec<String>,
            _items: Vec<InlineListItem>,
            _selected: Option<InlineListSelection>,
            _search: Option<InlineListSearchConfig>,
        ) {
        }
        pub fn show_list_modal_with_footer(
            &self,
            _title: String,
            _lines: Vec<String>,
            _items: Vec<InlineListItem>,
            _selected: Option<InlineListSelection>,
            _search: Option<InlineListSearchConfig>,
            _footer_hint: Option<String>,
        ) {
        }
        pub fn show_list_modal_with_status(
            &self,
            _title: String,
            _lines: Vec<String>,
            _items: Vec<InlineListItem>,
            _selected: Option<InlineListSelection>,
            _search: Option<InlineListSearchConfig>,
            _footer_hint: Option<String>,
            _status: Option<vtcode_commons::ui_protocol::InlineStatus>,
        ) {
        }
        pub fn close_modal(&self) {
            self.send_command(InlineCommand::CloseModal);
        }
        pub fn restore_input_draft(&self, input: SubmittedInput) {
            self.send_command(InlineCommand::RestoreInputDraft(input));
        }
        pub fn set_image_input_enabled(&self, enabled: bool) {
            self.send_command(InlineCommand::SetImageInputEnabled(enabled));
        }
        pub fn set_reasoning_stage(&self, stage: Option<String>) {
            self.send_command(InlineCommand::SetReasoningStage(stage));
        }
    }

    /// Headless session — events never arrive.
    pub struct InlineSession {
        pub handle: InlineHandle,
        pub events: UnboundedReceiver<InlineEvent>,
    }

    impl InlineSession {
        pub async fn next_event(&mut self) -> Option<InlineEvent> {
            if let Some(event) = self.handle.take_deferred_event() {
                return Some(event);
            }
            self.events.recv().await
        }

        /// Headless stub: there is no TUI task to await, so shutdown is
        /// always considered complete immediately.
        pub async fn wait_for_exit(&mut self, _timeout: std::time::Duration) -> bool {
            true
        }

        pub fn clone_inline_handle(&self) -> InlineHandle {
            self.handle.clone()
        }
    }

    /// Headless appearance config with sensible defaults.
    #[derive(Debug, Clone, Default)]
    pub struct SessionAppearanceConfig {
        pub theme: String,
        pub ui_mode: super::UiMode,
        pub show_sidebar: bool,
        pub min_content_width: u16,
        pub min_navigation_width: u16,
        pub navigation_width_percent: u8,
        /// Bottom padding rows under the transcript.
        ///
        /// Retained for config compatibility only; the live transcript reads
        /// `vtcode_ui::tui::config::constants::ui::INLINE_TRANSCRIPT_BOTTOM_PADDING`,
        /// so a non-zero value set here has no render effect.
        pub transcript_bottom_padding: u16,
        pub dim_completed_todos: bool,
        pub message_block_spacing: u8,
        pub layout_mode: super::LayoutModeOverride,
        pub reasoning_display_mode: super::ReasoningDisplayMode,
        pub reasoning_visible_default: bool,
        pub thinking_display: super::ThinkingBlockState,
        pub vim_mode: bool,
        pub screen_reader_mode: bool,
        pub reduce_motion_mode: bool,
        pub reduce_motion_keep_progress_animation: bool,
        pub show_transcript_review_hints: bool,
        pub show_transcript_review_shortcut_guide: bool,
        pub show_transcript_review_close_button: bool,
        pub diff_preview_mode: vtcode_commons::ui_protocol::DiffPreviewMode,
        pub customization: (),
    }

    /// Build an [`InlineTheme`](super::InlineTheme) from core theme styles.
    pub fn theme_from_styles(styles: &ThemeStyles) -> super::InlineTheme {
        super::theme_from_color_fields(
            styles.foreground,
            styles.background,
            styles.primary,
            styles.secondary,
            styles.tool,
            styles.tool_detail,
            styles.pty_output,
            styles.error,
            styles.warning,
        )
    }
}

#[cfg(not(feature = "tui"))]
pub use headless::*;
