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

        error_indicators.iter().any(|indicator| lower_content.contains(indicator))
    }

    pub fn new(theme: InlineTheme, placeholder: Option<String>, view_rows: u16) -> Self {
        Self::new_with_logs(theme, placeholder, view_rows, true, None, "Agent TUI".to_string())
    }

    pub(crate) fn new_with_logs(
        theme: InlineTheme,
        placeholder: Option<String>,
        view_rows: u16,
        show_logs: bool,
        appearance: Option<AppearanceConfig>,
        app_name: String,
    ) -> Self {
        Self::new_with_options(theme, placeholder, view_rows, show_logs, appearance, app_name, None)
    }

    pub(crate) fn new_with_bindings(
        theme: InlineTheme,
        placeholder: Option<String>,
        view_rows: u16,
        show_logs: bool,
        appearance: Option<AppearanceConfig>,
        app_name: String,
        bindings: BindingStore,
    ) -> Self {
        Self::new_with_options(theme, placeholder, view_rows, show_logs, appearance, app_name, Some(bindings))
    }

    fn new_with_options(
        theme: InlineTheme,
        placeholder: Option<String>,
        view_rows: u16,
        show_logs: bool,
        appearance: Option<AppearanceConfig>,
        app_name: String,
        bindings: Option<BindingStore>,
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
            thinking_runs: ThinkingRunIndex::default(),
            styles: SessionStyles::new(theme.clone()),
            theme,
            appearance,
            auto_color_scheme: false,
            header_context: InlineHeaderContext::default(),
            labels: MessageLabels::default(),

            // --- Prompt/Input Display ---
            prompt_prefix: USER_PREFIX.to_string(),
            prompt_style: InlineTextStyle::default(),
            placeholder,
            placeholder_style: None,
            input_status_left: None,
            footer_context_status: None,
            footer_context_right: None,
            footer_context_configured: false,
            input_status_right: None,
            copy_notification_until: None,
            copy_notification_failed: false,
            copy_notification_chars: 0,
            drag_auto_scroll: None,
            input_compact_mode: false,

            // --- UI State ---
            navigation_state: ListState::default(), // Kept for backward compatibility
            input_enabled: true,
            program_status: super::super::program_status::ProgramStatus::default(),
            activity_state: ActivityState::Idle,
            image_input_enabled: false,
            cursor_visible: true,
            render_state: RenderState::new(),
            transcript_clear_required: true,
            evicted_message_count: 0,
            pending_new_messages: 0,
            last_change_line_idx: None,
            should_exit: false,
            last_interrupt_press: None,
            last_escape_press: None,
            scroll_cursor_steady_until: None,
            last_shimmer_active: false,
            view_rows: resolved_rows,
            input_height: Self::input_block_height_for_lines(1),
            transcript_rows: initial_transcript_rows,
            transcript_width: 0,
            transcript_view_top: 0,
            areas: SessionAreas::default(),
            sticky_prompt_target: None,
            sticky_prompt_preview_cache: None,
            leading_user_prompt_truncated: false,
            transcript_file_link_targets: Vec::new(),
            modal_link_targets: Vec::new(),
            hovered_transcript_file_link: None,
            last_mouse_position: None,
            last_link_open: None,
            pending_link_open: None,
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
            transcript_presentation_revision: 0,
            first_dirty_line: None,
            in_tool_code_fence: false,

            // --- Prompt Suggestions ---
            suggested_prompt_state: SuggestedPromptState::default(),
            inline_prompt_suggestion: InlinePromptSuggestionState::default(),

            // --- Thinking Indicator ---
            thinking_spinner: ThinkingSpinner::new(),
            shimmer_state: ShimmerState::new(),
            progress: progress::TransientProgress::default(),

            // --- Reverse Search ---
            reverse_search_state: reverse_search::ReverseSearchState::new(),

            // --- PTY Session Management ---
            active_pty_sessions: None,

            // --- Background Activity ---
            background_activity_count: 0,
            background_finished_count: 0,
            background_indicator_hits: Vec::new(),

            // --- Keybinding store ---
            bindings: bindings.unwrap_or_default(),

            // --- Clipboard for yank/paste operations ---
            clipboard: String::new(),
            vim_state: VimState::new(vim_mode_enabled),

            // --- Mouse Text Selection ---
            mouse_selection: MouseSelectionState::new(),
            mouse_drag_target: MouseDragTarget::None,
            fullscreen: FullscreenSessionState::default(),

            // --- Performance Caching ---
            header_lines_cache: None,
            header_block_title_cache: None,
            header_height_cache: hashbrown::HashMap::new(),
            queued_inputs_preview_cache: None,
            subprocess_entries_preview_cache: None,
            input_render_cache: None,

            // --- Terminal Title ---
            app_name,
            workspace_root: None,
            terminal_title_items: None,
            terminal_title_thread_label: None,
            terminal_title_git_branch: None,
            terminal_title_task_progress: None,
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
            InlineMessageKind::Agent | InlineMessageKind::Policy | InlineMessageKind::Tool | InlineMessageKind::Error
        ) && self.thinking_spinner.is_active
        {
            self.thinking_spinner.stop();
            self.mark_visual_dirty();
        }
    }
}
