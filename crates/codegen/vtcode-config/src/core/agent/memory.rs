//! Codex-compatible memories and persistent-memory configuration.

use serde::{Deserialize, Serialize};

/// Codex-compatible memories configuration.
///
/// Controls whether VT Code extracts durable context from completed threads
/// and injects it into future sessions. Mirrors the Codex `[memories]` table.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct MemoriesConfig {
    /// Controls whether newly completed threads can be stored as
    /// memory-generation inputs.
    #[serde(default = "default_memories_generate")]
    pub(crate) generate_memories: bool,

    /// Controls whether VT Code injects existing memories into future sessions.
    #[serde(default = "default_memories_use")]
    pub(crate) use_memories: bool,

    /// Overrides the model used for per-thread memory extraction.
    #[serde(default)]
    pub extract_model: Option<String>,

    /// Overrides the model used for global memory consolidation.
    #[serde(default)]
    pub consolidation_model: Option<String>,

    /// Number of recent sessions scanned by batch memory extraction
    /// (`run_batch_memory_extraction` / `/memory rebuild --batch`).
    #[serde(default = "default_memories_batch_sessions")]
    pub batch_sessions: usize,

    /// Maximum concurrent per-session reads during batch extraction.
    #[serde(default = "default_memories_batch_concurrency")]
    pub batch_concurrency: usize,
}

impl Default for MemoriesConfig {
    fn default() -> Self {
        Self {
            generate_memories: default_memories_generate(),
            use_memories: default_memories_use(),
            extract_model: None,
            consolidation_model: None,
            batch_sessions: default_memories_batch_sessions(),
            batch_concurrency: default_memories_batch_concurrency(),
        }
    }
}

#[inline]
const fn default_memories_generate() -> bool {
    true
}

#[inline]
const fn default_memories_use() -> bool {
    true
}

#[inline]
const fn default_memories_batch_sessions() -> usize {
    50
}

#[inline]
const fn default_memories_batch_concurrency() -> usize {
    8
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PersistentMemoryConfig {
    /// Toggle main-session persistent memory for this repository. Natural-language saves resolve
    /// "it", "this", and "that" only against the immediately preceding assistant answer and
    /// require confirmation; identity names and aliases are stored as preferences.
    #[serde(default = "default_persistent_memory_enabled")]
    pub enabled: bool,

    /// Write durable memory after completed turns and session finalization
    #[serde(default = "default_persistent_memory_auto_write")]
    pub auto_write: bool,

    /// Optional user-local directory override for persistent memory storage
    #[serde(default)]
    pub directory_override: Option<String>,

    /// Startup line budget scanned from memory_summary.md before VT Code renders a compact startup summary
    #[serde(default = "default_persistent_memory_startup_line_limit")]
    pub startup_line_limit: usize,

    /// Startup byte budget scanned from memory_summary.md before VT Code renders a compact startup summary
    #[serde(default = "default_persistent_memory_startup_byte_limit")]
    pub startup_byte_limit: usize,

    /// Startup token budget for the injected memory excerpt. `0` disables the
    /// token cap and keeps only the line/byte budgets.
    #[serde(default = "default_persistent_memory_startup_token_budget")]
    pub startup_token_budget: usize,

    /// Codex-compatible memories sub-configuration
    #[serde(default)]
    pub memories: MemoriesConfig,
}

impl Default for PersistentMemoryConfig {
    fn default() -> Self {
        Self {
            enabled: default_persistent_memory_enabled(),
            auto_write: default_persistent_memory_auto_write(),
            directory_override: None,
            startup_line_limit: default_persistent_memory_startup_line_limit(),
            startup_byte_limit: default_persistent_memory_startup_byte_limit(),
            startup_token_budget: default_persistent_memory_startup_token_budget(),
            memories: MemoriesConfig::default(),
        }
    }
}

impl PersistentMemoryConfig {
    pub(crate) fn validate(&self) -> Result<(), String> {
        if self.startup_line_limit == 0 {
            return Err("persistent_memory.startup_line_limit must be greater than 0".to_string());
        }

        if self.startup_byte_limit == 0 {
            return Err("persistent_memory.startup_byte_limit must be greater than 0".to_string());
        }

        Ok(())
    }
}

#[inline]
const fn default_persistent_memory_enabled() -> bool {
    false
}

#[inline]
const fn default_persistent_memory_auto_write() -> bool {
    true
}

#[inline]
const fn default_persistent_memory_startup_line_limit() -> usize {
    200
}

#[inline]
const fn default_persistent_memory_startup_byte_limit() -> usize {
    25 * 1024
}

#[inline]
const fn default_persistent_memory_startup_token_budget() -> usize {
    5000
}
