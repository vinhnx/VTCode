use crate::core::agent::events::ActiveCommandHandle;
use crate::core::agent::events::ExecEventRecorder;
use crate::core::agent::state::TaskRunState;
use crate::llm::provider::LLMResponse;

pub(super) struct ProviderResponseSummary {
    pub(super) response: LLMResponse,
    pub(super) content: String,
    pub(super) reasoning: Option<String>,
    pub(super) agent_message_streamed: bool,
    pub(super) reasoning_recorded: bool,
}

pub(super) struct ToolFailureContext<'a> {
    pub(super) agent_prefix: &'a str,
    pub(super) task_state: &'a mut TaskRunState,
    pub(super) event_recorder: &'a mut ExecEventRecorder,
    pub(super) command_event: &'a ActiveCommandHandle,
    pub(super) is_gemini: bool,
}
