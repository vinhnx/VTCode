use std::{
    collections::VecDeque,
    sync::Arc,
    time::{Duration, Instant},
};

#[cfg(test)]
use anstyle::Color as AnsiColorEnum;
use anstyle::RgbColor;
use ratatui::crossterm::event::{
    Event as CrosstermEvent, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEvent, MouseEventKind,
};

use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    text::{Line, Span, Text},
    widgets::{Clear, ListState, Widget},
};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

use super::{
    style::{measure_text_width, ratatui_color_from_ansi, ratatui_style_from_inline},
    types::{
        InlineCommand, InlineEvent, InlineHeaderContext, InlineListSelection, InlineMessageKind, InlineTextStyle,
        InlineTheme, OverlayRequest,
    },
};
use crate::tui::config::constants::ui;
use crate::tui::core_tui::types::LocalAgentEntry;
use crate::tui::options::FullscreenInteractionSettings;
use crate::tui::ui::tui::widgets::SessionWidget;

mod frame_layout;
mod header;
mod impl_events;
mod impl_init;
mod impl_input;
mod impl_layout;
mod impl_logs;
mod impl_render;
mod impl_scroll;
mod impl_style;
pub(crate) mod inline_list;
mod input;
pub(crate) mod input_manager;
pub(crate) mod list_navigator;
pub(crate) mod list_panel;
mod message;
pub mod modal;
pub mod mouse_selection;
mod navigation;
mod queue;
pub mod render;
mod scroll;
pub mod styling;
mod text_utils;
mod textarea_bridge;
mod transcript;
pub mod utils;
pub mod wrapping;

// New modular components (refactored from main session.rs)
mod command;
mod editing;

pub mod action;
pub(crate) mod clipboard_image;
pub mod config;
mod driver;
mod events;
pub(crate) mod message_renderer;
mod messages;
pub(crate) mod mode_switch_guard;
mod reflow;
pub(crate) mod reverse_search;
mod spinner;
mod state;
pub mod terminal_capabilities;
mod terminal_title;
#[cfg(test)]
mod tests;
mod tool_renderer;
mod transcript_links;
mod vim;

use self::input_manager::InputManager;
pub(crate) use self::message::TranscriptLine;
use self::message::{MessageLabels, MessageLine};
use self::modal::{ModalState, WizardModalState};

pub use self::action::{Action, BindingStore, parse_key_binding};
use self::config::AppearanceConfig;
pub(crate) use self::input::status_requires_shimmer;
use self::mouse_selection::MouseSelectionState;
use self::queue::QueueOverlay;
use self::scroll::ScrollManager;
pub(crate) use self::spinner::pulse_spinner_frame_for_phase;
use self::spinner::{ShimmerState, ThinkingSpinner};
use self::styling::SessionStyles;
use self::transcript::TranscriptReflowCache;
use self::transcript_links::TranscriptFileLinkTarget;
pub(crate) use self::transcript_links::TranscriptLinkClickAction;
use self::vim::VimState;
#[cfg(test)]
use super::types::InlineHeaderHighlight;
// TaskPlan integration intentionally omitted in this UI crate.
use crate::tui::ui::tui::log::{LogEntry, highlight_log_entry};

const USER_PREFIX: &str = "";
const PLACEHOLDER_COLOR: RgbColor = RgbColor(ui::PLACEHOLDER_R, ui::PLACEHOLDER_G, ui::PLACEHOLDER_B);
const MAX_LOG_LINES: usize = 256;
const MAX_LOG_DRAIN_PER_TICK: usize = 256;

#[derive(Clone, Debug)]
struct CollapsedPaste {
    line_index: usize,
    full_text: String,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct SuggestedPromptState {
    pub(crate) active: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum InlinePromptSuggestionSource {
    Llm,
    Local,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct InlinePromptSuggestionState {
    pub(crate) suggestion: Option<String>,
    pub(crate) source: Option<InlinePromptSuggestionSource>,
}

pub(crate) enum ActiveOverlay {
    Modal(Box<ModalState>),
    Wizard(Box<WizardModalState>),
}

impl ActiveOverlay {
    fn as_modal(&self) -> Option<&ModalState> {
        match self {
            Self::Modal(state) => Some(state),
            Self::Wizard(_) => None,
        }
    }

    fn as_modal_mut(&mut self) -> Option<&mut ModalState> {
        match self {
            Self::Modal(state) => Some(state),
            Self::Wizard(_) => None,
        }
    }

    fn as_wizard(&self) -> Option<&WizardModalState> {
        match self {
            Self::Wizard(state) => Some(state),
            Self::Modal(_) => None,
        }
    }

    fn as_wizard_mut(&mut self) -> Option<&mut WizardModalState> {
        match self {
            Self::Wizard(state) => Some(state),
            Self::Modal(_) => None,
        }
    }

    fn restore_input(&self) -> bool {
        match self {
            Self::Modal(state) => state.restore_input,
            Self::Wizard(_) => true,
        }
    }

    fn restore_cursor(&self) -> bool {
        match self {
            Self::Modal(state) => state.restore_cursor,
            Self::Wizard(_) => true,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum MouseDragTarget {
    #[default]
    None,
    Transcript,
    ModalText,
    Input,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct FullscreenSessionState {
    pub(crate) active: bool,
    pub(crate) interaction: FullscreenInteractionSettings,
}

pub struct Session {
    // --- Managers (Phase 2) ---
    /// Manages user input, cursor, and command history
    pub(crate) input_manager: InputManager,
    /// Manages scroll state and viewport metrics
    pub(crate) scroll_manager: ScrollManager,
    user_scrolled: bool,

    // --- Message Management ---
    pub(crate) lines: Vec<MessageLine>,
    collapsed_pastes: Vec<CollapsedPaste>,
    /// Thinking/reasoning run bookkeeping (collapse overrides + the run
    /// currently streaming), isolated behind a narrow interface so the policy
    /// can evolve and be tested independently of transcript storage.
    pub(crate) thinking_runs: ThinkingRunIndex,
    pub(crate) theme: InlineTheme,
    pub(crate) styles: SessionStyles,
    pub(crate) appearance: AppearanceConfig,
    pub(crate) header_context: InlineHeaderContext,
    pub(crate) header_rows: u16,
    pub(crate) labels: MessageLabels,

    // --- Prompt/Input Display ---
    prompt_prefix: String,
    prompt_style: InlineTextStyle,
    placeholder: Option<String>,
    placeholder_style: Option<InlineTextStyle>,
    pub(crate) input_status_left: Option<String>,
    pub(crate) input_status_right: Option<String>,
    /// Transient "copied" confirmation shown in the input status row.
    copy_notification_until: Option<Instant>,
    input_compact_mode: bool,

    // --- UI State ---
    #[expect(dead_code)]
    navigation_state: ListState,
    input_enabled: bool,
    image_input_enabled: bool,
    cursor_visible: bool,
    pub(crate) needs_redraw: bool,
    pub(crate) needs_full_clear: bool,
    /// Track whether the transcript viewport must be cleared before repainting.
    pub(crate) transcript_clear_required: bool,
    should_exit: bool,
    pub(crate) last_interrupt_press: Option<Instant>,
    scroll_cursor_steady_until: Option<Instant>,
    last_shimmer_active: bool,
    pub(crate) view_rows: u16,
    pub(crate) input_height: u16,
    pub(crate) transcript_rows: u16,
    pub(crate) transcript_width: u16,
    pub(crate) transcript_view_top: usize,
    transcript_area: Option<Rect>,
    input_area: Option<Rect>,
    bottom_panel_area: Option<Rect>,
    modal_list_area: Option<Rect>,
    modal_text_areas: Vec<Rect>,
    transcript_file_link_targets: Vec<TranscriptFileLinkTarget>,
    modal_link_targets: Vec<TranscriptFileLinkTarget>,
    hovered_transcript_file_link: Option<usize>,
    last_mouse_position: Option<(u16, u16)>,
    last_link_open: Option<(String, Instant)>,
    pending_link_open: Option<String>,
    held_key_modifiers: KeyModifiers,

    // --- Logging ---
    log_receiver: Option<UnboundedReceiver<LogEntry>>,
    log_lines: VecDeque<Arc<Text<'static>>>,
    log_cached_text: Option<Arc<Text<'static>>>,
    log_evicted: bool,
    pub(crate) show_logs: bool,

    // --- Rendering ---
    transcript_cache: Option<TranscriptReflowCache>,
    /// Cache of visible lines by (scroll_offset, width) - shared via Arc for zero-copy reads
    /// Avoids expensive clone on cache hits
    pub(crate) visible_lines_cache: Option<(usize, u16, usize, Arc<Vec<TranscriptLine>>)>,
    pub(crate) queued_inputs: Vec<String>,
    pub(crate) local_agents: Vec<LocalAgentEntry>,
    pub(crate) local_agents_drawer_visible: bool,
    pub(crate) subprocess_entries: Vec<String>,
    pub(crate) subagent_preview: Option<String>,
    queue_overlay_cache: Option<QueueOverlay>,
    queue_overlay_version: u64,
    active_overlay: Option<ActiveOverlay>,
    overlay_queue: VecDeque<OverlayRequest>,
    last_overlay_list_selection: Option<InlineListSelection>,
    last_overlay_list_was_last: bool,
    line_revision_counter: u64,
    /// Track the first line that needs reflow/update to avoid O(N) scans
    first_dirty_line: Option<usize>,
    in_tool_code_fence: bool,

    // --- Prompt Suggestions ---
    pub(crate) suggested_prompt_state: SuggestedPromptState,
    pub(crate) inline_prompt_suggestion: InlinePromptSuggestionState,

    // --- Thinking Indicator ---
    pub(crate) thinking_spinner: ThinkingSpinner,
    pub(crate) shimmer_state: ShimmerState,

    // --- Reverse Search ---
    pub(crate) reverse_search_state: reverse_search::ReverseSearchState,

    // --- PTY Session Management ---
    pub(crate) active_pty_sessions: Option<Arc<std::sync::atomic::AtomicUsize>>,

    // --- Keybinding store ---
    pub(crate) bindings: BindingStore,

    // --- Clipboard for yank/paste operations ---
    #[expect(dead_code)]
    pub(crate) clipboard: String,
    pub(crate) vim_state: VimState,

    // --- Mouse Text Selection ---
    pub(crate) mouse_selection: MouseSelectionState,
    pub(crate) mouse_drag_target: MouseDragTarget,
    pub(crate) fullscreen: FullscreenSessionState,

    pub(crate) skip_confirmations: bool,

    // --- Performance Caching ---
    pub(crate) header_lines_cache: Option<Vec<Line<'static>>>,
    pub(crate) header_height_cache: hashbrown::HashMap<u16, u16>,
    pub(crate) queued_inputs_preview_cache: Option<Vec<String>>,
    pub(crate) subprocess_entries_preview_cache: Option<Vec<String>>,

    // --- Terminal Title ---
    /// Product/app name used in terminal title branding
    pub(crate) app_name: String,
    /// Workspace root path for dynamic title generation
    pub(crate) workspace_root: Option<std::path::PathBuf>,
    /// Raw config items for terminal title rendering (`None` means use defaults).
    pub(crate) terminal_title_items: Option<Vec<String>>,
    /// Active thread label shown in terminal title when configured.
    pub(crate) terminal_title_thread_label: Option<String>,
    /// Active git branch shown in terminal title when configured.
    pub(crate) terminal_title_git_branch: Option<String>,
    /// Latest task tracker progress label extracted from the task panel.
    pub(crate) terminal_title_task_progress: Option<String>,
    /// Last set terminal title to avoid redundant updates
    last_terminal_title: Option<String>,

    // --- Streaming State ---
    /// Track if the assistant is currently streaming a final answer.
    /// When true, user input should be queued instead of submitted immediately
    /// to prevent race conditions with turn completion (see GitHub #12569).
    pub(crate) is_streaming_final_answer: bool,

    // --- Double-Esc Detection ---
    /// Timestamp of the last Escape key press for double-Esc detection.
    pub(crate) last_esc_press: Option<Instant>,
}

/// Per-session index of thinking/reasoning (`Policy`) runs.
///
/// Owns the collapse overrides and the start of the run currently streaming,
/// exposing them through a narrow interface. Keeping this state behind guard
/// rails isolates thinking-run bookkeeping from transcript storage so the
/// policy can be tested and evolved without coupling to `Session`.
#[derive(Debug, Default, Clone)]
pub(crate) struct ThinkingRunIndex {
    /// Explicit collapse overrides keyed by run-start line index. Absence means
    /// "use the config default" (`appearance.thinking_collapsed_by_default()`).
    collapsed: std::collections::HashMap<usize, bool>,
    /// Start line index of the reasoning run currently being streamed, if any.
    /// `None` when no reasoning is actively streaming. Because the transcript
    /// only ever appends lines, this index stays valid until `clear_screen`.
    active_start: Option<usize>,
    /// Start timestamps keyed by run-start line index.
    start_times: std::collections::HashMap<usize, Instant>,
    /// Elapsed run durations keyed by run-start line index.
    durations: std::collections::HashMap<usize, Duration>,
}

impl ThinkingRunIndex {
    /// Whether the run starting at `start` should render collapsed, falling
    /// back to `default` when no explicit override exists.
    pub(crate) fn is_collapsed(&self, start: usize, default: bool) -> bool {
        self.collapsed.get(&start).copied().unwrap_or(default)
    }

    /// Set the explicit collapse state for the run starting at `start`.
    pub(crate) fn set_collapsed(&mut self, start: usize, collapsed: bool) {
        self.collapsed.insert(start, collapsed);
    }

    /// Record the start of a newly begun reasoning run.
    pub(crate) fn begin_run(&mut self, start: usize) {
        self.active_start = Some(start);
        self.start_times.entry(start).or_insert_with(Instant::now);
    }

    /// End the active reasoning run (a non-reasoning line was appended).
    pub(crate) fn end_run(&mut self) {
        if let Some(start) = self.active_start {
            if let Some(start_time) = self.start_times.get(&start) {
                let duration = start_time.elapsed();
                self.durations.insert(start, duration);
            }
        }
        self.active_start = None;
    }

    /// Whether the reasoning run starting at `start` is currently active (streaming).
    pub(crate) fn is_active(&self, start: usize) -> bool {
        self.active_start == Some(start)
    }

    /// Duration of the reasoning run starting at `start`, if recorded.
    pub(crate) fn duration(&self, start: usize) -> Option<Duration> {
        self.durations
            .get(&start)
            .copied()
            .or_else(|| self.start_times.get(&start).map(|t| t.elapsed()))
    }

    /// Start line index of the reasoning run currently streaming, if any.
    pub(crate) fn active_start(&self) -> Option<usize> {
        self.active_start
    }

    /// Reset all tracked runs (e.g. on `clear_screen`).
    pub(crate) fn clear(&mut self) {
        self.collapsed.clear();
        self.start_times.clear();
        self.durations.clear();
        self.active_start = None;
    }
}
