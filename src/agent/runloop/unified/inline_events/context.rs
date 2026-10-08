use anyhow::{Context, Result};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Notify;

use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::hooks::LifecycleHookEngine;
use vtcode_core::llm::provider::{self as uni};
use vtcode_core::tools::exec_session::ExecSessionManager;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};
use vtcode_ui::tui::app::ExecSessionAction;
use vtcode_ui::tui::app::{
    InlineEvent, InlineHandle, InlineHeaderContext, SubmittedInput, TransientEvent, TransientHotkeyAction,
    TransientSelectionChange, TransientSubmission,
};

use crate::agent::runloop::model_picker::ModelPickerState;
use crate::agent::runloop::unified::context_manager::ContextManager;
use crate::agent::runloop::unified::inline_events::harness::HarnessEventEmitter;
use crate::agent::runloop::unified::model_selection::ModelSwitchCompactionTargets;
use crate::agent::runloop::unified::palettes::ActivePalette;
use crate::agent::runloop::unified::session_setup::{EditorOpenDispatcher, EditorOpenRequestSender};
use crate::agent::runloop::unified::state::SessionStats;
use crate::agent::runloop::welcome::SessionBootstrap;

use super::action::InlineLoopAction;
use super::control::InlineControlProcessor;
use super::input::InlineInputProcessor;
use super::interrupts::InlineInterruptCoordinator;
use super::modal::InlineModalProcessor;
use super::queue::InlineQueueState;
use super::state::InlineEventState;

pub(crate) struct InlineEventContext<'a> {
    handle: &'a InlineHandle,
    state: InlineEventState<'a>,
    modal: InlineModalProcessor<'a>,
    ctrl_c_state: &'a Arc<crate::agent::runloop::unified::state::CtrlCState>,
    ctrl_c_notify: &'a Arc<Notify>,
    editor_workspace: PathBuf,
    editor_open_sender: Option<EditorOpenRequestSender>,
    editor_open_dispatcher: Option<Arc<EditorOpenDispatcher>>,
    exec_sessions: Option<ExecSessionManager>,
    harness_emitter: Option<&'a HarnessEventEmitter>,
}

impl<'a> InlineEventContext<'a> {
    #[expect(
        clippy::too_many_arguments,
        reason = "Intentional compatibility, platform, test, or API-shape suppression."
    )]
    pub(crate) fn new(
        renderer: &'a mut AnsiRenderer,
        handle: &'a InlineHandle,
        interrupts: InlineInterruptCoordinator<'a>,
        ctrl_c_notice_displayed: &'a mut bool,
        header_context: &'a mut InlineHeaderContext,
        model_picker_state: &'a mut Option<ModelPickerState>,
        palette_state: &'a mut Option<ActivePalette>,
        config: &'a mut CoreAgentConfig,
        vt_cfg: &'a mut Option<VTCodeConfig>,
        provider_client: &'a mut Box<dyn uni::LLMProvider>,
        ctrl_c_state: &'a Arc<crate::agent::runloop::unified::state::CtrlCState>,
        ctrl_c_notify: &'a Arc<Notify>,
        session_bootstrap: &'a SessionBootstrap,
        full_auto: bool,
        conversation_history: &'a mut Vec<uni::Message>,
        session_stats: &'a mut SessionStats,
        context_manager: &'a mut ContextManager,
        session_id: &'a str,
        thread_id: &'a str,
        lifecycle_hooks: Option<&'a LifecycleHookEngine>,
        harness_emitter: Option<&'a HarnessEventEmitter>,
    ) -> Self {
        let editor_workspace = config.workspace.clone();
        let state = InlineEventState::new(renderer, interrupts, ctrl_c_notice_displayed);
        let modal = InlineModalProcessor::new(
            handle,
            header_context,
            model_picker_state,
            palette_state,
            config,
            vt_cfg,
            provider_client,
            ctrl_c_state,
            ctrl_c_notify,
            session_bootstrap,
            full_auto,
            ModelSwitchCompactionTargets {
                history: conversation_history,
                session_stats,
                context_manager,
                session_id,
                thread_id,
                lifecycle_hooks,
                harness_emitter,
            },
        );

        Self {
            handle,
            harness_emitter,
            state,
            modal,
            ctrl_c_state,
            ctrl_c_notify,
            editor_workspace,
            editor_open_sender: None,
            editor_open_dispatcher: None,
            exec_sessions: None,
        }
    }

    pub(crate) fn set_exec_session_manager(&mut self, exec_sessions: ExecSessionManager) {
        self.exec_sessions = Some(exec_sessions);
    }

    pub(crate) fn set_editor_open_sink(
        &mut self,
        sender: EditorOpenRequestSender,
        dispatcher: Arc<EditorOpenDispatcher>,
    ) {
        self.editor_open_sender = Some(sender);
        self.editor_open_dispatcher = Some(dispatcher);
    }

    pub(crate) async fn process_event(
        &mut self,
        event: InlineEvent,
        queue: &mut InlineQueueState<'_>,
    ) -> Result<InlineLoopAction> {
        let action = match event {
            InlineEvent::Submit(text) => self.submit_to_focused_exec_session(text).await?,
            InlineEvent::WebmcpSubmit(text) => self.input_processor().submit_prompt(text),
            InlineEvent::QueueSubmit(text) => {
                let primary_agent = self.modal.active_primary_agent_name();
                self.input_processor().queue_submit(text, queue, primary_agent)
            }
            InlineEvent::ProcessLatestQueued => {
                self.state.reset_interrupt_state();
                queue.prefer_latest_next();
                InlineLoopAction::Continue
            }
            InlineEvent::Steer(input) => {
                if input.has_attachments() {
                    self.state.reset_interrupt_state();
                    self.state.renderer().line(
                        MessageStyle::Warning,
                        "Live steering supports text only. Remove image attachments before steering.",
                    )?;
                    self.modal.restore_input_draft(input);
                    InlineLoopAction::Continue
                } else if self.ctrl_c_state.take_steer_delivered() {
                    // Callback already handed this steer to the live steering
                    // channel (mid-turn injection). Stay passive so the message
                    // is not queued twice.
                    self.input_processor().passive()
                } else {
                    // Steering was unavailable (no sender, closed channel, or
                    // the callback never ran). Queue the message so it is
                    // processed once the agent is ready instead of vanishing.
                    self.state.reset_interrupt_state();
                    let primary_agent = self.modal.active_primary_agent_name();
                    self.input_processor().queue_submit(input, queue, primary_agent)
                }
            }
            InlineEvent::Pause | InlineEvent::Resume => {
                self.state.reset_interrupt_state();
                self.input_processor().passive()
            }
            InlineEvent::EditQueue => {
                self.state.reset_interrupt_state();
                queue.edit_latest();
                InlineLoopAction::Continue
            }
            InlineEvent::Transient(overlay_event) => match overlay_event {
                TransientEvent::SelectionChanged(TransientSelectionChange::List(selection)) => {
                    self.modal.handle_preview(self.state.renderer(), selection)?
                }
                TransientEvent::SelectionChanged(TransientSelectionChange::DiffTrustMode { .. }) => {
                    self.state.reset_interrupt_state();
                    self.input_processor().passive()
                }
                TransientEvent::Submitted(TransientSubmission::Selection(selection)) => {
                    self.state.reset_interrupt_state();
                    self.modal.handle_submit(self.state.renderer(), selection).await?
                }
                TransientEvent::Submitted(TransientSubmission::Wizard(selections)) => {
                    self.state.reset_interrupt_state();
                    self.modal.handle_wizard_submit(self.state.renderer(), selections).await?
                }
                TransientEvent::Submitted(TransientSubmission::DiffApply) => {
                    self.state.reset_interrupt_state();
                    InlineLoopAction::DiffApproved
                }
                TransientEvent::Submitted(TransientSubmission::DiffReject) => {
                    self.state.reset_interrupt_state();
                    InlineLoopAction::DiffRejected
                }
                TransientEvent::Submitted(
                    TransientSubmission::DiffProceed | TransientSubmission::DiffReload | TransientSubmission::DiffAbort,
                ) => {
                    self.state.reset_interrupt_state();
                    self.input_processor().passive()
                }
                TransientEvent::Submitted(TransientSubmission::Hotkey(action)) => {
                    self.state.reset_interrupt_state();
                    match action {
                        // The plan-approval overlay (`Ready to code?`) owns the
                        // `LaunchEditor` (`Ctrl+G`) hotkey: `execute_plan_approval`
                        // intercepts it in `wait_for_overlay_submission`, opens
                        // the persisted plan file, and re-shows the overlay.
                        // Seeding a bare `/edit` here would dismiss the modal,
                        // lose the approval, and open an empty draft instead of
                        // the plan file (the reported Ctrl+G bug). Keep it
                        // passive so the approval wait owns the outcome.
                        TransientHotkeyAction::LaunchEditor
                        | TransientHotkeyAction::ReloadSubagentInspector
                        | TransientHotkeyAction::GracefulStopSubagent
                        | TransientHotkeyAction::ForceCancelSubagent
                        | TransientHotkeyAction::OpenSourceThread
                        | TransientHotkeyAction::FocusJobOutput
                        | TransientHotkeyAction::InterruptJob
                        | TransientHotkeyAction::PreviewJobSnapshot => self.input_processor().passive(),
                    }
                }
                TransientEvent::Cancelled => {
                    self.state.reset_interrupt_state();
                    self.modal.handle_cancel(self.state.renderer())?
                }
            },
            InlineEvent::Cancel => self.control_processor().cancel()?,
            InlineEvent::ForceCancelPtySession => self.handle_force_cancel_pty_session().await?,
            InlineEvent::Exit => self.control_processor().exit()?,
            InlineEvent::Interrupt => self.handle_interrupt(),
            InlineEvent::BackgroundOperation => {
                if let Some(exec_sessions) = self.exec_sessions.as_ref()
                    && let Some(result) = exec_sessions.take_background_shortcut_result()
                {
                    match result {
                        vtcode_core::tools::exec_session::BackgroundShortcutResult::Requested => {
                            self.handle.show_local_agents();
                            self.input_processor().passive()
                        }
                        vtcode_core::tools::exec_session::BackgroundShortcutResult::AtCapacity => {
                            self.state.renderer().line(
                                MessageStyle::Warning,
                                "Cannot background the foreground process: the runtime already has three live background processes. Wait for or close one first.",
                            )?;
                            self.input_processor().passive()
                        }
                    }
                } else {
                    self.input_processor().submit("/subprocesses toggle".into())
                }
            }
            InlineEvent::ExecSessionAction { id, action } => self.handle_exec_session_action(id, action).await?,
            InlineEvent::LaunchEditor { draft } => InlineLoopAction::LaunchEditorWithDraft { draft },
            InlineEvent::RequestInlinePromptSuggestion(draft) => {
                self.state.reset_interrupt_state();
                InlineLoopAction::RequestInlinePromptSuggestion(draft)
            }
            InlineEvent::CyclePrimaryAgent => {
                self.state.reset_interrupt_state();
                InlineLoopAction::CyclePrimaryAgent
            }
            InlineEvent::CyclePrimaryAgentPrevious => {
                self.state.reset_interrupt_state();
                InlineLoopAction::CyclePrimaryAgentPrevious
            }
            InlineEvent::SelectPrimaryAgent { name } => {
                self.state.reset_interrupt_state();
                InlineLoopAction::SelectPrimaryAgent { name }
            }
            InlineEvent::ToggleToolDisplayMode => {
                self.state.reset_interrupt_state();
                let mode = self.state.renderer().toggle_tool_display_mode();
                let label = match mode {
                    vtcode_core::config::ToolDisplayMode::Compact => "compact",
                    vtcode_core::config::ToolDisplayMode::Expanded | vtcode_core::config::ToolDisplayMode::Unknown => {
                        "expanded"
                    }
                };
                self.state
                    .renderer()
                    .line(MessageStyle::Status, &format!("Tool summaries: {label}"))?;
                InlineLoopAction::Continue
            }
            InlineEvent::OpenToolOutputInEditor(text) => {
                self.state.reset_interrupt_state();
                InlineLoopAction::OpenToolOutputInEditor(text)
            }
            InlineEvent::OpenToolOutputScrollback(text) => {
                self.state.reset_interrupt_state();
                InlineLoopAction::OpenToolOutputScrollback(text)
            }
            InlineEvent::OpenFileInEditor(path) => {
                self.state.reset_interrupt_state();
                // The TUI event callback may have forwarded this same event
                // instance out-of-band already (immediate mid-turn open); the
                // dispatcher pairs the two deliveries so the click opens
                // exactly once.
                if let (Some(sender), Some(dispatcher)) =
                    (self.editor_open_sender.as_ref(), self.editor_open_dispatcher.as_ref())
                {
                    dispatcher.try_forward_deferred(sender, &path, &self.editor_workspace);
                }
                InlineLoopAction::Continue
            }
            InlineEvent::OpenUrl(url) => {
                if let Some(reference) = url.strip_prefix("vtcode-evidence:") {
                    let mut fields = reference.split(':');
                    let session = fields.next();
                    let offset = fields.next().and_then(|s| s.parse::<u64>().ok());
                    let digest = fields.next();
                    if fields.next().is_none()
                        && let Some(emitter) = self.harness_emitter
                        && let (Some(session), Some(offset), Some(digest)) = (session, offset, digest)
                    {
                        let evidence = async {
                            let model = emitter
                                .explanation(vtcode_memory::explanation::ExplanationScope::Session)
                                .await?;
                            let reference = model
                                .evidence_references()
                                .into_iter()
                                .find(|e| e.session_id == session && e.offset == offset && e.digest == digest)
                                .context("evidence expired")?;
                            emitter.evidence(reference, 0).await
                        }
                        .await;
                        match evidence {
                            Ok(page) => {
                                self.handle.review_evidence(page.text.lines().map(str::to_owned).collect());
                            }
                            Err(_) => self
                                .state
                                .renderer()
                                .line(MessageStyle::Warning, "Evidence is unavailable or expired.")?,
                        }
                    }
                    return Ok(InlineLoopAction::Continue);
                }
                self.state.reset_interrupt_state();
                self.modal.request_url_guard(self.state.renderer(), url)?
            }

            InlineEvent::ScrollLineUp
            | InlineEvent::ScrollLineDown
            | InlineEvent::ScrollPageUp
            | InlineEvent::ScrollPageDown
            | InlineEvent::JumpToLastChange
            | InlineEvent::FileSelected(_)
            | InlineEvent::HistoryPrevious
            | InlineEvent::HistoryNext => self.input_processor().passive(),
        };

        Ok(action)
    }

    async fn submit_to_focused_exec_session(&mut self, input: SubmittedInput) -> Result<InlineLoopAction> {
        // Slash commands are application actions, not stdin for the focused
        // process. This also keeps keyboard-generated commands such as
        // `/config subprocess` and `/model` usable while a session is focused.
        if input.text.trim_start().starts_with('/') {
            return Ok(self.input_processor().submit(input));
        }

        let Some(exec_sessions) = self.exec_sessions.clone() else {
            return Ok(self.input_processor().submit(input));
        };
        let Some(session_id) = exec_sessions.focused_session_id() else {
            return Ok(self.input_processor().submit(input));
        };

        if input.has_attachments() {
            self.state.renderer().line(
                MessageStyle::Warning,
                "Focused exec sessions accept text input only; remove attachments before sending a line.",
            )?;
            self.modal.restore_input_draft(input);
            return Ok(self.input_processor().passive());
        }

        let line = input.text.clone();
        match exec_sessions.send_input_to_session(&session_id, line.as_bytes(), true).await {
            Ok(_) => {
                self.state
                    .renderer()
                    .line(MessageStyle::Info, &format!("Sent input to exec session {session_id}."))?;
            }
            Err(error) => {
                exec_sessions.clear_focused_session();
                self.modal.restore_input_draft(input);
                self.state.renderer().line(
                    MessageStyle::Error,
                    &format!("Failed to send input to exec session {session_id}: {error}"),
                )?;
            }
        }
        Ok(self.input_processor().passive())
    }

    /// Escape hatch: force-stop foreground exec sessions so a stuck PTY
    /// cannot keep the runloop loading and the composer locked. Background
    /// sessions are user-owned and stay untouched.
    async fn handle_force_cancel_pty_session(&mut self) -> Result<InlineLoopAction> {
        self.state.reset_interrupt_state();
        let Some(exec_sessions) = self.exec_sessions.clone() else {
            self.state
                .renderer()
                .line(MessageStyle::Warning, "No exec session manager is available to force-cancel.")?;
            return Ok(self.input_processor().passive());
        };

        self.state
            .renderer()
            .line(MessageStyle::Status, "Force-cancelling foreground exec sessions...")?;
        let (stopped, closed, failed) = exec_sessions.force_cancel_foreground_sessions().await;
        if stopped + closed + failed == 0 {
            self.state
                .renderer()
                .line(MessageStyle::Info, "No foreground exec sessions to force-cancel.")?;
        } else {
            let mut parts = Vec::new();
            if stopped > 0 {
                parts.push(format!("{stopped} force-terminated"));
            }
            if closed > 0 {
                parts.push(format!("{closed} closed"));
            }
            if failed > 0 {
                parts.push(format!("{failed} failed"));
            }
            let style = if failed > 0 {
                MessageStyle::Warning
            } else {
                MessageStyle::Info
            };
            self.state
                .renderer()
                .line(style, &format!("Force-cancel complete: {}.", parts.join(", ")))?;
        }
        Ok(self.input_processor().passive())
    }

    async fn handle_exec_session_action(
        &mut self,
        session_id: String,
        action: ExecSessionAction,
    ) -> Result<InlineLoopAction> {
        self.state.reset_interrupt_state();
        let Some(exec_sessions) = self.exec_sessions.clone() else {
            self.state
                .renderer()
                .line(MessageStyle::Error, "Exec session manager is not available.")?;
            return Ok(self.input_processor().passive());
        };

        self.handle.hide_local_agents();
        match action {
            ExecSessionAction::Inspect => {
                let snapshot = match exec_sessions.background_session_snapshot(&session_id).await {
                    Ok(snapshot) => snapshot,
                    Err(error) => return self.render_exec_session_error(&session_id, action, error),
                };
                self.render_exec_session_inspection(&snapshot)?;
            }
            ExecSessionAction::Preview => {
                let snapshot = match exec_sessions.background_session_snapshot(&session_id).await {
                    Ok(snapshot) => snapshot,
                    Err(error) => return self.render_exec_session_error(&session_id, action, error),
                };
                self.handle.show_modal(
                    format!("Exec session {}", snapshot.metadata.id.as_str()),
                    exec_session_modal_lines(&snapshot),
                    None,
                );
            }
            ExecSessionAction::GracefulTerminate => {
                // Coordinate with the runloop's live backend state before
                // signalling. Terminating an already-exited session is a
                // no-op kill that would misreport as a fresh termination;
                // surface the truthful `exited (code)` status instead and
                // retain the preview for inspection. `is_session_completed`
                // also clears focus/pending promotion so the composer stops
                // routing input to a dead session.
                match exec_sessions.is_session_completed(&session_id).await {
                    Ok(Some(code)) => {
                        let _ = exec_sessions.background_session_snapshot(&session_id).await;
                        self.state.renderer().line(
                            MessageStyle::Info,
                            &format!(
                                "Exec session {session_id} already exited ({code}); output retained. Use ForceTerminateOrClose to close it."
                            ),
                        )?;
                    }
                    Ok(None) => {
                        if let Err(error) = exec_sessions.terminate_session(&session_id).await {
                            return self.render_exec_session_error(&session_id, action, error);
                        }
                        // Peek the retained preview so the drawer/inspect path
                        // shows output captured up to termination.
                        let _ = exec_sessions.background_session_snapshot(&session_id).await;
                        self.state.renderer().line(
                            MessageStyle::Info,
                            &format!("Requested graceful termination for exec session {session_id}."),
                        )?;
                    }
                    Err(error) => return self.render_exec_session_error(&session_id, action, error),
                }
            }
            ExecSessionAction::ForceTerminateOrClose => {
                let already_exited = match exec_sessions.force_terminate_or_close(&session_id).await {
                    Ok(already_exited) => already_exited,
                    Err(error) => return self.render_exec_session_error(&session_id, action, error),
                };
                let message = if already_exited {
                    format!("Closed completed exec session {session_id}.")
                } else {
                    format!("Force-terminated exec session {session_id}; it remains visible until closed.")
                };
                self.state.renderer().line(MessageStyle::Info, &message)?;
            }
            ExecSessionAction::Focus => {
                if exec_sessions.focused_session_id().as_deref() == Some(session_id.as_str()) {
                    exec_sessions.clear_focused_session();
                    self.state.renderer().line(
                        MessageStyle::Info,
                        &format!("Unfocused exec session {session_id}; submitted lines return to the composer."),
                    )?;
                } else {
                    if let Err(error) = exec_sessions.focus_background_session(&session_id).await {
                        return self.render_exec_session_error(&session_id, action, error);
                    }
                    self.state.renderer().line(
                        MessageStyle::Info,
                        &format!("Focused exec session {session_id}; submitted lines go to its stdin."),
                    )?;
                }
            }
        }

        Ok(self.input_processor().passive())
    }

    fn render_exec_session_error(
        &mut self,
        session_id: &str,
        action: ExecSessionAction,
        error: anyhow::Error,
    ) -> Result<InlineLoopAction> {
        // Session-891943: the only debug ERROR was a UI action on an already
        // exited session whose result was recorded. Log the action and cause
        // chain so a missing session is not mistaken for a lost verifier
        // result, and tell the user to reuse recorded output.
        tracing::warn!(
            session_id = %session_id,
            action = ?action,
            error = %format!("{error:#}"),
            "exec session action failed"
        );
        self.state.renderer().line(
            MessageStyle::Error,
            &format!(
                "Exec session {session_id} action {action:?} failed: {error:#}. If its command already exited, reuse the recorded output instead of rerunning."
            ),
        )?;
        Ok(self.input_processor().passive())
    }

    fn render_exec_session_inspection(
        &mut self,
        snapshot: &vtcode_core::tools::exec_session::ExecSessionUiSnapshot,
    ) -> Result<()> {
        let metadata = &snapshot.metadata;
        self.state.renderer().line(
            MessageStyle::Info,
            &format!("Exec session {}: {}", metadata.id.as_str(), metadata.command_label()),
        )?;
        self.state.renderer().line(
            MessageStyle::Output,
            &format!(
                "Status: {} · cwd {} · pid {}",
                metadata.status_label(),
                metadata.working_dir.as_deref().unwrap_or("unknown"),
                metadata.child_pid.map_or_else(|| "-".to_string(), |pid| pid.to_string())
            ),
        )?;
        for line in bounded_preview_or_placeholder(&snapshot.preview).lines() {
            self.state.renderer().line(MessageStyle::Output, line)?;
        }
        Ok(())
    }

    fn handle_interrupt(&mut self) -> InlineLoopAction {
        let _ = self.modal.handle_cancel(self.state.renderer());
        // Esc / Ctrl+C from the TUI is a local cancellation request. In raw
        // mode crossterm delivers Ctrl+C as a key event, so route it through a
        // cancellation-only path and reserve emergency double-signal exit for
        // the OS signal handler.
        crate::agent::runloop::unified::stop_requests::request_local_cancel(self.ctrl_c_state, self.ctrl_c_notify);
        InlineLoopAction::Continue
    }

    fn input_processor(&mut self) -> InlineInputProcessor<'_, 'a> {
        InlineInputProcessor::new(&mut self.state)
    }

    fn control_processor(&mut self) -> InlineControlProcessor<'_, 'a> {
        InlineControlProcessor::new(&mut self.state)
    }
}

fn bounded_preview_or_placeholder(preview: &str) -> &str {
    if preview.trim().is_empty() {
        "(no output yet)"
    } else {
        preview
    }
}

fn exec_session_modal_lines(snapshot: &vtcode_core::tools::exec_session::ExecSessionUiSnapshot) -> Vec<String> {
    let metadata = &snapshot.metadata;
    vec![
        format!("Command: {}", metadata.command_label()),
        format!("Status: {}", metadata.status_label()),
        format!("Working dir: {}", metadata.working_dir.as_deref().unwrap_or("unknown")),
        format!("PID: {}", metadata.child_pid.map_or_else(|| "-".to_string(), |pid| pid.to_string())),
        format!("Preview:\n{}", bounded_preview_or_placeholder(&snapshot.preview)),
    ]
}
