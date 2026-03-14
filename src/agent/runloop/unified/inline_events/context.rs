use anyhow::Result;

use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::llm::provider::{self as uni};
use vtcode_core::utils::ansi::AnsiRenderer;
use vtcode_tui::{
    InlineEvent, InlineHandle, OverlayEvent, OverlayHotkeyAction, OverlaySelectionChange,
    OverlaySubmission,
};

use crate::agent::runloop::model_picker::ModelPickerState;
use crate::agent::runloop::unified::palettes::ActivePalette;
use crate::agent::runloop::welcome::SessionBootstrap;

use super::action::InlineLoopAction;
use super::control::InlineControlProcessor;
use super::input::InlineInputProcessor;
use super::interrupts::InlineInterruptCoordinator;
use super::modal::InlineModalProcessor;
use super::queue::InlineQueueState;
use super::state::InlineEventState;

pub(crate) struct InlineEventContext<'a> {
    state: InlineEventState<'a>,
    modal: InlineModalProcessor<'a>,
}

impl<'a> InlineEventContext<'a> {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        renderer: &'a mut AnsiRenderer,
        handle: &'a InlineHandle,
        interrupts: InlineInterruptCoordinator<'a>,
        ctrl_c_notice_displayed: &'a mut bool,
        model_picker_state: &'a mut Option<ModelPickerState>,
        palette_state: &'a mut Option<ActivePalette>,
        config: &'a mut CoreAgentConfig,
        vt_cfg: &'a mut Option<VTCodeConfig>,
        provider_client: &'a mut Box<dyn uni::LLMProvider>,
        session_bootstrap: &'a SessionBootstrap,
        full_auto: bool,
    ) -> Self {
        let state = InlineEventState::new(renderer, interrupts, ctrl_c_notice_displayed);
        let modal = InlineModalProcessor::new(
            handle,
            model_picker_state,
            palette_state,
            config,
            vt_cfg,
            provider_client,
            session_bootstrap,
            full_auto,
        );

        Self { state, modal }
    }

    pub(crate) async fn process_event(
        &mut self,
        event: InlineEvent,
        queue: &mut InlineQueueState<'_>,
    ) -> Result<InlineLoopAction> {
        let action = match event {
            InlineEvent::Submit(text) => self.input_processor().submit(text),
            InlineEvent::QueueSubmit(text) => self.input_processor().queue_submit(text, queue),
            InlineEvent::ProcessLatestQueued => {
                self.state.reset_interrupt_state();
                queue.prefer_latest_next();
                InlineLoopAction::Continue
            }
            InlineEvent::Steer(_) | InlineEvent::Pause | InlineEvent::Resume => {
                self.state.reset_interrupt_state();
                self.input_processor().passive()
            }
            InlineEvent::EditQueue => {
                self.state.reset_interrupt_state();
                queue.edit_latest();
                InlineLoopAction::Continue
            }
            InlineEvent::Overlay(overlay_event) => match overlay_event {
                OverlayEvent::SelectionChanged(OverlaySelectionChange::List(selection)) => self
                    .modal
                    .handle_preview(self.state.renderer(), selection)?,
                OverlayEvent::SelectionChanged(OverlaySelectionChange::DiffTrustMode {
                    ..
                }) => {
                    self.state.reset_interrupt_state();
                    self.input_processor().passive()
                }
                OverlayEvent::Submitted(OverlaySubmission::Selection(selection)) => {
                    self.state.reset_interrupt_state();
                    self.modal
                        .handle_submit(self.state.renderer(), selection)
                        .await?
                }
                OverlayEvent::Submitted(OverlaySubmission::Wizard(_)) => {
                    self.state.reset_interrupt_state();
                    self.input_processor().passive()
                }
                OverlayEvent::Submitted(OverlaySubmission::DiffApply) => {
                    self.state.reset_interrupt_state();
                    InlineLoopAction::DiffApproved
                }
                OverlayEvent::Submitted(OverlaySubmission::DiffReject) => {
                    self.state.reset_interrupt_state();
                    InlineLoopAction::DiffRejected
                }
                OverlayEvent::Submitted(
                    OverlaySubmission::DiffProceed
                    | OverlaySubmission::DiffReload
                    | OverlaySubmission::DiffAbort,
                ) => {
                    self.state.reset_interrupt_state();
                    self.input_processor().passive()
                }
                OverlayEvent::Submitted(OverlaySubmission::Hotkey(action)) => {
                    self.state.reset_interrupt_state();
                    match action {
                        OverlayHotkeyAction::LaunchEditor => {
                            self.input_processor().submit("/edit".to_string())
                        }
                    }
                }
                OverlayEvent::Cancelled => {
                    self.state.reset_interrupt_state();
                    self.modal.handle_cancel(self.state.renderer())?
                }
            },
            InlineEvent::Cancel => self.control_processor().cancel()?,
            InlineEvent::ForceCancelPtySession => {
                self.control_processor().force_cancel_pty_session()?
            }
            InlineEvent::Exit => self.control_processor().exit()?,
            InlineEvent::Interrupt => self.handle_interrupt(),
            InlineEvent::BackgroundOperation => {
                // Ctrl+B pressed: handle background operation
                self.input_processor().passive()
            }
            InlineEvent::LaunchEditor => {
                // Ctrl+E pressed: submit /edit command
                self.input_processor().submit("/edit".to_string())
            }
            InlineEvent::OpenFileInEditor(path) => {
                self.input_processor().submit(format!("/edit {}", path))
            }
            InlineEvent::OpenUrl(url) => {
                self.state.reset_interrupt_state();
                let _ = webbrowser::open(&url);
                InlineLoopAction::Continue
            }

            InlineEvent::ScrollLineUp
            | InlineEvent::ScrollLineDown
            | InlineEvent::ScrollPageUp
            | InlineEvent::ScrollPageDown
            | InlineEvent::FileSelected(_)
            | InlineEvent::HistoryPrevious
            | InlineEvent::HistoryNext => self.input_processor().passive(),
            InlineEvent::ToggleMode => {
                // Shift+Tab: Cycle editing modes via /mode command
                self.input_processor().submit("/mode".to_string())
            }
        };

        Ok(action)
    }

    fn handle_interrupt(&self) -> InlineLoopAction {
        self.state.interrupts().action_for_interrupt()
    }

    fn input_processor(&mut self) -> InlineInputProcessor<'_, 'a> {
        InlineInputProcessor::new(&mut self.state)
    }

    fn control_processor(&mut self) -> InlineControlProcessor<'_, 'a> {
        InlineControlProcessor::new(&mut self.state)
    }
}
