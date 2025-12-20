use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::context::{ConversationMemory, EntityResolver, ProactiveGatherer, WorkspaceState};
use vtcode_core::llm::{factory::create_provider_with_config, provider as uni};

const MIN_PROMPT_LENGTH_FOR_REFINEMENT: usize = 20;
const MIN_PROMPT_WORDS_FOR_REFINEMENT: usize = 4;
const SHORT_PROMPT_WORD_THRESHOLD: usize = 6;
const MAX_REFINED_WORD_MULTIPLIER: usize = 3;
const MIN_KEYWORD_LENGTH: usize = 3;
const MIN_KEYWORD_OVERLAP_RATIO: f32 = 0.5;

/// Combined refinement and enrichment function (Phase 3 integration)
pub(crate) async fn refine_and_enrich_prompt(
    raw: &str,
    cfg: &CoreAgentConfig,
    vt_cfg: Option<&VTCodeConfig>,
) -> String {
    // Step 1: Apply standard refinement first
    let refined = refine_user_prompt_if_enabled(raw, cfg, vt_cfg).await;

    // Step 2: Apply vibe coding enrichment if enabled
    if let Some(vtc) = vt_cfg
        && should_enrich_prompt(&refined, Some(vtc))
    {
        let enricher = PromptEnricher::new(cfg.workspace.clone(), vtc.clone());
        let enriched = enricher.enrich_vague_prompt(&refined).await;
        return enriched.to_llm_prompt();
    }

    refined
}

pub(crate) async fn refine_user_prompt_if_enabled(
    raw: &str,
    cfg: &CoreAgentConfig,
    vt_cfg: Option<&VTCodeConfig>,
) -> String {
    if std::env::var("VTCODE_PROMPT_REFINER_STUB").is_ok() {
        return format!("[REFINED] {}", raw);
    }
    let Some(vtc) = vt_cfg else {
        return raw.to_string();
    };
    if !vtc.agent.refine_prompts_enabled {
        return raw.to_string();
    }

    if !should_attempt_refinement(raw) {
        return raw.to_string();
    }

    let provider_name = if cfg.provider.trim().is_empty() {
        "gemini".to_string()
    } else {
        cfg.provider.to_lowercase()
    };

    let refiner_model = if !vtc.agent.refine_prompts_model.is_empty() {
        vtc.agent.refine_prompts_model.clone()
    } else {
        match provider_name.as_str() {
            "openai" => vtcode_core::config::constants::models::openai::GPT_5_MINI.to_string(),
            _ => cfg.model.clone(),
        }
    };

    let Ok(refiner) = create_provider_with_config(
        &provider_name,
        Some(cfg.api_key.clone()),
        None,
        Some(refiner_model.clone()),
        Some(cfg.prompt_cache.clone()),
        None,
        None,
    ) else {
        return raw.to_string();
    };

    let supports_effort = refiner.supports_reasoning_effort(&refiner_model);
    let reasoning_effort = if supports_effort {
        Some(vtc.agent.reasoning_effort)
    } else {
        None
    };
    let req = uni::LLMRequest {
        messages: vec![uni::Message::user(raw.to_string())],
        system_prompt: None,
        tools: None,
        model: refiner_model,
        max_tokens: Some(vtc.agent.refine_max_tokens),
        temperature: Some(vtc.agent.refine_temperature),
        stream: false,
        tool_choice: Some(uni::ToolChoice::none()),
        parallel_tool_calls: None,
        parallel_tool_config: None,
        reasoning_effort,
        output_format: None,
        verbosity: None,
    };

    match refiner
        .generate(req)
        .await
        .map(|response| response.content.unwrap_or_default())
    {
        Ok(text) if should_accept_refinement(raw, &text) => {
            // If the user's prompt looks like a debug/analyze request, append a concise tools hint
            let lower = text.to_lowercase();
            let debug_triggers = [
                "debug",
                "analyze",
                "error",
                "fix",
                "issue",
                "troubleshoot",
                "diagnose",
            ];
            if debug_triggers.iter().any(|token| lower.contains(token)) {
                format!(
                    "{}\n\nNote: For diagnostics, prefer using tools: debug_agent, analyze_agent, search_tools.",
                    text
                )
            } else {
                text
            }
        }
        _ => raw.to_string(),
    }
}

fn should_attempt_refinement(raw: &str) -> bool {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return false;
    }

    let char_len = trimmed.chars().count();
    let word_count = trimmed.split_whitespace().count();

    char_len >= MIN_PROMPT_LENGTH_FOR_REFINEMENT && word_count >= MIN_PROMPT_WORDS_FOR_REFINEMENT
}

fn should_accept_refinement(raw: &str, refined: &str) -> bool {
    let trimmed = refined.trim();
    if trimmed.is_empty() {
        return false;
    }

    if trimmed.eq_ignore_ascii_case(raw.trim()) {
        return true;
    }

    // Avoid allocating Vecs when we only need word counts for early checks.
    let raw_word_count = raw.split_whitespace().count();
    if raw_word_count < MIN_PROMPT_WORDS_FOR_REFINEMENT {
        return false;
    }

    let refined_word_count = trimmed.split_whitespace().count();
    if raw_word_count <= SHORT_PROMPT_WORD_THRESHOLD
        && refined_word_count > raw_word_count * MAX_REFINED_WORD_MULTIPLIER
    {
        return false;
    }

    let refined_lower = trimmed.to_lowercase();
    let suspicious_prefixes = ["hello", "hi", "hey", "greetings", "i'm", "i am"];
    if suspicious_prefixes
        .iter()
        .any(|prefix| refined_lower.starts_with(prefix))
    {
        return false;
    }
    let suspicious_phrases = ["how can i help you", "i'm here to", "let me know if"];
    if suspicious_phrases
        .iter()
        .any(|phrase| refined_lower.contains(phrase))
    {
        return false;
    }

    let raw_keywords = keyword_set(raw);
    if raw_keywords.is_empty() {
        return true;
    }
    let refined_keywords = keyword_set(trimmed);
    let overlap = raw_keywords.intersection(&refined_keywords).count() as f32;
    let ratio = overlap / raw_keywords.len() as f32;
    ratio >= MIN_KEYWORD_OVERLAP_RATIO
}

fn keyword_set(text: &str) -> HashSet<String> {
    text.split_whitespace()
        .map(|token| token.trim_matches(|ch: char| !ch.is_alphanumeric()))
        .filter(|token| token.len() >= MIN_KEYWORD_LENGTH)
        .map(|token| token.to_ascii_lowercase())
        .collect()
}

// ============================================================================
// Vibe Coding Support - Lazy/Vague Request Enrichment
// ============================================================================

/// Vague patterns that indicate casual, imprecise requests
const VAGUE_PATTERNS: &[&str] = &[
    r"\bit\b",    // "make it blue"
    r"\bthat\b",  // "fix that bug"
    r"\bthe\b",   // "decrease the padding"
    r"\bthis\b",  // "update this"
    r"\bhere\b",  // "add here"
    r"\bthese\b", // "remove these"
    r"\bthose\b", // "change those"
];

/// A vague reference detected in the prompt
#[derive(Debug, Clone)]
pub struct VagueReference {
    pub term: String,
    #[allow(dead_code)]
    pub position: usize,
}

/// Resolution of a vague reference to a concrete entity
#[derive(Debug, Clone)]
pub struct EntityResolution {
    pub original: String,
    pub resolved: String,
    pub file: String,
    pub line: usize,
    pub confidence: f32,
}

/// Enriched prompt with context and resolutions
#[derive(Debug, Clone)]
pub struct EnrichedPrompt {
    pub original: String,
    pub resolutions: Vec<EntityResolution>,
    pub recent_files: Vec<String>,
    pub inferred_values: Vec<(String, String)>,
    pub context_hints: Vec<String>,
}

impl EnrichedPrompt {
    /// Create new enriched prompt
    pub fn new(original: String) -> Self {
        Self {
            original,
            resolutions: Vec::new(),
            recent_files: Vec::new(),
            inferred_values: Vec::new(),
            context_hints: Vec::new(),
        }
    }

    /// Add an entity resolution
    pub fn add_resolution(&mut self, resolution: EntityResolution) {
        self.resolutions.push(resolution);
    }

    /// Add a recent file
    pub fn add_recent_file(&mut self, file: String) {
        if !self.recent_files.contains(&file) {
            self.recent_files.push(file);
        }
    }

    /// Add an inferred value
    pub fn add_inferred_value(&mut self, expression: String, value: String) {
        self.inferred_values.push((expression, value));
    }

    /// Add a context hint
    pub fn add_context_hint(&mut self, hint: String) {
        self.context_hints.push(hint);
    }

    /// Convert to LLM prompt format
    pub fn to_llm_prompt(&self) -> String {
        let mut prompt = format!("User request: {}\n\n", self.original);

        if !self.resolutions.is_empty() {
            prompt.push_str("Resolved references:\n");
            for resolution in &self.resolutions {
                prompt.push_str(&format!(
                    "- \"{}\" → {} in {}:{} (confidence: {:.0}%)\n",
                    resolution.original,
                    resolution.resolved,
                    resolution.file,
                    resolution.line,
                    resolution.confidence * 100.0
                ));
            }
            prompt.push('\n');
        }

        if !self.inferred_values.is_empty() {
            prompt.push_str("Inferred values:\n");
            for (expr, value) in &self.inferred_values {
                prompt.push_str(&format!("- \"{}\" → {}\n", expr, value));
            }
            prompt.push('\n');
        }

        if !self.recent_files.is_empty() {
            prompt.push_str("Recent context:\n");
            for file in self.recent_files.iter().take(5) {
                prompt.push_str(&format!("- Last edited: {}\n", file));
            }
            prompt.push('\n');
        }

        if !self.context_hints.is_empty() {
            prompt.push_str("Context hints:\n");
            for hint in &self.context_hints {
                prompt.push_str(&format!("- {}\n", hint));
            }
            prompt.push('\n');
        }

        prompt.push_str("Please interpret the user's request using this context.");
        prompt
    }
}

/// Detect vague references in a prompt
pub fn detect_vague_references(prompt: &str) -> Vec<VagueReference> {
    let mut references = Vec::new();
    let prompt_lower = prompt.to_lowercase();

    for pattern in VAGUE_PATTERNS {
        // Simple word boundary check (not full regex for now)
        let pattern_word = pattern.trim_start_matches(r"\b").trim_end_matches(r"\b");

        for (idx, word) in prompt_lower.split_whitespace().enumerate() {
            let cleaned = word.trim_matches(|c: char| !c.is_alphanumeric());
            if cleaned == pattern_word {
                references.push(VagueReference {
                    term: cleaned.to_string(),
                    position: idx,
                });
            }
        }
    }

    references
}

/// Check if prompt should be enriched (vibe coding enabled)
pub fn should_enrich_prompt(prompt: &str, vt_cfg: Option<&VTCodeConfig>) -> bool {
    let Some(vtc) = vt_cfg else {
        return false;
    };

    // Vibe coding must be enabled
    if !vtc.agent.vibe_coding.enabled {
        return false;
    }

    // Check minimum thresholds
    let char_len = prompt.trim().chars().count();
    let word_count = prompt.split_whitespace().count();

    if char_len < vtc.agent.vibe_coding.min_prompt_length {
        return false;
    }

    if word_count < vtc.agent.vibe_coding.min_prompt_words {
        return false;
    }

    // Check if prompt contains vague references
    let references = detect_vague_references(prompt);
    !references.is_empty()
}

/// Orchestrator that ties together all vibe coding components
pub struct PromptEnricher {
    /// Entity resolver for fuzzy matching
    #[allow(dead_code)]
    entity_resolver: Arc<RwLock<EntityResolver>>,

    /// Workspace state tracker
    #[allow(dead_code)]
    workspace_state: Arc<RwLock<WorkspaceState>>,

    /// Conversation memory for pronoun resolution
    #[allow(dead_code)]
    conversation_memory: Arc<RwLock<ConversationMemory>>,

    /// Proactive context gatherer
    #[allow(dead_code)]
    proactive_gatherer: Arc<ProactiveGatherer>,

    /// Configuration
    vt_cfg: VTCodeConfig,
}

impl PromptEnricher {
    /// Create new enricher
    pub fn new(workspace_root: PathBuf, vt_cfg: VTCodeConfig) -> Self {
        let workspace_state = Arc::new(RwLock::new(WorkspaceState::new()));
        let entity_resolver = Arc::new(RwLock::new(EntityResolver::with_cache(
            workspace_root.clone(),
            PathBuf::from(&vt_cfg.agent.vibe_coding.entity_index_cache),
        )));
        let conversation_memory = Arc::new(RwLock::new(ConversationMemory::new()));
        let proactive_gatherer = Arc::new(ProactiveGatherer::new(
            workspace_root,
            workspace_state.clone(),
        ));

        Self {
            entity_resolver,
            workspace_state,
            conversation_memory,
            proactive_gatherer,
            vt_cfg,
        }
    }

    /// Create enricher with existing components (for testing)
    #[allow(dead_code)]
    pub fn with_components(
        entity_resolver: Arc<RwLock<EntityResolver>>,
        workspace_state: Arc<RwLock<WorkspaceState>>,
        conversation_memory: Arc<RwLock<ConversationMemory>>,
        proactive_gatherer: Arc<ProactiveGatherer>,
        vt_cfg: VTCodeConfig,
    ) -> Self {
        Self {
            entity_resolver,
            workspace_state,
            conversation_memory,
            proactive_gatherer,
            vt_cfg,
        }
    }

    /// Enrich a vague/lazy prompt with contextual information
    pub async fn enrich_vague_prompt(&self, prompt: &str) -> EnrichedPrompt {
        let mut enriched = EnrichedPrompt::new(prompt.to_string());

        // Step 1: Detect vague patterns
        let vague_refs = detect_vague_references(prompt);

        if vague_refs.is_empty() {
            // No vague references, return original
            return enriched;
        }

        // Step 2: Resolve entities (if enabled)
        if self.vt_cfg.agent.vibe_coding.enable_entity_resolution {
            let resolver = self.entity_resolver.read().await;
            for vague_ref in &vague_refs {
                if let Some(entity_match) = resolver.resolve(&vague_ref.term)
                    && let Some(location) = entity_match.locations.first()
                {
                    enriched.add_resolution(EntityResolution {
                        original: vague_ref.term.clone(),
                        resolved: entity_match.entity.clone(),
                        file: location.path.display().to_string(),
                        line: location.line_start,
                        confidence: entity_match.total_score(),
                    });
                }
            }
        }

        // Step 3: Add recent files from workspace state (if enabled)
        if self.vt_cfg.agent.vibe_coding.track_workspace_state {
            let state = self.workspace_state.read().await;
            let recent_files = state.recent_files(5);
            for file_activity in recent_files {
                enriched.add_recent_file(file_activity.path.display().to_string());
            }
        }

        // Step 4: Resolve pronouns from conversation memory (if enabled)
        if self.vt_cfg.agent.vibe_coding.enable_conversation_memory {
            let memory = self.conversation_memory.read().await;
            for vague_ref in &vague_refs {
                // Check if it's a pronoun
                let is_pronoun = matches!(vague_ref.term.as_str(), "it" | "that" | "this");
                if is_pronoun {
                    // Use turn 0 as placeholder - will be improved in Phase 3 integration
                    if let Some(entity_name) = memory.resolve_pronoun(&vague_ref.term, 0) {
                        enriched.add_context_hint(format!(
                            "\"{}\" likely refers to: {}",
                            vague_ref.term, entity_name
                        ));
                    }
                }
            }
        }

        // Step 5: Infer values for relative expressions (if enabled)
        if self
            .vt_cfg
            .agent
            .vibe_coding
            .enable_relative_value_inference
        {
            let state = self.workspace_state.read().await;
            if let Some(resolved_value) = state.resolve_relative_value(prompt) {
                enriched.add_inferred_value(prompt.to_string(), resolved_value);
            }
        }

        enriched
    }

    /// Get reference to workspace state (for tool execution tracking)
    #[allow(dead_code)]
    pub fn workspace_state(&self) -> Arc<RwLock<WorkspaceState>> {
        self.workspace_state.clone()
    }

    /// Get reference to conversation memory (for message tracking)
    #[allow(dead_code)]
    pub fn conversation_memory(&self) -> Arc<RwLock<ConversationMemory>> {
        self.conversation_memory.clone()
    }

    /// Get reference to entity resolver (for index updates)
    #[allow(dead_code)]
    pub fn entity_resolver(&self) -> Arc<RwLock<EntityResolver>> {
        self.entity_resolver.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use vtcode_core::config::core::PromptCachingConfig;
    use vtcode_core::config::models::Provider;
    use vtcode_core::config::types::{
        ModelSelectionSource, ReasoningEffortLevel, UiSurfacePreference,
    };
    use vtcode_core::core::agent::snapshots::{
        DEFAULT_CHECKPOINTS_ENABLED, DEFAULT_MAX_AGE_DAYS, DEFAULT_MAX_SNAPSHOTS,
    };

    #[tokio::test]
    async fn test_prompt_refinement_applies_to_gemini_when_flag_disabled() {
        unsafe {
            std::env::set_var("VTCODE_PROMPT_REFINER_STUB", "1");
        }

        let cfg = CoreAgentConfig {
            model: vtcode_core::config::constants::models::google::GEMINI_2_5_FLASH_PREVIEW
                .to_string(),
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
        };

        let mut vt = VTCodeConfig::default();
        vt.agent.refine_prompts_enabled = true;

        let raw = "make me a list of files";
        let out = refine_user_prompt_if_enabled(raw, &cfg, Some(&vt)).await;

        assert!(out.starts_with("[REFINED] "));

        unsafe {
            std::env::remove_var("VTCODE_PROMPT_REFINER_STUB");
        }
    }

    #[test]
    fn test_should_attempt_refinement_skips_short_inputs() {
        assert!(!should_attempt_refinement("hi"));
        assert!(!should_attempt_refinement("add docs"));
        assert!(should_attempt_refinement(
            "summarize the latest commit changes"
        ));
    }

    #[test]
    fn test_should_accept_refinement_rejects_role_play() {
        let raw = "hello";
        let refined = "Hello! How can I help you today?";
        assert!(!should_accept_refinement(raw, refined));

        let technical_raw = "describe vtcode streaming parser";
        let technical_refined =
            "Provide a detailed description of the vtcode streaming parser implementation.";
        assert!(should_accept_refinement(technical_raw, technical_refined));
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
        };

        let mut vt_cfg = VTCodeConfig::default();
        vt_cfg.agent.vibe_coding.enabled = true;
        vt_cfg.agent.vibe_coding.enable_relative_value_inference = true;
        vt_cfg.agent.vibe_coding.track_workspace_state = true;

        // Create enricher to set up workspace state
        let enricher = PromptEnricher::new(workspace_root.clone(), vt_cfg.clone());

        // Simulate recent file edit with padding value
        {
            let state_arc = enricher.workspace_state();
            let mut state = state_arc.write().await;
            state.record_change(
                PathBuf::from("src/styles.css"),
                Some("  padding: 32px;".to_string()),
                "  padding: 32px;".to_string(),
            );
        }

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
        };

        let mut vt_cfg = VTCodeConfig::default();
        vt_cfg.agent.vibe_coding.enabled = true;
        vt_cfg.agent.vibe_coding.enable_relative_value_inference = true;

        let enricher = PromptEnricher::new(workspace_root.clone(), vt_cfg.clone());

        // Test with JSON config value
        {
            let state_arc = enricher.workspace_state();
            let mut state = state_arc.write().await;
            state.record_change(
                PathBuf::from("config.json"),
                None,
                r#"  "timeout": 5000,"#.to_string(),
            );
        }

        let prompt = "double the timeout";
        let enriched = enricher.enrich_vague_prompt(prompt).await;

        if !enriched.inferred_values.is_empty() {
            let (_, value) = &enriched.inferred_values[0];
            // Should calculate 5000 * 2 = 10000
            assert!(value.contains("10000"));
        }
    }
}
