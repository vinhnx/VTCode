use super::ZedAgent;
use crate::acp::acp_connection;
use agent_client_protocol as acp;
use agent_client_protocol::AgentSideConnection;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use vtcode_core::llm::provider::{FinishReason, Message};

use super::super::constants::*;
use super::super::types::{SessionData, SessionHandle};

impl ZedAgent {
    pub(crate) fn register_session(&self) -> acp::SessionId {
        let raw_id = self.next_session_id.get();
        self.next_session_id.set(raw_id + 1);
        let session_id = acp::SessionId::new(Arc::from(format!("{SESSION_PREFIX}-{raw_id}")));
        let handle = SessionHandle {
            data: Rc::new(RefCell::new(SessionData {
                _session_id: session_id.clone(),
                messages: Vec::with_capacity(20),
                tool_notice_sent: false,
                current_mode: acp::SessionModeId::new(MODE_ID_CODE),
            })),
            cancel_flag: Rc::new(Cell::new(false)),
        };
        self.sessions
            .borrow_mut()
            .insert(session_id.clone(), handle);
        session_id
    }

    pub(crate) fn session_handle(&self, session_id: &acp::SessionId) -> Option<SessionHandle> {
        self.sessions.borrow().get(session_id).cloned()
    }

    pub(super) fn push_message(&self, session: &SessionHandle, message: Message) {
        session.data.borrow_mut().messages.push(message);
    }

    pub(super) fn should_send_tool_notice(&self, session: &SessionHandle) -> bool {
        !session.data.borrow().tool_notice_sent
    }

    pub(super) fn mark_tool_notice_sent(&self, session: &SessionHandle) {
        session.data.borrow_mut().tool_notice_sent = true;
    }

    pub(super) fn update_session_mode(
        &self,
        session: &SessionHandle,
        mode_id: acp::SessionModeId,
    ) -> bool {
        let mut data = session.data.borrow_mut();
        if data.current_mode == mode_id {
            return false;
        }
        data.current_mode = mode_id;
        true
    }

    pub(super) fn resolved_messages(&self, session: &SessionHandle) -> Vec<Message> {
        let mut messages = Vec::with_capacity(10); // Pre-allocate for typical message count
        if !self.system_prompt.trim().is_empty() {
            messages.push(Message::system(self.system_prompt.clone()));
        }

        let history = session.data.borrow();
        messages.extend(history.messages.iter().cloned());
        messages
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
