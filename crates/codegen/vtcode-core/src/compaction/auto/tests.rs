use super::*;
use crate::compaction::SUPPRESS_STICKY;
use crate::compaction::two_pass::fingerprint_prefix;
use crate::llm::provider::{
    LLMError, LLMNormalizedStream, LLMProvider, LLMRequest, LLMResponse, Message, NormalizedStreamEvent, ToolChoice,
};
use async_trait::async_trait;
use futures::stream;
use std::sync::Mutex;

struct FailingProvider;

struct SuccessfulProvider;

struct NativeStandaloneProvider {
    output: Vec<Message>,
}

#[async_trait]
impl LLMProvider for SuccessfulProvider {
    fn name(&self) -> &str {
        "successful"
    }

    async fn generate(&self, _request: LLMRequest) -> Result<LLMResponse, LLMError> {
        Ok(LLMResponse::new("successful-model", "summary"))
    }

    fn supported_models(&self) -> Vec<String> {
        vec!["successful-model".to_string()]
    }

    fn validate_request(&self, _request: &LLMRequest) -> Result<(), LLMError> {
        Ok(())
    }

    fn effective_context_size(&self, _model: &str) -> usize {
        4_096
    }
}

#[async_trait]
impl LLMProvider for NativeStandaloneProvider {
    fn name(&self) -> &str {
        "native-standalone"
    }

    async fn generate(&self, _request: LLMRequest) -> Result<LLMResponse, LLMError> {
        Ok(LLMResponse::new("native-model", "summary"))
    }

    fn supported_models(&self) -> Vec<String> {
        vec!["native-model".to_string()]
    }

    fn validate_request(&self, _request: &LLMRequest) -> Result<(), LLMError> {
        Ok(())
    }

    fn supports_manual_openai_compaction(&self, _model: &str) -> bool {
        true
    }

    fn effective_context_size(&self, _model: &str) -> usize {
        4_096
    }

    async fn compact_history_with_options(
        &self,
        _model: &str,
        _history: &[Message],
        _options: &crate::llm::provider::ResponsesCompactionOptions,
    ) -> Result<Vec<Message>, LLMError> {
        Ok(self.output.clone())
    }
}

#[async_trait]
impl LLMProvider for FailingProvider {
    fn name(&self) -> &str {
        "failing"
    }

    async fn generate(&self, _request: LLMRequest) -> Result<LLMResponse, LLMError> {
        Err(LLMError::Provider {
            message: "context length exceeded".to_string(),
            metadata: None,
        })
    }

    fn supported_models(&self) -> Vec<String> {
        vec!["failing-model".to_string()]
    }

    fn validate_request(&self, _request: &LLMRequest) -> Result<(), LLMError> {
        Ok(())
    }

    fn effective_context_size(&self, _model: &str) -> usize {
        1000
    }
}

struct CapturingProvider {
    last_request: Mutex<Option<LLMRequest>>,
}

#[async_trait]
impl LLMProvider for CapturingProvider {
    fn name(&self) -> &str {
        "capturing"
    }

    fn supports_streaming(&self) -> bool {
        true
    }

    fn supports_non_streaming(&self, _model: &str) -> bool {
        false
    }

    async fn generate(&self, _request: LLMRequest) -> Result<LLMResponse, LLMError> {
        Err(LLMError::Provider {
            message: "capturing provider is streaming-only".to_string(),
            metadata: None,
        })
    }

    async fn stream_normalized(&self, request: LLMRequest) -> Result<LLMNormalizedStream, LLMError> {
        *self.last_request.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(request);
        Ok(Box::pin(stream::iter(vec![Ok(NormalizedStreamEvent::Done {
            response: Box::new(LLMResponse::new("capturing-model", "summary")),
        })])))
    }

    fn supported_models(&self) -> Vec<String> {
        vec!["capturing-model".to_string()]
    }

    fn validate_request(&self, _request: &LLMRequest) -> Result<(), LLMError> {
        Ok(())
    }

    fn effective_context_size(&self, _model: &str) -> usize {
        4_096
    }
}

#[test]
fn readiness_preserves_threshold_boundary_and_force_gates() {
    let provider = SuccessfulProvider;
    let mut cfg = VTCodeConfig::default();
    cfg.agent.harness.auto_compaction_enabled = true;
    let mut disabled_cfg = cfg.clone();
    disabled_cfg.agent.harness.auto_compaction_enabled = false;
    let mut suppressed = SUPPRESS_NONE;
    let mut input = AutoCompactionInput {
        provider: &provider,
        model: "successful-model",
        session_id: "readiness",
        workspace_root: Path::new("."),
        vt_cfg: Some(&cfg),
        current_token_usage: 2_999,
        reserved_output_tokens: 96,
        touched_files: &[],
        engine_cfg: CompactionConfig::default(),
        manual_options: ManualCompactionOptions::default(),
        placement: MemoryEnvelopePlacement::BeforeLastUserOrSummary,
        prefire: None,
        auto_compact_suppressed: &mut suppressed,
        force_compaction: false,
        steering_update: None,
        parent_context: None,
    };
    // The provider has a 4,096-token window; after the 96-token output
    // reserve, the default 75% threshold is exactly 3,000 tokens.
    assert!(!input.should_compact());
    input.current_token_usage = 3_000;
    assert!(input.should_compact());
    input.current_token_usage = 2_999;
    input.force_compaction = true;
    assert!(input.should_compact(), "forced boundaries bypass pressure only");
    input.vt_cfg = Some(&disabled_cfg);
    assert!(!input.should_compact());
    input.vt_cfg = None;
    assert!(!input.should_compact());
    input.vt_cfg = Some(&cfg);
    *input.auto_compact_suppressed = SUPPRESS_STICKY;
    assert!(!input.should_compact());
}

#[tokio::test]
async fn suppressed_state_skips_compaction() {
    let provider = FailingProvider;
    let mut vt_cfg = VTCodeConfig::default();
    vt_cfg.agent.harness.auto_compaction_enabled = true;
    let mut history = vec![Message::user("hello".to_string())];
    let mut suppressed = SUPPRESS_STICKY;

    let result = auto_compact_messages(
        AutoCompactionInput {
            provider: &provider,
            model: "failing-model",
            session_id: "s1",
            workspace_root: Path::new("."),
            vt_cfg: Some(&vt_cfg),
            reserved_output_tokens: 0,
            current_token_usage: 900,
            touched_files: &[],
            engine_cfg: CompactionConfig::default(),
            manual_options: ManualCompactionOptions::default(),
            placement: MemoryEnvelopePlacement::BeforeLastUserOrSummary,
            prefire: None,
            auto_compact_suppressed: &mut suppressed,
            force_compaction: false,
            steering_update: None,
            parent_context: None,
        },
        &mut history,
    )
    .await;

    assert!(result.unwrap().is_none());
    assert_eq!(suppressed, SUPPRESS_STICKY);
}

#[tokio::test]
async fn failure_sets_suppression_on_deterministic_error() {
    let provider = FailingProvider;
    let mut vt_cfg = VTCodeConfig::default();
    vt_cfg.agent.harness.auto_compaction_enabled = true;
    let mut history = vec![Message::user("hello".to_string())];
    let mut suppressed = SUPPRESS_NONE;

    let result = auto_compact_messages(
        AutoCompactionInput {
            provider: &provider,
            model: "failing-model",
            session_id: "s1",
            workspace_root: Path::new("."),
            vt_cfg: Some(&vt_cfg),
            reserved_output_tokens: 0,
            // Crosses the effective threshold (provider capacity 4_096,
            // zero reserve) so the deterministic provider failure is reached.
            current_token_usage: 4_100,
            touched_files: &[],
            engine_cfg: CompactionConfig {
                keep_last_messages: 0,
                ..CompactionConfig::default()
            },
            manual_options: ManualCompactionOptions::default(),
            placement: MemoryEnvelopePlacement::BeforeLastUserOrSummary,
            prefire: None,
            auto_compact_suppressed: &mut suppressed,
            force_compaction: false,
            steering_update: None,
            parent_context: None,
        },
        &mut history,
    )
    .await;

    assert!(result.is_err());
    assert_eq!(suppressed, SUPPRESS_STICKY);
}

#[tokio::test]
async fn automatic_compaction_persists_live_steering_snapshot() {
    let provider = SuccessfulProvider;
    let mut vt_cfg = VTCodeConfig::default();
    vt_cfg.agent.harness.auto_compaction_enabled = true;
    vt_cfg.context.dynamic.enabled = true;
    vt_cfg.context.dynamic.persist_history = true;
    let workspace = tempfile::tempdir().expect("tempdir");
    // The history must exceed the ~1 KiB continuity tail (4 KiB window)
    // so compaction genuinely shrinks instead of reporting already-compact.
    let mut history = (0..60)
        .map(|index| Message::user(format!("request {index} {}", "detail ".repeat(25))))
        .collect::<Vec<_>>();
    let intent = crate::core::agent::steering::QueuedFollowUpIntent::from_parts("intent-1", "finish the request");
    let steering_update = SessionMemoryEnvelopeUpdate {
        pending_intents: Some(vec![intent.clone()]),
        applied_intent_ids: vec!["applied-1".to_string()],
        ..Default::default()
    };
    let mut suppressed = SUPPRESS_NONE;

    let result = auto_compact_messages(
        AutoCompactionInput {
            provider: &provider,
            model: "successful-model",
            session_id: "session-1",
            workspace_root: workspace.path(),
            vt_cfg: Some(&vt_cfg),
            reserved_output_tokens: 0,
            // Crosses the effective threshold (provider capacity 4_096,
            // zero reserve) so automatic compaction runs.
            current_token_usage: 4_100,
            touched_files: &[],
            engine_cfg: CompactionConfig {
                keep_last_messages: 0,
                ..CompactionConfig::default()
            },
            manual_options: ManualCompactionOptions::default(),
            placement: MemoryEnvelopePlacement::BeforeLastUserOrSummary,
            prefire: None,
            auto_compact_suppressed: &mut suppressed,
            force_compaction: false,
            steering_update: Some(&steering_update),
            parent_context: None,
        },
        &mut history,
    )
    .await
    .expect("automatic compaction should succeed")
    .expect("automatic compaction should run");

    let envelope = result.envelope.expect("compaction should produce an envelope");
    assert_eq!(envelope.pending_intents, vec![intent]);
    assert_eq!(envelope.applied_intent_ids, vec!["applied-1"]);
}

#[tokio::test]
async fn automatic_native_compaction_preserves_provider_window_when_persisting_envelope() {
    let provider_output = vec![
        Message::assistant(String::new()).with_reasoning_details(Some(vec![serde_json::json!({
            "type": "compaction",
            "content": null,
            "signature": "signed-provider-state",
        })])),
        Message::user("provider-retained".to_string()),
    ];
    let provider = NativeStandaloneProvider { output: provider_output.clone() };
    let mut vt_cfg = VTCodeConfig::default();
    vt_cfg.agent.harness.auto_compaction_enabled = true;
    vt_cfg.context.dynamic.enabled = true;
    vt_cfg.context.dynamic.persist_history = true;
    let workspace = tempfile::tempdir().expect("tempdir");
    let mut history = (0..12)
        .map(|index| Message::user(format!("request {index}")))
        .collect::<Vec<_>>();
    let mut suppressed = SUPPRESS_NONE;

    let result = auto_compact_messages(
        AutoCompactionInput {
            provider: &provider,
            model: "native-model",
            session_id: "session-1",
            workspace_root: workspace.path(),
            vt_cfg: Some(&vt_cfg),
            reserved_output_tokens: 0,
            current_token_usage: 4_100,
            touched_files: &[],
            engine_cfg: CompactionConfig {
                keep_last_messages: 0,
                ..CompactionConfig::default()
            },
            manual_options: ManualCompactionOptions::default(),
            placement: MemoryEnvelopePlacement::Start,
            prefire: None,
            auto_compact_suppressed: &mut suppressed,
            force_compaction: false,
            steering_update: None,
            parent_context: None,
        },
        &mut history,
    )
    .await
    .expect("automatic native compaction should succeed")
    .expect("automatic native compaction should run");

    assert_eq!(result.mode, CompactionMode::Provider);
    assert!(result.envelope.is_some());
    assert_eq!(history, provider_output, "provider output is the canonical replay window");
}

#[tokio::test]
async fn prefire_pass2_fork_reuses_parent_prefix() {
    use crate::compaction::{AsyncCompactionCache, PrefireState};

    let provider = CapturingProvider { last_request: Mutex::new(None) };
    let mut vt_cfg = VTCodeConfig::default();
    vt_cfg.agent.harness.auto_compaction_enabled = true;
    let workspace = tempfile::tempdir().expect("tempdir");
    // Sized past the ~1 KiB continuity tail so the two-pass rebuild
    // shrinks the history instead of being discarded as already-compact.
    let history = (0..60)
        .map(|index| Message::user(format!("request {index} {}", "detail ".repeat(25))))
        .collect::<Vec<_>>();
    let mut history = history;
    let prefix_len = 50;
    let prefire = PrefireState::default();
    prefire.store(AsyncCompactionCache {
        note1: "prior summary".to_string(),
        prefix_len,
        fingerprint: fingerprint_prefix(&history[..prefix_len]),
        model_slug: "capturing-model".to_string(),
        pass1_latency_ms: 5,
    });
    let mut suppressed = SUPPRESS_NONE;

    let result = auto_compact_messages(
        AutoCompactionInput {
            provider: &provider,
            model: "capturing-model",
            session_id: "session-1",
            workspace_root: workspace.path(),
            vt_cfg: Some(&vt_cfg),
            reserved_output_tokens: 0,
            // Crosses the effective threshold (provider capacity 4_096,
            // zero reserve) so automatic compaction runs.
            current_token_usage: 4_100,
            touched_files: &[],
            engine_cfg: CompactionConfig {
                keep_last_messages: 0,
                ..CompactionConfig::default()
            },
            manual_options: ManualCompactionOptions::default(),
            placement: MemoryEnvelopePlacement::BeforeLastUserOrSummary,
            prefire: Some(&prefire),
            auto_compact_suppressed: &mut suppressed,
            force_compaction: false,
            steering_update: None,
            parent_context: Some(CompactionParentContext {
                system_prompt: Some(std::sync::Arc::from("parent system")),
                tools: None,
            }),
        },
        &mut history,
    )
    .await
    .expect("automatic compaction should succeed")
    .expect("prefire two-pass compaction should run");

    assert_eq!(result.mode, CompactionMode::Local);
    let captured = provider.last_request.lock().unwrap().clone().expect("captured pass-2 request");
    assert_eq!(captured.system_prompt.as_deref(), Some("parent system"));
    assert!(matches!(captured.tool_choice, Some(ToolChoice::None)));
    assert!(!captured.messages.is_empty());
}
