use agent_client_protocol as acp;
use std::cell::{Cell, RefCell};
use std::mem::discriminant;
use std::rc::Rc;
use tokio::sync::oneshot;
use vtcode_core::llm::provider::Message;

use super::constants::{PLAN_STEP_ANALYZE, PLAN_STEP_GATHER_CONTEXT, PLAN_STEP_RESPOND};

pub(crate) enum ToolRuntime<'a> {
    Enabled,
    Disabled(ToolDisableReason<'a>),
}

#[derive(Clone, Copy)]
pub(crate) enum ToolDisableReason<'a> {
    Provider { provider: &'a str, model: &'a str },
    ClientCapabilities,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum RunTerminalMode {
    Terminal,
    Pty,
}

pub(crate) struct PlanProgress {
    entries: Vec<acp::PlanEntry>,
    analyze_index: usize,
    gather_index: Option<usize>,
    respond_index: usize,
}

impl PlanProgress {
    pub(crate) fn new(include_context_step: bool) -> Self {
        let mut entries = Vec::with_capacity(3); // Pre-allocate for typical plan entries (analyze, gather, respond)

        let analyze_index = entries.len();
        entries.push(acp::PlanEntry::new(
            PLAN_STEP_ANALYZE,
            acp::PlanEntryPriority::High,
            acp::PlanEntryStatus::InProgress,
        ));

        let gather_index = if include_context_step {
            let index = entries.len();
            entries.push(acp::PlanEntry::new(
                PLAN_STEP_GATHER_CONTEXT,
                acp::PlanEntryPriority::Medium,
                acp::PlanEntryStatus::Pending,
            ));
            Some(index)
        } else {
            None
        };

        let respond_index = entries.len();
        entries.push(acp::PlanEntry::new(
            PLAN_STEP_RESPOND,
            acp::PlanEntryPriority::High,
            acp::PlanEntryStatus::Pending,
        ));

        Self {
            entries,
            analyze_index,
            gather_index,
            respond_index,
        }
    }

    pub(crate) fn has_entries(&self) -> bool {
        !self.entries.is_empty()
    }

    pub(crate) fn update_status(&mut self, index: usize, status: acp::PlanEntryStatus) -> bool {
        if discriminant(&self.entries[index].status) == discriminant(&status) {
            return false;
        }

        self.entries[index].status = status;
        true
    }

    pub(crate) fn complete_analysis(&mut self) -> bool {
        if discriminant(&self.entries[self.analyze_index].status)
            != discriminant(&acp::PlanEntryStatus::Completed)
        {
            return self.update_status(self.analyze_index, acp::PlanEntryStatus::Completed);
        }
        false
    }

    pub(crate) fn start_context(&mut self) -> bool {
        if let Some(index) = self.gather_index
            && discriminant(&self.entries[index].status)
                == discriminant(&acp::PlanEntryStatus::Pending)
        {
            return self.update_status(index, acp::PlanEntryStatus::InProgress);
        }
        false
    }

    pub(crate) fn complete_context(&mut self) -> bool {
        if let Some(index) = self.gather_index
            && discriminant(&self.entries[index].status)
                != discriminant(&acp::PlanEntryStatus::Completed)
        {
            return self.update_status(index, acp::PlanEntryStatus::Completed);
        }
        false
    }

    pub(crate) fn has_context_step(&self) -> bool {
        self.gather_index.is_some()
    }

    pub(crate) fn context_completed(&self) -> bool {
        self.gather_index
            .map(|index| {
                discriminant(&self.entries[index].status)
                    == discriminant(&acp::PlanEntryStatus::Completed)
            })
            .unwrap_or(true)
    }

    pub(crate) fn start_response(&mut self) -> bool {
        if discriminant(&self.entries[self.respond_index].status)
            == discriminant(&acp::PlanEntryStatus::Pending)
        {
            return self.update_status(self.respond_index, acp::PlanEntryStatus::InProgress);
        }
        false
    }

    pub(crate) fn complete_response(&mut self) -> bool {
        if discriminant(&self.entries[self.respond_index].status)
            != discriminant(&acp::PlanEntryStatus::Completed)
        {
            return self.update_status(self.respond_index, acp::PlanEntryStatus::Completed);
        }
        false
    }

    pub(crate) fn to_plan(&self) -> acp::Plan {
        acp::Plan::new(self.entries.clone())
    }
}

pub(crate) struct ToolCallResult {
    pub(crate) tool_call_id: String,
    pub(crate) llm_response: String,
}

#[derive(Clone)]
pub(crate) struct SessionHandle {
    pub(crate) data: Rc<RefCell<SessionData>>,
    pub(crate) cancel_flag: Rc<Cell<bool>>,
}

pub(crate) struct SessionData {
    pub(crate) session_id: acp::SessionId,
    pub(crate) messages: Vec<Message>,
    pub(crate) tool_notice_sent: bool,
    pub(crate) current_mode: acp::SessionModeId,
}

pub(crate) struct NotificationEnvelope {
    pub(crate) notification: acp::SessionNotification,
    pub(crate) completion: oneshot::Sender<()>,
}
