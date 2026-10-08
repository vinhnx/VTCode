use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use tokio::sync::Notify;

use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::exec::events::{InterjectedEvent, InterjectionSource, RedirectKind, ThreadEvent};
use vtcode_core::hooks::LifecycleHookEngine;
use vtcode_core::llm::provider::{self as uni};
use vtcode_core::utils::ansi::AnsiRenderer;
use vtcode_ui::tui::app::{InlineEvent, InlineHandle, InlineHeaderContext, InlineSession, SubmittedInput};

use crate::agent::runloop::model_picker::ModelPickerState;
use crate::agent::runloop::unified::context_manager::ContextManager;
use crate::agent::runloop::unified::inline_events::harness::HarnessEventEmitter;
use crate::agent::runloop::unified::palettes::ActivePalette;
use crate::agent::runloop::unified::session_setup::{EditorOpenDispatcher, EditorOpenRequestSender};
use crate::agent::runloop::unified::state::{CtrlCState, SessionStats};
use crate::agent::runloop::welcome::SessionBootstrap;
use crate::updater::{StartupUpdateNotice, display_update_notice};

use super::{InlineEventContext, InlineInterruptCoordinator, InlineLoopAction, InlineQueueState, QueuedInput};

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
    ctrl_c_state: &'a Arc<CtrlCState>,
    ctrl_c_notify: &'a Arc<Notify>,
    session_bootstrap: &'a SessionBootstrap,
    full_auto: bool,
    startup_update_notice_rx: &'a mut Option<tokio::sync::mpsc::UnboundedReceiver<StartupUpdateNotice>>,
    header_context: &'a mut InlineHeaderContext,
    use_unicode: bool,
    conversation_history: &'a mut Vec<uni::Message>,
    session_stats: &'a mut SessionStats,
    context_manager: &'a mut ContextManager,
    session_id: &'a str,
    thread_id: &'a str,
    lifecycle_hooks: Option<&'a LifecycleHookEngine>,
    harness_emitter: Option<&'a HarnessEventEmitter>,
    editor_open_sender: &'a EditorOpenRequestSender,
    editor_open_dispatcher: Arc<EditorOpenDispatcher>,
    exec_sessions: Option<vtcode_core::tools::exec_session::ExecSessionManager>,
    background_completion_notify: Option<Arc<Notify>>,
    exec_completion_notify: Option<Arc<Notify>>,
    webmcp_prompt_receiver: &'a mut Option<tokio::sync::mpsc::Receiver<String>>,
    idle_wake_delay: Duration,
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
            ctrl_c_state,
            ctrl_c_notify,
            session_bootstrap,
            full_auto,
            startup_update_notice_rx,
            header_context,
            use_unicode,
            conversation_history,
            session_stats,
            context_manager,
            session_id,
            thread_id,
            lifecycle_hooks,
            harness_emitter,
            editor_open_sender,
            editor_open_dispatcher,
            exec_sessions,
            background_completion_notify,
            exec_completion_notify,
            webmcp_prompt_receiver,
            idle_wake_delay,
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
            ctrl_c_state,
            ctrl_c_notify,
            startup_update_notice_rx,
            header_context,
            use_unicode,
            conversation_history,
            session_stats,
            context_manager,
            session_id,
            thread_id,
            lifecycle_hooks,
            harness_emitter,
            editor_open_sender,
            editor_open_dispatcher,
            exec_sessions,
            background_completion_notify,
            exec_completion_notify,
            webmcp_prompt_receiver,
            idle_wake_delay,
        }
    }

    async fn poll(mut self, session: &mut InlineSession, ctrl_c_notify: &Arc<Notify>) -> Result<InlineLoopAction> {
        if let Some(action) = self.ensure_interrupt_notice()? {
            return Ok(action);
        }

        // Consume every already-buffered event BEFORE taking a submission.
        // Otherwise each queued message dispatched as its own turn because
        // buffered events trickled in one per interaction cycle and the queue
        // never held more than a single item at a boundary. Queue-affecting
        // events return Continue so the drain keeps folding them into the
        // queue; the first real action (direct submit, exit, …) dispatches
        // immediately and stops the drain. A transient processing error is
        // recorded but does not abandon the remaining buffered events; it is
        // surfaced only after the drain finishes.
        let mut drain_error = None;
        // A burst of buffered events (large paste, resize storm, held key) is
        // the async analogue of Redis request pipelining: every `try_recv` is
        // immediately ready, so draining without yielding monopolizes the
        // runtime worker and starves I/O and other tasks. Yield after a batch
        // to stay fair while keeping batching for the common small drain.
        const BUFFERED_EVENTS_PER_YIELD: usize = 32;
        let mut drained_since_yield = 0usize;
        while let Ok(event) = session.events.try_recv() {
            match self.process_buffered_event(event).await {
                Ok(InlineLoopAction::Continue) => {}
                Ok(action) => {
                    if let Some(err) = drain_error {
                        tracing::error!(error = %err, "inline event processing failed earlier in the drain; dispatching action anyway");
                    }
                    return Ok(action);
                }
                Err(err) => {
                    if drain_error.is_none() {
                        tracing::warn!(error = %err, "inline event processing failed; draining remaining buffered events");
                    }
                    drain_error.get_or_insert(err);
                }
            }

            drained_since_yield += 1;
            if drained_since_yield >= BUFFERED_EVENTS_PER_YIELD {
                drained_since_yield = 0;
                tokio::task::yield_now().await;
            }
        }
        // Publish one consistent overlay snapshot after the drain batch so
        // intermediate partial lists cannot wipe optimistic UI entries that
        // are still sitting in the event channel.
        self.queue.flush_sync();
        if let Some(err) = drain_error {
            return Err(err);
        }

        if let Some(action) = self.take_queued_submission() {
            return Ok(action);
        }

        // If the TUI event stream has been dropped the session cannot produce
        // further input; polling would spin a 100% CPU busy-loop because
        // next_event() resolves to None instantly on a closed channel. Exit.
        if session.events.is_closed() && !session.handle.has_deferred_event() {
            return Ok(InlineLoopAction::Exit(vtcode_core::hooks::SessionEndReason::Exit));
        }

        let maybe_event = tokio::select! {
            biased;

            event = session.next_event() => event,
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
            prompt = recv_webmcp_prompt(self.webmcp_prompt_receiver) => {
                prompt.map(|prompt| InlineEvent::WebmcpSubmit(prompt.into()))
            }
            _ = wait_for_background_completion(
                self.background_completion_notify.as_ref(),
                self.exec_completion_notify.as_ref(),
            ) => None,
            _ = ctrl_c_notify.notified() => None,
            _ = tokio::time::sleep(self.idle_wake_delay) => None,
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

        self.process_buffered_event(event).await
    }

    async fn process_buffered_event(&mut self, event: InlineEvent) -> Result<InlineLoopAction> {
        let interjection = match &event {
            InlineEvent::Submit(input) if !input.is_empty() => {
                Some((InterjectionSource::Direct, Self::count_images(input)))
            }
            InlineEvent::WebmcpSubmit(input) if !input.is_empty() => {
                Some((InterjectionSource::Queue, Self::count_images(input)))
            }
            _ => None,
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
        let ctrl_c_state = self.ctrl_c_state;
        let ctrl_c_notify = self.ctrl_c_notify;
        let conversation_history = &mut *self.conversation_history;
        let session_stats = &mut *self.session_stats;
        let context_manager = &mut *self.context_manager;
        let session_id = self.session_id;
        let thread_id = self.thread_id;
        let lifecycle_hooks = self.lifecycle_hooks;
        let harness_emitter = self.harness_emitter;
        let mut context = InlineEventContext::new(
            renderer,
            handle,
            interrupts,
            ctrl_c_notice_displayed,
            &mut *self.header_context,
            model_picker_state,
            palette_state,
            config,
            vt_cfg,
            provider_client,
            ctrl_c_state,
            ctrl_c_notify,
            session_bootstrap,
            full_auto,
            conversation_history,
            session_stats,
            context_manager,
            session_id,
            thread_id,
            lifecycle_hooks,
            harness_emitter,
        );

        context.set_editor_open_sink(self.editor_open_sender.clone(), self.editor_open_dispatcher.clone());
        if let Some(exec_sessions) = self.exec_sessions.clone() {
            context.set_exec_session_manager(exec_sessions);
        }
        let action = context.process_event(event, &mut self.queue).await?;
        self.queue.flush_sync();
        if let Some((source, image_count)) = interjection
            && matches!(&action, InlineLoopAction::Submit(_) | InlineLoopAction::SubmitPrompt(_))
        {
            let text = match &action {
                InlineLoopAction::Submit(input) | InlineLoopAction::SubmitPrompt(input) => Some(input.text.as_str()),
                _ => None,
            };
            self.emit_interjected(source, image_count, text);
        }
        Ok(action)
    }

    fn ensure_interrupt_notice(&mut self) -> Result<Option<InlineLoopAction>> {
        let handled = self.interrupts.ensure_notice_displayed(
            self.ctrl_c_notice_displayed,
            self.renderer,
            self.handle,
            self.default_placeholder,
            &mut self.queue,
        )?;
        self.queue.flush_sync();
        if handled {
            return Ok(Some(InlineLoopAction::Continue));
        }

        Ok(None)
    }

    fn count_images(input: &SubmittedInput) -> u32 {
        input.attachments.iter().filter(|part| part.is_image()).count() as u32
    }

    fn emit_interjected(&self, source: InterjectionSource, image_count: u32, text: Option<&str>) {
        let Some(emitter) = self.harness_emitter else { return };
        let _ = emitter.emit(ThreadEvent::Interjected(InterjectedEvent {
            text: text.map(|s| Box::new(s.to_owned())),
            source,
            image_count,
            redirect_kind: RedirectKind::Interjection,
        }));
    }

    fn take_queued_submission(&mut self) -> Option<InlineLoopAction> {
        if self.ctrl_c_state.is_cancel_requested()
            || self.ctrl_c_state.is_cancel_handled()
            || self.ctrl_c_state.is_exit_requested()
        {
            return None;
        }
        let queued = self.queue.take_batched_submission()?;
        self.queue.flush_sync();
        if queued.input.is_empty() {
            return Some(InlineLoopAction::Continue);
        }
        self.emit_interjected(InterjectionSource::Queue, Self::count_images(&queued.input), Some(&queued.input.text));
        Some(InlineLoopAction::SubmitQueued(queued))
    }

    fn exit_action(&self) -> Option<InlineLoopAction> {
        match self.interrupts.action_for_interrupt() {
            InlineLoopAction::Exit(reason) => Some(InlineLoopAction::Exit(reason)),
            InlineLoopAction::Continue => None,
            InlineLoopAction::Submit(_) => None,
            InlineLoopAction::SubmitPrompt(_) => None,
            InlineLoopAction::SubmitQueued(_) => None,
            InlineLoopAction::CyclePrimaryAgent => None,
            InlineLoopAction::CyclePrimaryAgentPrevious => None,
            InlineLoopAction::SelectPrimaryAgent { .. } => None,
            InlineLoopAction::RequestInlinePromptSuggestion(_) => None,
            InlineLoopAction::OpenToolOutputInEditor(_) => None,
            InlineLoopAction::OpenToolOutputScrollback(_) => None,
            InlineLoopAction::ResumeSession(_) => None,
            InlineLoopAction::ForkSession { .. } => None,
            InlineLoopAction::PlanApproved { .. } => None,
            InlineLoopAction::PlanEditRequested => None,
            InlineLoopAction::LaunchEditorWithDraft { .. } => None,
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
    pub queued_inputs: &'a mut VecDeque<QueuedInput>,
    pub prefer_latest_queued_input_once: &'a mut bool,
    pub model_picker_state: &'a mut Option<ModelPickerState>,
    pub palette_state: &'a mut Option<ActivePalette>,
    pub config: &'a mut CoreAgentConfig,
    pub vt_cfg: &'a mut Option<VTCodeConfig>,
    pub provider_client: &'a mut Box<dyn uni::LLMProvider>,
    pub ctrl_c_state: &'a Arc<CtrlCState>,
    pub ctrl_c_notify: &'a Arc<Notify>,
    pub session_bootstrap: &'a SessionBootstrap,
    pub full_auto: bool,
    pub startup_update_notice_rx: &'a mut Option<tokio::sync::mpsc::UnboundedReceiver<StartupUpdateNotice>>,
    pub header_context: &'a mut InlineHeaderContext,
    pub use_unicode: bool,
    pub conversation_history: &'a mut Vec<uni::Message>,
    pub session_stats: &'a mut SessionStats,
    pub context_manager: &'a mut ContextManager,
    pub session_id: &'a str,
    pub thread_id: &'a str,
    pub lifecycle_hooks: Option<&'a LifecycleHookEngine>,
    pub harness_emitter: Option<&'a HarnessEventEmitter>,
    pub editor_open_sender: &'a EditorOpenRequestSender,
    pub editor_open_dispatcher: Arc<EditorOpenDispatcher>,
    pub exec_sessions: Option<vtcode_core::tools::exec_session::ExecSessionManager>,
    pub background_completion_notify: Option<Arc<Notify>>,
    pub exec_completion_notify: Option<Arc<Notify>>,
    pub webmcp_prompt_receiver: &'a mut Option<tokio::sync::mpsc::Receiver<String>>,
    pub idle_wake_delay: Duration,
}

pub(crate) async fn poll_inline_loop_action(
    session: &mut InlineSession,
    ctrl_c_notify: &Arc<Notify>,
    resources: InlineEventLoopResources<'_>,
) -> Result<InlineLoopAction> {
    InlineEventLoop::new(resources).poll(session, ctrl_c_notify).await
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

async fn recv_webmcp_prompt(receiver: &mut Option<tokio::sync::mpsc::Receiver<String>>) -> Option<String> {
    match receiver.as_mut() {
        Some(rx) => match rx.recv().await {
            Some(prompt) => Some(prompt),
            None => {
                *receiver = None;
                None
            }
        },
        None => std::future::pending().await,
    }
}

async fn wait_for_background_completion(controller_notify: Option<&Arc<Notify>>, exec_notify: Option<&Arc<Notify>>) {
    match (controller_notify, exec_notify) {
        (Some(controller), Some(exec)) => {
            tokio::select! {
                _ = controller.notified() => {},
                _ = exec.notified() => {},
            }
        }
        (Some(notify), None) | (None, Some(notify)) => notify.notified().await,
        (None, None) => std::future::pending().await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use semver::Version;
    use std::collections::VecDeque;
    use tokio::sync::Notify;
    use vtcode_core::config::core::PromptCachingConfig;
    use vtcode_core::config::models::Provider;
    use vtcode_core::config::types::{
        AgentConfig as CoreAgentConfig, ModelSelectionSource, ReasoningEffortLevel, UiSurfacePreference,
    };
    use vtcode_core::core::agent::snapshots::{
        DEFAULT_CHECKPOINTS_ENABLED, DEFAULT_MAX_AGE_DAYS, DEFAULT_MAX_SNAPSHOTS,
    };
    use vtcode_core::llm::provider::{LLMError, LLMRequest, LLMResponse};
    use vtcode_ui::tui::app::InlineEvent;

    #[derive(Clone)]
    struct DummyProvider;

    #[async_trait]
    impl uni::LLMProvider for DummyProvider {
        fn name(&self) -> &str {
            "dummy"
        }

        async fn generate(&self, _request: LLMRequest) -> Result<LLMResponse, LLMError> {
            Ok(LLMResponse {
                content: None,
                model: "dummy-model".to_string(),
                tool_calls: None,
                usage: None,
                finish_reason: uni::FinishReason::Stop,
                reasoning: None,
                reasoning_details: None,
                organization_id: None,
                request_id: None,
                tool_references: vec![],
                compaction: None,
            })
        }

        fn supported_models(&self) -> Vec<String> {
            vec!["dummy-model".to_string()]
        }

        fn validate_request(&self, _request: &LLMRequest) -> Result<(), LLMError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn background_completion_wait_consumes_retained_controller_permit() {
        let controller_notify = Arc::new(Notify::new());
        controller_notify.notify_one();

        tokio::time::timeout(Duration::from_millis(50), wait_for_background_completion(Some(&controller_notify), None))
            .await
            .expect("a completion published before the idle wait must still wake it");
    }

    #[tokio::test]
    async fn raw_exec_completion_wakes_when_both_sources_are_installed() {
        let controller_notify = Arc::new(Notify::new());
        let exec_notify = Arc::new(Notify::new());
        exec_notify.notify_one();

        tokio::time::timeout(
            Duration::from_millis(50),
            wait_for_background_completion(Some(&controller_notify), Some(&exec_notify)),
        )
        .await
        .expect("either completion source must wake the idle loop");
    }

    fn runtime_config() -> CoreAgentConfig {
        CoreAgentConfig {
            model: vtcode_core::config::constants::models::google::GEMINI_3_FLASH_PREVIEW.to_string(),
            api_key: "test-key".to_string(),
            provider: "gemini".to_string(),
            api_key_env: Provider::Gemini.default_api_key_env().to_string(),
            workspace: std::env::current_dir().expect("current_dir"),
            verbose: false,
            quiet: false,
            theme: vtcode_core::ui::theme::DEFAULT_THEME_ID.to_string(),
            reasoning_effort: ReasoningEffortLevel::default(),
            ui_surface: UiSurfacePreference::default(),
            prompt_cache: PromptCachingConfig::default(),
            model_source: ModelSelectionSource::WorkspaceConfig,
            custom_api_keys: std::collections::BTreeMap::new(),
            checkpointing_enabled: DEFAULT_CHECKPOINTS_ENABLED,
            checkpointing_storage_dir: None,
            checkpointing_max_snapshots: DEFAULT_MAX_SNAPSHOTS,
            checkpointing_max_age_days: Some(DEFAULT_MAX_AGE_DAYS),
            max_conversation_turns: 1000,
            model_behavior: None,
            openai_chatgpt_auth: None,
        }
    }

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

    #[tokio::test]
    async fn poll_inline_loop_action_respects_idle_wake_delay() {
        let (command_tx, _command_rx) = tokio::sync::mpsc::unbounded_channel();
        let (event_tx, event_rx) = tokio::sync::mpsc::unbounded_channel::<InlineEvent>();
        let handle = InlineHandle::new_for_tests(command_tx);
        let mut session = InlineSession {
            handle: handle.clone(),
            events: event_rx,
            worker: None,
        };
        let mut renderer = AnsiRenderer::with_inline_ui(handle.clone(), Default::default());
        let ctrl_c_state = Arc::new(CtrlCState::new());
        let interrupts = InlineInterruptCoordinator::new(ctrl_c_state.as_ref());
        let mut ctrl_c_notice_displayed = false;
        let default_placeholder = None;
        let mut queued_inputs = VecDeque::new();
        let mut prefer_latest_queued_input_once = false;
        let mut model_picker_state = None;
        let mut palette_state = None;
        let mut config = runtime_config();
        let mut vt_cfg = None;
        let mut provider_client: Box<dyn uni::LLMProvider> = Box::new(DummyProvider);
        let session_bootstrap = SessionBootstrap::default();
        let mut startup_update_notice_rx = None;
        let mut header_context = InlineHeaderContext::default();
        let ctrl_c_notify = Arc::new(Notify::new());
        let (editor_open_sender, _editor_open_receiver) =
            crate::agent::runloop::unified::session_setup::bounded_editor_open_requests();
        let mut webmcp_prompt_receiver = None;

        let resources = InlineEventLoopResources {
            renderer: &mut renderer,
            handle: &handle,
            interrupts,
            ctrl_c_notice_displayed: &mut ctrl_c_notice_displayed,
            default_placeholder: &default_placeholder,
            queued_inputs: &mut queued_inputs,
            prefer_latest_queued_input_once: &mut prefer_latest_queued_input_once,
            model_picker_state: &mut model_picker_state,
            palette_state: &mut palette_state,
            config: &mut config,
            vt_cfg: &mut vt_cfg,
            provider_client: &mut provider_client,
            session_bootstrap: &session_bootstrap,
            full_auto: false,
            startup_update_notice_rx: &mut startup_update_notice_rx,
            header_context: &mut header_context,
            use_unicode: true,
            conversation_history: &mut Vec::new(),
            session_stats: &mut SessionStats::default(),
            context_manager: &mut ContextManager::default_for_test(),
            session_id: "test-session",
            thread_id: "test-thread",
            lifecycle_hooks: None,
            harness_emitter: None,
            editor_open_sender: &editor_open_sender,
            editor_open_dispatcher: Arc::new(EditorOpenDispatcher::new(true)),
            exec_sessions: None,
            background_completion_notify: None,
            exec_completion_notify: None,
            webmcp_prompt_receiver: &mut webmcp_prompt_receiver,
            idle_wake_delay: Duration::from_millis(5),
            ctrl_c_state: &ctrl_c_state,
            ctrl_c_notify: &ctrl_c_notify,
        };

        let action = tokio::time::timeout(
            Duration::from_millis(50),
            poll_inline_loop_action(&mut session, &ctrl_c_notify, resources),
        )
        .await
        .expect("idle wake should return promptly")
        .expect("poll should succeed");

        assert!(matches!(action, InlineLoopAction::Continue));
        drop(event_tx);
    }

    #[tokio::test]
    async fn poll_inline_loop_action_exits_when_event_stream_is_closed() {
        let (command_tx, _command_rx) = tokio::sync::mpsc::unbounded_channel();
        let (event_tx, event_rx) = tokio::sync::mpsc::unbounded_channel::<InlineEvent>();
        drop(event_tx); // drop the sender so the stream is closed from the start
        let handle = InlineHandle::new_for_tests(command_tx);
        let mut session = InlineSession {
            handle: handle.clone(),
            events: event_rx,
            worker: None,
        };
        let mut renderer = AnsiRenderer::with_inline_ui(handle.clone(), Default::default());
        let ctrl_c_state = Arc::new(CtrlCState::new());
        let interrupts = InlineInterruptCoordinator::new(ctrl_c_state.as_ref());
        let mut ctrl_c_notice_displayed = false;
        let default_placeholder = None;
        let mut queued_inputs = VecDeque::new();
        let mut prefer_latest_queued_input_once = false;
        let mut model_picker_state = None;
        let mut palette_state = None;
        let mut config = runtime_config();
        let mut vt_cfg = None;
        let mut provider_client: Box<dyn uni::LLMProvider> = Box::new(DummyProvider);
        let session_bootstrap = SessionBootstrap::default();
        let mut startup_update_notice_rx = None;
        let mut header_context = InlineHeaderContext::default();
        let ctrl_c_notify = Arc::new(Notify::new());
        let (editor_open_sender, _editor_open_receiver) =
            crate::agent::runloop::unified::session_setup::bounded_editor_open_requests();
        let mut webmcp_prompt_receiver = None;

        let resources = InlineEventLoopResources {
            renderer: &mut renderer,
            handle: &handle,
            interrupts,
            ctrl_c_notice_displayed: &mut ctrl_c_notice_displayed,
            default_placeholder: &default_placeholder,
            queued_inputs: &mut queued_inputs,
            prefer_latest_queued_input_once: &mut prefer_latest_queued_input_once,
            model_picker_state: &mut model_picker_state,
            palette_state: &mut palette_state,
            config: &mut config,
            vt_cfg: &mut vt_cfg,
            provider_client: &mut provider_client,
            session_bootstrap: &session_bootstrap,
            full_auto: false,
            startup_update_notice_rx: &mut startup_update_notice_rx,
            header_context: &mut header_context,
            use_unicode: true,
            conversation_history: &mut Vec::new(),
            session_stats: &mut SessionStats::default(),
            context_manager: &mut ContextManager::default_for_test(),
            session_id: "test-session",
            thread_id: "test-thread",
            lifecycle_hooks: None,
            harness_emitter: None,
            editor_open_sender: &editor_open_sender,
            editor_open_dispatcher: Arc::new(EditorOpenDispatcher::new(true)),
            exec_sessions: None,
            background_completion_notify: None,
            exec_completion_notify: None,
            webmcp_prompt_receiver: &mut webmcp_prompt_receiver,
            idle_wake_delay: Duration::from_millis(5),
            ctrl_c_state: &ctrl_c_state,
            ctrl_c_notify: &ctrl_c_notify,
        };

        let action = poll_inline_loop_action(&mut session, &ctrl_c_notify, resources)
            .await
            .expect("poll should succeed");

        assert!(matches!(action, InlineLoopAction::Exit(_)), "a closed event stream must exit rather than spin");
    }
}
