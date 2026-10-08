//! Stateless control-command forwarding shared by the core and app handles.
//!
//! State-aware methods, overlay routing, and visibility-specific helpers remain
//! local. Each handle retains its own channel and `send_command` behavior.

macro_rules! impl_inline_control_methods {
    ($command:ident) => {
        pub fn suspend_event_loop(&self) {
            self.send_command($command::SuspendEventLoop);
        }

        pub fn resume_event_loop(&self) {
            self.send_command($command::ResumeEventLoop);
        }

        pub fn clear_input_queue(&self) {
            self.send_command($command::ClearInputQueue);
        }

        pub fn stop_event_stream(&self) {
            self.send_command($command::StopEventStream);
        }

        pub fn start_event_stream(&self) {
            self.send_command($command::StartEventStream);
        }

        pub fn set_prompt(&self, prefix: String, style: vtcode_commons::ui_protocol::InlineTextStyle) {
            self.send_command($command::SetPrompt { prefix, style });
        }

        pub fn set_placeholder(&self, hint: Option<String>) {
            self.set_placeholder_with_style(hint, None);
        }

        pub fn set_header_context(&self, context: vtcode_commons::ui_protocol::InlineHeaderContext) {
            self.send_command($command::SetHeaderContext { context: Box::new(context) });
        }

        pub fn set_input_status(&self, left: Option<String>, right: Option<String>) {
            self.send_command($command::SetInputStatus { left, right });
        }

        /// Set the configured status line, retained while progress occupies the transcript.
        pub fn set_configured_input_status(&self, left: Option<String>, right: Option<String>) {
            self.send_command($command::SetConfiguredInputStatus { left, right });
        }

        pub fn update_progress(&self, update: vtcode_commons::ui_protocol::ProgressUpdate) {
            self.send_command($command::UpdateProgress(update));
        }

        pub fn program_status(&self, update: vtcode_commons::program_status::ProgramStatusUpdate) {
            self.send_command($command::ProgramStatus(update));
        }

        pub fn program_status_wait(
            &self,
            kind: vtcode_commons::program_status::InteractionKind,
        ) -> $crate::tui::core_tui::types::ProgramStatusWaitGuard {
            use vtcode_commons::program_status::ProgramStatusUpdate;
            use $crate::tui::core_tui::types::ProgramStatusWaitGuard;
            let token = ProgramStatusWaitGuard::token();
            self.program_status(ProgramStatusUpdate::Wait { token, kind });
            let handle = self.clone();
            ProgramStatusWaitGuard::new(move || handle.program_status(ProgramStatusUpdate::Resume { token }))
        }

        pub fn set_activity_state(&self, state: vtcode_commons::ui_protocol::ActivityState) {
            self.send_command($command::SetActivityState(state));
        }

        pub fn set_terminal_title_items(&self, items: Option<Vec<String>>) {
            self.send_command($command::SetTerminalTitleItems { items });
        }

        pub fn set_terminal_title_thread_label(&self, label: Option<String>) {
            self.send_command($command::SetTerminalTitleThreadLabel { label });
        }

        pub fn set_terminal_title_git_branch(&self, branch: Option<String>) {
            self.send_command($command::SetTerminalTitleGitBranch { branch });
        }

        pub fn set_theme(&self, theme: vtcode_commons::ui_protocol::InlineTheme) {
            self.send_command($command::SetTheme { theme });
        }

        pub fn set_color_scheme_auto(&self, enabled: bool) {
            self.send_command($command::SetColorSchemeAuto { enabled });
        }

        pub fn set_appearance(&self, appearance: $crate::tui::core_tui::session::config::AppearanceConfig) {
            self.send_command($command::SetAppearance { appearance });
        }

        pub fn set_fullscreen_interaction(&self, interaction: $crate::tui::options::FullscreenInteractionSettings) {
            self.send_command($command::SetFullscreenInteraction { interaction });
        }

        pub fn set_vim_mode_enabled(&self, enabled: bool) {
            self.send_command($command::SetVimModeEnabled(enabled));
        }

        pub fn set_queued_inputs(&self, entries: Vec<String>) {
            self.send_command($command::SetQueuedInputs { entries });
        }

        pub fn set_subprocess_entries(&self, entries: Vec<String>) {
            self.send_command($command::SetSubprocessEntries { entries });
        }

        pub fn set_subagent_preview(&self, text: Option<String>) {
            self.send_command($command::SetSubagentPreview { text });
        }

        pub fn set_primary_agent(&self, name: Option<String>, color: Option<String>) {
            self.send_command($command::SetPrimaryAgent { name, color });
        }

        pub fn set_cursor_visible(&self, visible: bool) {
            self.send_command($command::SetCursorVisible(visible));
        }

        pub fn set_input_enabled(&self, enabled: bool) {
            self.send_command($command::SetInputEnabled(enabled));
        }

        pub fn set_image_input_enabled(&self, enabled: bool) {
            self.send_command($command::SetImageInputEnabled(enabled));
        }

        pub fn set_input(&self, content: String) {
            self.send_command($command::SetInput(content));
        }

        pub fn restore_input_draft(&self, input: $crate::tui::core_tui::types::SubmittedInput) {
            self.send_command($command::RestoreInputDraft(input));
        }

        pub fn apply_suggested_prompt(&self, content: String) {
            self.send_command($command::ApplySuggestedPrompt(content));
        }

        pub fn set_inline_prompt_suggestion(&self, suggestion: String, llm_generated: bool) {
            self.send_command($command::SetInlinePromptSuggestion { suggestion, llm_generated });
        }

        pub fn clear_inline_prompt_suggestion(&self) {
            self.send_command($command::ClearInlinePromptSuggestion);
        }

        pub fn clear_input(&self) {
            self.send_command($command::ClearInput);
        }

        pub fn force_redraw(&self) {
            self.send_command($command::ForceRedraw);
        }

        pub fn shutdown(&self) {
            self.send_command($command::Shutdown);
        }

        pub fn clear_screen(&self) {
            self.send_command($command::ClearScreen);
        }

        pub fn set_skip_confirmations(&self, skip: bool) {
            self.send_command($command::SetSkipConfirmations(skip));
        }

        pub fn set_reasoning_stage(&self, stage: Option<String>) {
            self.send_command($command::SetReasoningStage(stage));
        }
    };
}

pub(crate) use impl_inline_control_methods;

#[cfg(test)]
mod tests;
