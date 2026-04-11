use super::{
CompactionContext, CompactionState, GroundedFactRecord,
build_server_compaction_context_management, build_summarized_fork_history,
compact_history_for_recovery_in_place, compact_history_from_index_in_place,
compact_history_in_place, compact_history_in_place_with_events,
inject_latest_memory_envelope, latest_memory_envelope_path_for_session,
manual_openai_compact_history_in_place, maybe_auto_compact_history,
resolve_compaction_threshold,
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
use vtcode_core::config::constants::tools as tool_names;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::llm::provider::{
    LLMError, LLMProvider, LLMRequest, LLMResponse, Message, MessageRole,
    ResponsesCompactionOptions, ToolCall,
};

struct LocalCompactionProvider;

struct ProviderCompactionProvider;

struct NoOpProviderCompactionProvider;

struct FailingProviderCompactionProvider;

struct RecordingProviderCompactionProvider {
    seen_history: Arc<RwLock<Vec<Message>>>,
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

#[async_trait]
impl LLMProvider for ProviderCompactionProvider {
    fn name(&self) -> &str {
        "provider-stub"
    }

    async fn generate(&self, _request: LLMRequest) -> Result<LLMResponse, LLMError> {
        Ok(LLMResponse::new("stub-model", "summary"))
    }

    async fn compact_history(
        &self,
        _model: &str,
        history: &[Message],
    ) -> Result<Vec<Message>, LLMError> {
        let mut compacted = Vec::new();
        compacted.push(Message::system(
            "Previous conversation summary:\nProvider compacted history".to_string(),
        ));
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

    async fn compact_history(
        &self,
        _model: &str,
        history: &[Message],
    ) -> Result<Vec<Message>, LLMError> {
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
        Ok(LLMResponse::new("stub-model", "summary"))
    }

    async fn compact_history(
        &self,
        _model: &str,
        _history: &[Message],
    ) -> Result<Vec<Message>, LLMError> {
        Err(LLMError::Provider {
            message: "provider compaction failed".to_string(),
            metadata: None,
        })
    }

    fn supports_responses_compaction(&self, _model: &str) -> bool {
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

    async fn compact_history(
        &self,
        _model: &str,
        history: &[Message],
    ) -> Result<Vec<Message>, LLMError> {
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

fn assert_local_compaction_history(history: &[Message], envelope_index: usize) {
    assert_local_compaction_history_with_user_count(history, envelope_index, 4);
}

fn assert_local_compaction_history_with_user_count(
    history: &[Message],
    envelope_index: usize,
    retained_user_messages: usize,
) {
    assert_eq!(history.len(), retained_user_messages + 2);
    assert!(
        history[envelope_index]
            .content
            .as_text()
            .contains("[Session Memory Envelope]")
    );
    assert_eq!(
        history.len(),
        history
            .iter()
            .filter(|message| {
                message.role == MessageRole::System || message.role == MessageRole::User
            })
            .count()
    );
    assert!(history.iter().any(|message| {
        message.role == MessageRole::System
            && message
                .content
                .as_text()
                .contains("Previous conversation summary")
    }));
    assert_eq!(
        history
            .iter()
            .filter(|message| message.role == MessageRole::User)
            .count(),
        retained_user_messages
    );
}

fn read_file_tool_call(id: &str, path: &str) -> ToolCall {
    ToolCall::function(
        id.to_string(),
        tool_names::READ_FILE.to_string(),
        json!({ "path": path }).to_string(),
    )
}

fn unified_file_read_tool_call(id: &str, path: &str) -> ToolCall {
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
    ContextManager::new(
        "You are VT Code.".to_string(),
        (),
        std::sync::Arc::new(RwLock::new(HashMap::new())),
        None,
    )
}

#[tokio::test]
async fn manual_compaction_succeeds_without_server_side_support() {
    let temp = tempdir().expect("tempdir");
    let provider = LocalCompactionProvider;
    let mut history = test_history();
    let mut session_stats = SessionStats::default();
    session_stats.set_previous_response_chain("stub", "stub-model", Some("resp_123"), &[]);
    let mut context_manager = test_context_manager();
    context_manager.update_token_usage(&Some(Usage {
        prompt_tokens: 900,
        completion_tokens: 10,
        total_tokens: 910,
        ..Usage::default()
    }));

    let outcome = compact_history_in_place(
        &provider,
        "stub-model",
        "session-alpha",
        temp.path(),
        Some(&VTCodeConfig::default()),
        &mut history,
        &mut session_stats,
        &mut context_manager,
    )
    .await
    .expect("manual compaction succeeds")
    .expect("history should compact");

    assert_eq!(outcome.original_len, 12);
    assert_eq!(outcome.compacted_len, 5);
    assert_local_compaction_history(&history, 0);
    assert_eq!(
        session_stats.previous_response_id_for("stub", "stub-model"),
        None
    );
    assert!(context_manager.current_token_usage() < 900);
    assert!(latest_memory_envelope_path_for_session(temp.path(), "session-alpha").is_some());
}

#[tokio::test]
async fn manual_compaction_emits_local_compaction_boundary_event() {
    let temp = tempdir().expect("tempdir");
    let provider = LocalCompactionProvider;
    let harness_path = temp.path().join("harness.jsonl");
    let harness_emitter = HarnessEventEmitter::new(harness_path.clone()).expect("emitter");
    let mut history = test_history();
    let mut session_stats = SessionStats::default();
    let mut context_manager = test_context_manager();

    let outcome = compact_history_in_place_with_events(
        CompactionContext::new(
            &provider,
            "stub-model",
            "session-alpha",
            "thread-alpha",
            temp.path(),
            Some(&VTCodeConfig::default()),
            None,
            Some(&harness_emitter),
        ),
        CompactionState::new(&mut history, &mut session_stats, &mut context_manager),
        vtcode_core::exec::events::CompactionTrigger::Manual,
    )
    .await
    .expect("compaction succeeds")
    .expect("history should compact");

    assert_eq!(
        outcome.mode,
        vtcode_core::exec::events::CompactionMode::Local
    );
    let content = fs::read_to_string(harness_path).expect("read harness log");
    assert!(content.contains("\"type\":\"thread.compact_boundary\""));
    assert!(content.contains("\"mode\":\"local\""));
}

#[tokio::test]
async fn provider_compaction_emits_provider_boundary_event() {
    let temp = tempdir().expect("tempdir");
    let provider = ProviderCompactionProvider;
    let harness_path = temp.path().join("provider-harness.jsonl");
    let harness_emitter = HarnessEventEmitter::new(harness_path.clone()).expect("emitter");
    let mut history = test_history();
    let mut session_stats = SessionStats::default();
    let mut context_manager = test_context_manager();

    let outcome = compact_history_in_place_with_events(
        CompactionContext::new(
            &provider,
            "stub-model",
            "session-alpha",
            "thread-alpha",
            temp.path(),
            Some(&VTCodeConfig::default()),
            None,
            Some(&harness_emitter),
        ),
        CompactionState::new(&mut history, &mut session_stats, &mut context_manager),
        vtcode_core::exec::events::CompactionTrigger::Manual,
    )
    .await
    .expect("compaction succeeds")
    .expect("history should compact");

    assert_eq!(
        outcome.mode,
        vtcode_core::exec::events::CompactionMode::Provider
    );
    let content = fs::read_to_string(harness_path).expect("read harness log");
    assert!(content.contains("\"type\":\"thread.compact_boundary\""));
    assert!(content.contains("\"mode\":\"provider\""));
}

#[tokio::test]
async fn manual_openai_compaction_clears_previous_response_chain() {
    let temp = tempdir().expect("tempdir");
    let provider = ProviderCompactionProvider;
    let mut history = test_history();
    let mut session_stats = SessionStats::default();
    session_stats.set_previous_response_chain(
        "provider-stub",
        "stub-model",
        Some("resp_123"),
        &[],
    );
    let mut context_manager = test_context_manager();

    let outcome = manual_openai_compact_history_in_place(
        CompactionContext::new(
            &provider,
            "stub-model",
            "session-alpha",
            "thread-alpha",
            temp.path(),
            Some(&VTCodeConfig::default()),
            None,
            None,
        ),
        CompactionState::new(&mut history, &mut session_stats, &mut context_manager),
        &ResponsesCompactionOptions::default(),
    )
    .await
    .expect("manual OpenAI compaction succeeds")
    .expect("history should compact");

    assert_eq!(
        outcome.mode,
        vtcode_core::exec::events::CompactionMode::Provider
    );
    assert_eq!(
        session_stats.previous_response_id_for("provider-stub", "stub-model"),
        None
    );
}

#[tokio::test]
async fn manual_openai_compaction_rejects_unsupported_provider_without_local_fallback() {
    let temp = tempdir().expect("tempdir");
    let provider = LocalCompactionProvider;
    let mut history = test_history();
    let original_history = history.clone();
    let mut session_stats = SessionStats::default();
    let mut context_manager = test_context_manager();

    let err = manual_openai_compact_history_in_place(
        CompactionContext::new(
            &provider,
            "stub-model",
            "session-alpha",
            "thread-alpha",
            temp.path(),
            Some(&VTCodeConfig::default()),
            None,
            None,
        ),
        CompactionState::new(&mut history, &mut session_stats, &mut context_manager),
        &ResponsesCompactionOptions::default(),
    )
    .await
    .expect_err("unsupported provider should fail");

    assert!(err.to_string().contains(
        "Manual `/compact` is available only for the native OpenAI provider on api.openai.com"
    ));
    assert_eq!(history, original_history);
}

#[tokio::test]
async fn manual_openai_compaction_noop_preserves_existing_history() {
    let temp = tempdir().expect("tempdir");
    let provider = NoOpProviderCompactionProvider;
    let mut history = test_history_with_memory_envelope();
    let original_history = history.clone();
    let mut session_stats = SessionStats::default();
    let mut context_manager = test_context_manager();

    let outcome = manual_openai_compact_history_in_place(
        CompactionContext::new(
            &provider,
            "stub-model",
            "session-alpha",
            "thread-alpha",
            temp.path(),
            Some(&VTCodeConfig::default()),
            None,
            None,
        ),
        CompactionState::new(&mut history, &mut session_stats, &mut context_manager),
        &ResponsesCompactionOptions::default(),
    )
    .await
    .expect("noop compaction succeeds");

    assert!(outcome.is_none());
    assert_eq!(history, original_history);
}

#[tokio::test]
async fn provider_compaction_noop_preserves_existing_history() {
    let temp = tempdir().expect("tempdir");
    let provider = NoOpProviderCompactionProvider;
    let mut history = test_history_with_memory_envelope();
    let original_history = history.clone();
    let mut session_stats = SessionStats::default();
    let mut context_manager = test_context_manager();

    let outcome = compact_history_in_place_with_events(
        CompactionContext::new(
            &provider,
            "stub-model",
            "session-alpha",
            "thread-alpha",
            temp.path(),
            Some(&VTCodeConfig::default()),
            None,
            None,
        ),
        CompactionState::new(&mut history, &mut session_stats, &mut context_manager),
        vtcode_core::exec::events::CompactionTrigger::Manual,
    )
    .await
    .expect("noop compaction succeeds");

    assert!(outcome.is_none());
    assert_eq!(history, original_history);
}

#[tokio::test]
async fn provider_compaction_preserves_original_repeated_file_reads() {
    let temp = tempdir().expect("tempdir");
    let seen_history = Arc::new(RwLock::new(Vec::new()));
    let provider = RecordingProviderCompactionProvider {
        seen_history: Arc::clone(&seen_history),
    };
    let mut history = vec![
        assistant_with_tool_call(read_file_tool_call("call-1", "src/lib.rs")),
        Message::tool_response_with_origin(
            "call-1".to_string(),
            json!({
                "file_path": "src/lib.rs",
                "start_line": 1,
                "end_line": 40,
                "result": "older contents"
            })
            .to_string(),
            tool_names::READ_FILE.to_string(),
        ),
        assistant_with_tool_call(read_file_tool_call("call-2", "src/lib.rs")),
        Message::tool_response_with_origin(
            "call-2".to_string(),
            json!({
                "file_path": "src/lib.rs",
                "start_line": 1,
                "end_line": 40,
                "result": "newer contents"
            })
            .to_string(),
            tool_names::READ_FILE.to_string(),
        ),
    ];
    history.extend(test_history());
    let original_history = history.clone();
    let mut session_stats = SessionStats::default();
    let mut context_manager = test_context_manager();

    let outcome = compact_history_in_place_with_events(
        CompactionContext::new(
            &provider,
            "stub-model",
            "session-alpha",
            "thread-alpha",
            temp.path(),
            Some(&VTCodeConfig::default()),
            None,
            None,
        ),
        CompactionState::new(&mut history, &mut session_stats, &mut context_manager),
        vtcode_core::exec::events::CompactionTrigger::Manual,
    )
    .await
    .expect("provider compaction succeeds");

    assert!(outcome.is_none());
    assert_eq!(history, original_history);

    let seen = seen_history.read().await.clone();
    assert_eq!(seen.len(), original_history.len());
    assert!(seen[1].content.as_text().contains("older contents"));
    assert!(!seen[1].content.as_text().contains("deduped_read"));
}

#[test]
fn dedup_repeated_file_reads_rewrites_only_older_exact_matches() {
    let history = vec![
        assistant_with_tool_call(read_file_tool_call("call-1", "src/lib.rs")),
        Message::tool_response_with_origin(
            "call-1".to_string(),
            json!({
                "file_path": "src/lib.rs",
                "start_line": 1,
                "end_line": 40,
                "result": "older contents"
            })
            .to_string(),
            tool_names::READ_FILE.to_string(),
        ),
        assistant_with_tool_call(unified_file_read_tool_call("call-2", "src/lib.rs")),
        Message::tool_response(
            "call-2".to_string(),
            json!({
                "path": "src/lib.rs",
                "start_line": 1,
                "end_line": 40,
                "result": "newer contents"
            })
            .to_string(),
        ),
    ];

    let deduped = super::dedup_repeated_file_reads_for_local_compaction(&history);

    let older_payload: serde_json::Value =
        serde_json::from_str(deduped[1].content.as_text().as_ref()).expect("json payload");
    assert_eq!(
        older_payload
            .get("deduped_read")
            .and_then(serde_json::Value::as_bool),
        Some(true)
    );
    assert_eq!(
        older_payload
            .get("note")
            .and_then(serde_json::Value::as_str),
        Some(super::DEDUPED_FILE_READ_NOTE)
    );
    assert_eq!(
        older_payload
            .get("file_path")
            .and_then(serde_json::Value::as_str),
        Some("src/lib.rs")
    );
    assert!(deduped[3].content.as_text().contains("newer contents"));
    assert!(!deduped[3].content.as_text().contains("deduped_read"));
}

#[test]
fn dedup_repeated_file_reads_keeps_different_slices_and_chunked_reads() {
    let different_slice_history = vec![
        assistant_with_tool_call(read_file_tool_call("call-1", "src/lib.rs")),
        Message::tool_response_with_origin(
            "call-1".to_string(),
            json!({
                "file_path": "src/lib.rs",
                "start_line": 1,
                "end_line": 20,
                "result": "slice one"
            })
            .to_string(),
            tool_names::READ_FILE.to_string(),
        ),
        assistant_with_tool_call(read_file_tool_call("call-2", "src/lib.rs")),
        Message::tool_response_with_origin(
            "call-2".to_string(),
            json!({
                "file_path": "src/lib.rs",
                "start_line": 21,
                "end_line": 40,
                "result": "slice two"
            })
            .to_string(),
            tool_names::READ_FILE.to_string(),
        ),
    ];
    let chunked_history = vec![
        assistant_with_tool_call(read_file_tool_call("call-3", "src/lib.rs")),
        Message::tool_response_with_origin(
            "call-3".to_string(),
            json!({
                "file_path": "src/lib.rs",
                "start_line": 1,
                "end_line": 40,
                "result": "first chunk",
                "spool_chunked": true,
                "has_more": true
            })
            .to_string(),
            tool_names::READ_FILE.to_string(),
        ),
        assistant_with_tool_call(read_file_tool_call("call-4", "src/lib.rs")),
        Message::tool_response_with_origin(
            "call-4".to_string(),
            json!({
                "file_path": "src/lib.rs",
                "start_line": 1,
                "end_line": 40,
                "result": "second chunk",
                "spool_chunked": true,
                "has_more": false
            })
            .to_string(),
            tool_names::READ_FILE.to_string(),
        ),
    ];

    assert_eq!(
        super::dedup_repeated_file_reads_for_local_compaction(&different_slice_history),
        different_slice_history
    );
    assert_eq!(
        super::dedup_repeated_file_reads_for_local_compaction(&chunked_history),
        chunked_history
    );
}

#[test]
fn recovery_context_previews_include_latest_user_request_and_recent_distinct_tool_outputs() {
    let history = vec![
        Message::user("first request".to_string()),
        Message::tool_response("call-1".to_string(), "duplicate output".to_string()),
        Message::tool_response("call-2".to_string(), "distinct output".to_string()),
        Message::tool_response("call-3".to_string(), "duplicate output".to_string()),
        Message::user("latest request".to_string()),
    ];

    let previews = super::build_recovery_context_previews_with_workspace(&history, None);

    assert_eq!(previews[0], "Latest user request: latest request");
    assert_eq!(previews[1], "Tool output 1: duplicate output");
    assert_eq!(previews[2], "Tool output 2: distinct output");
    assert_eq!(previews.len(), 3);
}

#[test]
fn recovery_context_previews_fall_back_to_latest_assistant_text_when_needed() {
    let history = vec![Message::assistant("assistant summary".to_string())];

    let previews = super::build_recovery_context_previews_with_workspace(&history, None);

    assert_eq!(previews, vec!["Latest assistant text: assistant summary"]);
}

#[test]
fn recovery_context_previews_extract_structured_tool_guidance() {
    let history = vec![
        Message::user("use structured search".to_string()),
        Message::tool_response(
            "call-1".to_string(),
            json!({
                "backend": "ast-grep",
                "matches": [],
                "path": "src/agent",
                "is_recoverable": true,
                "hint": "Pattern looks like a code fragment.",
                "next_action": "Retry with a larger parseable pattern.",
                "fallback_tool": "unified_search",
                "fallback_tool_args": {"action": "structural", "path": "src/agent"}
            })
            .to_string(),
        ),
    ];

    let previews = super::build_recovery_context_previews_with_workspace(&history, None);

    assert_eq!(previews[0], "Latest user request: use structured search");
    assert!(previews[1].contains("No matches found in src/agent"));
    assert!(previews[1].contains("Pattern looks like a code fragment."));
    assert!(previews[1].contains("Next action: Retry with a larger parseable pattern."));
    assert!(previews[1].contains("Fallback tool: unified_search"));
}

#[test]
fn recovery_context_previews_extract_nested_error_guidance_and_spool_excerpt() {
    let temp = tempdir().expect("tempdir");
    let spool_dir = temp.path().join(".vtcode/context/tool_outputs");
    fs::create_dir_all(&spool_dir).expect("spool dir");
    let spool_path = spool_dir.join("read_1.txt");
    fs::write(
        &spool_path,
        (1..=40)
            .map(|idx| format!("spooled-line-{idx}"))
            .collect::<Vec<_>>()
            .join("\n"),
    )
    .expect("spool file");

    let history = vec![
        Message::user("review the read failure".to_string()),
        Message::tool_response(
            "call-1".to_string(),
            json!({
                "path": "src/main.rs",
                "spool_path": ".vtcode/context/tool_outputs/read_1.txt",
                "error": {
                    "message": "Read failed",
                    "hint": "Inspect the spooled content.",
                    "next_action": "Retry with a smaller slice."
                }
            })
            .to_string(),
        ),
    ];

    let previews =
        super::build_recovery_context_previews_with_workspace(&history, Some(temp.path()));

    assert_eq!(previews[0], "Latest user request: review the read failure");
    assert!(previews[1].contains("Read failed"));
    assert!(previews[1].contains("Inspect the spooled content."));
    assert!(previews[1].contains("Next action: Retry with a smaller slice."));
    assert!(previews[1].contains("source_path: src/main.rs"));
    assert!(previews[1].contains("Spool excerpt:"));
    assert!(previews[1].contains("spooled-line-1"));
}

#[test]
fn recovery_context_previews_prefer_substantive_reads_over_recent_low_signal_outputs() {
    let history = vec![
        Message::user("tell me more".to_string()),
        Message::tool_response(
            "call-1".to_string(),
            json!({
                "path": "README.md",
                "content": "VT Code is an open-source coding agent with LLM-native code understanding."
            })
            .to_string(),
        ),
        Message::tool_response(
            "call-2".to_string(),
            json!({
                "path": "docs/ARCHITECTURE.md",
                "content": "VT Code follows a modular architecture designed for maintainability and extensibility."
            })
            .to_string(),
        ),
        Message::tool_response(
            "call-3".to_string(),
            json!({
                "count": 20,
                "items": [{"path": "docs/ide"}]
            })
            .to_string(),
        ),
        Message::tool_response(
            "call-4".to_string(),
            json!({
                "error": "Repeated reads of 'docs/ARCHITECTURE.md' with limited progress detected.",
                "next_action": "Try an alternative tool or narrower scope."
            })
            .to_string(),
        ),
    ];

    let previews = super::build_recovery_context_previews_with_workspace(&history, None);

    assert_eq!(previews[0], "Latest user request: tell me more");
    assert!(previews[1].contains("VT Code follows a modular architecture"));
    assert!(previews[2].contains("VT Code is an open-source coding agent"));
    assert!(previews[3].contains("Repeated reads of 'docs/ARCHITECTURE.md'"));
    assert!(
        previews
            .iter()
            .all(|preview| !preview.contains("Listed 20 items")),
        "low-signal listing should be dropped when richer previews exist: {previews:?}"
    );
}

#[test]
fn legacy_memory_envelope_deserializes_with_new_fields_defaulted() {
    let envelope: super::SessionMemoryEnvelope = serde_json::from_value(json!({
        "session_id": "session-alpha",
        "summary": "Persisted summary",
        "task_summary": "Task tracker",
        "spec_summary": null,
        "evaluation_summary": null,
        "grounded_facts": [{
            "fact": "fact",
            "source": "tool:read_file"
        }],
        "touched_files": ["src/lib.rs"],
        "history_artifact_path": ".vtcode/history/session-alpha.jsonl",
        "generated_at": "2026-03-14T00:00:00Z"
    }))
    .expect("legacy envelope should deserialize");

    assert_eq!(envelope.schema_version, None);
    assert_eq!(envelope.objective, None);
    assert!(envelope.constraints.is_empty());
    assert!(envelope.open_questions.is_empty());
    assert!(envelope.verification_todo.is_empty());
    assert!(envelope.delegation_notes.is_empty());
}

#[test]
fn refresh_session_memory_envelope_merges_existing_continuity_fields() {
    let temp = tempdir().expect("tempdir");
    let history_dir = temp.path().join(".vtcode").join("history");
    fs::create_dir_all(&history_dir).expect("history dir");
    fs::create_dir_all(temp.path().join(".vtcode").join("tasks")).expect("tasks dir");
    fs::write(
        temp.path()
            .join(".vtcode")
            .join("tasks")
            .join("current_task.md"),
        "# Ship compaction cleanup\n- [ ] Run cargo nextest\n- [x] Wire in config\n",
    )
    .expect("write task");
    fs::write(
        temp.path()
            .join(".vtcode")
            .join("tasks")
            .join("current_spec.md"),
        "# Spec\nKeep local compaction aligned with summarized forks.\n",
    )
    .expect("write spec");
    fs::write(
        temp.path()
            .join(".vtcode")
            .join("tasks")
            .join("current_evaluation.md"),
        "# Eval\nNeed a regression test for repeated reads.\n",
    )
    .expect("write eval");

    let prior_envelope = super::SessionMemoryEnvelope {
        session_id: "session-alpha".to_string(),
        schema_version: Some(super::SESSION_MEMORY_ENVELOPE_SCHEMA_VERSION),
        summary: "Prior summary".to_string(),
        objective: Some("Keep continuity".to_string()),
        task_summary: Some("Older task summary".to_string()),
        spec_summary: None,
        evaluation_summary: None,
        constraints: vec!["Do not redesign the harness".to_string()],
        grounded_facts: vec![GroundedFactRecord {
            fact: "Existing grounded fact".to_string(),
            source: "tool:read_file".to_string(),
        }],
        touched_files: vec!["src/old.rs".to_string()],
        open_questions: vec!["What should summarized forks retain?".to_string()],
        verification_todo: vec!["Confirm refresh runs at turn boundaries.".to_string()],
        delegation_notes: vec!["explorer: looked at compaction flow".to_string()],
        history_artifact_path: Some(".vtcode/history/session-alpha_0001.jsonl".to_string()),
        generated_at: "2026-03-14T00:00:00Z".to_string(),
    };
    fs::write(
        history_dir.join("session-alpha.memory.json"),
        serde_json::to_string_pretty(&prior_envelope).expect("serialize envelope"),
    )
    .expect("write envelope");

    let mut history = vec![
        Message::user("Continue the compaction work.".to_string()),
        Message::assistant("I will update the local compaction path.".to_string()),
    ];
    let mut session_stats = SessionStats::default();
    session_stats.record_touched_files(["src/new.rs".to_string()]);

    let update = super::SessionMemoryEnvelopeUpdate {
        grounded_facts: vec![GroundedFactRecord {
            fact: "Child agent confirmed the parser contract.".to_string(),
            source: "subagent:reviewer".to_string(),
        }],
        touched_files: vec!["src/child.rs".to_string()],
        open_questions: vec!["Should dedup cover batch reads?".to_string()],
        verification_todo: vec!["Run cargo check".to_string()],
        delegation_notes: vec!["reviewer: parser contract validated".to_string()],
        ..Default::default()
    };

    let envelope = super::refresh_session_memory_envelope(
        temp.path(),
        "session-alpha",
        Some(&VTCodeConfig::default()),
        &mut history,
        &session_stats,
        Some(&update),
    )
    .expect("refresh succeeds")
    .expect("envelope should be refreshed");

    assert_eq!(
        envelope.objective.as_deref(),
        Some("Ship compaction cleanup")
    );
    assert!(
        envelope
            .constraints
            .contains(&"Do not redesign the harness".to_string())
    );
    assert!(
        envelope
            .spec_summary
            .as_deref()
            .is_some_and(|summary| summary.contains("Keep local compaction aligned"))
    );
    assert!(
        envelope
            .evaluation_summary
            .as_deref()
            .is_some_and(|summary| summary.contains("Need a regression test"))
    );
    assert!(
        envelope
            .open_questions
            .contains(&"Should dedup cover batch reads?".to_string())
    );
    assert!(
        envelope
            .verification_todo
            .iter()
            .any(|item| item.contains("Run cargo nextest"))
    );
    assert!(
        envelope
            .verification_todo
            .contains(&"Run cargo check".to_string())
    );
    assert!(
        envelope
            .delegation_notes
            .contains(&"reviewer: parser contract validated".to_string())
    );
    assert!(envelope.touched_files.contains(&"src/new.rs".to_string()));
    assert!(envelope.touched_files.contains(&"src/child.rs".to_string()));
    assert!(
        history[0]
            .content
            .as_text()
            .contains("[Session Memory Envelope]")
    );
}

#[tokio::test]
async fn provider_compaction_error_preserves_existing_history() {
    let temp = tempdir().expect("tempdir");
    let provider = FailingProviderCompactionProvider;
    let mut history = test_history_with_memory_envelope();
    let original_history = history.clone();
    let mut session_stats = SessionStats::default();
    let mut context_manager = test_context_manager();

    let err = compact_history_in_place_with_events(
        CompactionContext::new(
            &provider,
            "stub-model",
            "session-alpha",
            "thread-alpha",
            temp.path(),
            Some(&VTCodeConfig::default()),
            None,
            None,
        ),
        CompactionState::new(&mut history, &mut session_stats, &mut context_manager),
        vtcode_core::exec::events::CompactionTrigger::Manual,
    )
    .await
    .expect_err("failing provider should fail");

    assert!(!err.to_string().is_empty());
    assert_eq!(history, original_history);
}

#[tokio::test]
async fn auto_compaction_replaces_history_and_clears_response_chain() {
    let temp = tempdir().expect("tempdir");
    let provider = LocalCompactionProvider;
    let mut vt_cfg = VTCodeConfig::default();
    vt_cfg.agent.harness.auto_compaction_enabled = true;
    vt_cfg.agent.harness.auto_compaction_threshold_tokens = Some(700);

    let mut history = test_history();
    let mut session_stats = SessionStats::default();
    session_stats.set_previous_response_chain("stub", "stub-model", Some("resp_123"), &[]);
    let mut context_manager = test_context_manager();
    context_manager.update_token_usage(&Some(Usage {
        prompt_tokens: 900,
        completion_tokens: 10,
        total_tokens: 910,
        ..Usage::default()
    }));

    let outcome = maybe_auto_compact_history(
        CompactionContext::new(
            &provider,
            "stub-model",
            "session-alpha",
            "thread-alpha",
            temp.path(),
            Some(&vt_cfg),
            None,
            None,
        ),
        CompactionState::new(&mut history, &mut session_stats, &mut context_manager),
    )
    .await
    .expect("auto compaction succeeds")
    .expect("history should compact");

    assert_eq!(outcome.original_len, 12);
    assert_eq!(outcome.compacted_len, 5);
    assert_local_compaction_history(&history, 4);
    assert!(
        history[0]
            .content
            .as_text()
            .contains("Previous conversation summary")
    );
    assert_eq!(history[5].role, MessageRole::User);
    assert_eq!(
        session_stats.previous_response_id_for("stub", "stub-model"),
        None
    );
    assert!(context_manager.current_token_usage() < 700);
    assert!(latest_memory_envelope_path_for_session(temp.path(), "session-alpha").is_some());
}

#[tokio::test]
async fn targeted_compaction_preserves_prefix_and_replaces_suffix() {
    let temp = tempdir().expect("tempdir");
    let provider = LocalCompactionProvider;
    let mut history = test_history();
    let preserved_prefix = history[..1].to_vec();
    let mut session_stats = SessionStats::default();
    let mut context_manager = test_context_manager();
    context_manager.update_token_usage(&Some(Usage {
        prompt_tokens: 900,
        completion_tokens: 10,
        total_tokens: 910,
        ..Usage::default()
    }));

    let outcome = compact_history_from_index_in_place(
        &provider,
        "stub-model",
        "session-alpha",
        temp.path(),
        Some(&VTCodeConfig::default()),
        &mut history,
        1,
        &mut session_stats,
        &mut context_manager,
    )
    .await
    .expect("targeted compaction succeeds")
    .expect("history should compact");

    assert_eq!(&history[..1], preserved_prefix.as_slice());
    assert_eq!(outcome.original_len, 12);
    assert_eq!(outcome.compacted_len, 5);
    assert_eq!(history.len(), 6);
    assert!(
        history[1]
            .content
            .as_text()
            .contains("[Session Memory Envelope]")
    );
    assert!(
        history[2]
            .content
            .as_text()
            .contains("Previous conversation summary")
    );
    assert!(latest_memory_envelope_path_for_session(temp.path(), "session-alpha").is_none());
}

#[tokio::test]
async fn recovery_compaction_preserves_current_turn_suffix_and_emits_event() {
    let temp = tempdir().expect("tempdir");
    let provider = LocalCompactionProvider;
    let harness_path = temp.path().join("recovery-harness.jsonl");
    let harness_emitter = HarnessEventEmitter::new(harness_path.clone()).expect("emitter");
    let mut history = test_history();
    history.push(Message::system("Previous turn already completed tool execution. Reuse the latest tool outputs in history instead of rerunning the same exploration. If those tool outputs include `critical_note`, `hint`, `next_action`, `fallback_tool`, `fallback_tool_args`, or `rerun_hint`, follow that guidance first.".to_string()));
    history.push(Message::system("Model follow-up failed after tool activity. Tools are disabled on the next pass; provide a direct textual response from the current context and reuse the latest tool outputs already in history.".to_string()));
    history.push(Message::user("current-turn".to_string()));
    history.push(Message::assistant("".to_string()));
    history.push(Message::tool_response(
        "call-current".to_string(),
        "{\"ok\":true}".to_string(),
    ));
    let preserved_suffix = history[12..].to_vec();
    let mut session_stats = SessionStats::default();
    let mut context_manager = test_context_manager();

    let outcome = compact_history_for_recovery_in_place(
        CompactionContext::new(
            &provider,
            "stub-model",
            "session-alpha",
            "thread-alpha",
            temp.path(),
            Some(&VTCodeConfig::default()),
            None,
            Some(&harness_emitter),
        ),
        CompactionState::new(&mut history, &mut session_stats, &mut context_manager),
        12,
    )
    .await
    .expect("recovery compaction succeeds")
    .expect("history should compact");

    assert_eq!(
        history[history.len() - preserved_suffix.len()..],
        preserved_suffix
    );
    assert!(outcome.compacted_len < outcome.original_len);

    let content = fs::read_to_string(harness_path).expect("read harness log");
    assert!(content.contains("\"type\":\"thread.compact_boundary\""));
    assert!(content.contains("\"trigger\":\"recovery\""));
    assert!(content.contains("\"mode\":\"local\""));
}

#[tokio::test]
async fn recovery_compaction_uses_provider_mode_when_supported() {
    let temp = tempdir().expect("tempdir");
    let provider = ProviderCompactionProvider;
    let harness_path = temp.path().join("provider-recovery-harness.jsonl");
    let harness_emitter = HarnessEventEmitter::new(harness_path.clone()).expect("emitter");
    let mut history = test_history();
    history.push(Message::user("current-turn".to_string()));
    history.push(Message::assistant("".to_string()));
    history.push(Message::tool_response(
        "call-current".to_string(),
        "{\"ok\":true}".to_string(),
    ));
    let preserved_suffix = history[12..].to_vec();
    let mut session_stats = SessionStats::default();
    let mut context_manager = test_context_manager();

    let outcome = compact_history_for_recovery_in_place(
        CompactionContext::new(
            &provider,
            "stub-model",
            "session-alpha",
            "thread-alpha",
            temp.path(),
            Some(&VTCodeConfig::default()),
            None,
            Some(&harness_emitter),
        ),
        CompactionState::new(&mut history, &mut session_stats, &mut context_manager),
        12,
    )
    .await
    .expect("provider recovery compaction succeeds")
    .expect("history should compact");

    assert_eq!(
        outcome.mode,
        vtcode_core::exec::events::CompactionMode::Provider
    );
    assert_eq!(
        history[history.len() - preserved_suffix.len()..],
        preserved_suffix
    );

    let content = fs::read_to_string(harness_path).expect("read harness log");
    assert!(content.contains("\"trigger\":\"recovery\""));
    assert!(content.contains("\"mode\":\"provider\""));
}

#[test]
fn inject_latest_memory_envelope_rehydrates_resume_history() {
    let temp = tempdir().expect("tempdir");
    let history_dir = temp.path().join(".vtcode").join("history");
    fs::create_dir_all(&history_dir).expect("history dir");
    let envelope_path = history_dir.join("resume-session_001.memory.json");
    let envelope = super::SessionMemoryEnvelope {
        session_id: "resume-session".to_string(),
        schema_version: Some(super::SESSION_MEMORY_ENVELOPE_SCHEMA_VERSION),
        summary: "Persisted summary".to_string(),
        objective: None,
        task_summary: Some("Tracker: - [ ] Follow up".to_string()),
        spec_summary: None,
        evaluation_summary: None,
        constraints: Vec::new(),
        grounded_facts: vec![GroundedFactRecord {
            fact: "Cargo.toml declares vtcode-core".to_string(),
            source: "tool:read_file".to_string(),
        }],
        touched_files: vec!["Cargo.toml".to_string()],
        open_questions: Vec::new(),
        verification_todo: Vec::new(),
        delegation_notes: Vec::new(),
        history_artifact_path: Some(".vtcode/history/resume-session_001.jsonl".to_string()),
        generated_at: "2026-03-14T00:00:00Z".to_string(),
    };
    fs::write(
        &envelope_path,
        serde_json::to_string_pretty(&envelope).expect("serialize envelope"),
    )
    .expect("write envelope");

    let mut history = vec![Message::user("resume".to_string())];
    assert!(inject_latest_memory_envelope(
        temp.path(),
        "resume-session",
        &mut history
    ));
    assert!(history[0].content.as_text().contains("Persisted summary"));
    assert!(history[0].content.as_text().contains("Cargo.toml"));
}

#[test]
fn inject_latest_memory_envelope_is_session_scoped() {
    let temp = tempdir().expect("tempdir");
    let history_dir = temp.path().join(".vtcode").join("history");
    fs::create_dir_all(&history_dir).expect("history dir");

    for (session_id, summary) in [
        ("session-alpha", "Alpha summary"),
        ("session-beta", "Beta summary"),
    ] {
        let envelope_path = history_dir.join(format!("{session_id}_0001.memory.json"));
        let envelope = super::SessionMemoryEnvelope {
            session_id: session_id.to_string(),
            schema_version: Some(super::SESSION_MEMORY_ENVELOPE_SCHEMA_VERSION),
            summary: summary.to_string(),
            objective: None,
            task_summary: None,
            spec_summary: None,
            evaluation_summary: None,
            constraints: Vec::new(),
            grounded_facts: Vec::new(),
            touched_files: Vec::new(),
            open_questions: Vec::new(),
            verification_todo: Vec::new(),
            delegation_notes: Vec::new(),
            history_artifact_path: None,
            generated_at: "2026-03-14T00:00:00Z".to_string(),
        };
        fs::write(
            envelope_path,
            serde_json::to_string_pretty(&envelope).expect("serialize envelope"),
        )
        .expect("write envelope");
    }

    let mut history = vec![Message::user("resume".to_string())];
    assert!(inject_latest_memory_envelope(
        temp.path(),
        "session-beta",
        &mut history
    ));
    assert!(history[0].content.as_text().contains("Beta summary"));
    assert!(!history[0].content.as_text().contains("Alpha summary"));
}

#[test]
fn inject_latest_memory_envelope_requires_exact_session_prefix_match() {
    let temp = tempdir().expect("tempdir");
    let history_dir = temp.path().join(".vtcode").join("history");
    fs::create_dir_all(&history_dir).expect("history dir");

    for (file_name, summary) in [
        ("session-a_0001.memory.json", "Exact summary"),
        ("session-alpha_0002.memory.json", "Wrong summary"),
    ] {
        let envelope = super::SessionMemoryEnvelope {
            session_id: "session-a".to_string(),
            schema_version: Some(super::SESSION_MEMORY_ENVELOPE_SCHEMA_VERSION),
            summary: summary.to_string(),
            objective: None,
            task_summary: None,
            spec_summary: None,
            evaluation_summary: None,
            constraints: Vec::new(),
            grounded_facts: Vec::new(),
            touched_files: Vec::new(),
            open_questions: Vec::new(),
            verification_todo: Vec::new(),
            delegation_notes: Vec::new(),
            history_artifact_path: None,
            generated_at: "2026-03-14T00:00:00Z".to_string(),
        };
        fs::write(
            history_dir.join(file_name),
            serde_json::to_string_pretty(&envelope).expect("serialize envelope"),
        )
        .expect("write envelope");
    }

    let mut history = vec![Message::user("resume".to_string())];
    assert!(inject_latest_memory_envelope(
        temp.path(),
        "session-a",
        &mut history
    ));
    assert!(history[0].content.as_text().contains("Exact summary"));
    assert!(!history[0].content.as_text().contains("Wrong summary"));
}

#[tokio::test]
async fn no_envelope_written_when_dynamic_history_is_disabled() {
    let temp = tempdir().expect("tempdir");
    let provider = LocalCompactionProvider;
    let mut vt_cfg = VTCodeConfig::default();
    vt_cfg.context.dynamic.enabled = false;

    let mut history = test_history();
    let mut session_stats = SessionStats::default();
    let mut context_manager = test_context_manager();

    compact_history_in_place(
        &provider,
        "stub-model",
        "session-alpha",
        temp.path(),
        Some(&vt_cfg),
        &mut history,
        &mut session_stats,
        &mut context_manager,
    )
    .await
    .expect("compaction succeeds");

    assert!(latest_memory_envelope_path_for_session(temp.path(), "session-alpha").is_none());
    assert!(
        history[0]
            .content
            .as_text()
            .contains("Previous conversation summary")
    );
}

#[tokio::test]
async fn persisted_envelope_uses_recorded_touched_files_only() {
    let temp = tempdir().expect("tempdir");
    let provider = LocalCompactionProvider;
    let mut history = test_history();
    history.push(Message::user(
        "Mentioning docs/example.md in prose should not populate touched files.".to_string(),
    ));
    let mut session_stats = SessionStats::default();
    session_stats.record_touched_files(["src/main.rs".to_string(), "Cargo.toml".to_string()]);
    let mut context_manager = test_context_manager();

    compact_history_in_place(
        &provider,
        "stub-model",
        "session-alpha",
        temp.path(),
        Some(&VTCodeConfig::default()),
        &mut history,
        &mut session_stats,
        &mut context_manager,
    )
    .await
    .expect("compaction succeeds");

    let envelope_path = latest_memory_envelope_path_for_session(temp.path(), "session-alpha")
        .expect("envelope path");
    let envelope: super::SessionMemoryEnvelope =
        serde_json::from_str(&fs::read_to_string(envelope_path).expect("read envelope"))
            .expect("parse envelope");

    assert_eq!(
        envelope.touched_files,
        vec!["src/main.rs".to_string(), "Cargo.toml".to_string()]
    );
    assert_eq!(envelope.session_id, "session-alpha");
}

#[test]
fn inject_latest_memory_envelope_uses_exact_session_id_when_prefixes_collide() {
    let temp = tempdir().expect("tempdir");
    let history_dir = temp.path().join(".vtcode").join("history");
    fs::create_dir_all(&history_dir).expect("history dir");

    let session_alpha = "01234567890123456789012345678901-alpha";
    let session_beta = "01234567890123456789012345678901-beta";

    for (session_id, summary, suffix) in [
        (session_alpha, "Alpha summary", "0001"),
        (session_beta, "Beta summary", "0002"),
    ] {
        let envelope = super::SessionMemoryEnvelope {
            session_id: session_id.to_string(),
            schema_version: Some(super::SESSION_MEMORY_ENVELOPE_SCHEMA_VERSION),
            summary: summary.to_string(),
            objective: None,
            task_summary: None,
            spec_summary: None,
            evaluation_summary: None,
            constraints: Vec::new(),
            grounded_facts: Vec::new(),
            touched_files: Vec::new(),
            open_questions: Vec::new(),
            verification_todo: Vec::new(),
            delegation_notes: Vec::new(),
            history_artifact_path: None,
            generated_at: "2026-03-14T00:00:00Z".to_string(),
        };
        let file_name = format!("{}_{suffix}.memory.json", &session_id[..32]);
        fs::write(
            history_dir.join(file_name),
            serde_json::to_string_pretty(&envelope).expect("serialize envelope"),
        )
        .expect("write envelope");
    }

    let mut history = vec![Message::user("resume".to_string())];
    assert!(inject_latest_memory_envelope(
        temp.path(),
        session_alpha,
        &mut history
    ));
    assert!(history[0].content.as_text().contains("Alpha summary"));
    assert!(!history[0].content.as_text().contains("Beta summary"));
}

#[tokio::test]
async fn compaction_strips_existing_memory_envelope_before_recompacting() {
    let temp = tempdir().expect("tempdir");
    let provider = LocalCompactionProvider;
    let mut history = test_history();
    history.insert(
        0,
        Message::system("[Session Memory Envelope]\nSummary:\nPersisted summary".to_string()),
    );
    let mut session_stats = SessionStats::default();
    let mut context_manager = test_context_manager();

    let outcome = compact_history_in_place(
        &provider,
        "stub-model",
        "session-alpha",
        temp.path(),
        Some(&VTCodeConfig::default()),
        &mut history,
        &mut session_stats,
        &mut context_manager,
    )
    .await
    .expect("compaction succeeds")
    .expect("history should compact");

    assert_eq!(outcome.original_len, 12);
    assert_eq!(outcome.compacted_len, 5);
    assert_eq!(
        history
            .iter()
            .filter(|message| message
                .content
                .as_text()
                .contains("[Session Memory Envelope]"))
            .count(),
        1
    );
}

#[tokio::test]
async fn summarized_fork_history_reuses_compaction_pipeline_and_prior_envelope() {
    let temp = tempdir().expect("tempdir");
    let history_dir = temp.path().join(".vtcode").join("history");
    fs::create_dir_all(&history_dir).expect("history dir");
    let source_envelope = super::SessionMemoryEnvelope {
        session_id: "session-source".to_string(),
        schema_version: Some(super::SESSION_MEMORY_ENVELOPE_SCHEMA_VERSION),
        summary: "Prior source summary".to_string(),
        objective: Some("Keep the source session moving".to_string()),
        task_summary: Some("Tracker: keep going".to_string()),
        spec_summary: None,
        evaluation_summary: None,
        constraints: Vec::new(),
        grounded_facts: vec![GroundedFactRecord {
            fact: "src/lib.rs was updated".to_string(),
            source: "tool:write_file".to_string(),
        }],
        touched_files: vec!["src/lib.rs".to_string()],
        open_questions: Vec::new(),
        verification_todo: Vec::new(),
        delegation_notes: Vec::new(),
        history_artifact_path: Some(".vtcode/history/session-source_0001.jsonl".to_string()),
        generated_at: "2026-03-14T00:00:00Z".to_string(),
    };
    fs::write(
        history_dir.join("session-source_0001.memory.json"),
        serde_json::to_string_pretty(&source_envelope).expect("serialize envelope"),
    )
    .expect("write envelope");

    let compacted = build_summarized_fork_history(
        &LocalCompactionProvider,
        "stub-model",
        "session-source",
        "session-target",
        temp.path(),
        Some(&VTCodeConfig::default()),
        &test_history(),
    )
    .await
    .expect("summarized fork history");

    assert_eq!(compacted.len(), 6);
    assert!(
        compacted[0]
            .content
            .as_text()
            .contains("[Session Memory Envelope]")
    );
    assert!(compacted[0].content.as_text().contains("src/lib.rs"));
    assert!(
        compacted[1]
            .content
            .as_text()
            .contains("Previous conversation summary")
    );
    assert_eq!(
        compacted
            .iter()
            .filter(|message| message.role == MessageRole::User)
            .count(),
        4
    );
    assert!(compacted.iter().all(
        |message| message.role == MessageRole::System || message.role == MessageRole::User
    ));
}

#[tokio::test]
async fn local_and_fork_compaction_share_retained_user_budget() {
    let temp = tempdir().expect("tempdir");
    let provider = LocalCompactionProvider;
    let mut vt_cfg = VTCodeConfig::default();
    vt_cfg.context.dynamic.retained_user_messages = 2;

    let mut history = test_history();
    let mut session_stats = SessionStats::default();
    let mut context_manager = test_context_manager();

    compact_history_in_place(
        &provider,
        "stub-model",
        "session-alpha",
        temp.path(),
        Some(&vt_cfg),
        &mut history,
        &mut session_stats,
        &mut context_manager,
    )
    .await
    .expect("compaction succeeds")
    .expect("history should compact");

    assert_local_compaction_history_with_user_count(&history, 0, 2);

    let compacted = build_summarized_fork_history(
        &provider,
        "stub-model",
        "session-alpha",
        "session-beta",
        temp.path(),
        Some(&vt_cfg),
        &test_history(),
    )
    .await
    .expect("summarized fork history");

    assert_eq!(
        compacted
            .iter()
            .filter(|message| message.role == MessageRole::User)
            .count(),
        2
    );
}

#[test]
fn grounded_fact_extraction_dedupes_caps_and_skips_errors() {
    let history = vec![
        Message::tool_response_with_origin(
            "call_1".to_string(),
            "{\"result\":\"Cargo.toml declares vtcode-core\"}".to_string(),
            "read_file".to_string(),
        ),
        Message::tool_response_with_origin(
            "call_2".to_string(),
            "{\"result\":\"cargo.toml declares vtcode-core\"}".to_string(),
            "read_file".to_string(),
        ),
        Message::tool_response_with_origin(
            "call_3".to_string(),
            "{\"error\":\"denied\"}".to_string(),
            "read_file".to_string(),
        ),
        Message::user("I prefer concise answers.".to_string()),
    ];

    let facts = super::dedup_latest_facts(&history, 5);
    assert_eq!(facts.len(), 2);
    assert!(facts.iter().any(|fact| fact.source == "tool:read_file"));
    assert!(facts.iter().any(|fact| fact.source == "user_assertion"));
}

#[test]
fn resolve_compaction_threshold_prefers_configured_value() {
    assert_eq!(resolve_compaction_threshold(Some(42), 200_000), Some(42));
}

#[test]
fn resolve_compaction_threshold_uses_context_ratio_when_unset() {
    assert_eq!(resolve_compaction_threshold(None, 200_000), Some(180_000));
}

#[test]
fn resolve_compaction_threshold_clamps_to_context_size() {
    assert_eq!(
        resolve_compaction_threshold(Some(300_000), 200_000),
        Some(200_000)
    );
}

#[test]
fn resolve_compaction_threshold_requires_context_or_override() {
    assert_eq!(resolve_compaction_threshold(None, 0), None);
}

#[test]
fn build_server_compaction_context_management_creates_openai_payload() {
    assert_eq!(
        build_server_compaction_context_management(Some(512), 2_000),
        Some(json!([{
            "type": "compaction",
            "compact_threshold": 512,
        }]))
    );
}
