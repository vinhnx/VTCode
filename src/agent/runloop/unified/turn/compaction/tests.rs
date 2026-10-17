use super::{
    CompactionContext, CompactionState, DEDUPED_FILE_READ_NOTE, GroundedFactRecord,
    SESSION_MEMORY_ENVELOPE_SCHEMA_VERSION, SessionMemoryEnvelope, SessionMemoryEnvelopeUpdate,
    build_recovery_context_previews_with_workspace, build_server_compaction_context_management,
    build_summarized_fork_history, compact_history_for_recovery_in_place, compact_history_from_index_in_place,
    compact_history_in_place, compact_history_in_place_with_events, compact_history_on_model_switch_in_place,
    dedup_latest_facts, dedup_repeated_file_reads_for_local_compaction, effective_compaction_threshold,
    inject_latest_memory_envelope, latest_memory_envelope_path_for_session, manual_compact_history_in_place,
    maybe_auto_compact_history, refresh_session_memory_envelope, resolve_compaction_threshold,
};
use crate::agent::runloop::unified::context_manager::ContextManager;
use crate::agent::runloop::unified::inline_events::harness::HarnessEventEmitter;
use crate::agent::runloop::unified::state::SessionStats;
use async_trait::async_trait;
use hashbrown::HashMap;
use serde_json::json;
use std::fs;
use std::sync::Arc;
use tempfile::tempdir;
use tokio::sync::RwLock;
use vtcode_commons::llm::Usage;
use vtcode_core::compaction::ManualCompactionOptions;
use vtcode_core::compaction::effective_session_context_budget;
use vtcode_core::compaction::memory_envelope::DEFAULT_OUTPUT_RESERVE_TOKENS;
use vtcode_core::config::constants::tools as tool_names;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::llm::provider::{
    LLMError, LLMProvider, LLMRequest, LLMResponse, Message, MessageRole, ResponsesCompactionOptions, ToolCall,
};

struct LocalCompactionProvider;

struct ProviderCompactionProvider;

struct NoOpProviderCompactionProvider;

struct FailingProviderCompactionProvider;

/// Inline-dispatched provider (Anthropic-shaped) that rejects the
/// `compact_20260112` inline edit but serves plain summary requests. Models a
/// provider where NativeInline cannot fire; the recovery path must fall back to
/// Local summarization rather than aborting.
struct InlineRejectingRecoveryProvider;

struct RecordingProviderCompactionProvider {
    seen_history: Arc<RwLock<Vec<Message>>>,
}

struct ContextSizedProvider {
    context_size: usize,
    provider_name: &'static str,
}

#[async_trait]
impl LLMProvider for ContextSizedProvider {
    fn name(&self) -> &str {
        self.provider_name
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
        self.context_size
    }
}

#[async_trait]
impl LLMProvider for LocalCompactionProvider {
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
        1_000
    }
}

/// Local-summarization stub with a realistic window. `LocalCompactionProvider`
/// resolves to a ~1 KiB tail *and* a ~360-token summarizer budget, so any
/// history large enough to shrink gets its summary/envelope truncated back
/// off by output bounding. Tests that assert on compacted shape (summary and
/// envelope present, message counts shrunk) need a window where the tail and
/// the output budget agree.
struct RoomyLocalCompactionProvider;

#[async_trait]
impl LLMProvider for RoomyLocalCompactionProvider {
    fn name(&self) -> &str {
        "stub-roomy"
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
        128_000
    }
}

#[async_trait]
impl LLMProvider for ProviderCompactionProvider {
    fn name(&self) -> &str {
        "provider-stub"
    }

    async fn generate(&self, _request: LLMRequest) -> Result<LLMResponse, LLMError> {
        Ok(LLMResponse::new("stub-model", "summary"))
    }

    async fn compact_history(&self, _model: &str, history: &[Message]) -> Result<Vec<Message>, LLMError> {
        let mut compacted = Vec::new();
        compacted.push(Message::system("Previous conversation summary:\nProvider compacted history".to_string()));
        compacted.extend(history.iter().rev().take(2).cloned().collect::<Vec<_>>());
        compacted.reverse();
        Ok(compacted)
    }

    async fn compact_history_with_options(
        &self,
        model: &str,
        history: &[Message],
        _options: &ResponsesCompactionOptions,
    ) -> Result<Vec<Message>, LLMError> {
        self.compact_history(model, history).await
    }

    fn supports_responses_compaction(&self, _model: &str) -> bool {
        true
    }

    fn supports_manual_openai_compaction(&self, _model: &str) -> bool {
        true
    }

    fn supported_models(&self) -> Vec<String> {
        vec!["stub-model".to_string()]
    }

    fn validate_request(&self, _request: &LLMRequest) -> Result<(), LLMError> {
        Ok(())
    }

    fn effective_context_size(&self, _model: &str) -> usize {
        1_000
    }
}

#[async_trait]
impl LLMProvider for NoOpProviderCompactionProvider {
    fn name(&self) -> &str {
        "noop-provider-stub"
    }

    async fn generate(&self, _request: LLMRequest) -> Result<LLMResponse, LLMError> {
        Ok(LLMResponse::new("stub-model", "summary"))
    }

    async fn compact_history(&self, _model: &str, history: &[Message]) -> Result<Vec<Message>, LLMError> {
        Ok(history.to_vec())
    }

    async fn compact_history_with_options(
        &self,
        _model: &str,
        history: &[Message],
        _options: &ResponsesCompactionOptions,
    ) -> Result<Vec<Message>, LLMError> {
        Ok(history.to_vec())
    }

    fn supports_responses_compaction(&self, _model: &str) -> bool {
        true
    }

    fn supports_manual_openai_compaction(&self, _model: &str) -> bool {
        true
    }

    fn supported_models(&self) -> Vec<String> {
        vec!["stub-model".to_string()]
    }

    fn validate_request(&self, _request: &LLMRequest) -> Result<(), LLMError> {
        Ok(())
    }

    fn effective_context_size(&self, _model: &str) -> usize {
        1_000
    }
}

#[async_trait]
impl LLMProvider for FailingProviderCompactionProvider {
    fn name(&self) -> &str {
        "failing-provider-stub"
    }

    async fn generate(&self, _request: LLMRequest) -> Result<LLMResponse, LLMError> {
        // Simulate a true infrastructure failure: the provider cannot produce any
        // response. Under the unified dispatch this provider routes to NativeInline
        // (it does not override `supports_manual_openai_compaction`), so `generate` is the
        // only call path; failing it here makes Local fallback impossible, so the
        // compaction propagates `Err` and the existing history is preserved.
        Err(LLMError::Provider {
            message: "provider unreachable".to_string(),
            metadata: None,
        })
    }

    async fn compact_history(&self, _model: &str, _history: &[Message]) -> Result<Vec<Message>, LLMError> {
        Err(LLMError::Provider {
            message: "provider compaction failed".to_string(),
            metadata: None,
        })
    }

    fn supports_responses_compaction(&self, _model: &str) -> bool {
        true
    }

    fn supports_native_inline_compaction(&self, _model: &str) -> bool {
        true
    }

    fn supported_models(&self) -> Vec<String> {
        vec!["stub-model".to_string()]
    }

    fn validate_request(&self, _request: &LLMRequest) -> Result<(), LLMError> {
        Ok(())
    }

    fn effective_context_size(&self, _model: &str) -> usize {
        1_000
    }
}

#[async_trait]
impl LLMProvider for RecordingProviderCompactionProvider {
    fn name(&self) -> &str {
        "recording-provider-stub"
    }

    async fn generate(&self, _request: LLMRequest) -> Result<LLMResponse, LLMError> {
        Ok(LLMResponse::new("stub-model", "summary"))
    }

    async fn compact_history(&self, _model: &str, history: &[Message]) -> Result<Vec<Message>, LLMError> {
        *self.seen_history.write().await = history.to_vec();
        Ok(history.to_vec())
    }

    async fn compact_history_with_options(
        &self,
        _model: &str,
        history: &[Message],
        _options: &ResponsesCompactionOptions,
    ) -> Result<Vec<Message>, LLMError> {
        self.compact_history("stub-model", history).await
    }

    fn supports_responses_compaction(&self, _model: &str) -> bool {
        true
    }

    fn supports_manual_openai_compaction(&self, _model: &str) -> bool {
        // Routes to NativeStandalone (uses `compact_history_with_options`) so the
        // recorded history reflects the provider-native compaction input.
        true
    }

    fn supported_models(&self) -> Vec<String> {
        vec!["stub-model".to_string()]
    }

    fn validate_request(&self, _request: &LLMRequest) -> Result<(), LLMError> {
        Ok(())
    }

    fn effective_context_size(&self, _model: &str) -> usize {
        1_000
    }
}

#[async_trait]
impl LLMProvider for InlineRejectingRecoveryProvider {
    fn name(&self) -> &str {
        "inline-rejecting-recovery"
    }

    async fn generate(&self, request: LLMRequest) -> Result<LLMResponse, LLMError> {
        // Reject the inline compaction request (carries `context_management`);
        // serve the Local summary request (no `context_management`).
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

    fn effective_context_size(&self, _model: &str) -> usize {
        // Roomy window so the continuity tail does not cover the fixture and
        // the Local fallback genuinely shrinks the history.
        128_000
    }
}

fn test_history() -> Vec<Message> {
    vec![
        Message::user("message-0".to_string()),
        Message::assistant("assistant-0".to_string()),
        Message::tool_response("call-0".to_string(), "tool-0".to_string()),
        Message::user("message-1".to_string()),
        Message::assistant("assistant-1".to_string()),
        Message::tool_response("call-1".to_string(), "tool-1".to_string()),
        Message::user("message-2".to_string()),
        Message::assistant("assistant-2".to_string()),
        Message::tool_response("call-2".to_string(), "tool-2".to_string()),
        Message::user("message-3".to_string()),
        Message::assistant("assistant-3".to_string()),
        Message::tool_response("call-3".to_string(), "tool-3".to_string()),
    ]
}

fn test_history_with_memory_envelope() -> Vec<Message> {
    let mut history = vec![Message::system(
        "[Session Memory Envelope]\nSummary:\nExisting summary".to_string(),
    )];
    history.extend(test_history());
    history
}

/// Build a tool-heavy history that exceeds the continuity-tail budget, so
/// local compaction genuinely shrinks it. Each cycle is a complete
/// user-anchored protocol group with a distinct command (defeats file-read
/// de-duplication) and a sizable tool output. Mirrors real sessions where
/// dozens of assistant/tool turns sit behind many user messages. Size the
/// cycles to the provider window under test (see `ROOMY_HISTORY_CYCLES`).
fn tool_heavy_history(cycles: usize) -> Vec<Message> {
    let mut history = Vec::with_capacity(cycles.saturating_mul(3).saturating_add(2));
    for index in 0..cycles {
        history.push(Message::user(format!("Investigate failure {index} in module alpha and report the root cause")));
        let call = ToolCall::function(
            format!("call-{index}"),
            tool_names::EXEC_COMMAND.to_string(),
            json!({ "cmd": format!("grep -rn pattern-{index} src/module-{index}.rs") }).to_string(),
        );
        let mut assistant = Message::assistant(format!("Running diagnostics for failure {index}"));
        assistant.tool_calls = Some(vec![call]);
        history.push(assistant);
        let output =
            format!("diagnostic output for failure {index}: {}", "all checks passed without errors. ".repeat(40));
        history.push(Message::tool_response(format!("call-{index}"), output));
    }
    history.push(Message::user("Summarize the investigation so far".to_string()));
    history.push(Message::assistant("All investigated failures share the same root cause.".to_string()));
    history
}

/// Cycles sized for roomy providers (tens of KiB continuity tails).
const ROOMY_HISTORY_CYCLES: usize = 120;

/// Mirror of the reported `86 -> 88` session: many tiny assistant/tool turns
/// behind two user messages, fitting entirely within the continuity tail.
/// Compaction must report already-compact instead of growing the history.
fn tail_fitting_tool_history() -> Vec<Message> {
    let mut history = vec![Message::user(
        "refine and update README at Why VT Code section".to_string(),
    )];
    for index in 0..40 {
        history.push(Message::assistant(format!("progress note {index}")));
        history.push(Message::tool_response(format!("call-{index}"), format!("ok {index}")));
    }
    history.push(Message::user("continue".to_string()));
    history.push(Message::assistant("working on it".to_string()));
    history
}

fn assert_local_compaction_history(history: &[Message], _old_envelope_index: usize) {
    assert_local_compaction_history_with_user_count(history, 4);
}

fn assert_local_compaction_history_with_user_count(history: &[Message], _retained_user_messages: usize) {
    let envelope_found = history.iter().any(|message| {
        message.role == MessageRole::System && message.content.as_text().contains("[Session Memory Envelope]")
    });
    let summary_found = history.iter().any(|message| {
        message.role == MessageRole::System && message.content.as_text().contains("Previous conversation summary")
    });
    assert!(
        envelope_found || summary_found,
        "Expected either a memory envelope or a summary in the compacted history"
    );
    let user_count = history.iter().filter(|message| message.role == MessageRole::User).count();
    assert!(user_count >= 1, "Expected at least 1 user message, got {user_count}");
    assert!(history.len() >= 2, "history.len()={} should be at least 2", history.len());
}

fn assert_history_contains_messages(history: &[Message], expected_messages: &[Message]) {
    for expected in expected_messages {
        assert!(
            history.iter().any(|message| message == expected),
            "compacted history lost the {:?} message {:?}",
            expected.role,
            expected.content.as_text()
        );
    }
}

fn read_file_tool_call(id: &str, path: &str) -> ToolCall {
    ToolCall::function(id.to_string(), tool_names::READ_FILE.to_string(), json!({ "path": path }).to_string())
}

fn file_operation_read_tool_call(id: &str, path: &str) -> ToolCall {
    ToolCall::function(
        id.to_string(),
        tool_names::UNIFIED_FILE.to_string(),
        json!({ "action": "read", "path": path }).to_string(),
    )
}

fn assistant_with_tool_call(tool_call: ToolCall) -> Message {
    let mut message = Message::assistant(String::new());
    message.tool_calls = Some(vec![tool_call]);
    message
}

fn test_context_manager() -> ContextManager {
    ContextManager::new("You are VT Code.".to_string(), (), Arc::new(RwLock::new(HashMap::new())), None)
}

pub mod auto;
pub mod compact;
pub mod dedup;
pub mod envelope;
pub mod fork;
pub mod persist;
pub mod previews;
pub mod thresholds;
