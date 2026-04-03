//! TUI protocol types and session interface.
//!
//! When the `tui` feature is enabled, this module re-exports the full app-layer
//! protocol from `vtcode-tui`.  When the feature is disabled (headless build),
//! it re-exports the shared data types from `vtcode-commons` and provides
//! lightweight no-op stubs for `InlineHandle`, `InlineSession`, and
//! `InlineEvent`.

// ── Shared data types (always available from vtcode-commons) ────────────────

pub use vtcode_commons::ui_protocol::{
    EditingMode, InlineHeaderContext, InlineHeaderHighlight, InlineHeaderStatusBadge,
    InlineHeaderStatusTone, InlineLinkRange, InlineLinkTarget, InlineListItem,
    InlineListSearchConfig, InlineListSelection, InlineMessageKind, InlineSegment, InlineTextStyle,
    InlineTheme, LayoutModeOverride, PlanContent, PlanPhase, PlanStep, ReasoningDisplayMode,
    RewindAction, SecurePromptConfig, SessionSurface, SlashCommandItem, UiMode, WizardModalMode,
    WizardStep, convert_style, theme_from_color_fields,
};

pub use vtcode_commons::ui_protocol::KeyboardProtocolSettings;

// ── Full TUI re-exports (feature = "tui") ───────────────────────────────────

#[cfg(feature = "tui")]
pub use vtcode_tui::app::*;

// ── Headless stubs (feature = "tui" disabled) ───────────────────────────────

#[cfg(not(feature = "tui"))]
mod headless {
    use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

    use super::{
        InlineListItem, InlineListSearchConfig, InlineListSelection, InlineMessageKind,
        InlineSegment, SecurePromptConfig,
    };

    use crate::ui::theme::ThemeStyles;

    /// Headless `InlineEvent` — all variants present so match arms compile.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub enum InlineEvent {
        Submit(String),
        QueueSubmit(String),
        Steer(String),
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
        OpenFileInEditor(String),
        OpenUrl(String),
        LaunchEditor,
        ForceCancelPtySession,
        RequestInlinePromptSuggestion(String),
        ToggleMode,
        HistoryPrevious,
        HistoryNext,
    }

    /// No-op handle for headless builds. All methods silently discard.
    #[derive(Clone, Debug)]
    pub struct InlineHandle;

    impl InlineHandle {
        pub fn new_for_tests(_sender: UnboundedSender<()>) -> Self {
            Self
        }

        pub fn append_line(&self, _kind: InlineMessageKind, _segments: Vec<InlineSegment>) {}
        pub fn append_pasted_message(
            &self,
            _kind: InlineMessageKind,
            _text: String,
            _line_count: usize,
        ) {
        }
        pub fn inline(&self, _kind: InlineMessageKind, _segment: InlineSegment) {}
        pub fn replace_last(
            &self,
            _count: usize,
            _kind: InlineMessageKind,
            _lines: Vec<Vec<InlineSegment>>,
        ) {
        }
        pub fn force_redraw(&self) {}
        pub fn shutdown(&self) {}
        pub fn clear_screen(&self) {}
        pub fn show_modal(
            &self,
            _title: String,
            _lines: Vec<String>,
            _secure_prompt: Option<SecurePromptConfig>,
        ) {
        }
        pub fn show_list_modal(
            &self,
            _title: String,
            _lines: Vec<String>,
            _items: Vec<InlineListItem>,
            _selected: Option<InlineListSelection>,
            _search: Option<InlineListSearchConfig>,
        ) {
        }
        pub fn close_modal(&self) {}
        pub fn set_reasoning_stage(&self, _stage: Option<String>) {}
    }

    /// Headless session — events never arrive.
    pub struct InlineSession {
        pub handle: InlineHandle,
        pub events: UnboundedReceiver<InlineEvent>,
    }

    impl InlineSession {
        pub async fn next_event(&mut self) -> Option<InlineEvent> {
            self.events.recv().await
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
        pub transcript_bottom_padding: u16,
        pub dim_completed_todos: bool,
        pub message_block_spacing: u8,
        pub layout_mode: super::LayoutModeOverride,
        pub reasoning_display_mode: super::ReasoningDisplayMode,
        pub reasoning_visible_default: bool,
        pub vim_mode: bool,
        pub screen_reader_mode: bool,
        pub reduce_motion_mode: bool,
        pub reduce_motion_keep_progress_animation: bool,
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
        )
    }
}

#[cfg(not(feature = "tui"))]
pub use headless::*;
