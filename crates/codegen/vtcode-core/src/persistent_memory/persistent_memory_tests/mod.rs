pub(super) use super::*;
pub(super) use crate::llm::provider::{
    FinishReason, LLMError, LLMNormalizedStream, LLMProvider, LLMRequest, LLMResponse, NormalizedStreamEvent,
};
pub(super) use crate::llm::resolve_api_key_for_model_route;
pub(super) use async_trait::async_trait;
pub(super) use futures::stream;
pub(super) use std::sync::Mutex;
pub(super) use tempfile::tempdir;

struct StaticProvider {
    response: &'static str,
    supports_structured_output: bool,
    last_request: Mutex<Option<LLMRequest>>,
}

impl StaticProvider {
    fn new(response: &'static str) -> Self {
        Self {
            response,
            supports_structured_output: true,
            last_request: Mutex::new(None),
        }
    }

    fn prompt_only_json(response: &'static str) -> Self {
        Self {
            response,
            supports_structured_output: false,
            last_request: Mutex::new(None),
        }
    }

    fn last_request(&self) -> LLMRequest {
        self.last_request
            .lock()
            .expect("request lock")
            .clone()
            .expect("request recorded")
    }
}

#[async_trait]
impl LLMProvider for StaticProvider {
    fn name(&self) -> &str {
        "static"
    }

    async fn generate(&self, request: LLMRequest) -> std::result::Result<LLMResponse, LLMError> {
        *self.last_request.lock().expect("request lock") = Some(request);
        Ok(LLMResponse::new("stub-model", self.response))
    }

    fn supported_models(&self) -> Vec<String> {
        vec!["stub-model".to_string()]
    }

    fn validate_request(&self, _request: &LLMRequest) -> std::result::Result<(), LLMError> {
        Ok(())
    }

    fn supports_structured_output(&self, _model: &str) -> bool {
        self.supports_structured_output
    }
}

fn message_history() -> Vec<Message> {
    vec![
        Message::user("I prefer cargo nextest".to_string()),
        Message::tool_response_with_origin(
            "call-1".to_string(),
            serde_json::json!({"summary":"Tests live under vtcode-core/tests"}).to_string(),
            "code_search".to_string(),
        ),
    ]
}

fn runtime_config(workspace: &Path) -> RuntimeAgentConfig {
    RuntimeAgentConfig {
        model: "gpt-5".to_string(),
        api_key: "test-key".to_string(),
        provider: "openai".to_string(),
        openai_chatgpt_auth: None,
        api_key_env: "OPENAI_API_KEY".to_string(),
        workspace: workspace.to_path_buf(),
        verbose: false,
        quiet: false,
        theme: "ciapre".to_string(),
        reasoning_effort: crate::config::types::ReasoningEffortLevel::None,
        ui_surface: crate::config::types::UiSurfacePreference::Auto,
        prompt_cache: crate::config::PromptCachingConfig::default(),
        model_source: crate::config::types::ModelSelectionSource::WorkspaceConfig,
        custom_api_keys: Default::default(),
        checkpointing_enabled: true,
        checkpointing_storage_dir: None,
        checkpointing_max_snapshots: 10,
        checkpointing_max_age_days: Some(7),
        max_conversation_turns: 10,
        model_behavior: None,
    }
}

fn enabled_memory_config() -> PersistentMemoryConfig {
    PersistentMemoryConfig { enabled: true, ..PersistentMemoryConfig::default() }
}

fn enabled_memory_config_for(workspace: &Path) -> PersistentMemoryConfig {
    PersistentMemoryConfig {
        enabled: true,
        directory_override: Some(workspace.join(".memory").display().to_string()),
        ..PersistentMemoryConfig::default()
    }
}

fn enabled_vt_memory_config_for(workspace: &Path) -> VTCodeConfig {
    let mut config = VTCodeConfig::default();
    config.features.memories = true;
    config.agent.persistent_memory = enabled_memory_config_for(workspace);
    config
}

#[derive(Clone)]
struct StreamingOnlyMemoryProvider {
    response: &'static str,
}

#[async_trait]
impl LLMProvider for StreamingOnlyMemoryProvider {
    fn name(&self) -> &str {
        "streaming-memory"
    }

    fn supports_streaming(&self) -> bool {
        true
    }

    fn supports_non_streaming(&self, _model: &str) -> bool {
        false
    }

    async fn generate(&self, _request: LLMRequest) -> std::result::Result<LLMResponse, LLMError> {
        panic!("generate should not be called for streaming-only provider")
    }

    async fn stream_normalized(&self, _request: LLMRequest) -> std::result::Result<LLMNormalizedStream, LLMError> {
        Ok(Box::pin(stream::iter(vec![Ok(NormalizedStreamEvent::Done {
            response: Box::new(LLMResponse {
                content: Some(self.response.to_string()),
                model: "stub-model".to_string(),
                tool_calls: None,
                usage: None,
                finish_reason: FinishReason::Stop,
                reasoning: None,
                reasoning_details: None,
                organization_id: None,
                request_id: None,
                tool_references: Vec::new(),
                compaction: None,
            }),
        })])))
    }

    fn supported_models(&self) -> Vec<String> {
        vec!["stub-model".to_string()]
    }

    fn validate_request(&self, _request: &LLMRequest) -> std::result::Result<(), LLMError> {
        Ok(())
    }
}

// Regression test for the O(n) dedup in `collect_all_memory_matches`.
// Locks down two invariants of the "keep last occurrence with move-to-end
// ordering" semantics that the O(n²) -> O(n) rewrite must preserve:
//   1. Duplicate facts (equal after whitespace normalization + ASCII
//      lowercasing) collapse to a single entry, and the retained entry is
//      the *last* one in source order (its `source` is the last writer's).
//   2. Non-duplicate entries keep their relative order, and the collapsed
//      entry moves to the position of its last occurrence.
// Concrete: prefs=[A(old), B(b)], repo=[A(new), C(c)] -> [B, A(new), C].
#[tokio::test]
async fn collect_all_memory_matches_dedup_keeps_last_occurrence_and_order() {
    let workspace = tempdir().expect("workspace");
    let config = enabled_memory_config_for(workspace.path());
    scaffold_persistent_memory(&config, workspace.path())
        .await
        .expect("scaffold")
        .expect("status");
    let memory_dir = resolve_persistent_memory_dir(&config, workspace.path())
        .expect("memory dir")
        .expect("resolved dir");
    let files = PersistentMemoryFiles::new(memory_dir, false);

    // Two preferences: A (with extra spacing, will be overwritten) and B.
    tokio::fs::write(
        &files.preferences_file,
        render_topic_file(
            MemoryTopic::Preferences,
            &[
                GroundedFactRecord {
                    fact: "Prefer  cargo   nextest".to_string(),
                    source: encode_topic_source(MemoryTopic::Preferences, "old.md"),
                },
                GroundedFactRecord {
                    fact: "Prefer pnpm for scripts".to_string(),
                    source: encode_topic_source(MemoryTopic::Preferences, "b.md"),
                },
            ],
        ),
    )
    .await
    .expect("preferences");
    // Repository facts: A again (different case/spacing, last writer) and C.
    tokio::fs::write(
        &files.repository_facts_file,
        render_topic_file(
            MemoryTopic::RepositoryFacts,
            &[
                GroundedFactRecord {
                    fact: "prefer cargo nextest".to_string(),
                    source: encode_topic_source(MemoryTopic::RepositoryFacts, "new.md"),
                },
                GroundedFactRecord {
                    fact: "The repo uses Rust stable".to_string(),
                    source: encode_topic_source(MemoryTopic::RepositoryFacts, "c.md"),
                },
            ],
        ),
    )
    .await
    .expect("repository facts");

    let matches = collect_all_memory_matches(&files).await.expect("dedup");

    // [A(old), B(b), A(new), C(c)] -> keep last A, move-to-end -> [B, A(new), C]
    assert_eq!(
        matches.iter().map(|m| (m.fact.as_str(), m.source.as_str())).collect::<Vec<_>>(),
        vec![
            ("Prefer pnpm for scripts", "b.md"),
            ("prefer cargo nextest", "new.md"),
            ("The repo uses Rust stable", "c.md"),
        ],
        "dedup must keep the last occurrence and move it to its last position"
    );

    // No duplicate normalized facts survive.
    let mut normalized: Vec<String> = matches
        .iter()
        .map(|m| normalize_whitespace(&m.fact).to_ascii_lowercase())
        .collect();
    normalized.sort();
    let mut deduped = normalized.clone();
    deduped.dedup();
    assert_eq!(normalized.len(), deduped.len(), "no duplicate facts should remain");
}

mod cleanup_migration_locks;
mod extraction_planner;
mod fallback_rebuild;
mod reads_persist;
