//! Provider-native computer-use calls for bound routes.
//!
//! Ordinary `LLMResponse.tool_calls` carry function calls. Bound computer-use
//! turns return [`NativeResponse`] instead: decoded native calls plus any
//! ordinary calls from the same turn, with the verbatim assistant blocks the
//! next request replays. Native payloads stay in memory and move by value:
//! these types implement neither serialization nor `Clone`, and their manual
//! `Debug` impls reveal routing metadata only.

use serde_json::Value;
use vtcode_computer_use::Action;

use super::{FinishReason, ToolCall, Usage};

/// Which provider produced a native call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeProvider {
    /// OpenAI Responses `computer_call` item.
    OpenAi,
    /// Anthropic Messages computer-toolset `tool_use` block.
    Anthropic,
}

impl NativeProvider {
    fn as_str(self) -> &'static str {
        match self {
            Self::OpenAi => "openai",
            Self::Anthropic => "anthropic",
        }
    }
}

/// One decoded native call in wire order.
pub struct NativeCall {
    /// Provider call identity (`call_id` or tool-use `id`).
    pub id: String,
    /// Which provider produced the call.
    pub provider: NativeProvider,
    /// Position in the original content/output array, so results merge back
    /// with ordinary calls after the computer batch runs.
    pub index: usize,
    /// Decoded provider-native body.
    pub body: NativeCallBody,
}

/// Decoded provider-native call body.
pub enum NativeCallBody {
    /// Ordered OpenAI actions with their original item for stateless replay.
    OpenAi {
        /// Original `computer_call` item, owned for replay.
        replay_item: Value,
        /// Actions in execution order, ending in a screenshot capture.
        actions: Vec<Action>,
    },
    /// One Anthropic member call. A malformed member retains its correlation
    /// identity with the decode failure so the runtime can answer that block
    /// and halt the batch.
    Anthropic {
        /// Toolset member name, e.g. `"left_click"`.
        name: String,
        /// Decoded action or its failure message.
        action: Result<Action, String>,
    },
}

/// Bound-route response: ordinary calls for existing dispatch, natives for
/// the computer controller, verbatim blocks for history replay.
pub struct NativeResponse {
    /// Joined text content, when the turn produced any.
    pub content: Option<String>,
    /// Model that generated the response.
    pub model: String,
    /// Token usage statistics.
    pub usage: Option<Usage>,
    /// Why the response finished.
    pub finish_reason: FinishReason,
    /// Joined thinking text, when the turn produced any.
    pub reasoning: Option<String>,
    /// Ordinary function calls in wire order.
    pub tool_calls: Vec<ToolCall>,
    /// Decoded native calls in wire order.
    pub native_calls: Vec<NativeCall>,
    /// Verbatim assistant content blocks for replay on the next request.
    pub assistant_blocks: Vec<Value>,
    /// Request ID from the provider.
    pub request_id: Option<String>,
    /// Organization ID from the provider.
    pub organization_id: Option<String>,
}

// Debug reveals routing metadata only. Text content, thinking, assistant
// blocks, and decoded actions never implement it, so payloads cannot reach
// logs through these types.
impl std::fmt::Debug for NativeCall {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let body = match &self.body {
            NativeCallBody::OpenAi { actions, .. } => format!("openai:{}actions", actions.len()),
            NativeCallBody::Anthropic { name, action } => {
                format!("anthropic:{name}:{}", if action.is_ok() { "valid" } else { "invalid" })
            }
        };
        f.debug_struct("NativeCall")
            .field("id", &self.id)
            .field("provider", &self.provider.as_str())
            .field("index", &self.index)
            .field("body", &body)
            .finish()
    }
}

impl std::fmt::Debug for NativeResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NativeResponse")
            .field("model", &self.model)
            .field("finish_reason", &self.finish_reason)
            .field("usage", &self.usage)
            .field("tool_call_ids", &self.tool_calls.iter().map(|call| call.id.as_str()).collect::<Vec<_>>())
            .field("native_calls", &self.native_calls)
            .finish()
    }
}
