use super::ZedAgent;
use crate::acp_connection;
use agent_client_protocol as acp;
use agent_client_protocol::AgentSideConnection;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use vtcode_core::config::types::ReasoningEffortLevel;
use vtcode_core::core::interfaces::SessionMode;
use vtcode_core::core::threads::{ThreadBootstrap, build_thread_archive_metadata};
use vtcode_core::llm::ModelResolver;
use vtcode_core::llm::provider::{FinishReason, Message};
use vtcode_core::utils::session_archive::find_session_by_identifier;

use super::super::constants::SESSION_PREFIX;
use super::super::helpers::{session_config_options, session_mode_id, session_mode_prompt};
use super::super::types::{SessionData, SessionHandle};

impl ZedAgent {
    fn session_reasoning_effort_for_thread(
        &self,
        thread: &vtcode_core::core::threads::ThreadRuntimeHandle,
    ) -> ReasoningEffortLevel {
        thread
            .metadata()
            .and_then(|metadata| ReasoningEffortLevel::parse(&metadata.reasoning_effort))
            .unwrap_or(self.config.reasoning_effort)
    }

    fn session_mode_for_thread(
        &self,
        thread: &vtcode_core::core::threads::ThreadRuntimeHandle,
    ) -> SessionMode {
        thread
            .metadata()
            .and_then(|metadata| metadata.session_mode)
            .as_deref()
            .and_then(SessionMode::parse)
            .unwrap_or(SessionMode::Code)
    }

    fn sync_thread_reasoning_effort(
        &self,
        thread: &vtcode_core::core::threads::ThreadRuntimeHandle,
        reasoning_effort: ReasoningEffortLevel,
    ) {
        if let Some(mut metadata) = thread.metadata() {
            metadata.reasoning_effort = reasoning_effort.as_str().to_string();
            thread.replace_metadata(Some(metadata));
        }
    }

    fn sync_thread_mode(
        &self,
        thread: &vtcode_core::core::threads::ThreadRuntimeHandle,
        mode: SessionMode,
    ) {
        if let Some(mut metadata) = thread.metadata() {
            metadata.session_mode = Some(mode.as_str().to_string());
            thread.replace_metadata(Some(metadata));
        }
    }

    pub(super) fn model_supports_thought_level(&self) -> bool {
        ModelResolver::resolve(Some(&self.config.provider), &self.config.model, &[], None)
            .map(|resolved| resolved.reasoning_supported())
            .unwrap_or(false)
    }

    fn build_session_handle(
        &self,
        session_id: acp::SessionId,
        thread: vtcode_core::core::threads::ThreadRuntimeHandle,
    ) -> SessionHandle {
        let reasoning_effort = self.session_reasoning_effort_for_thread(&thread);
        let current_mode = self.session_mode_for_thread(&thread);
        SessionHandle {
            data: Rc::new(RefCell::new(SessionData {
                _session_id: session_id,
                thread,
                tool_notice_sent: false,
                current_mode,
                reasoning_effort,
            })),
            cancel_flag: Rc::new(Cell::new(false)),
        }
    }

    pub(crate) fn register_session(&self) -> acp::SessionId {
        let raw_id = self.next_session_id.get();
        self.next_session_id.set(raw_id + 1);
        let session_id = acp::SessionId::new(Arc::from(format!("{SESSION_PREFIX}-{raw_id}")));
        let mut metadata = build_thread_archive_metadata(
            self.config.workspace.as_path(),
            &self.config.model,
            &self.config.provider,
            &self.config.theme,
            self.config.reasoning_effort.as_str(),
        );
        metadata.session_mode = Some(SessionMode::Code.as_str().to_string());
        let thread = self.thread_manager.start_thread_with_identifier(
            session_id.0.to_string(),
            ThreadBootstrap::new(Some(metadata)),
        );
        let handle = self.build_session_handle(session_id.clone(), thread);
        self.sessions
            .borrow_mut()
            .insert(session_id.clone(), handle);
        session_id
    }

    pub(crate) fn session_handle(&self, session_id: &acp::SessionId) -> Option<SessionHandle> {
        self.sessions.borrow().get(session_id).cloned()
    }

    pub(super) fn push_message(&self, session: &SessionHandle, message: Message) {
        session.data.borrow().thread.append_message(message);
    }

    pub(super) fn should_send_tool_notice(&self, session: &SessionHandle) -> bool {
        !session.data.borrow().tool_notice_sent
    }

    pub(super) fn mark_tool_notice_sent(&self, session: &SessionHandle) {
        session.data.borrow_mut().tool_notice_sent = true;
    }

    pub(super) fn update_session_mode(&self, session: &SessionHandle, mode: SessionMode) -> bool {
        let mut data = session.data.borrow_mut();
        if data.current_mode == mode {
            return false;
        }
        data.current_mode = mode;
        self.sync_thread_mode(&data.thread, mode);
        true
    }

    pub(super) async fn apply_session_mode(
        &self,
        session_id: &acp::SessionId,
        session: &SessionHandle,
        mode: SessionMode,
    ) -> Result<bool, acp::Error> {
        if !self.update_session_mode(session, mode) {
            return Ok(false);
        }

        let config_options = self.current_session_config_options(session);
        self.send_update(
            session_id,
            acp::SessionUpdate::CurrentModeUpdate(acp::CurrentModeUpdate::new(session_mode_id(
                mode,
            ))),
        )
        .await?;
        self.send_update(
            session_id,
            acp::SessionUpdate::ConfigOptionUpdate(acp::ConfigOptionUpdate::new(config_options)),
        )
        .await?;

        Ok(true)
    }

    pub(super) fn update_session_reasoning_effort(
        &self,
        session: &SessionHandle,
        reasoning_effort: ReasoningEffortLevel,
    ) -> bool {
        let mut data = session.data.borrow_mut();
        if data.reasoning_effort == reasoning_effort {
            return false;
        }
        data.reasoning_effort = reasoning_effort;
        self.sync_thread_reasoning_effort(&data.thread, reasoning_effort);
        true
    }

    pub(super) fn current_session_config_options(
        &self,
        session: &SessionHandle,
    ) -> Vec<acp::SessionConfigOption> {
        let data = session.data.borrow();
        session_config_options(
            data.current_mode,
            data.reasoning_effort,
            self.model_supports_thought_level(),
        )
    }

    pub(super) fn resolved_messages(&self, session: &SessionHandle) -> Vec<Message> {
        let mut messages = Vec::with_capacity(10); // Pre-allocate for typical message count
        if !self.system_prompt.trim().is_empty() {
            messages.push(Message::system(self.system_prompt.clone()));
        }

        let history = session.data.borrow();
        if let Some(prompt) = session_mode_prompt(history.current_mode) {
            messages.push(Message::system(prompt.to_string()));
        }
        messages.extend(history.thread.messages());
        messages
    }

    pub(super) async fn attach_thread_from_archive(
        &self,
        session_id: &acp::SessionId,
        identifier: &str,
    ) -> anyhow::Result<SessionHandle> {
        let listing = find_session_by_identifier(identifier)
            .await?
            .ok_or_else(|| anyhow::anyhow!("unknown archived session '{identifier}'"))?;
        let thread = self.thread_manager.start_thread_with_identifier(
            listing.identifier(),
            ThreadBootstrap::from_listing(listing),
        );
        let handle = self.build_session_handle(session_id.clone(), thread);
        self.sessions
            .borrow_mut()
            .insert(session_id.clone(), handle.clone());
        Ok(handle)
    }

    pub(super) fn stop_reason_from_finish(finish: FinishReason) -> acp::StopReason {
        match finish {
            FinishReason::Stop | FinishReason::ToolCalls => acp::StopReason::EndTurn,
            FinishReason::Length => acp::StopReason::MaxTokens,
            FinishReason::ContentFilter | FinishReason::Refusal | FinishReason::Error(_) => {
                acp::StopReason::Refusal
            }
            FinishReason::Pause => acp::StopReason::EndTurn, // Map Pause to EndTurn as a fallback for ACP
        }
    }

    pub(super) fn client(&self) -> Option<Arc<AgentSideConnection>> {
        acp_connection()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::zed::types::NotificationEnvelope;
    use assert_fs::TempDir;
    use chrono::Utc;
    use std::collections::BTreeMap;
    use std::path::Path;
    use tokio::sync::mpsc;
    use vtcode_core::config::core::PromptCachingConfig;
    use vtcode_core::config::types::{
        AgentConfig as CoreAgentConfig, ModelSelectionSource, UiSurfacePreference,
    };
    use vtcode_core::config::{AgentClientProtocolZedConfig, CommandsConfig, ToolsConfig};
    use vtcode_core::core::agent::snapshots::{
        DEFAULT_CHECKPOINTS_ENABLED, DEFAULT_MAX_AGE_DAYS, DEFAULT_MAX_SNAPSHOTS,
    };
    use vtcode_core::utils::session_archive::{
        SessionArchiveMetadata, SessionListing, SessionSnapshot,
    };

    async fn build_agent(workspace: &Path) -> ZedAgent {
        let core_config = CoreAgentConfig {
            model: "gpt-5.4".to_string(),
            api_key: String::new(),
            provider: "openai".to_string(),
            api_key_env: "TEST_API_KEY".to_string(),
            workspace: workspace.to_path_buf(),
            verbose: false,
            quiet: false,
            theme: "test".to_string(),
            reasoning_effort: ReasoningEffortLevel::Low,
            ui_surface: UiSurfacePreference::default(),
            prompt_cache: PromptCachingConfig::default(),
            model_source: ModelSelectionSource::WorkspaceConfig,
            custom_api_keys: BTreeMap::new(),
            checkpointing_enabled: DEFAULT_CHECKPOINTS_ENABLED,
            checkpointing_storage_dir: None,
            checkpointing_max_snapshots: DEFAULT_MAX_SNAPSHOTS,
            checkpointing_max_age_days: Some(DEFAULT_MAX_AGE_DAYS),
            max_conversation_turns: 1000,
            model_behavior: None,
            openai_chatgpt_auth: None,
        };

        let (tx, mut rx) = mpsc::unbounded_channel::<NotificationEnvelope>();
        tokio::spawn(async move {
            while let Some(envelope) = rx.recv().await {
                let _ = envelope.completion.send(());
            }
        });

        ZedAgent::new(
            core_config,
            AgentClientProtocolZedConfig::default(),
            ToolsConfig::default(),
            CommandsConfig::default(),
            String::new(),
            tx,
            Some("Zed".to_string()),
        )
        .await
    }

    #[tokio::test]
    async fn build_session_handle_restores_session_config_from_thread_metadata() {
        let temp = TempDir::new().unwrap();
        let agent = build_agent(temp.path()).await;
        let listing = SessionListing {
            path: temp.path().join("session-vtcode-acp-archive.json"),
            snapshot: SessionSnapshot {
                metadata: SessionArchiveMetadata::new(
                    "vtcode",
                    temp.path().to_string_lossy(),
                    "gpt-5.4",
                    "openai",
                    "test",
                    "xhigh",
                ),
                started_at: Utc::now(),
                ended_at: Utc::now(),
                total_messages: 0,
                distinct_tools: Vec::new(),
                transcript: Vec::new(),
                messages: Vec::new(),
                progress: None,
                error_logs: Vec::new(),
            },
        };
        let thread = agent.thread_manager.start_thread_with_identifier(
            "session-vtcode-acp-archive",
            ThreadBootstrap::from_listing({
                let mut listing = listing;
                listing.snapshot.metadata.session_mode = Some("architect".to_string());
                listing
            }),
        );

        let handle = agent.build_session_handle(acp::SessionId::new("session-1"), thread);

        assert_eq!(handle.data.borrow().current_mode, SessionMode::Architect);
        assert_eq!(
            handle.data.borrow().reasoning_effort,
            ReasoningEffortLevel::XHigh
        );
    }

    #[tokio::test]
    async fn update_session_mode_syncs_thread_metadata() {
        let temp = TempDir::new().unwrap();
        let agent = build_agent(temp.path()).await;
        let session_id = agent.register_session();
        let session = agent.session_handle(&session_id).unwrap();

        assert!(agent.update_session_mode(&session, SessionMode::Ask));
        assert_eq!(
            session
                .data
                .borrow()
                .thread
                .metadata()
                .unwrap()
                .session_mode
                .as_deref(),
            Some("ask")
        );
    }

    #[tokio::test]
    async fn update_session_reasoning_effort_syncs_thread_metadata() {
        let temp = TempDir::new().unwrap();
        let agent = build_agent(temp.path()).await;
        let session_id = agent.register_session();
        let session = agent.session_handle(&session_id).unwrap();

        assert!(agent.update_session_reasoning_effort(&session, ReasoningEffortLevel::High));
        assert_eq!(
            session
                .data
                .borrow()
                .thread
                .metadata()
                .unwrap()
                .reasoning_effort,
            "high"
        );
    }

    #[tokio::test]
    async fn current_session_config_options_omit_thought_level_when_model_lacks_support() {
        let temp = TempDir::new().unwrap();
        let mut agent = build_agent(temp.path()).await;
        agent.config.model = "claude-haiku-3".to_string();
        agent.config.provider = "anthropic".to_string();
        let session_id = agent.register_session();
        let session = agent.session_handle(&session_id).unwrap();

        let config_options = agent.current_session_config_options(&session);

        assert_eq!(config_options.len(), 1);
        assert_eq!(config_options[0].id, acp::SessionConfigId::new("mode"));
    }
}
