use super::ZedAgent;
use crate::acp::acp_connection;
use agent_client_protocol as acp;
use agent_client_protocol::AgentSideConnection;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use vtcode_core::core::interfaces::SessionMode;
use vtcode_core::core::threads::{ThreadBootstrap, build_thread_archive_metadata};
use vtcode_core::llm::provider::{FinishReason, Message};
use vtcode_core::utils::session_archive::find_session_by_identifier;

use super::super::constants::SESSION_PREFIX;
use super::super::helpers::{session_mode_id, session_mode_prompt};
use super::super::types::{SessionData, SessionHandle};

impl ZedAgent {
    fn build_session_handle(
        &self,
        session_id: acp::SessionId,
        thread: vtcode_core::core::threads::ThreadRuntimeHandle,
    ) -> SessionHandle {
        SessionHandle {
            data: Rc::new(RefCell::new(SessionData {
                _session_id: session_id,
                thread,
                tool_notice_sent: false,
                current_mode: SessionMode::Code,
            })),
            cancel_flag: Rc::new(Cell::new(false)),
        }
    }

    pub(crate) fn register_session(&self) -> acp::SessionId {
        let raw_id = self.next_session_id.get();
        self.next_session_id.set(raw_id + 1);
        let session_id = acp::SessionId::new(Arc::from(format!("{SESSION_PREFIX}-{raw_id}")));
        let metadata = build_thread_archive_metadata(
            self.config.workspace.as_path(),
            &self.config.model,
            &self.config.provider,
            &self.config.theme,
            self.config.reasoning_effort.as_str(),
        );
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

        self.send_update(
            session_id,
            acp::SessionUpdate::CurrentModeUpdate(acp::CurrentModeUpdate::new(session_mode_id(
                mode,
            ))),
        )
        .await?;

        Ok(true)
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
