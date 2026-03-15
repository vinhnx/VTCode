use std::collections::VecDeque;
use std::time::Duration;

use anyhow::Result;
use tokio::sync::Notify;

use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::llm::provider::{self as uni};
use vtcode_core::utils::ansi::AnsiRenderer;
use vtcode_tui::{InlineHandle, InlineHeaderContext, InlineSession};

use crate::agent::runloop::model_picker::ModelPickerState;
use crate::agent::runloop::unified::palettes::ActivePalette;
use crate::agent::runloop::welcome::SessionBootstrap;
use crate::updater::{StartupUpdateNotice, display_update_notice};

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
    startup_update_notice_rx:
        &'a mut Option<tokio::sync::mpsc::UnboundedReceiver<StartupUpdateNotice>>,
    header_context: &'a mut InlineHeaderContext,
    use_unicode: bool,
    conversation_history_len: usize,
}

enum StartupUpdateEvent {
    Notice(StartupUpdateNotice),
    Closed,
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
            prefer_latest_queued_input_once,
            model_picker_state,
            palette_state,
            config,
            vt_cfg,
            provider_client,
            session_bootstrap,
            full_auto,
            startup_update_notice_rx,
            header_context,
            use_unicode,
            conversation_history_len,
        } = resources;

        Self {
            renderer,
            handle,
            interrupts,
            ctrl_c_notice_displayed,
            default_placeholder,
            queue: InlineQueueState::new(handle, queued_inputs, prefer_latest_queued_input_once),
            model_picker_state,
            palette_state,
            config,
            vt_cfg,
            provider_client,
            session_bootstrap,
            full_auto,
            startup_update_notice_rx,
            header_context,
            use_unicode,
            conversation_history_len,
        }
    }

    async fn poll(
        mut self,
        session: &mut InlineSession,
        ctrl_c_notify: &Notify,
    ) -> Result<InlineLoopAction> {
        const INLINE_EVENT_POLL_TICK: Duration = Duration::from_millis(500);

        if let Some(action) = self.ensure_interrupt_notice()? {
            return Ok(action);
        }

        if let Some(action) = self.take_queued_submission() {
            return Ok(action);
        }

        let maybe_event = tokio::select! {
            biased;

            notice = recv_startup_update_notice(self.startup_update_notice_rx) => {
                match notice {
                    StartupUpdateEvent::Notice(notice) => {
                        display_update_notice(
                            self.handle,
                            self.header_context,
                            self.use_unicode,
                            &notice,
                        );
                    }
                    StartupUpdateEvent::Closed => {}
                }
                None
            }
            _ = ctrl_c_notify.notified() => None,
            _ = tokio::time::sleep(INLINE_EVENT_POLL_TICK) => None,
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
        let conversation_history_len = self.conversation_history_len;
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
            conversation_history_len,
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
        self.queue.take_next_submission().map(|queued| {
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
            InlineLoopAction::ResumeSession(_) => None,
            InlineLoopAction::ForkSession(_) => None,
            InlineLoopAction::PlanApproved { .. } => None,
            InlineLoopAction::PlanEditRequested => None,
            InlineLoopAction::DiffApproved => None,
            InlineLoopAction::DiffRejected => None,
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
    pub prefer_latest_queued_input_once: &'a mut bool,
    pub model_picker_state: &'a mut Option<ModelPickerState>,
    pub palette_state: &'a mut Option<ActivePalette>,
    pub config: &'a mut CoreAgentConfig,
    pub vt_cfg: &'a mut Option<VTCodeConfig>,
    pub provider_client: &'a mut Box<dyn uni::LLMProvider>,
    pub session_bootstrap: &'a SessionBootstrap,
    pub full_auto: bool,
    pub startup_update_notice_rx:
        &'a mut Option<tokio::sync::mpsc::UnboundedReceiver<StartupUpdateNotice>>,
    pub header_context: &'a mut InlineHeaderContext,
    pub use_unicode: bool,
    pub conversation_history_len: usize,
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

async fn recv_startup_update_notice(
    receiver: &mut Option<tokio::sync::mpsc::UnboundedReceiver<StartupUpdateNotice>>,
) -> StartupUpdateEvent {
    match receiver.as_mut() {
        Some(rx) => match rx.recv().await {
            Some(notice) => StartupUpdateEvent::Notice(notice),
            None => {
                *receiver = None;
                StartupUpdateEvent::Closed
            }
        },
        None => std::future::pending().await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use semver::Version;

    #[tokio::test]
    async fn closed_update_receiver_is_cleared() {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        drop(tx);
        let mut receiver = Some(rx);

        let event = recv_startup_update_notice(&mut receiver).await;
        assert!(matches!(event, StartupUpdateEvent::Closed));
        assert!(receiver.is_none());
    }

    #[tokio::test]
    async fn notice_receiver_returns_notice_without_clearing_channel() {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let updater = crate::updater::Updater::new("0.111.0").expect("updater");
        tx.send(updater.notice_for_version(Version::parse("0.113.0").expect("version")))
            .expect("send notice");
        let mut receiver = Some(rx);

        let event = recv_startup_update_notice(&mut receiver).await;
        assert!(matches!(event, StartupUpdateEvent::Notice(_)));
        assert!(receiver.is_some());
    }
}
