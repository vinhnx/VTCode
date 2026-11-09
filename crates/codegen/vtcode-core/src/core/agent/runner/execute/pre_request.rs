//! Pre-request input derivation for the `execute_task` turn loop.
//!
//! Computes per-turn sampling overrides, reasoning/verbosity levels, token
//! caps, and provider identity, and replays new Gemini conversation entries
//! into session state before the request is built.

use super::super::AgentRunner;
use crate::config::models::{ModelId, Provider as ModelProvider};
use crate::config::types::{ReasoningEffortLevel, VerbosityLevel};
use crate::core::agent::runtime::AgentRuntime;
use crate::llm::provider::{Message, SamplingOverrides};
use crate::llm::providers::gemini::wire::Part;
use vtcode_commons::tokens::heuristic_tokens_from_byte_len;

/// Per-turn inputs derived before building an LLM request.
pub(super) struct TurnRequestInputs {
    pub sampling_overrides: SamplingOverrides,
    pub turn_reasoning: Option<ReasoningEffortLevel>,
    pub turn_verbosity: Option<VerbosityLevel>,
    pub max_tokens: Option<u32>,
    pub provider_kind: ModelProvider,
    pub planning_synthesis_turn: bool,
}

impl AgentRunner {
    /// Derive per-turn request inputs and replay new Gemini conversation
    /// entries into session state.
    pub(super) fn derive_turn_request_inputs(
        &self,
        runtime: &mut AgentRuntime,
        turn_model: &str,
        turn: usize,
        is_simple_task: bool,
        planning_synthesis_required: bool,
    ) -> TurnRequestInputs {
        if std::env::var_os("VTCODE_DEBUG_PROVIDER").is_some() {
            tracing::debug!(provider_client = self.provider_client.name(), turn_model, "Provider debug turn selection");
        }
        let sampling_overrides = self.provider_client.sampling_overrides(turn_model);
        let turn_reasoning = if is_simple_task
            && self
                .provider_client
                .supported_reasoning_efforts(turn_model)
                .contains(&ReasoningEffortLevel::Minimal.as_str())
        {
            Some(ReasoningEffortLevel::Minimal)
        } else {
            sampling_overrides.reasoning_effort.or(self.reasoning_effort)
        };
        let turn_verbosity = if is_simple_task {
            Some(VerbosityLevel::Low)
        } else {
            self.verbosity
        };
        let max_tokens = sampling_overrides
            .max_tokens
            .or(Some(if self.tool_registry.is_planning_active() {
                4096
            } else if is_simple_task {
                800
            } else {
                2000
            }));

        let provider_kind = turn_model
            .parse::<ModelId>()
            .map(|model| model.provider())
            .unwrap_or(ModelProvider::Gemini);

        if matches!(provider_kind, ModelProvider::Gemini)
            && runtime.state.conversation.len() > runtime.state.last_processed_message_idx
        {
            // Collect new messages first to avoid borrow conflict:
            // conversation is immutably borrowed in the loop, but
            // adjust_token_count/push need &mut state.
            // Previous assistant turns are replayed as `model`. Every
            // other entry is either harness steering or a duplicate of
            // a user message already recorded in `state.messages`, so
            // tag the replay synthetic; reverse history keeps the
            // original user-typed copy.
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|duration| duration.as_secs())
                .unwrap_or(0);
            let new_messages: Vec<Message> = runtime.state.conversation[runtime.state.last_processed_message_idx..]
                .iter()
                .map(|content| {
                    let mut text = String::new();
                    for part in &content.parts {
                        if let Part::Text { text: part_text, .. } = part {
                            if !text.is_empty() {
                                text.push('\n');
                            }
                            text.push_str(part_text);
                        }
                    }
                    match content.role.as_str() {
                        "model" => Message::assistant(text),
                        _ => {
                            let tokens = heuristic_tokens_from_byte_len(text.len());
                            Message::user(text).with_metadata(
                                crate::core::message_metadata::MessageMetadata::harness_notice(now, tokens),
                            )
                        }
                    }
                })
                .collect();
            let batch_tokens: usize = new_messages.iter().map(|m| m.estimate_tokens()).sum();
            runtime.state.adjust_token_count(batch_tokens as isize);
            runtime.state.messages_mut().extend(new_messages);
            runtime.state.last_processed_message_idx = runtime.state.conversation.len();
        }

        let planning_synthesis_turn =
            self.tool_registry.is_planning_active() && (planning_synthesis_required || turn + 1 == self.max_turns);
        TurnRequestInputs {
            sampling_overrides,
            turn_reasoning,
            turn_verbosity,
            max_tokens,
            provider_kind,
            planning_synthesis_turn,
        }
    }
}
