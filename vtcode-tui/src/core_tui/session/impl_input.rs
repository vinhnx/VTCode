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
        match command {
            InlineCommand::AppendLine { kind, segments } => {
                self.clear_thinking_spinner_if_active(kind);
                self.push_line(kind, segments);
                self.transcript_content_changed = true;
            }
            InlineCommand::AppendPastedMessage {
                kind,
                text,
                line_count,
            } => {
                self.clear_thinking_spinner_if_active(kind);
                self.append_pasted_message(kind, text, line_count);
                self.transcript_content_changed = true;
            }
            InlineCommand::Inline { kind, segment } => {
                self.clear_thinking_spinner_if_active(kind);
                self.append_inline(kind, segment);
                self.transcript_content_changed = true;
            }
            InlineCommand::ReplaceLast { count, kind, lines } => {
                self.clear_thinking_spinner_if_active(kind);
                self.replace_last(count, kind, lines);
                self.transcript_content_changed = true;
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
                self.invalidate_scroll_metrics();
            }
            InlineCommand::SetHeaderContext { context } => {
                self.header_context = context;
                self.needs_redraw = true;
            }
            InlineCommand::SetInputStatus { left, right } => {
                self.input_status_left = left;
                self.input_status_right = right;
                if self.thinking_spinner.is_active {
                    self.thinking_spinner.stop();
                }
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
            InlineCommand::SetQueuedInputs { entries } => {
                self.set_queued_inputs_entries(entries);
                self.mark_dirty();
            }
            InlineCommand::SetCursorVisible(value) => {
                self.cursor_visible = value;
            }
            InlineCommand::SetInputEnabled(value) => {
                self.input_enabled = value;
                slash::update_slash_suggestions(self);
            }
            InlineCommand::SetInput(content) => {
                // Check if the content appears to be an error message
                // If it looks like an error, redirect to transcript instead
                if Self::is_error_content(&content) {
                    // Add error to transcript instead of input field
                    crate::utils::transcript::display_error(&content);
                } else {
                    self.input_manager.set_content(content);
                    self.input_compact_mode = self.input_compact_placeholder().is_some();
                    self.scroll_manager.set_offset(0);
                    slash::update_slash_suggestions(self);
                }
            }
            InlineCommand::ClearInput => {
                command::clear_input(self);
            }
            InlineCommand::ForceRedraw => {
                self.mark_dirty();
            }
            InlineCommand::ShowModal {
                title,
                lines,
                secure_prompt,
            } => {
                self.show_modal(title, lines, secure_prompt);
            }
            InlineCommand::ShowListModal {
                title,
                lines,
                items,
                selected,
                search,
            } => {
                self.show_list_modal(title, lines, items, selected, search);
            }
            InlineCommand::ShowWizardModal {
                title,
                steps,
                current_step,
                search,
                mode,
            } => {
                self.show_wizard_modal(title, steps, current_step, search, mode);
            }
            InlineCommand::CloseModal => {
                self.close_modal();
            }
            InlineCommand::LoadFilePalette { files, workspace } => {
                self.load_file_palette(files, workspace);
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
                self.header_context.editing_mode = mode;
                self.needs_redraw = true;
            }
            InlineCommand::SetAutonomousMode(enabled) => {
                self.header_context.autonomous_mode = enabled;
                self.needs_redraw = true;
            }
            InlineCommand::ShowPlanConfirmation { plan } => {
                command::show_plan_confirmation_modal(self, *plan);
            }
            InlineCommand::ShowDiffPreview {
                file_path,
                before,
                after,
                hunks,
                current_hunk,
            } => {
                command::show_diff_preview(self, file_path, before, after, hunks, current_hunk);
            }
            InlineCommand::SetSkipConfirmations(skip) => {
                self.skip_confirmations = skip;
                if skip {
                    self.close_modal();
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
