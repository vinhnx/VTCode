use anyhow::Result;

use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::llm::provider::{self as uni};
use vtcode_core::utils::ansi::AnsiRenderer;
use vtcode_tui::{InlineEvent, InlineHandle};

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
    team_active: bool,
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
        team_active: bool,
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

        Self {
            state,
            modal,
            team_active,
        }
    }

    pub(crate) async fn process_event(
        &mut self,
        event: InlineEvent,
        queue: &mut InlineQueueState<'_>,
    ) -> Result<InlineLoopAction> {
        let action = match event {
            InlineEvent::Submit(text) => self.input_processor().submit(text),
            InlineEvent::QueueSubmit(text) => self.input_processor().queue_submit(text, queue),
            InlineEvent::EditQueue => {
                self.state.reset_interrupt_state();
                queue.edit_latest();
                InlineLoopAction::Continue
            }
            InlineEvent::ListModalSubmit(selection) => {
                self.state.reset_interrupt_state();
                self.modal
                    .handle_submit(self.state.renderer(), selection)
                    .await?
            }
            InlineEvent::ListModalCancel => {
                self.state.reset_interrupt_state();
                self.modal.handle_cancel(self.state.renderer())?
            }
            InlineEvent::WizardModalSubmit(_) => {
                self.state.reset_interrupt_state();
                self.input_processor().passive()
            }
            InlineEvent::WizardModalStepComplete { .. } => {
                self.state.reset_interrupt_state();
                self.input_processor().passive()
            }
            InlineEvent::WizardModalBack { .. } => {
                self.state.reset_interrupt_state();
                self.input_processor().passive()
            }
            InlineEvent::WizardModalCancel => {
                self.state.reset_interrupt_state();
                self.input_processor().passive()
            }
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

            InlineEvent::ScrollLineUp
            | InlineEvent::ScrollLineDown
            | InlineEvent::ScrollPageUp
            | InlineEvent::ScrollPageDown
            | InlineEvent::FileSelected(_)
            | InlineEvent::HistoryPrevious
            | InlineEvent::HistoryNext => self.input_processor().passive(),
            InlineEvent::ToggleMode => {
                if self.team_active {
                    InlineLoopAction::ToggleDelegateMode
                } else {
                    // Shift+Tab: Cycle editing modes via /mode command
                    self.input_processor().submit("/mode".to_string())
                }
            }
            InlineEvent::TeamPrev => InlineLoopAction::SwitchTeammate(
                crate::agent::runloop::unified::inline_events::TeamSwitchDirection::Previous,
            ),
            InlineEvent::TeamNext => InlineLoopAction::SwitchTeammate(
                crate::agent::runloop::unified::inline_events::TeamSwitchDirection::Next,
            ),
            InlineEvent::PlanConfirmation(result) => {
                use vtcode_tui::PlanConfirmationResult;
                // Handle plan confirmation result (Claude Code style HITL)
                match result {
                    PlanConfirmationResult::Execute => InlineLoopAction::PlanApproved {
                        auto_accept: false,
                        clear_context: false,
                    },
                    PlanConfirmationResult::AutoAccept => InlineLoopAction::PlanApproved {
                        auto_accept: true,
                        clear_context: false,
                    },
                    PlanConfirmationResult::ClearContextAutoAccept => {
                        InlineLoopAction::PlanApproved {
                            auto_accept: true,
                            clear_context: true,
                        }
                    }
                    PlanConfirmationResult::EditPlan => InlineLoopAction::PlanEditRequested,
                    PlanConfirmationResult::Cancel => InlineLoopAction::Continue,
                }
            }
            InlineEvent::DiffPreviewApply => {
                self.state.reset_interrupt_state();
                InlineLoopAction::DiffApproved
            }
            InlineEvent::DiffPreviewReject => {
                self.state.reset_interrupt_state();
                InlineLoopAction::DiffRejected
            }
            InlineEvent::DiffPreviewTrustChanged { .. } => {
                self.state.reset_interrupt_state();
                self.input_processor().passive()
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
