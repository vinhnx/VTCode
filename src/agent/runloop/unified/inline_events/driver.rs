use std::collections::VecDeque;

use anyhow::Result;
use tokio::sync::Notify;

use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::core::interfaces::ui::UiSession;
use vtcode_core::llm::provider::{self as uni};
use vtcode_core::ui::tui::{InlineHandle, InlineSession};
use vtcode_core::utils::ansi::AnsiRenderer;

use crate::agent::runloop::model_picker::ModelPickerState;
use crate::agent::runloop::unified::palettes::ActivePalette;
use crate::agent::runloop::welcome::SessionBootstrap;

use super::{InlineEventContext, InlineInterruptCoordinator, InlineLoopAction, InlineQueueState};

struct InlineEventLoop<'a> {
    renderer: &'a mut AnsiRenderer,
    handle: &'a InlineHandle,
    interrupts: InlineInterruptCoordinator<'a>,
    ctrl_c_notice_displayed: &'a mut bool,
    default_placeholder: &'a Option<String>,
    queue: InlineQueueState<'a>,
    model_picker_state: &'a mut Option<ModelPickerState>,
    palette_state: &'a mut Option<ActivePalette>,
    config: &'a mut CoreAgentConfig,
    vt_cfg: &'a mut Option<VTCodeConfig>,
    provider_client: &'a mut Box<dyn uni::LLMProvider>,
    session_bootstrap: &'a SessionBootstrap,
    full_auto: bool,
}

impl<'a> InlineEventLoop<'a> {
    fn new(resources: InlineEventLoopResources<'a>) -> Self {
        let InlineEventLoopResources {
            renderer,
            handle,
            interrupts,
            ctrl_c_notice_displayed,
            default_placeholder,
            queued_inputs,
            model_picker_state,
            palette_state,
            config,
            vt_cfg,
            provider_client,
            session_bootstrap,
            full_auto,
        } = resources;

        Self {
            renderer,
            handle,
            interrupts,
            ctrl_c_notice_displayed,
            default_placeholder,
            queue: InlineQueueState::new(handle, queued_inputs),
            model_picker_state,
            palette_state,
            config,
            vt_cfg,
            provider_client,
            session_bootstrap,
            full_auto,
        }
    }

    async fn poll(
        mut self,
        session: &mut InlineSession,
        ctrl_c_notify: &Notify,
    ) -> Result<InlineLoopAction> {
        if let Some(action) = self.ensure_interrupt_notice()? {
            return Ok(action);
        }

        if let Some(action) = self.take_queued_submission() {
            return Ok(action);
        }

        let maybe_event = tokio::select! {
            biased;

            _ = ctrl_c_notify.notified() => None,
            event = session.next_event() => event,
        };

        if let Some(action) = self.exit_action() {
            return Ok(action);
        }

        if let Some(action) = self.ensure_interrupt_notice()? {
            return Ok(action);
        }

        let Some(event) = maybe_event else {
            return Ok(InlineLoopAction::Continue);
        };

        let interrupts = self.interrupts;
        let handle = self.handle;
        let session_bootstrap = self.session_bootstrap;
        let full_auto = self.full_auto;
        let ctrl_c_notice_displayed = &mut *self.ctrl_c_notice_displayed;
        let renderer = &mut *self.renderer;
        let model_picker_state = &mut *self.model_picker_state;
        let palette_state = &mut *self.palette_state;
        let config = &mut *self.config;
        let vt_cfg = &mut *self.vt_cfg;
        let provider_client = &mut *self.provider_client;

        let mut context = InlineEventContext::new(
            renderer,
            handle,
            interrupts,
            ctrl_c_notice_displayed,
            model_picker_state,
            palette_state,
            config,
            vt_cfg,
            provider_client,
            session_bootstrap,
            full_auto,
        );

        context.process_event(event, &mut self.queue).await
    }

    fn ensure_interrupt_notice(&mut self) -> Result<Option<InlineLoopAction>> {
        if self.interrupts.ensure_notice_displayed(
            self.ctrl_c_notice_displayed,
            self.renderer,
            self.handle,
            self.default_placeholder,
            &mut self.queue,
        )? {
            return Ok(Some(InlineLoopAction::Continue));
        }

        Ok(None)
    }

    fn take_queued_submission(&mut self) -> Option<InlineLoopAction> {
        self.queue.pop_front().map(|queued| {
            if queued.is_empty() {
                InlineLoopAction::Continue
            } else {
                InlineLoopAction::Submit(queued)
            }
        })
    }

    fn exit_action(&self) -> Option<InlineLoopAction> {
        match self.interrupts.action_for_interrupt() {
            InlineLoopAction::Exit(reason) => Some(InlineLoopAction::Exit(reason)),
            InlineLoopAction::Continue => None,
            InlineLoopAction::Submit(_) => None,
        }
    }
}

pub(crate) struct InlineEventLoopResources<'a> {
    pub renderer: &'a mut AnsiRenderer,
    pub handle: &'a InlineHandle,
    pub interrupts: InlineInterruptCoordinator<'a>,
    pub ctrl_c_notice_displayed: &'a mut bool,
    pub default_placeholder: &'a Option<String>,
    pub queued_inputs: &'a mut VecDeque<String>,
    pub model_picker_state: &'a mut Option<ModelPickerState>,
    pub palette_state: &'a mut Option<ActivePalette>,
    pub config: &'a mut CoreAgentConfig,
    pub vt_cfg: &'a mut Option<VTCodeConfig>,
    pub provider_client: &'a mut Box<dyn uni::LLMProvider>,
    pub session_bootstrap: &'a SessionBootstrap,
    pub full_auto: bool,
}

pub(crate) async fn poll_inline_loop_action(
    session: &mut InlineSession,
    ctrl_c_notify: &Notify,
    resources: InlineEventLoopResources<'_>,
) -> Result<InlineLoopAction> {
    InlineEventLoop::new(resources)
        .poll(session, ctrl_c_notify)
        .await
}
