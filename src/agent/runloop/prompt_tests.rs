use super::*;
use std::collections::BTreeMap;
use vtcode_commons::env_lock;
use vtcode_core::config::core::PromptCachingConfig;
use vtcode_core::config::models::Provider;
use vtcode_core::config::types::{ModelSelectionSource, ReasoningEffortLevel, UiSurfacePreference};
use vtcode_core::core::agent::snapshots::{DEFAULT_CHECKPOINTS_ENABLED, DEFAULT_MAX_AGE_DAYS, DEFAULT_MAX_SNAPSHOTS};

#[tokio::test]
async fn test_prompt_refinement_applies_to_gemini_when_flag_disabled() {
    let env_guard = env_lock::lock();
    env_guard.set_var("VTCODE_PROMPT_REFINER_STUB", "1");

    let cfg = CoreAgentConfig {
        model: vtcode_core::config::constants::models::google::GEMINI_3_FLASH_PREVIEW.to_string(),
        api_key: "test".to_string(),
        provider: "gemini".to_string(),
        api_key_env: Provider::Gemini.default_api_key_env().to_string(),
        workspace: std::env::current_dir().unwrap(),
        verbose: false,
        theme: vtcode_core::ui::theme::DEFAULT_THEME_ID.to_string(),
        reasoning_effort: ReasoningEffortLevel::default(),
        ui_surface: UiSurfacePreference::default(),
        prompt_cache: PromptCachingConfig::default(),
        model_source: ModelSelectionSource::WorkspaceConfig,
        custom_api_keys: BTreeMap::new(),
        checkpointing_enabled: DEFAULT_CHECKPOINTS_ENABLED,
        checkpointing_storage_dir: None,
        checkpointing_max_snapshots: DEFAULT_MAX_SNAPSHOTS,
        checkpointing_max_age_days: Some(DEFAULT_MAX_AGE_DAYS),
        quiet: false,
        max_conversation_turns: 1000,
        model_behavior: None,
        openai_chatgpt_auth: None,
    };

    let mut vt = VTCodeConfig::default();
    vt.agent.refine_prompts_enabled = true;

    let raw = "make me a list of files";
    let out = refine_user_prompt_if_enabled(raw, &cfg, Some(&vt)).await;

    assert!(out.starts_with("[REFINED] "));

    env_guard.remove_var("VTCODE_PROMPT_REFINER_STUB");
}

#[test]
fn test_should_attempt_refinement_skips_short_inputs() {
    assert!(!should_attempt_refinement("hi"));
    assert!(!should_attempt_refinement("add docs"));
    assert!(should_attempt_refinement("summarize the latest commit changes"));
}

#[test]
fn test_should_accept_refinement_rejects_role_play() {
    let raw = "hello";
    let refined = "Hello! How can I help you today?";
    assert!(!should_accept_refinement(raw, refined));

    let technical_raw = "describe vtcode streaming parser";
    let technical_refined = "Provide a detailed description of the vtcode streaming parser implementation.";
    assert!(should_accept_refinement(technical_raw, technical_refined));
}

#[test]
fn refinement_request_preserves_input_and_defines_rewrite_contract() {
    let raw = "Review `src/parser.rs`; do not edit files or run commands.";
    let request = refinement_request(&PlainRoute, raw, "test-model", &["low", "high"], 0.3);
    assert_eq!(request.messages.len(), 1);
    assert_eq!(request.messages[0].content.as_text(), raw);
    let system = request.system_prompt.as_deref().unwrap();
    assert!(system.contains("Do not answer or execute it"));
    assert!(system.contains("negations, authorization limits, paths, identifiers"));
    assert!(system.contains("Add no requirements or facts"));
    assert!(system.contains("return it unchanged"));
    assert!(system.contains("Return only the rewritten request"));
    assert!(system.contains("without changing files"));
    assert_eq!(request.reasoning_effort, Some(ReasoningEffortLevel::Low));
    assert_eq!(request.temperature, None);
    assert!(matches!(request.tool_choice, Some(uni::ToolChoice::None)));
    assert!(request.tools.is_none());
}

#[test]
fn refinement_request_keeps_sampling_only_for_routes_that_accept_it() {
    use vtcode_core::llm::provider::{LLMError, LLMProvider, LLMRequest, LLMResponse, SamplingOverrides};

    #[derive(Clone, Copy)]
    struct SamplingRoute {
        drops_sampling: bool,
    }

    #[async_trait::async_trait]
    impl LLMProvider for SamplingRoute {
        fn name(&self) -> &str {
            "sampling-route"
        }

        async fn generate(&self, _request: LLMRequest) -> Result<LLMResponse, LLMError> {
            Err(LLMError::Provider { message: "test route".to_string(), metadata: None })
        }

        fn sampling_overrides(&self, _model: &str) -> SamplingOverrides {
            SamplingOverrides {
                suppresses_sampling: self.drops_sampling,
                ..Default::default()
            }
        }

        fn supported_models(&self) -> Vec<String> {
            Vec::new()
        }

        fn validate_request(&self, _request: &LLMRequest) -> Result<(), LLMError> {
            Ok(())
        }
    }

    let short = refinement_request(
        &SamplingRoute { drops_sampling: false },
        "Review the parser without edits",
        "plain",
        &[],
        0.2,
    );
    assert_eq!(short.temperature, Some(0.2));

    // Anthropic Messages rejects sampling outright, so a route without
    // advertised efforts must not carry the refinement temperature either.
    let anthropic_short = refinement_request(
        &SamplingRoute { drops_sampling: true },
        "Review the parser without edits",
        "plain",
        &[],
        0.2,
    );
    assert_eq!(anthropic_short.temperature, None);
}

/// Route with the default (sampling-accepting) policy.
struct PlainRoute;

#[async_trait::async_trait]
impl vtcode_core::llm::provider::LLMProvider for PlainRoute {
    fn name(&self) -> &str {
        "plain-route"
    }

    async fn generate(
        &self,
        _request: vtcode_core::llm::provider::LLMRequest,
    ) -> Result<vtcode_core::llm::provider::LLMResponse, vtcode_core::llm::provider::LLMError> {
        Err(vtcode_core::llm::provider::LLMError::Provider { message: "test route".to_string(), metadata: None })
    }

    fn supported_models(&self) -> Vec<String> {
        Vec::new()
    }

    fn validate_request(
        &self,
        _request: &vtcode_core::llm::provider::LLMRequest,
    ) -> Result<(), vtcode_core::llm::provider::LLMError> {
        Ok(())
    }
}

#[test]
fn refinement_request_bounds_output_and_omits_unsupported_effort() {
    let short = refinement_request(&PlainRoute, "Review the parser without edits", "plain", &[], 0.2);
    let long = refinement_request(&PlainRoute, &"parser context ".repeat(10_000), "reasoning", &["high"], 0.4);
    assert!(short.max_tokens.is_some_and(|tokens| (2048..8192).contains(&tokens)));
    assert_eq!(long.max_tokens, Some(8192));
    assert_eq!(short.reasoning_effort, None);
    assert_eq!(long.reasoning_effort, None);
    assert_eq!(short.temperature, Some(0.2));
    assert_eq!(long.temperature, None);
}

#[test]
fn accepted_refinement_rejects_incomplete_or_tool_call_responses() {
    let raw = "Review parser errors without changing files";
    let complete = uni::LLMResponse {
        content: Some(raw.to_string()),
        finish_reason: uni::FinishReason::Stop,
        ..Default::default()
    };
    assert_eq!(accepted_refinement(raw, complete.clone()), Some(raw.to_string()));
    for finish_reason in [uni::FinishReason::Length, uni::FinishReason::ToolCalls] {
        assert_eq!(accepted_refinement(raw, uni::LLMResponse { finish_reason, ..complete.clone() }), None);
    }
    assert_eq!(accepted_refinement(raw, uni::LLMResponse { content: None, ..complete }), None);
}

// Vibe coding tests
#[test]
fn test_detect_vague_references() {
    let prompt = "make it blue";
    let refs = detect_vague_references(prompt);
    assert_eq!(refs.len(), 1);
    assert_eq!(refs[0].term, "it");

    let prompt2 = "fix that bug in the sidebar";
    let refs2 = detect_vague_references(prompt2);
    assert_eq!(refs2.len(), 2); // "that" and "the"
    assert!(refs2.iter().any(|r| r.term == "that"));
    assert!(refs2.iter().any(|r| r.term == "the"));

    let prompt3 = "decrease the padding by half";
    let refs3 = detect_vague_references(prompt3);
    assert_eq!(refs3.len(), 1);
    assert_eq!(refs3[0].term, "the");
}

#[test]
fn test_detect_vague_references_no_matches() {
    let prompt = "create a new function called handleSubmit";
    let refs = detect_vague_references(prompt);
    assert_eq!(refs.len(), 0); // "a" is not in VAGUE_PATTERNS, so no matches
}

#[test]
fn test_enriched_prompt_to_llm_prompt() {
    let mut enriched = EnrichedPrompt::new("make it blue".to_string());

    enriched.add_resolution(EntityResolution {
        original: "it".to_string(),
        resolved: "Sidebar".to_string(),
        file: "src/components/Sidebar.tsx".to_string(),
        line: 15,
        confidence: 0.95,
    });

    enriched.add_recent_file("src/styles/main.css".to_string());
    enriched.add_inferred_value("blue".to_string(), "#0000FF".to_string());

    let prompt = enriched.to_llm_prompt();

    assert!(prompt.contains("User request: make it blue"));
    assert!(prompt.contains("Resolved references:"));
    assert!(prompt.contains("\"it\" → Sidebar"));
    assert!(prompt.contains("src/components/Sidebar.tsx:15"));
    assert!(prompt.contains("confidence: 95%"));
    assert!(prompt.contains("Recent context:"));
    assert!(prompt.contains("src/styles/main.css"));
    assert!(prompt.contains("Inferred values:"));
    assert!(prompt.contains("\"blue\" → #0000FF"));
}

#[test]
fn test_should_enrich_prompt_disabled() {
    let vt_cfg = VTCodeConfig::default();
    // Default has vibe_coding.enabled = false

    let prompt = "make it blue";
    assert!(!should_enrich_prompt(prompt, Some(&vt_cfg)));
}

#[test]
fn test_should_enrich_prompt_enabled() {
    let mut vt_cfg = VTCodeConfig::default();
    vt_cfg.agent.vibe_coding.enabled = true;

    // Has vague reference "it"
    let prompt = "make it blue";
    assert!(should_enrich_prompt(prompt, Some(&vt_cfg)));

    // No vague references
    let prompt2 = "create a new function";
    assert!(!should_enrich_prompt(prompt2, Some(&vt_cfg)));
}

#[test]
fn test_should_enrich_prompt_too_short() {
    let mut vt_cfg = VTCodeConfig::default();
    vt_cfg.agent.vibe_coding.enabled = true;
    vt_cfg.agent.vibe_coding.min_prompt_length = 10;
    vt_cfg.agent.vibe_coding.min_prompt_words = 3;

    // Too short (2 words)
    let prompt = "make it";
    assert!(!should_enrich_prompt(prompt, Some(&vt_cfg)));

    // Long enough (3 words)
    let prompt2 = "make it blue";
    assert!(should_enrich_prompt(prompt2, Some(&vt_cfg)));
}

#[tokio::test]
async fn test_prompt_enricher_new() {
    let workspace_root = std::env::current_dir().unwrap();
    let vt_cfg = VTCodeConfig::default();

    let enricher = PromptEnricher::new(workspace_root, vt_cfg);

    // Verify components are initialized
    assert!(enricher.entity_resolver.read().await.index_is_empty());
    let state = enricher.workspace_state.read().await;
    assert_eq!(state.recent_files(10).len(), 0);
}

#[tokio::test]
async fn test_prompt_enricher_enrich_no_vague_refs() {
    let workspace_root = std::env::current_dir().unwrap();
    let mut vt_cfg = VTCodeConfig::default();
    vt_cfg.agent.vibe_coding.enabled = true;

    let enricher = PromptEnricher::new(workspace_root, vt_cfg);

    let prompt = "create a new function called handleSubmit";
    let enriched = enricher.enrich_vague_prompt(prompt).await;

    // No vague references, should return original
    assert_eq!(enriched.original, prompt);
    assert_eq!(enriched.resolutions.len(), 0);
}

#[tokio::test]
async fn test_prompt_enricher_enrich_with_vague_refs() {
    let workspace_root = std::env::current_dir().unwrap();
    let mut vt_cfg = VTCodeConfig::default();
    vt_cfg.agent.vibe_coding.enabled = true;
    vt_cfg.agent.vibe_coding.enable_entity_resolution = true;
    vt_cfg.agent.vibe_coding.track_workspace_state = true;

    let enricher = PromptEnricher::new(workspace_root, vt_cfg);

    // Add a recent file to workspace state
    {
        let mut state = enricher.workspace_state.write().await;
        state.record_file_access(
            &PathBuf::from("src/test.rs"),
            vtcode_core::context::workspace_state::ActivityType::Edit,
        );
    }

    let prompt = "make it blue";
    let enriched = enricher.enrich_vague_prompt(prompt).await;

    // Should detect "it" as vague reference
    assert_eq!(enriched.original, prompt);
    // Should have recent file added
    assert_eq!(enriched.recent_files.len(), 1);
    assert_eq!(enriched.recent_files[0], "src/test.rs");
}

#[tokio::test]
async fn test_prompt_enricher_to_llm_prompt_format() {
    let workspace_root = std::env::current_dir().unwrap();
    let mut vt_cfg = VTCodeConfig::default();
    vt_cfg.agent.vibe_coding.enabled = true;
    vt_cfg.agent.vibe_coding.track_workspace_state = true;

    let enricher = PromptEnricher::new(workspace_root, vt_cfg);

    // Add a recent file
    {
        let mut state = enricher.workspace_state.write().await;
        state.record_file_access(
            &PathBuf::from("src/components/Sidebar.tsx"),
            vtcode_core::context::workspace_state::ActivityType::Edit,
        );
    }

    let prompt = "update this component";
    let enriched = enricher.enrich_vague_prompt(prompt).await;
    let llm_prompt = enriched.to_llm_prompt();

    // Verify format
    assert!(llm_prompt.contains("User request:"));
    assert!(llm_prompt.contains("update this component"));
    assert!(llm_prompt.contains("Recent context:"));
    assert!(llm_prompt.contains("src/components/Sidebar.tsx"));
}

// Phase 3 Integration Tests
#[tokio::test]
async fn test_refine_and_enrich_prompt_disabled() {
    let workspace_root = std::env::current_dir().unwrap();
    let cfg = CoreAgentConfig {
        model: "test-model".to_string(),
        api_key: "test-key".to_string(),
        provider: "test".to_string(),
        api_key_env: "TEST_API_KEY".to_string(),
        workspace: workspace_root,
        verbose: false,
        theme: vtcode_core::ui::theme::DEFAULT_THEME_ID.to_string(),
        reasoning_effort: ReasoningEffortLevel::default(),
        ui_surface: UiSurfacePreference::default(),
        prompt_cache: PromptCachingConfig::default(),
        model_source: ModelSelectionSource::WorkspaceConfig,
        custom_api_keys: BTreeMap::new(),
        checkpointing_enabled: DEFAULT_CHECKPOINTS_ENABLED,
        checkpointing_storage_dir: None,
        checkpointing_max_snapshots: DEFAULT_MAX_SNAPSHOTS,
        checkpointing_max_age_days: Some(DEFAULT_MAX_AGE_DAYS),
        quiet: false,
        max_conversation_turns: 1000,
        model_behavior: None,
        openai_chatgpt_auth: None,
    };

    let vt_cfg = VTCodeConfig::default(); // vibe_coding disabled by default

    let prompt = "make it blue";
    let result = refine_and_enrich_prompt(prompt, &cfg, Some(&vt_cfg)).await;

    // Should return original since vibe coding is disabled
    assert_eq!(result, prompt);
}

#[tokio::test]
async fn test_refine_and_enrich_prompt_enabled() {
    let workspace_root = std::env::current_dir().unwrap();
    let cfg = CoreAgentConfig {
        model: "test-model".to_string(),
        api_key: "test-key".to_string(),
        provider: "test".to_string(),
        api_key_env: "TEST_API_KEY".to_string(),
        workspace: workspace_root,
        verbose: false,
        theme: vtcode_core::ui::theme::DEFAULT_THEME_ID.to_string(),
        reasoning_effort: ReasoningEffortLevel::default(),
        ui_surface: UiSurfacePreference::default(),
        prompt_cache: PromptCachingConfig::default(),
        model_source: ModelSelectionSource::WorkspaceConfig,
        custom_api_keys: BTreeMap::new(),
        checkpointing_enabled: DEFAULT_CHECKPOINTS_ENABLED,
        checkpointing_storage_dir: None,
        checkpointing_max_snapshots: DEFAULT_MAX_SNAPSHOTS,
        checkpointing_max_age_days: Some(DEFAULT_MAX_AGE_DAYS),
        quiet: false,
        max_conversation_turns: 1000,
        model_behavior: None,
        openai_chatgpt_auth: None,
    };

    let mut vt_cfg = VTCodeConfig::default();
    vt_cfg.agent.vibe_coding.enabled = true;

    let prompt = "make it blue";
    let result = refine_and_enrich_prompt(prompt, &cfg, Some(&vt_cfg)).await;

    // Should be enriched with context
    assert!(result.contains("User request:"));
    assert!(result.contains("make it blue"));
}

#[tokio::test]
async fn test_refine_and_enrich_prompt_no_vague_refs() {
    let workspace_root = std::env::current_dir().unwrap();
    let cfg = CoreAgentConfig {
        model: "test-model".to_string(),
        api_key: "test-key".to_string(),
        provider: "test".to_string(),
        api_key_env: "TEST_API_KEY".to_string(),
        workspace: workspace_root,
        verbose: false,
        theme: vtcode_core::ui::theme::DEFAULT_THEME_ID.to_string(),
        reasoning_effort: ReasoningEffortLevel::default(),
        ui_surface: UiSurfacePreference::default(),
        prompt_cache: PromptCachingConfig::default(),
        model_source: ModelSelectionSource::WorkspaceConfig,
        custom_api_keys: BTreeMap::new(),
        checkpointing_enabled: DEFAULT_CHECKPOINTS_ENABLED,
        checkpointing_storage_dir: None,
        checkpointing_max_snapshots: DEFAULT_MAX_SNAPSHOTS,
        checkpointing_max_age_days: Some(DEFAULT_MAX_AGE_DAYS),
        quiet: false,
        max_conversation_turns: 1000,
        model_behavior: None,
        openai_chatgpt_auth: None,
    };

    let mut vt_cfg = VTCodeConfig::default();
    vt_cfg.agent.vibe_coding.enabled = true;

    // Prompt with no vague references
    let prompt = "create a new function called handleSubmit";
    let result = refine_and_enrich_prompt(prompt, &cfg, Some(&vt_cfg)).await;

    // Should return original since no vague references detected
    assert_eq!(result, prompt);
}

// Phase 4: End-to-End Value Inference Tests
#[tokio::test]
async fn test_value_inference_decrease_by_half_milestone() {
    use std::path::PathBuf;

    let workspace_root = std::env::current_dir().unwrap();
    let _cfg = CoreAgentConfig {
        model: "test-model".to_string(),
        api_key: "test-key".to_string(),
        provider: "test".to_string(),
        api_key_env: "TEST_API_KEY".to_string(),
        workspace: workspace_root.clone(),
        verbose: false,
        theme: vtcode_core::ui::theme::DEFAULT_THEME_ID.to_string(),
        reasoning_effort: ReasoningEffortLevel::default(),
        ui_surface: UiSurfacePreference::default(),
        prompt_cache: PromptCachingConfig::default(),
        model_source: ModelSelectionSource::WorkspaceConfig,
        custom_api_keys: BTreeMap::new(),
        checkpointing_enabled: DEFAULT_CHECKPOINTS_ENABLED,
        checkpointing_storage_dir: None,
        checkpointing_max_snapshots: DEFAULT_MAX_SNAPSHOTS,
        checkpointing_max_age_days: Some(DEFAULT_MAX_AGE_DAYS),
        quiet: false,
        max_conversation_turns: 1000,
        model_behavior: None,
        openai_chatgpt_auth: None,
    };

    let mut vt_cfg = VTCodeConfig::default();
    vt_cfg.agent.vibe_coding.enabled = true;
    vt_cfg.agent.vibe_coding.enable_relative_value_inference = true;
    vt_cfg.agent.vibe_coding.track_workspace_state = true;

    // Create enricher to set up workspace state
    let enricher = PromptEnricher::new(workspace_root.clone(), vt_cfg.clone());

    // Simulate recent file edit with padding value
    enricher
        .record_workspace_change_for_test(
            PathBuf::from("src/styles.css"),
            Some("  padding: 32px;".to_string()),
            "  padding: 32px;".to_string(),
        )
        .await;

    // Test "decrease the padding by half"
    let prompt = "decrease the padding by half";
    let enriched = enricher.enrich_vague_prompt(prompt).await;

    // Should detect "the" as vague reference and infer value
    assert!(enriched.original.contains("padding"));
    assert!(!enriched.inferred_values.is_empty());

    // Should calculate half of 32 = 16
    let (_expr, value) = &enriched.inferred_values[0];
    assert!(value.contains("16"));
}

#[tokio::test]
async fn test_value_inference_multiple_patterns() {
    use std::path::PathBuf;

    let workspace_root = std::env::current_dir().unwrap();
    let _cfg = CoreAgentConfig {
        model: "test-model".to_string(),
        api_key: "test-key".to_string(),
        provider: "test".to_string(),
        api_key_env: "TEST_API_KEY".to_string(),
        workspace: workspace_root.clone(),
        verbose: false,
        theme: vtcode_core::ui::theme::DEFAULT_THEME_ID.to_string(),
        reasoning_effort: ReasoningEffortLevel::default(),
        ui_surface: UiSurfacePreference::default(),
        prompt_cache: PromptCachingConfig::default(),
        model_source: ModelSelectionSource::WorkspaceConfig,
        custom_api_keys: BTreeMap::new(),
        checkpointing_enabled: DEFAULT_CHECKPOINTS_ENABLED,
        checkpointing_storage_dir: None,
        checkpointing_max_snapshots: DEFAULT_MAX_SNAPSHOTS,
        checkpointing_max_age_days: Some(DEFAULT_MAX_AGE_DAYS),
        quiet: false,
        max_conversation_turns: 1000,
        model_behavior: None,
        openai_chatgpt_auth: None,
    };

    let mut vt_cfg = VTCodeConfig::default();
    vt_cfg.agent.vibe_coding.enabled = true;
    vt_cfg.agent.vibe_coding.enable_relative_value_inference = true;

    let enricher = PromptEnricher::new(workspace_root.clone(), vt_cfg.clone());

    // Test with JSON config value
    enricher
        .record_workspace_change_for_test(PathBuf::from("config.json"), None, r#"  "timeout": 5000,"#.to_string())
        .await;

    let prompt = "double the timeout";
    let enriched = enricher.enrich_vague_prompt(prompt).await;

    if !enriched.inferred_values.is_empty() {
        let (_, value) = &enriched.inferred_values[0];
        // Should calculate 5000 * 2 = 10000
        assert!(value.contains("10000"));
    }
}
