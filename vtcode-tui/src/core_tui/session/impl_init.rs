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
        Self::new_with_logs(
            theme,
            placeholder,
            view_rows,
            true,
            None,
            "Agent TUI".to_string(),
        )
    }

    pub fn new_with_logs(
        theme: InlineTheme,
        placeholder: Option<String>,
        view_rows: u16,
        show_logs: bool,
        appearance: Option<AppearanceConfig>,
        app_name: String,
    ) -> Self {
        Self::new_with_options(
            theme,
            placeholder,
            view_rows,
            show_logs,
            appearance,
            app_name,
        )
    }

    fn new_with_options(
        theme: InlineTheme,
        placeholder: Option<String>,
        view_rows: u16,
        show_logs: bool,
        appearance: Option<AppearanceConfig>,
        app_name: String,
    ) -> Self {
        let resolved_rows = view_rows.max(2);
        let initial_header_rows = ui::INLINE_HEADER_HEIGHT;
        let reserved_rows = initial_header_rows + Self::input_block_height_for_lines(1);
        let initial_transcript_rows = resolved_rows.saturating_sub(reserved_rows).max(1);

        let appearance = appearance.unwrap_or_default();
        let vim_mode_enabled = appearance.vim_mode;

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
            bottom_panel_area: None,
            modal_list_area: None,
            transcript_file_link_targets: Vec::new(),
            hovered_transcript_file_link: None,
            last_mouse_position: None,
            held_key_modifiers: KeyModifiers::empty(),

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
            local_agents: Vec::new(),
            local_agents_drawer_visible: false,
            subprocess_entries: Vec::new(),
            subagent_preview: None,
            queue_overlay_cache: None,
            queue_overlay_version: 0,
            active_overlay: None,
            overlay_queue: VecDeque::new(),
            last_overlay_list_selection: None,
            last_overlay_list_was_last: false,
            header_rows: initial_header_rows,
            line_revision_counter: 0,
            first_dirty_line: None,
            in_tool_code_fence: false,

            // --- Prompt Suggestions ---
            suggested_prompt_state: SuggestedPromptState::default(),
            inline_prompt_suggestion: InlinePromptSuggestionState::default(),

            // --- Thinking Indicator ---
            thinking_spinner: ThinkingSpinner::new(),
            shimmer_state: ShimmerState::new(),

            // --- Reverse Search ---
            reverse_search_state: reverse_search::ReverseSearchState::new(),

            // --- PTY Session Management ---
            active_pty_sessions: None,

            // --- Clipboard for yank/paste operations ---
            clipboard: String::new(),
            vim_state: VimState::new(vim_mode_enabled),

            // --- Mouse Text Selection ---
            mouse_selection: MouseSelectionState::new(),
            mouse_drag_target: MouseDragTarget::None,

            skip_confirmations: false,

            // --- Performance Caching ---
            header_lines_cache: None,
            header_height_cache: hashbrown::HashMap::new(),
            queued_inputs_preview_cache: None,
            subprocess_entries_preview_cache: None,

            // --- Terminal Title ---
            app_name,
            workspace_root: None,
            last_terminal_title: None,

            // --- Streaming State ---
            is_streaming_final_answer: false,
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
