use super::*;

impl Session {
    pub fn cursor(&self) -> usize {
        self.input_manager.cursor()
    }

    pub fn set_input(&mut self, text: impl Into<String>) {
        self.input_manager.set_content(text.into());
        self.input_compact_mode = self.input_compact_placeholder().is_some();
        self.mark_dirty();
    }

    pub fn set_cursor(&mut self, pos: usize) {
        self.input_manager.set_cursor(pos);
        self.mark_dirty();
    }

    pub fn process_key(&mut self, key: KeyEvent) -> Option<InlineEvent> {
        events::process_key(self, key)
    }

    pub fn handle_command(&mut self, command: InlineCommand) {
        // Track streaming state: set when agent starts responding
        if matches!(
            &command,
            InlineCommand::AppendLine { kind: InlineMessageKind::Agent, segments }
                if !segments.is_empty()
        ) || matches!(
            &command,
            InlineCommand::AppendPastedMessage { kind: InlineMessageKind::Agent, text, .. }
                if !text.is_empty()
        ) || matches!(
            &command,
            InlineCommand::Inline { kind: InlineMessageKind::Agent, segment }
                if !segment.text.is_empty()
        ) {
            self.is_streaming_final_answer = true;
        }

        // Clear streaming state on turn completion (status cleared)
        if let InlineCommand::SetInputStatus { left, right } = &command
            && self.is_streaming_final_answer
            && left.is_none()
            && right.is_none()
        {
            self.is_streaming_final_answer = false;
        }

        match command {
            InlineCommand::AppendLine { kind, segments } => {
                self.clear_thinking_spinner_if_active(kind);
                self.push_line(kind, segments);
                self.request_transcript_clear();
            }
            InlineCommand::AppendPastedMessage {
                kind,
                text,
                line_count,
            } => {
                self.clear_thinking_spinner_if_active(kind);
                self.append_pasted_message(kind, text, line_count);
                self.request_transcript_clear();
            }
            InlineCommand::Inline { kind, segment } => {
                self.clear_thinking_spinner_if_active(kind);
                self.append_inline(kind, segment);
                self.request_transcript_clear();
            }
            InlineCommand::ReplaceLast {
                count,
                kind,
                lines,
                link_ranges,
            } => {
                self.clear_thinking_spinner_if_active(kind);
                self.replace_last(count, kind, lines, link_ranges);
                self.request_transcript_clear();
            }
            InlineCommand::SetPrompt { prefix, style } => {
                self.prompt_prefix = prefix;
                self.prompt_style = style;
                self.ensure_prompt_style_color();
            }
            InlineCommand::SetPlaceholder { hint, style } => {
                self.placeholder = hint;
                self.placeholder_style = style;
            }
            InlineCommand::SetMessageLabels { agent, user } => {
                self.labels.agent = agent.filter(|label| !label.is_empty());
                self.labels.user = user.filter(|label| !label.is_empty());
                self.invalidate_transcript_cache();
                self.invalidate_scroll_metrics();
            }
            InlineCommand::SetHeaderContext { context } => {
                self.header_context = *context;
                self.invalidate_header_cache();
            }
            InlineCommand::SetInputStatus { left, right } => {
                self.input_status_left = left;
                self.input_status_right = right;
                if self.thinking_spinner.is_active {
                    self.thinking_spinner.stop();
                }
                self.needs_redraw = true;
            }
            InlineCommand::SetTerminalTitleItems { items } => {
                self.terminal_title_items = items;
                self.needs_redraw = true;
            }
            InlineCommand::SetTerminalTitleThreadLabel { label } => {
                self.terminal_title_thread_label = label.filter(|value| !value.trim().is_empty());
                self.needs_redraw = true;
            }
            InlineCommand::SetTerminalTitleGitBranch { branch } => {
                self.terminal_title_git_branch = branch.filter(|value| !value.trim().is_empty());
                self.needs_redraw = true;
            }
            InlineCommand::SetTheme { theme } => {
                let previous_theme = self.theme.clone();
                self.theme = theme.clone();
                self.styles.set_theme(theme);
                self.retint_lines_for_theme_change(&previous_theme);
                self.ensure_prompt_style_color();
                self.invalidate_transcript_cache();
            }
            InlineCommand::SetAppearance { appearance } => {
                self.appearance = appearance;
                self.invalidate_transcript_cache();
                self.invalidate_scroll_metrics();
            }
            InlineCommand::SetVimModeEnabled(enabled) => {
                self.vim_state.set_enabled(enabled);
                self.needs_redraw = true;
            }
            InlineCommand::SetQueuedInputs { entries } => {
                self.set_queued_inputs_entries(entries);
                self.mark_dirty();
            }
            InlineCommand::SetSubprocessEntries { entries } => {
                self.subprocess_entries = entries;
                self.invalidate_sidebar_cache();
            }
            InlineCommand::SetSubagentPreview { text } => {
                self.subagent_preview = text.filter(|value| !value.trim().is_empty());
                self.invalidate_sidebar_cache();
            }
            InlineCommand::SetCursorVisible(value) => {
                self.cursor_visible = value;
            }
            InlineCommand::SetInputEnabled(value) => {
                self.input_enabled = value;
            }
            InlineCommand::SetInput(content) => {
                // Check if the content appears to be an error message
                // If it looks like an error, redirect to transcript instead
                if Self::is_error_content(&content) {
                    // Add error to transcript instead of input field
                    crate::utils::transcript::display_error(&content);
                } else {
                    self.clear_suggested_prompt_state();
                    self.clear_inline_prompt_suggestion();
                    self.input_manager.set_content(content);
                    self.input_compact_mode = self.input_compact_placeholder().is_some();
                    self.scroll_manager.set_offset(0);
                }
            }
            InlineCommand::ApplySuggestedPrompt(content) => {
                self.apply_suggested_prompt(content);
                self.scroll_manager.set_offset(0);
            }
            InlineCommand::SetInlinePromptSuggestion {
                suggestion,
                llm_generated,
            } => {
                self.set_inline_prompt_suggestion(suggestion, llm_generated);
            }
            InlineCommand::ClearInlinePromptSuggestion => {
                self.clear_inline_prompt_suggestion();
            }
            InlineCommand::ClearInput => {
                command::clear_input(self);
            }
            InlineCommand::ForceRedraw => {
                self.mark_dirty();
            }
            InlineCommand::ShowOverlay { request } => {
                self.clear_inline_prompt_suggestion();
                self.show_overlay(*request);
            }
            InlineCommand::CloseOverlay => {
                self.close_overlay();
            }
            InlineCommand::ClearScreen => {
                self.clear_screen();
            }
            InlineCommand::SuspendEventLoop
            | InlineCommand::ResumeEventLoop
            | InlineCommand::ClearInputQueue => {
                // Handled by drive_terminal
            }
            InlineCommand::SetEditingMode(mode) => {
                self.clear_inline_prompt_suggestion();
                self.header_context.editing_mode = mode;
                self.needs_redraw = true;
            }
            InlineCommand::SetAutonomousMode(enabled) => {
                self.header_context.autonomous_mode = enabled;
                self.needs_redraw = true;
            }
            InlineCommand::SetSkipConfirmations(skip) => {
                self.skip_confirmations = skip;
                if skip {
                    self.close_overlay();
                }
            }
            InlineCommand::Shutdown => {
                self.request_exit();
            }
            InlineCommand::SetReasoningStage(stage) => {
                self.header_context.reasoning_stage = stage;
                self.invalidate_header_cache();
            }
        }
        self.needs_redraw = true;
    }
}
