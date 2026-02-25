use super::*;

impl Session {
    pub(super) fn is_error_content(content: &str) -> bool {
        // Check if message contains common error indicators
        let lower_content = content.to_lowercase();
        let error_indicators = [
            "error:",
            "error ",
            "error\n",
            "failed",
            "failure",
            "exception",
            "invalid",
            "not found",
            "couldn't",
            "can't",
            "cannot",
            "denied",
            "forbidden",
            "unauthorized",
            "timeout",
            "connection refused",
            "no such",
            "does not exist",
        ];

        error_indicators
            .iter()
            .any(|indicator| lower_content.contains(indicator))
    }

    pub fn new(theme: InlineTheme, placeholder: Option<String>, view_rows: u16) -> Self {
        Self::new_with_logs(theme, placeholder, view_rows, true)
    }

    pub fn new_with_logs(
        theme: InlineTheme,
        placeholder: Option<String>,
        view_rows: u16,
        show_logs: bool,
    ) -> Self {
        Self::new_with_config_and_logs(theme, placeholder, view_rows, show_logs, None)
    }

    pub fn new_with_config(
        theme: InlineTheme,
        placeholder: Option<String>,
        view_rows: u16,
    ) -> Result<Self> {
        let config_manager = crate::config::loader::ConfigManager::load()?;
        let config = config_manager.config().clone();
        Ok(Self::new_with_config_and_logs(
            theme,
            placeholder,
            view_rows,
            true,
            Some(config),
        ))
    }

    fn new_with_config_and_logs(
        theme: InlineTheme,
        placeholder: Option<String>,
        view_rows: u16,
        show_logs: bool,
        config: Option<VTCodeConfig>,
    ) -> Self {
        let resolved_rows = view_rows.max(2);
        let initial_header_rows = ui::INLINE_HEADER_HEIGHT;
        let reserved_rows = initial_header_rows + Self::input_block_height_for_lines(1);
        let initial_transcript_rows = resolved_rows.saturating_sub(reserved_rows).max(1);

        let appearance = config
            .as_ref()
            .map(AppearanceConfig::from_config)
            .unwrap_or_default();

        let mut session = Self {
            // --- Managers (Phase 2) ---
            input_manager: InputManager::new(),
            scroll_manager: ScrollManager::new(initial_transcript_rows),
            user_scrolled: false,

            // --- Message Management ---
            lines: Vec::with_capacity(64),
            collapsed_pastes: Vec::new(),
            styles: SessionStyles::new(theme.clone()),
            theme,
            appearance,
            header_context: InlineHeaderContext::default(),
            labels: MessageLabels::default(),

            // --- Prompt/Input Display ---
            prompt_prefix: USER_PREFIX.to_string(),
            prompt_style: InlineTextStyle::default(),
            placeholder,
            placeholder_style: None,
            input_status_left: None,
            input_status_right: None,
            input_compact_mode: false,

            // --- UI State ---
            slash_palette: SlashPalette::new(),
            navigation_state: ListState::default(), // Kept for backward compatibility
            input_enabled: true,
            cursor_visible: true,
            needs_redraw: true,
            needs_full_clear: false,
            transcript_content_changed: true,
            should_exit: false,
            scroll_cursor_steady_until: None,
            last_shimmer_active: false,
            view_rows: resolved_rows,
            input_height: Self::input_block_height_for_lines(1),
            transcript_rows: initial_transcript_rows,
            transcript_width: 0,
            transcript_view_top: 0,
            transcript_area: None,
            input_area: None,

            // --- Logging ---
            log_receiver: None,
            log_lines: VecDeque::with_capacity(MAX_LOG_LINES),
            log_cached_text: None,
            log_evicted: false,
            show_logs,

            // --- Rendering ---
            transcript_cache: None,
            visible_lines_cache: None,
            queued_inputs: Vec::with_capacity(4),
            queue_overlay_cache: None,
            queue_overlay_version: 0,
            modal: None,
            wizard_modal: None,
            header_rows: initial_header_rows,
            line_revision_counter: 0,
            first_dirty_line: None,
            in_tool_code_fence: false,

            // --- Palette Management ---
            config_palette: None,
            config_palette_active: false,
            file_palette: None,
            file_palette_active: false,
            deferred_file_browser_trigger: false,

            // --- Thinking Indicator ---
            thinking_spinner: ThinkingSpinner::new(),
            shimmer_state: ShimmerState::new(),

            // --- Reverse Search ---
            reverse_search_state: crate::ui::tui::session::reverse_search::ReverseSearchState::new(
            ),

            // --- History Picker (Ctrl+R fuzzy search) ---
            history_picker_state: crate::ui::tui::session::history_picker::HistoryPickerState::new(
            ),

            // --- PTY Session Management ---
            active_pty_sessions: None,

            // --- Clipboard for yank/paste operations ---
            clipboard: String::new(),

            // --- Mouse Text Selection ---
            mouse_selection: crate::ui::tui::session::mouse_selection::MouseSelectionState::new(),

            // --- Diff Preview Modal ---
            diff_preview: None,

            skip_confirmations: false,

            // --- Performance Caching ---
            header_lines_cache: None,
            header_height_cache: std::collections::HashMap::new(),
            queued_inputs_preview_cache: None,

            // --- Terminal Title ---
            workspace_root: None,
            last_terminal_title: None,
        };
        session.ensure_prompt_style_color();
        session
    }

    pub(super) fn clear_thinking_spinner_if_active(&mut self, kind: InlineMessageKind) {
        // Clear spinner when any substantive agent output arrives
        if matches!(
            kind,
            InlineMessageKind::Agent
                | InlineMessageKind::Policy
                | InlineMessageKind::Tool
                | InlineMessageKind::Error
        ) && self.thinking_spinner.is_active
        {
            self.thinking_spinner.stop();
            self.needs_redraw = true;
        }
    }
}
