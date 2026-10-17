//! Vibe-coding (lazy/vague request) configuration.

use serde::{Deserialize, Serialize};

/// Vibe coding configuration for lazy/vague request support
///
/// Enables intelligent context gathering and entity resolution to support
/// casual, imprecise requests like "make it blue" or "decrease by half".
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AgentVibeCodingConfig {
    /// Enable vibe coding support
    #[serde(default = "default_vibe_coding_enabled")]
    pub enabled: bool,

    /// Minimum prompt length for refinement (default: 5 chars)
    #[serde(default = "default_vibe_min_prompt_length")]
    pub min_prompt_length: usize,

    /// Minimum prompt words for refinement (default: 2 words)
    #[serde(default = "default_vibe_min_prompt_words")]
    pub min_prompt_words: usize,

    /// Enable fuzzy entity resolution
    #[serde(default = "default_vibe_entity_resolution")]
    pub enable_entity_resolution: bool,

    /// Entity index cache file path (relative to workspace)
    #[serde(default = "default_vibe_entity_cache")]
    pub entity_index_cache: String,

    /// Maximum entity matches to return (default: 5)
    #[serde(default = "default_vibe_max_entity_matches")]
    max_entity_matches: usize,

    /// Track workspace state (file activity, value changes)
    #[serde(default = "default_vibe_track_workspace")]
    pub track_workspace_state: bool,

    /// Maximum recent files to track (default: 20)
    #[serde(default = "default_vibe_max_recent_files")]
    max_recent_files: usize,

    /// Track value history for inference
    #[serde(default = "default_vibe_track_values")]
    track_value_history: bool,

    /// Enable conversation memory for pronoun resolution
    #[serde(default = "default_vibe_conversation_memory")]
    pub enable_conversation_memory: bool,

    /// Maximum conversation turns to remember (default: 50)
    #[serde(default = "default_vibe_max_memory_turns")]
    max_memory_turns: usize,

    /// Enable pronoun resolution (it, that, this)
    #[serde(default = "default_vibe_pronoun_resolution")]
    enable_pronoun_resolution: bool,

    /// Enable proactive context gathering
    #[serde(default = "default_vibe_proactive_context")]
    enable_proactive_context: bool,

    /// Maximum files to gather for context (default: 3)
    #[serde(default = "default_vibe_max_context_files")]
    max_context_files: usize,

    /// Maximum code snippets per file (default: 20 lines)
    #[serde(default = "default_vibe_max_snippets_per_file")]
    max_context_snippets_per_file: usize,

    /// Maximum search results to include (default: 5)
    #[serde(default = "default_vibe_max_search_results")]
    max_search_results: usize,

    /// Enable relative value inference (by half, double, etc.)
    #[serde(default = "default_vibe_value_inference")]
    pub enable_relative_value_inference: bool,
}

impl Default for AgentVibeCodingConfig {
    fn default() -> Self {
        Self {
            enabled: default_vibe_coding_enabled(),
            min_prompt_length: default_vibe_min_prompt_length(),
            min_prompt_words: default_vibe_min_prompt_words(),
            enable_entity_resolution: default_vibe_entity_resolution(),
            entity_index_cache: default_vibe_entity_cache(),
            max_entity_matches: default_vibe_max_entity_matches(),
            track_workspace_state: default_vibe_track_workspace(),
            max_recent_files: default_vibe_max_recent_files(),
            track_value_history: default_vibe_track_values(),
            enable_conversation_memory: default_vibe_conversation_memory(),
            max_memory_turns: default_vibe_max_memory_turns(),
            enable_pronoun_resolution: default_vibe_pronoun_resolution(),
            enable_proactive_context: default_vibe_proactive_context(),
            max_context_files: default_vibe_max_context_files(),
            max_context_snippets_per_file: default_vibe_max_snippets_per_file(),
            max_search_results: default_vibe_max_search_results(),
            enable_relative_value_inference: default_vibe_value_inference(),
        }
    }
}

// Vibe coding default functions
#[inline]
const fn default_vibe_coding_enabled() -> bool {
    false // Conservative default, opt-in
}

#[inline]
const fn default_vibe_min_prompt_length() -> usize {
    5
}

#[inline]
const fn default_vibe_min_prompt_words() -> usize {
    2
}

#[inline]
const fn default_vibe_entity_resolution() -> bool {
    true
}

#[inline]
fn default_vibe_entity_cache() -> String {
    ".vtcode/entity_index.json".into()
}

#[inline]
const fn default_vibe_max_entity_matches() -> usize {
    5
}

#[inline]
const fn default_vibe_track_workspace() -> bool {
    true
}

#[inline]
const fn default_vibe_max_recent_files() -> usize {
    20
}

#[inline]
const fn default_vibe_track_values() -> bool {
    true
}

#[inline]
const fn default_vibe_conversation_memory() -> bool {
    true
}

#[inline]
const fn default_vibe_max_memory_turns() -> usize {
    50
}

#[inline]
const fn default_vibe_pronoun_resolution() -> bool {
    true
}

#[inline]
const fn default_vibe_proactive_context() -> bool {
    true
}

#[inline]
const fn default_vibe_max_context_files() -> usize {
    3
}

#[inline]
const fn default_vibe_max_snippets_per_file() -> usize {
    20
}

#[inline]
const fn default_vibe_max_search_results() -> usize {
    5
}

#[inline]
const fn default_vibe_value_inference() -> bool {
    true
}
