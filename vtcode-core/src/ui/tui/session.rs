use std::{collections::VecDeque, sync::Arc, time::Instant};

use anyhow::Result;

#[cfg(test)]
use anstyle::Color as AnsiColorEnum;
use anstyle::RgbColor;
use ratatui::crossterm::event::{
    Event as CrosstermEvent, KeyCode, KeyEvent, KeyEventKind, MouseEvent, MouseEventKind,
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
        InlineCommand, InlineEvent, InlineHeaderContext, InlineMessageKind, InlineTextStyle,
        InlineTheme,
    },
};
use crate::config::constants::ui;
use crate::config::loader::VTCodeConfig;
use crate::ui::tui::widgets::SessionWidget;

pub mod file_palette;
mod header;
mod impl_config;
mod impl_events;
mod impl_init;
mod impl_input;
mod impl_layout;
mod impl_logs;
mod impl_render;
mod impl_scroll;
mod impl_style;
mod input;
mod input_manager;
mod message;
pub mod modal;
pub mod mouse_selection;
mod navigation;
mod palette_renderer;
mod queue;
pub mod render;
mod scroll;
pub mod slash;
pub mod slash_palette;
pub mod styling;
mod text_utils;
mod transcript;

// New modular components (refactored from main session.rs)
mod command;
pub mod config_palette;
mod editing;

pub mod config;
mod diff_preview;
mod events;
pub mod history_picker;
mod message_renderer;
mod messages;
mod palette;
mod reflow;
mod reverse_search;
mod spinner;
mod state;
pub mod terminal_capabilities;
mod terminal_title;
#[cfg(test)]
mod tests;
mod tool_renderer;
mod trust;

use self::config_palette::ConfigPalette;
use self::file_palette::FilePalette;
use self::history_picker::HistoryPickerState;
use self::input_manager::InputManager;
use self::message::{MessageLabels, MessageLine};
use self::modal::{ModalState, WizardModalState};

use self::config::AppearanceConfig;
pub(crate) use self::input::status_requires_shimmer;
use self::mouse_selection::MouseSelectionState;
use self::queue::QueueOverlay;
use self::scroll::ScrollManager;
use self::slash_palette::SlashPalette;
use self::spinner::{ShimmerState, ThinkingSpinner};
use self::styling::SessionStyles;
use self::transcript::TranscriptReflowCache;
#[cfg(test)]
use super::types::InlineHeaderHighlight;
// use crate::tools::TaskPlan; // Commented out - plan functionality removed
use crate::ui::tui::log::{LogEntry, highlight_log_entry};

const USER_PREFIX: &str = "";
const PLACEHOLDER_COLOR: RgbColor = RgbColor(0x88, 0x88, 0x88);
const MAX_LOG_LINES: usize = 256;
const MAX_LOG_DRAIN_PER_TICK: usize = 256;

#[derive(Clone, Debug)]
struct CollapsedPaste {
    line_index: usize,
    full_text: String,
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
    input_compact_mode: bool,

    // --- UI State ---
    slash_palette: SlashPalette,
    #[allow(dead_code)]
    navigation_state: ListState,
    input_enabled: bool,
    cursor_visible: bool,
    pub(crate) needs_redraw: bool,
    pub(crate) needs_full_clear: bool,
    /// Track if transcript content changed (not just scroll position)
    pub(crate) transcript_content_changed: bool,
    should_exit: bool,
    scroll_cursor_steady_until: Option<Instant>,
    last_shimmer_active: bool,
    pub(crate) view_rows: u16,
    pub(crate) input_height: u16,
    pub(crate) transcript_rows: u16,
    pub(crate) transcript_width: u16,
    pub(crate) transcript_view_top: usize,
    transcript_area: Option<Rect>,
    input_area: Option<Rect>,

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
    pub(crate) visible_lines_cache: Option<(usize, u16, Arc<Vec<Line<'static>>>)>,
    pub(crate) queued_inputs: Vec<String>,
    queue_overlay_cache: Option<QueueOverlay>,
    queue_overlay_version: u64,
    pub(crate) modal: Option<ModalState>,
    wizard_modal: Option<WizardModalState>,
    line_revision_counter: u64,
    /// Track the first line that needs reflow/update to avoid O(N) scans
    first_dirty_line: Option<usize>,
    in_tool_code_fence: bool,

    // --- Palette Management ---
    pub(crate) config_palette: Option<ConfigPalette>,
    pub(crate) config_palette_active: bool,
    pub(crate) file_palette: Option<FilePalette>,
    pub(crate) file_palette_active: bool,
    pub(crate) deferred_file_browser_trigger: bool,

    // --- Thinking Indicator ---
    pub(crate) thinking_spinner: ThinkingSpinner,
    pub(crate) shimmer_state: ShimmerState,

    // --- Reverse Search ---
    pub(crate) reverse_search_state: crate::ui::tui::session::reverse_search::ReverseSearchState,

    // --- History Picker (Ctrl+R fuzzy search) ---
    pub(crate) history_picker_state: HistoryPickerState,

    // --- PTY Session Management ---
    pub(crate) active_pty_sessions: Option<Arc<std::sync::atomic::AtomicUsize>>,

    // --- Clipboard for yank/paste operations ---
    #[allow(dead_code)]
    pub(crate) clipboard: String,

    // --- Mouse Text Selection ---
    pub(crate) mouse_selection: MouseSelectionState,

    // --- Diff Preview Modal ---
    pub(crate) diff_preview: Option<crate::ui::tui::types::DiffPreviewState>,

    pub(crate) skip_confirmations: bool,

    // --- Performance Caching ---
    pub(crate) header_lines_cache: Option<Vec<Line<'static>>>,
    pub(crate) header_height_cache: std::collections::HashMap<u16, u16>,
    pub(crate) queued_inputs_preview_cache: Option<Vec<String>>,

    // --- Terminal Title ---
    /// Workspace root path for dynamic title generation
    pub(crate) workspace_root: Option<std::path::PathBuf>,
    /// Last set terminal title to avoid redundant updates
    last_terminal_title: Option<String>,
}
