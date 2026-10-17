use super::{
    CONTINUITY_TAIL_TARGET_TOKENS, CompactionConfig, CompactionParentContext, CompactionRoutePolicy,
    CompactionStrategy, ManualCompactionOptions, TOOL_RESULT_PRUNE_TARGET_TOKENS, build_cache_safe_compaction_history,
    build_provider_compacted_history, build_summary_prompt, coherence_tool_call_pairs, collect_retained_user_messages,
    compact_history, compact_history_manual, compact_history_manual_with_budget,
    compact_history_manual_with_parent_context, compacted_history_shrinks, compaction_summary_request, continuity_tail,
    generate_summary_with_capacity_retry, manual_compaction_strategy, native_inline, prune_oversized_tool_outputs,
};
use crate::config::types::{ReasoningEffortLevel, VerbosityLevel};
use crate::exec::events::CompactionMode;
use crate::llm::provider::{
    LLMError, LLMNormalizedStream, LLMProvider, LLMRequest, LLMResponse, Message, MessageRole, NormalizedStreamEvent,
    ResponsesCompactionOptions,
};
use async_trait::async_trait;
use futures::stream;
use serde_json::json;
use std::sync::Arc;
use std::sync::Mutex;
use vtcode_commons::llm::{FinishReason, ToolCall};

struct StubProvider;

struct NativeCompactionProvider;

/// Provider that opts into the standalone manual-compaction path
/// (`supports_manual_openai_compaction -> true`), e.g. OpenAI `/responses/compact`.
struct ManualStandaloneProvider {
    last_options: Mutex<Option<ResponsesCompactionOptions>>,
    output: Vec<Message>,
}

/// Inline-compaction-capable provider (`supports_responses_compaction -> true`,
/// `supports_manual_openai_compaction -> false`), e.g. Anthropic `compact_20260112`.
/// Returns a `Pause` finish with a compaction block so the inline path succeeds.
struct InlinePauseProvider {
    last_request: Mutex<Option<LLMRequest>>,
    include_compaction_detail: bool,
}

/// Inline provider that also reports `supports_context_edits`, e.g. Anthropic
/// on a compaction-capable model. The inline request must carry the full
/// clearing ladder (thinking, tool uses, compact) in documented order.
struct ContextEditsInlineProvider {
    last_request: Mutex<Option<LLMRequest>>,
}

/// Local summarizer that rejects the first summary request with a
/// context-capacity error and succeeds on retry. Exercises the halved-budget
/// overflow retry in `summarize_locally`.
struct CapacityFailOnceProvider {
    attempts: Mutex<usize>,
    request_tokens: Mutex<Vec<usize>>,
    /// Number of leading requests to reject before succeeding.
    failures: usize,
}

/// Local summarizer that only supports normalized streaming. Its
/// non-streaming method deliberately returns an error so compaction tests
/// prove the capability-aware collection path is used.
struct StreamingOnlyCompactionProvider {
    generate_calls: Mutex<usize>,
    stream_calls: Mutex<usize>,
    stream_modes: Mutex<Vec<bool>>,
}

/// Unknown-window summarizer (`effective_context_size == 0`) that rejects
/// the first summary request with a capacity error. Exercises the
/// `None`-budget fallback so an unbounded first fork can still shrink.
struct UnknownBudgetFailOnceProvider {
    attempts: Mutex<usize>,
    request_tokens: Mutex<Vec<usize>>,
}

/// Local summarizer that returns an empty summary body. An empty
/// compaction summary must fail with a diagnostic instead of producing
/// an empty `Previous conversation summary:` history.
struct EmptySummaryProvider;

/// Capturing provider with no native support; used to assert the Local summary
/// request carries the manual options.
struct CapturingProvider {
    last_request: Mutex<Option<LLMRequest>>,
}

/// Local summarizer that exposes only the lower reasoning levels. This
/// exercises the compaction boundary's strict block and explicit
/// downgrade behavior before any hierarchical summary request is sent.
struct LimitedReasoningProvider {
    last_request: Mutex<Option<LLMRequest>>,
}

/// Inline-dispatched provider whose inline `generate` rejects the Anthropic
/// `compact_20260112` edit. Models providers that report
/// `supports_responses_compaction` but are not Anthropic-style inline
/// compactors; the dispatch must fall back to Local rather than aborting.
struct InlineRejectingProvider;

/// Models an OpenAI-compatible custom endpoint (non-`api.openai.com` host or
/// `provider_key_override`): it exposes the Responses API
/// (`supports_responses_compaction == true`) but neither the standalone
/// `/responses/compact` endpoint nor Anthropic inline compaction. The dispatch
/// must pick `Local` rather than misrouting it to `NativeInline` (which would
/// send an Anthropic `compact_20260112` edit only to be rejected).
struct CompatibleEndpointProvider;

#[async_trait]
impl LLMProvider for StubProvider {
    fn name(&self) -> &str {
        "stub"
    }

    async fn generate(&self, _request: LLMRequest) -> Result<LLMResponse, LLMError> {
        Ok(LLMResponse::new("stub-model", "summary"))
    }

    fn supported_models(&self) -> Vec<String> {
        vec!["stub-model".to_string()]
    }

    fn validate_request(&self, _request: &LLMRequest) -> Result<(), LLMError> {
        Ok(())
    }

    fn effective_context_size(&self, _model: &str) -> usize {
        32_768
    }
}

#[async_trait]
impl LLMProvider for NativeCompactionProvider {
    fn name(&self) -> &str {
        "native"
    }

    async fn generate(&self, _request: LLMRequest) -> Result<LLMResponse, LLMError> {
        Ok(LLMResponse::new("stub-model", "summary"))
    }

    fn supported_models(&self) -> Vec<String> {
        vec!["stub-model".to_string()]
    }

    fn validate_request(&self, _request: &LLMRequest) -> Result<(), LLMError> {
        Ok(())
    }

    fn supports_responses_compaction(&self, _model: &str) -> bool {
        true
    }

    async fn compact_history(&self, _model: &str, _history: &[Message]) -> Result<Vec<Message>, LLMError> {
        Ok(vec![Message::system("provider compacted".to_string())])
    }
}

#[async_trait]
impl LLMProvider for ManualStandaloneProvider {
    fn name(&self) -> &str {
        "manual-standalone"
    }

    async fn generate(&self, _request: LLMRequest) -> Result<LLMResponse, LLMError> {
        Ok(LLMResponse::new("stub-model", "summary"))
    }

    fn supported_models(&self) -> Vec<String> {
        vec!["stub-model".to_string()]
    }

    fn validate_request(&self, _request: &LLMRequest) -> Result<(), LLMError> {
        Ok(())
    }

    fn supports_manual_openai_compaction(&self, _model: &str) -> bool {
        true
    }

    fn supports_reasoning_effort(&self, _model: &str) -> bool {
        true
    }

    fn supported_reasoning_efforts(&self, _model: &str) -> &'static [&'static str] {
        &["minimal", "low", "medium", "high", "xhigh", "max"]
    }

    async fn compact_history_with_options(
        &self,
        _model: &str,
        _history: &[Message],
        options: &ResponsesCompactionOptions,
    ) -> Result<Vec<Message>, LLMError> {
        *self.last_options.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(options.clone());
        Ok(self.output.clone())
    }
}

#[async_trait]
impl LLMProvider for InlinePauseProvider {
    fn name(&self) -> &str {
        "inline-pause"
    }

    async fn generate(&self, request: LLMRequest) -> Result<LLMResponse, LLMError> {
        *self.last_request.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(request);
        let mut response = LLMResponse::new("stub-model", "compacted by provider");
        response.finish_reason = FinishReason::Pause;
        response.compaction = Some("provider compaction summary".to_string());
        if self.include_compaction_detail {
            response.reasoning_details = Some(vec![
                json!({
                    "type": "compaction",
                    "content": "provider compaction summary",
                    "signature": "provider-signature",
                    "opaque_extension": "preserve-me",
                })
                .to_string(),
            ]);
        }
        Ok(response)
    }

    fn supported_models(&self) -> Vec<String> {
        vec!["stub-model".to_string()]
    }

    fn validate_request(&self, _request: &LLMRequest) -> Result<(), LLMError> {
        Ok(())
    }

    fn supports_responses_compaction(&self, _model: &str) -> bool {
        true
    }

    fn supports_native_inline_compaction(&self, _model: &str) -> bool {
        true
    }

    fn effective_context_size(&self, _model: &str) -> usize {
        200_000
    }
}

#[async_trait]
impl LLMProvider for ContextEditsInlineProvider {
    fn name(&self) -> &str {
        "context-edits-inline"
    }

    async fn generate(&self, request: LLMRequest) -> Result<LLMResponse, LLMError> {
        *self.last_request.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(request);
        let mut response = LLMResponse::new("stub-model", "compacted by provider");
        response.finish_reason = FinishReason::Pause;
        response.compaction = Some("provider compaction summary".to_string());
        response.reasoning_details = Some(vec![
            json!({
                "type": "compaction",
                "content": "provider compaction summary",
                "signature": "provider-signature",
            })
            .to_string(),
        ]);
        Ok(response)
    }

    fn supported_models(&self) -> Vec<String> {
        vec!["stub-model".to_string()]
    }

    fn validate_request(&self, _request: &LLMRequest) -> Result<(), LLMError> {
        Ok(())
    }

    fn supports_responses_compaction(&self, _model: &str) -> bool {
        true
    }

    fn supports_context_edits(&self, _model: &str) -> bool {
        true
    }

    fn supports_native_inline_compaction(&self, _model: &str) -> bool {
        true
    }

    fn effective_context_size(&self, _model: &str) -> usize {
        200_000
    }
}

#[async_trait]
impl LLMProvider for CapacityFailOnceProvider {
    fn name(&self) -> &str {
        "capacity-fail-once"
    }

    async fn generate(&self, request: LLMRequest) -> Result<LLMResponse, LLMError> {
        let mut attempts = self.attempts.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        self.request_tokens
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(request.messages.iter().map(Message::estimate_tokens).sum());
        *attempts += 1;
        if *attempts <= self.failures {
            return Err(LLMError::Provider {
                message: "maximum context length exceeded".to_string(),
                metadata: None,
            });
        }
        Ok(LLMResponse::new("stub-model", "retried summary"))
    }

    fn supported_models(&self) -> Vec<String> {
        vec!["stub-model".to_string()]
    }

    fn validate_request(&self, _request: &LLMRequest) -> Result<(), LLMError> {
        Ok(())
    }

    fn effective_context_size(&self, _model: &str) -> usize {
        200_000
    }
}

#[async_trait]
impl LLMProvider for StreamingOnlyCompactionProvider {
    fn name(&self) -> &str {
        "streaming-only-compaction"
    }

    fn supports_streaming(&self) -> bool {
        true
    }

    fn supports_non_streaming(&self, _model: &str) -> bool {
        false
    }

    async fn generate(&self, _request: LLMRequest) -> Result<LLMResponse, LLMError> {
        *self.generate_calls.lock().unwrap_or_else(std::sync::PoisonError::into_inner) += 1;
        Err(LLMError::Provider {
            message: "streaming-only provider cannot generate non-streaming responses".to_string(),
            metadata: None,
        })
    }

    async fn stream_normalized(&self, request: LLMRequest) -> Result<LLMNormalizedStream, LLMError> {
        *self.stream_calls.lock().unwrap_or_else(std::sync::PoisonError::into_inner) += 1;
        self.stream_modes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(request.stream);
        Ok(Box::pin(stream::iter(vec![Ok(NormalizedStreamEvent::Done {
            response: Box::new(LLMResponse::new("stub-model", "streamed summary")),
        })])))
    }

    fn supported_models(&self) -> Vec<String> {
        vec!["stub-model".to_string()]
    }

    fn validate_request(&self, _request: &LLMRequest) -> Result<(), LLMError> {
        Ok(())
    }
}

#[async_trait]
impl LLMProvider for UnknownBudgetFailOnceProvider {
    fn name(&self) -> &str {
        "unknown-budget-fail-once"
    }

    async fn generate(&self, request: LLMRequest) -> Result<LLMResponse, LLMError> {
        let mut attempts = self.attempts.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        self.request_tokens
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(request.messages.iter().map(Message::estimate_tokens).sum());
        *attempts += 1;
        if *attempts == 1 {
            return Err(LLMError::Provider {
                message: "maximum context length exceeded".to_string(),
                metadata: None,
            });
        }
        Ok(LLMResponse::new("stub-model", "retried summary"))
    }

    fn supported_models(&self) -> Vec<String> {
        vec!["stub-model".to_string()]
    }

    fn validate_request(&self, _request: &LLMRequest) -> Result<(), LLMError> {
        Ok(())
    }

    fn effective_context_size(&self, _model: &str) -> usize {
        0
    }
}

#[async_trait]
impl LLMProvider for EmptySummaryProvider {
    fn name(&self) -> &str {
        "empty-summary"
    }

    async fn generate(&self, _request: LLMRequest) -> Result<LLMResponse, LLMError> {
        Ok(LLMResponse::new("stub-model", "   "))
    }

    fn supported_models(&self) -> Vec<String> {
        vec!["stub-model".to_string()]
    }

    fn validate_request(&self, _request: &LLMRequest) -> Result<(), LLMError> {
        Ok(())
    }

    fn effective_context_size(&self, _model: &str) -> usize {
        200_000
    }
}

#[async_trait]
impl LLMProvider for CapturingProvider {
    fn name(&self) -> &str {
        "capturing"
    }

    async fn generate(&self, request: LLMRequest) -> Result<LLMResponse, LLMError> {
        *self.last_request.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(request);
        Ok(LLMResponse::new("stub-model", "summary"))
    }

    fn supported_models(&self) -> Vec<String> {
        vec!["stub-model".to_string()]
    }

    fn validate_request(&self, _request: &LLMRequest) -> Result<(), LLMError> {
        Ok(())
    }

    fn supports_reasoning_effort(&self, _model: &str) -> bool {
        true
    }

    fn supported_reasoning_efforts(&self, _model: &str) -> &'static [&'static str] {
        &["minimal", "low", "medium", "high", "xhigh", "max"]
    }
}

#[async_trait]
impl LLMProvider for LimitedReasoningProvider {
    fn name(&self) -> &str {
        "limited-reasoning"
    }

    async fn generate(&self, request: LLMRequest) -> Result<LLMResponse, LLMError> {
        *self.last_request.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(request);
        Ok(LLMResponse::new("stub-model", "summary"))
    }

    fn supported_models(&self) -> Vec<String> {
        vec!["stub-model".to_string()]
    }

    fn validate_request(&self, _request: &LLMRequest) -> Result<(), LLMError> {
        Ok(())
    }

    fn supports_reasoning_effort(&self, _model: &str) -> bool {
        true
    }

    fn supported_reasoning_efforts(&self, _model: &str) -> &'static [&'static str] {
        &["low", "medium", "high"]
    }
}

#[async_trait]
impl LLMProvider for InlineRejectingProvider {
    fn name(&self) -> &str {
        "inline-rejecting"
    }

    async fn generate(&self, request: LLMRequest) -> Result<LLMResponse, LLMError> {
        // Reject only the inline compaction request (carries the Anthropic
        // `compact_20260112` edit); the Local summary request must succeed.
        if request.context_management.is_some() {
            return Err(LLMError::Provider {
                message: "provider rejected inline compact edit".to_string(),
                metadata: None,
            });
        }
        Ok(LLMResponse::new("stub-model", "summary"))
    }

    fn supported_models(&self) -> Vec<String> {
        vec!["stub-model".to_string()]
    }

    fn validate_request(&self, _request: &LLMRequest) -> Result<(), LLMError> {
        Ok(())
    }

    fn supports_responses_compaction(&self, _model: &str) -> bool {
        true
    }

    fn supports_native_inline_compaction(&self, _model: &str) -> bool {
        true
    }
}

#[async_trait]
impl LLMProvider for CompatibleEndpointProvider {
    fn name(&self) -> &str {
        "compatible-endpoint"
    }

    async fn generate(&self, _request: LLMRequest) -> Result<LLMResponse, LLMError> {
        Ok(LLMResponse::new("stub-model", "summary"))
    }

    fn supported_models(&self) -> Vec<String> {
        vec!["stub-model".to_string()]
    }

    fn validate_request(&self, _request: &LLMRequest) -> Result<(), LLMError> {
        Ok(())
    }

    // Reports Responses API support but neither standalone nor inline
    // compaction (defaults: supports_manual_openai_compaction and
    // supports_native_inline_compaction are both false).
    fn supports_responses_compaction(&self, _model: &str) -> bool {
        true
    }
}

fn sample_history() -> Vec<Message> {
    vec![
        Message::assistant("setup".to_string()),
        Message::user("first request".to_string()),
        Message::assistant("working".to_string()),
        Message::user("second request".to_string()),
    ]
}

fn canonical_standalone_output() -> Vec<Message> {
    vec![
        Message::user("retained by provider".to_string()),
        Message::assistant(String::new()).with_reasoning_details(Some(vec![json!({
            "type": "compaction",
            "id": "cmp_1",
            "encrypted_content": "opaque_state"
        })])),
    ]
}

/// Build an assistant message that carries a (single) pending tool call.
fn assistant_with_calls(content: &str, call_id: &str) -> Message {
    let mut message = Message::assistant(content.to_string());
    message.tool_calls = Some(vec![ToolCall {
        id: call_id.to_string(),
        call_type: "function".to_string(),
        function: None,
        text: None,
        thought_signature: None,
    }]);
    message
}

pub mod capacity;
pub mod coherence;
pub mod continuity;
pub mod fork;
pub mod manual;
pub mod policy;
pub mod route;
pub mod strategy;
