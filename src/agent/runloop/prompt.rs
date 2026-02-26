use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::context::{ConversationMemory, EntityResolver, ProactiveGatherer, WorkspaceState};
use vtcode_core::llm::{
    factory::{ProviderConfig, create_provider_with_config},
    provider as uni,
};

const MIN_PROMPT_LENGTH_FOR_REFINEMENT: usize = 20;
const MIN_PROMPT_WORDS_FOR_REFINEMENT: usize = 4;
const SHORT_PROMPT_WORD_THRESHOLD: usize = 6;
const MAX_REFINED_WORD_MULTIPLIER: usize = 3;
const MIN_KEYWORD_LENGTH: usize = 3;
const MIN_KEYWORD_OVERLAP_RATIO: f32 = 0.5;

#[path = "prompt_refinement.rs"]
mod prompt_refinement;
#[cfg(test)]
#[path = "prompt_tests.rs"]
mod prompt_tests;
use prompt_refinement::{should_accept_refinement, should_attempt_refinement};

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
    let Some(vtc) = vt_cfg else {
        return raw.to_string();
    };
    if !vtc.agent.refine_prompts_enabled {
        return raw.to_string();
    }
    if std::env::var("VTCODE_PROMPT_REFINER_STUB").is_ok() {
        return format!("[REFINED] {}", raw);
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
        ProviderConfig {
            api_key: Some(cfg.api_key.clone()),
            base_url: None,
            model: Some(refiner_model.clone()),
            prompt_cache: Some(cfg.prompt_cache.clone()),
            timeouts: None,
            anthropic: None,
            model_behavior: cfg.model_behavior.clone(),
        },
    ) else {
        return raw.to_string();
    };

    let supports_effort = refiner.supports_reasoning_effort(&refiner_model);
    let reasoning_effort = if supports_effort {
        Some(vtc.agent.reasoning_effort)
    } else {
        None
    };
    let temperature = if reasoning_effort.is_some()
        && matches!(provider_name.as_str(), "anthropic" | "minimax")
    {
        None
    } else {
        Some(vtc.agent.refine_temperature)
    };
    let req = uni::LLMRequest {
        messages: vec![uni::Message::user(raw.to_string())],
        model: refiner_model,
        temperature,
        tool_choice: Some(uni::ToolChoice::none()),
        reasoning_effort,
        ..Default::default()
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
    entity_resolver: Arc<RwLock<EntityResolver>>,

    /// Workspace state tracker
    workspace_state: Arc<RwLock<WorkspaceState>>,

    /// Conversation memory for pronoun resolution
    conversation_memory: Arc<RwLock<ConversationMemory>>,

    /// Proactive context gatherer (planned for Phase 3 integration)
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
