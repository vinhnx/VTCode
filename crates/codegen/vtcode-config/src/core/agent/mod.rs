use crate::constants::{defaults, execution, llm_generation, prompt_budget, tool_limits};
use crate::types::{
    ReasoningEffortLevel, ShellPromptProfile, SystemPromptMode, ToolDocumentationMode, UiSurfacePreference,
    VerbosityLevel,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

const DEFAULT_CHECKPOINTS_ENABLED: bool = true;
const DEFAULT_MAX_SNAPSHOTS: usize = 50;
const DEFAULT_MAX_AGE_DAYS: u64 = 30;

mod approval;
mod circuit_breaker;
mod codex_app_server;
mod harness;
mod memory;
mod onboarding;
mod open_responses;
mod prompt_suggestions;
mod small_model;
mod vibe_coding;

pub use approval::{AsyncApprovalConfig, ConfidenceEscalationConfig, SkepticPanelConfig};
pub use circuit_breaker::CircuitBreakerConfig;
pub use codex_app_server::AgentCodexAppServerConfig;
pub use harness::{
    AgentHarnessConfig, InspectionConfig, InspectionMode, ToolResultClearingConfig, TrackerContinuationConfig,
    VerificationAutoRecoveryConfig,
};
pub use memory::{MemoriesConfig, PersistentMemoryConfig};
pub use onboarding::AgentOnboardingConfig;
pub use open_responses::OpenResponsesConfig;
pub use prompt_suggestions::AgentPromptSuggestionsConfig;
pub use small_model::AgentSmallModelConfig;
pub use vibe_coding::AgentVibeCodingConfig;

/// Agent-wide configuration
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AgentConfig {
    /// Active provider for single-agent runs.
    #[serde(default = "default_provider")]
    pub provider: String,

    /// Environment variable that stores the API key for the active provider
    #[serde(default = "default_api_key_env")]
    pub api_key_env: String,

    /// Default model for new conversations.
    #[serde(default = "default_model")]
    pub default_model: String,

    /// UI theme identifier controlling ANSI styling
    #[serde(default = "default_theme")]
    pub theme: String,

    /// System prompt mode controlling prompt verbosity and token overhead.
    /// Options target lean base prompts: minimal (~500 tokens), lightweight (~750 tokens),
    /// default and specialized (~900 tokens) before dynamic runtime addenda.
    #[serde(default)]
    pub system_prompt_mode: SystemPromptMode,

    /// Soft token budget for the fully composed system prompt (character-based
    /// estimate, ~4 chars/token). Includes workspace instructions, guidelines,
    /// and runtime addenda on top of the base prompt.
    #[serde(default = "default_max_system_prompt_tokens")]
    pub max_system_prompt_tokens: u64,

    /// Warn when the composed system prompt exceeds `max_system_prompt_tokens`.
    #[serde(default = "default_system_prompt_budget_warning")]
    pub system_prompt_budget_warning: bool,

    /// Trim low-priority advisory system prompt sections when over budget.
    /// Base, shell-safety, and active-tool contracts are never trimmed.
    #[serde(default = "default_trim_system_prompt")]
    pub trim_system_prompt: bool,

    /// Tool documentation mode controlling token overhead for tool definitions
    /// Options: minimal, progressive (default, ~1.8k tokens for the builtin catalog), full
    /// Progressive: complete tool and parameter descriptions; only unusually long tails are trimmed at a sentence boundary (recommended)
    /// Minimal: first sentence of each tool description and no parameter descriptions (power users)
    /// Full: every tool and parameter description sent unmodified
    #[serde(default)]
    pub tool_documentation_mode: ToolDocumentationMode,

    /// Shell syntax profile used in model-facing command examples.
    /// This controls prompt wording only; command policy remains in the runtime.
    /// Values: auto, unix_like, powershell.
    #[serde(default)]
    pub shell_prompt_profile: ShellPromptProfile,

    /// Enable split tool results for massive token savings (Phase 4)
    /// When enabled, tools return dual-channel output:
    /// - llm_content: Concise summary sent to LLM (token-optimized, 53-95% reduction)
    /// - ui_content: Rich output displayed to user (full details preserved)
    ///   Applies to: exec_command, code_search, apply_patch, and retained internal helpers
    ///   Default: true (opt-out for compatibility), recommended for production use
    #[serde(default = "default_enable_split_tool_results")]
    pub enable_split_tool_results: bool,

    /// Enable TODO planning helper mode for structured task management
    #[serde(default = "default_todo_planning_mode")]
    pub todo_planning_mode: bool,

    /// Preferred rendering surface for the interactive chat UI (inline by default; auto, alternate, inline)
    #[serde(default)]
    pub ui_surface: UiSurfacePreference,

    /// Maximum number of conversation turns before auto-termination
    #[serde(default = "default_max_conversation_turns")]
    pub max_conversation_turns: usize,

    /// Maximum consecutive idle turns (no tool calls, no meaningful response) before
    /// the agent runner treats the session as stalled and aborts the loop.
    #[serde(default = "default_idle_turn_limit")]
    pub idle_turn_limit: usize,

    /// Reasoning depth for capable models (none to max).
    #[serde(default = "default_reasoning_effort")]
    pub reasoning_effort: ReasoningEffortLevel,
    /// Permit a lower supported effort with an explicit harness diagnostic.
    #[serde(default)]
    pub allow_reasoning_effort_downgrade: bool,

    /// Output verbosity for supported models (low to high).
    #[serde(default = "default_verbosity")]
    pub verbosity: VerbosityLevel,

    /// Sampling temperature (0.0 precise to 1.0 creative).
    #[serde(default = "default_temperature")]
    pub temperature: f32,

    /// Temperature for prompt refinement (0.0-1.0).
    #[serde(default = "default_refine_temperature")]
    pub refine_temperature: f32,

    /// Enable an extra self-review pass to refine final responses
    #[serde(default = "default_enable_self_review")]
    pub enable_self_review: bool,

    /// Maximum number of self-review passes
    #[serde(default = "default_max_review_passes")]
    pub max_review_passes: usize,

    /// Enable prompt refinement pass before sending to LLM
    #[serde(default = "default_refine_prompts_enabled")]
    pub refine_prompts_enabled: bool,

    /// Max refinement passes for prompt writing
    #[serde(default = "default_refine_max_passes")]
    pub refine_prompts_max_passes: usize,

    /// Optional model override for the refiner (empty = auto pick efficient sibling)
    #[serde(default)]
    pub refine_prompts_model: String,

    /// Small/lightweight model configuration for efficient operations
    /// Used for tasks like large file reads, parsing, git history, conversation summarization
    /// Typically 70-80% cheaper than main model; ~50% of VT Code's calls use this tier
    #[serde(default)]
    pub small_model: AgentSmallModelConfig,

    /// Inline prompt suggestion configuration for the chat composer
    #[serde(default)]
    pub prompt_suggestions: AgentPromptSuggestionsConfig,

    /// Session onboarding and welcome message configuration
    #[serde(default)]
    pub onboarding: AgentOnboardingConfig,

    /// Maximum bytes of AGENTS.md/CLAUDE.md content to load from project hierarchy
    #[serde(default = "default_project_doc_max_bytes")]
    pub project_doc_max_bytes: usize,

    /// Additional filenames to check when AGENTS.md is absent at a directory level.
    #[serde(default)]
    pub project_doc_fallback_filenames: Vec<String>,

    /// Maximum bytes of instruction content to load from AGENTS.md/CLAUDE.md hierarchy
    #[serde(default = "default_instruction_max_bytes", alias = "rule_doc_max_bytes")]
    pub instruction_max_bytes: usize,

    /// Additional instruction files or globs to merge into the hierarchy
    #[serde(default, alias = "instruction_paths", alias = "instructions")]
    pub instruction_files: Vec<String>,

    /// Instruction files or globs to exclude from AGENTS.md and rules discovery
    #[serde(default)]
    pub instruction_excludes: Vec<String>,

    /// Maximum recursive `@path` import depth for instruction and rule files
    #[serde(default = "default_instruction_import_max_depth")]
    pub instruction_import_max_depth: usize,

    /// Durable per-repository memory for main sessions
    #[serde(default)]
    pub persistent_memory: PersistentMemoryConfig,

    /// Provider/key identities captured from interactive configuration flows
    ///
    /// Note: Actual API keys are stored securely in the configured credential
    /// backend (OS keyring when available, otherwise encrypted file storage).
    /// Keys use `<provider>/<environment-variable>` identity keys and this
    /// field only tracks which identities have keys stored (for UI/migration purposes).
    /// The keys themselves are NOT serialized to the config file for security.
    #[serde(default, skip_serializing)]
    pub custom_api_keys: BTreeMap<String, String>,

    /// Preferred storage backend for credentials (OAuth tokens, API keys, etc.)
    ///
    /// - `keyring`: Use OS-specific secure storage (macOS Keychain, Windows Credential
    ///   Manager, Linux Secret Service), with encrypted-file fallback when unavailable.
    /// - `file`: Use AES-256-GCM encrypted file with machine-derived key
    /// - `auto`: Try keyring first, fall back to file if unavailable
    #[serde(default)]
    pub credential_storage_mode: crate::auth::AuthCredentialsStoreMode,

    /// Checkpointing configuration for automatic turn snapshots
    #[serde(default)]
    pub checkpointing: AgentCheckpointingConfig,

    /// Vibe coding configuration for lazy or vague request support
    #[serde(default)]
    pub vibe_coding: AgentVibeCodingConfig,

    /// Maximum number of retries for agent task execution (default: 2)
    /// When an agent task fails due to retryable errors (timeout, network, 503, etc.),
    /// it will be retried up to this many times with exponential backoff
    #[serde(default = "default_max_task_retries")]
    pub max_task_retries: u32,

    /// Harness configuration for turn-level budgets, telemetry, and execution limits
    #[serde(default)]
    pub harness: AgentHarnessConfig,

    /// Experimental Codex app-server sidecar configuration.
    #[serde(default)]
    pub codex_app_server: AgentCodexAppServerConfig,

    /// Include current date/time in system prompt for temporal awareness
    /// Helps LLM understand context for time-sensitive tasks (default: true)
    #[serde(default = "default_include_temporal_context")]
    pub include_temporal_context: bool,

    /// Use UTC instead of local time for temporal context in system prompts
    #[serde(default)]
    pub temporal_context_use_utc: bool,

    /// Include current working directory in system prompt (default: true)
    #[serde(default = "default_include_working_directory")]
    pub include_working_directory: bool,

    /// Controls inclusion of the structured reasoning tag instructions block.
    ///
    /// Behavior:
    /// - `Some(true)`: always include structured reasoning instructions.
    /// - `Some(false)`: never include structured reasoning instructions.
    /// - `None` (default): omit structured reasoning instructions in every prompt mode.
    ///
    /// Models with native reasoning do not need visible reasoning tags, so the
    /// block is opt-in for users who want tag-based reasoning guidance.
    #[serde(default)]
    pub include_structured_reasoning_tags: Option<bool>,

    /// Custom instructions provided by the user via configuration to guide agent behavior
    #[serde(default)]
    pub user_instructions: Option<String>,

    /// Require user confirmation before executing a plan generated in planning workflow
    /// When true, exiting planning workflow shows the implementation blueprint and
    /// requires explicit user approval before enabling edit tools.
    #[serde(default = "default_require_plan_confirmation")]
    pub require_plan_confirmation: bool,

    /// Circuit breaker configuration for resilient tool execution
    /// Controls when the agent should pause and ask for user guidance due to repeated failures
    #[serde(default)]
    pub circuit_breaker: CircuitBreakerConfig,

    /// Open Responses specification compliance configuration
    /// Enables vendor-neutral LLM API format for interoperable workflows
    #[serde(default)]
    pub open_responses: OpenResponsesConfig,
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "schema", schemars(rename_all = "snake_case"))]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ContinuationPolicy {
    Off,
    ExecOnly,
    #[default]
    All,
}

impl ContinuationPolicy {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::ExecOnly => "exec_only",
            Self::All => "all",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        let normalized = value.trim();
        if normalized.eq_ignore_ascii_case("off") {
            Some(Self::Off)
        } else if normalized.eq_ignore_ascii_case("exec_only") || normalized.eq_ignore_ascii_case("exec-only") {
            Some(Self::ExecOnly)
        } else if normalized.eq_ignore_ascii_case("all") {
            Some(Self::All)
        } else {
            None
        }
    }
}

impl<'de> Deserialize<'de> for ContinuationPolicy {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        Ok(Self::parse(&raw).unwrap_or_default())
    }
}

/// When to trigger a context reset — starting a clean session from external
/// artifacts only, discarding conversation history to clear noise and bad
/// assumptions. This is distinct from compaction, which preserves
/// conversational continuity within the same task/agent loop.
///
/// Following the context engineering pattern: "Context reset uses external
/// artifacts (files from note-taking, git logs, test results, task lists) as
/// startup material to open a clean new context/session. It does not preserve
/// the full conversation history, and can clear noise and bad assumptions so
/// that a new agent can reorient itself."
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "schema", schemars(rename_all = "snake_case"))]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextResetMode {
    /// Never reset — always carry forward conversation history (current behavior).
    #[default]
    Off,
    /// Reset when the progress monitor detects a stall (no forward progress for
    /// `context_reset_stall_threshold` consecutive turns).
    OnStall,
    /// Reset after every automatic compaction, so the post-compaction session
    /// starts from artifacts only rather than the compacted summary.
    OnCompaction,
}

impl ContextResetMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::OnStall => "on_stall",
            Self::OnCompaction => "on_compaction",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        let normalized = value.trim();
        if normalized.eq_ignore_ascii_case("off") {
            Some(Self::Off)
        } else if normalized.eq_ignore_ascii_case("on_stall") || normalized.eq_ignore_ascii_case("on-stall") {
            Some(Self::OnStall)
        } else if normalized.eq_ignore_ascii_case("on_compaction") || normalized.eq_ignore_ascii_case("on-compaction") {
            Some(Self::OnCompaction)
        } else {
            None
        }
    }
}

impl<'de> Deserialize<'de> for ContextResetMode {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        Ok(Self::parse(&raw).unwrap_or_default())
    }
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HarnessOrchestrationMode {
    #[default]
    PlanBuildEvaluate,
    Single,
}

impl HarnessOrchestrationMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Single => "single",
            Self::PlanBuildEvaluate => "plan_build_evaluate",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        let normalized = value.trim();
        if normalized.eq_ignore_ascii_case("single") {
            Some(Self::Single)
        } else if normalized.eq_ignore_ascii_case("plan_build_evaluate")
            || normalized.eq_ignore_ascii_case("plan-build-evaluate")
            || normalized.eq_ignore_ascii_case("planner_generator_evaluator")
            || normalized.eq_ignore_ascii_case("planner-generator-evaluator")
        {
            Some(Self::PlanBuildEvaluate)
        } else {
            None
        }
    }
}

impl<'de> Deserialize<'de> for HarnessOrchestrationMode {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        Ok(Self::parse(&raw).unwrap_or_default())
    }
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            provider: default_provider(),
            api_key_env: default_api_key_env(),
            default_model: default_model(),
            theme: default_theme(),
            system_prompt_mode: SystemPromptMode::default(),
            max_system_prompt_tokens: default_max_system_prompt_tokens(),
            system_prompt_budget_warning: default_system_prompt_budget_warning(),
            trim_system_prompt: default_trim_system_prompt(),
            tool_documentation_mode: ToolDocumentationMode::default(),
            shell_prompt_profile: ShellPromptProfile::default(),
            enable_split_tool_results: default_enable_split_tool_results(),
            todo_planning_mode: default_todo_planning_mode(),
            ui_surface: UiSurfacePreference::default(),
            max_conversation_turns: default_max_conversation_turns(),
            idle_turn_limit: default_idle_turn_limit(),
            reasoning_effort: default_reasoning_effort(),
            allow_reasoning_effort_downgrade: false,
            verbosity: default_verbosity(),
            temperature: default_temperature(),
            refine_temperature: default_refine_temperature(),
            enable_self_review: default_enable_self_review(),
            max_review_passes: default_max_review_passes(),
            refine_prompts_enabled: default_refine_prompts_enabled(),
            refine_prompts_max_passes: default_refine_max_passes(),
            refine_prompts_model: String::new(),
            small_model: AgentSmallModelConfig::default(),
            prompt_suggestions: AgentPromptSuggestionsConfig::default(),
            onboarding: AgentOnboardingConfig::default(),
            project_doc_max_bytes: default_project_doc_max_bytes(),
            project_doc_fallback_filenames: Vec::new(),
            instruction_max_bytes: default_instruction_max_bytes(),
            instruction_files: Vec::new(),
            instruction_excludes: Vec::new(),
            instruction_import_max_depth: default_instruction_import_max_depth(),
            persistent_memory: PersistentMemoryConfig::default(),
            custom_api_keys: BTreeMap::new(),
            credential_storage_mode: crate::auth::AuthCredentialsStoreMode::default(),
            checkpointing: AgentCheckpointingConfig::default(),
            vibe_coding: AgentVibeCodingConfig::default(),
            max_task_retries: default_max_task_retries(),
            harness: AgentHarnessConfig::default(),
            codex_app_server: AgentCodexAppServerConfig::default(),
            include_temporal_context: default_include_temporal_context(),
            temporal_context_use_utc: false, // Default to local time
            include_working_directory: default_include_working_directory(),
            include_structured_reasoning_tags: None,
            user_instructions: None,
            require_plan_confirmation: default_require_plan_confirmation(),
            circuit_breaker: CircuitBreakerConfig::default(),
            open_responses: OpenResponsesConfig::default(),
        }
    }
}

impl AgentConfig {
    /// Determine whether structured reasoning tag instructions should be included.
    pub fn should_include_structured_reasoning_tags(&self) -> bool {
        self.include_structured_reasoning_tags.unwrap_or(false)
    }

    /// Validate LLM generation parameters
    pub(crate) fn validate_llm_params(&self) -> Result<(), String> {
        // Validate temperature range
        if !(0.0..=1.0).contains(&self.temperature) {
            return Err(format!("temperature must be between 0.0 and 1.0, got {}", self.temperature));
        }

        if !(0.0..=1.0).contains(&self.refine_temperature) {
            return Err(format!("refine_temperature must be between 0.0 and 1.0, got {}", self.refine_temperature));
        }

        if self.instruction_import_max_depth == 0 {
            return Err("instruction_import_max_depth must be greater than 0".to_string());
        }

        if !(0.0..=1.0).contains(&self.harness.budget_warning_threshold) {
            return Err(format!(
                "harness.budget_warning_threshold must be between 0.0 and 1.0, got {}",
                self.harness.budget_warning_threshold
            ));
        }

        self.persistent_memory.validate()?;
        self.harness.tool_result_clearing.validate()?;

        Ok(())
    }
}

// Optimized: Use inline defaults with constants to reduce function call overhead
#[inline]
fn default_provider() -> String {
    defaults::DEFAULT_PROVIDER.into()
}

#[inline]
fn default_api_key_env() -> String {
    defaults::DEFAULT_API_KEY_ENV.into()
}

#[inline]
fn default_model() -> String {
    defaults::DEFAULT_MODEL.into()
}

#[inline]
fn default_theme() -> String {
    defaults::DEFAULT_THEME.into()
}

#[inline]
const fn default_todo_planning_mode() -> bool {
    true
}

#[inline]
const fn default_enable_split_tool_results() -> bool {
    true // Default: enabled for production use (84% token savings)
}

#[inline]
const fn default_max_conversation_turns() -> usize {
    tool_limits::DEFAULT_MAX_CONVERSATION_TURNS
}

#[inline]
const fn default_idle_turn_limit() -> usize {
    execution::IDLE_TURN_LIMIT
}

#[inline]
fn default_reasoning_effort() -> ReasoningEffortLevel {
    ReasoningEffortLevel::None
}

#[inline]
fn default_verbosity() -> VerbosityLevel {
    VerbosityLevel::default()
}

#[inline]
const fn default_temperature() -> f32 {
    llm_generation::DEFAULT_TEMPERATURE
}

#[inline]
const fn default_refine_temperature() -> f32 {
    llm_generation::DEFAULT_REFINE_TEMPERATURE
}

#[inline]
const fn default_enable_self_review() -> bool {
    false
}

#[inline]
const fn default_max_review_passes() -> usize {
    1
}

#[inline]
const fn default_refine_prompts_enabled() -> bool {
    false
}

#[inline]
const fn default_refine_max_passes() -> usize {
    1
}

#[inline]
const fn default_max_system_prompt_tokens() -> u64 {
    prompt_budget::DEFAULT_MAX_SYSTEM_PROMPT_TOKENS
}

#[inline]
const fn default_system_prompt_budget_warning() -> bool {
    true
}

#[inline]
const fn default_trim_system_prompt() -> bool {
    true
}

#[inline]
const fn default_project_doc_max_bytes() -> usize {
    prompt_budget::DEFAULT_MAX_BYTES
}

#[inline]
const fn default_instruction_max_bytes() -> usize {
    prompt_budget::DEFAULT_MAX_BYTES
}

#[inline]
const fn default_instruction_import_max_depth() -> usize {
    5
}

#[inline]
const fn default_max_task_retries() -> u32 {
    2 // Retry twice on transient failures
}

#[inline]
const fn default_include_temporal_context() -> bool {
    true // Enable by default - minimal overhead (~20 tokens)
}

#[inline]
const fn default_include_working_directory() -> bool {
    true // Enable by default - minimal overhead (~10 tokens)
}

#[inline]
const fn default_require_plan_confirmation() -> bool {
    true // Default: require confirmation (HITL pattern)
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AgentCheckpointingConfig {
    /// Enable automatic checkpoints after each successful turn
    #[serde(default = "default_checkpointing_enabled")]
    pub enabled: bool,

    /// Optional custom directory for storing checkpoints (relative to workspace or absolute)
    #[serde(default)]
    pub storage_dir: Option<String>,

    /// Maximum number of checkpoints to retain on disk
    #[serde(default = "default_checkpointing_max_snapshots")]
    pub max_snapshots: usize,

    /// Maximum age in days before checkpoints are removed automatically (None disables)
    #[serde(default = "default_checkpointing_max_age_days")]
    pub max_age_days: Option<u64>,
}

impl Default for AgentCheckpointingConfig {
    fn default() -> Self {
        Self {
            enabled: default_checkpointing_enabled(),
            storage_dir: None,
            max_snapshots: default_checkpointing_max_snapshots(),
            max_age_days: default_checkpointing_max_age_days(),
        }
    }
}

#[inline]
const fn default_checkpointing_enabled() -> bool {
    DEFAULT_CHECKPOINTS_ENABLED
}

#[inline]
const fn default_checkpointing_max_snapshots() -> usize {
    DEFAULT_MAX_SNAPSHOTS
}

#[inline]
const fn default_checkpointing_max_age_days() -> Option<u64> {
    Some(DEFAULT_MAX_AGE_DAYS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_continuation_policy_defaults_and_parses() {
        assert_eq!(ContinuationPolicy::default(), ContinuationPolicy::All);
        assert_eq!(ContinuationPolicy::parse("off"), Some(ContinuationPolicy::Off));
        assert_eq!(ContinuationPolicy::parse("exec-only"), Some(ContinuationPolicy::ExecOnly));
        assert_eq!(ContinuationPolicy::parse("all"), Some(ContinuationPolicy::All));
        assert_eq!(ContinuationPolicy::parse("invalid"), None);
    }

    #[test]
    fn test_tracker_continuation_defaults_and_deserializes() {
        assert!(TrackerContinuationConfig::default().auto_continue_tracker);
        assert_eq!(TrackerContinuationConfig::default().cross_turn_turns, 32);
        let parsed: AgentHarnessConfig =
            toml::from_str("[continuation]\nauto_continue_tracker = false\ncross_turn_turns = 3")
                .expect("valid harness config");
        assert!(!parsed.continuation.auto_continue_tracker);
        assert_eq!(parsed.continuation.cross_turn_turns, 3);
        let fallback: AgentHarnessConfig = toml::from_str("").expect("empty harness config");
        assert!(fallback.continuation.auto_continue_tracker);
        assert_eq!(fallback.continuation.cross_turn_turns, 32);
    }

    #[test]
    fn test_harness_config_continuation_policy_deserializes_with_fallback() {
        let parsed: AgentHarnessConfig = toml::from_str("continuation_policy = \"all\"").expect("valid harness config");
        assert_eq!(parsed.continuation_policy, ContinuationPolicy::All);

        let fallback: AgentHarnessConfig =
            toml::from_str("continuation_policy = \"unexpected\"").expect("fallback config");
        assert_eq!(fallback.continuation_policy, ContinuationPolicy::All);
    }

    #[test]
    fn test_harness_config_tool_call_budget_defaults_to_120() {
        assert_eq!(AgentHarnessConfig::default().max_tool_calls_per_turn, tool_limits::DEFAULT_MAX_TOOL_CALLS_PER_TURN);

        let parsed: AgentHarnessConfig = toml::from_str("").expect("default harness config");
        assert_eq!(parsed.max_tool_calls_per_turn, tool_limits::DEFAULT_MAX_TOOL_CALLS_PER_TURN);
    }

    #[test]
    fn test_harness_orchestration_mode_defaults_and_parses() {
        assert_eq!(HarnessOrchestrationMode::default(), HarnessOrchestrationMode::PlanBuildEvaluate);
        assert_eq!(HarnessOrchestrationMode::parse("single"), Some(HarnessOrchestrationMode::Single));
        assert_eq!(
            HarnessOrchestrationMode::parse("plan_build_evaluate"),
            Some(HarnessOrchestrationMode::PlanBuildEvaluate)
        );
        assert_eq!(
            HarnessOrchestrationMode::parse("planner-generator-evaluator"),
            Some(HarnessOrchestrationMode::PlanBuildEvaluate)
        );
        assert_eq!(HarnessOrchestrationMode::parse("unexpected"), None);
    }

    #[test]
    fn test_harness_config_orchestration_deserializes_with_fallback() {
        let parsed: AgentHarnessConfig =
            toml::from_str("orchestration_mode = \"plan_build_evaluate\"").expect("valid harness config");
        assert_eq!(parsed.orchestration_mode, HarnessOrchestrationMode::PlanBuildEvaluate);
        assert_eq!(parsed.max_revision_rounds, 2);

        let fallback: AgentHarnessConfig =
            toml::from_str("orchestration_mode = \"unexpected\"").expect("fallback config");
        assert_eq!(fallback.orchestration_mode, HarnessOrchestrationMode::PlanBuildEvaluate);
    }

    /// Drift guard for hand-written enum string mappings. Each of these enums
    /// exposes the same value through serde, `as_str()`, and `parse()`; see
    /// [`crate::test_support::assert_string_enum_lockstep`].
    #[test]
    fn string_config_enums_keep_as_str_and_serde_in_lockstep() {
        use crate::test_support::assert_string_enum_lockstep;

        assert_string_enum_lockstep!(
            ContinuationPolicy,
            [
                ContinuationPolicy::Off,
                ContinuationPolicy::ExecOnly,
                ContinuationPolicy::All
            ]
        );
        assert_string_enum_lockstep!(
            ContextResetMode,
            [
                ContextResetMode::Off,
                ContextResetMode::OnStall,
                ContextResetMode::OnCompaction
            ]
        );
        assert_string_enum_lockstep!(
            HarnessOrchestrationMode,
            [
                HarnessOrchestrationMode::Single,
                HarnessOrchestrationMode::PlanBuildEvaluate
            ]
        );
    }

    #[test]
    fn test_verification_auto_recovery_defaults_to_bounded_auto_execute() {
        let config = VerificationAutoRecoveryConfig::default();
        assert!(config.auto_execute);
        assert_eq!(config.in_turn_attempts, 2);
        assert_eq!(config.cross_turn_turns, 2);
        assert_eq!(config.default_verifier_override, None);
        assert_eq!(config.max_consecutive_failures, 3);
    }

    #[test]
    fn test_verification_auto_recovery_survives_missing_field_for_backward_compatibility() {
        // Older vtcode.toml files predate [agent.harness.verification]: the
        // whole table and each field must fall back to defaults.
        let without_table: AgentHarnessConfig = toml::from_str("").expect("default harness config");
        assert!(without_table.verification.auto_execute);
        assert_eq!(without_table.verification.in_turn_attempts, 2);

        let partial: VerificationAutoRecoveryConfig =
            toml::from_str("auto_execute = false").expect("minimal verification config parses");
        assert!(!partial.auto_execute);
        assert_eq!(partial.in_turn_attempts, 2);
        assert_eq!(partial.max_consecutive_failures, 3);

        let full: VerificationAutoRecoveryConfig = toml::from_str(
            "auto_execute = true\nin_turn_attempts = 1\ncross_turn_turns = 0\ndefault_verifier_override = \"cargo nextest run -p mycrate\"\nmax_consecutive_failures = 5",
        )
        .expect("full verification config parses");
        assert_eq!(full.in_turn_attempts, 1);
        assert_eq!(full.cross_turn_turns, 0);
        assert_eq!(full.default_verifier_override.as_deref(), Some("cargo nextest run -p mycrate"));
        assert_eq!(full.max_consecutive_failures, 5);
    }

    #[test]
    fn test_plan_confirmation_config_default() {
        let config = AgentConfig::default();
        assert!(config.require_plan_confirmation);
    }

    #[test]
    fn test_system_prompt_budget_defaults() {
        let config = AgentConfig::default();
        assert_eq!(config.max_system_prompt_tokens, prompt_budget::DEFAULT_MAX_SYSTEM_PROMPT_TOKENS);
        assert!(config.system_prompt_budget_warning);
        assert!(config.trim_system_prompt);

        let parsed: AgentConfig = toml::from_str(
            r#"
max_system_prompt_tokens = 4000
system_prompt_budget_warning = false
trim_system_prompt = true
"#,
        )
        .expect("agent config should parse");
        assert_eq!(parsed.max_system_prompt_tokens, 4000);
        assert!(!parsed.system_prompt_budget_warning);
        assert!(parsed.trim_system_prompt);
    }

    #[test]
    fn test_budget_warning_threshold_default_and_validation() {
        let config = AgentConfig::default();
        assert!((config.harness.budget_warning_threshold - 0.75).abs() < f64::EPSILON);
        assert!(config.validate_llm_params().is_ok());

        let parsed: AgentHarnessConfig = toml::from_str(
            r#"
max_budget_usd = 5.0
budget_warning_threshold = 0.5
"#,
        )
        .expect("harness config should parse");
        assert_eq!(parsed.max_budget_usd, Some(5.0));
        assert!((parsed.budget_warning_threshold - 0.5).abs() < f64::EPSILON);

        let mut invalid = AgentConfig::default();
        invalid.harness.budget_warning_threshold = 1.5;
        assert!(invalid.validate_llm_params().is_err());
    }

    #[test]
    fn test_persistent_memory_is_disabled_by_default() {
        let config = AgentConfig::default();
        assert!(!config.persistent_memory.enabled);
        assert!(config.persistent_memory.auto_write);
    }

    #[test]
    fn test_tool_result_clearing_defaults() {
        let config = AgentConfig::default();
        let clearing = config.harness.tool_result_clearing;

        assert!(clearing.enabled);
        assert_eq!(clearing.trigger_tokens, 40_000);
        assert_eq!(clearing.keep_tool_uses, 2);
        assert_eq!(clearing.keep_recent_tool_batches, 2);
        assert_eq!(clearing.clear_at_least_tokens, 30_000);
        assert!(clearing.clear_tool_inputs);
    }

    #[test]
    fn test_tool_result_clearing_missing_key_defaults_clear_tool_inputs_true() {
        let parsed: AgentHarnessConfig = toml::from_str(
            r#"
                [tool_result_clearing]
                enabled = true
                trigger_tokens = 40000
                keep_tool_uses = 2
                clear_at_least_tokens = 30000
            "#,
        )
        .expect("valid harness config");

        assert!(parsed.tool_result_clearing.clear_tool_inputs);
        assert_eq!(parsed.tool_result_clearing.keep_recent_tool_batches, 2);
    }

    #[test]
    fn tool_result_clearing_recent_batches_round_trip_including_compatibility() {
        for batches in [0, 1, 2, 9] {
            let parsed: AgentHarnessConfig =
                toml::from_str(&format!("[tool_result_clearing]\nkeep_recent_tool_batches = {batches}")).unwrap();
            assert!(parsed.tool_result_clearing.validate().is_ok());
            let restored: AgentHarnessConfig = toml::from_str(&toml::to_string(&parsed).unwrap()).unwrap();
            assert_eq!(restored.tool_result_clearing.keep_recent_tool_batches, batches);
        }
    }

    #[test]
    fn test_tool_result_clearing_explicit_false_opts_out_of_input_clearing() {
        let parsed: AgentHarnessConfig = toml::from_str(
            r#"
                [tool_result_clearing]
                clear_tool_inputs = false
            "#,
        )
        .expect("valid harness config");

        assert!(!parsed.tool_result_clearing.clear_tool_inputs);
    }

    #[test]
    fn test_codex_app_server_experimental_features_default_to_disabled() {
        let config = AgentConfig::default();

        assert!(!config.codex_app_server.experimental_features);
    }

    #[test]
    fn test_codex_app_server_experimental_features_parse_from_toml() {
        let parsed: AgentCodexAppServerConfig = toml::from_str(
            r#"
                command = "codex"
                args = ["app-server"]
                startup_timeout_secs = 15
                experimental_features = true
            "#,
        )
        .expect("valid codex app-server config");

        assert!(parsed.experimental_features);
        assert_eq!(parsed.startup_timeout_secs, 15);
    }

    #[test]
    fn test_tool_result_clearing_parses_and_validates() {
        let parsed: AgentHarnessConfig = toml::from_str(
            r#"
                [tool_result_clearing]
                enabled = true
                trigger_tokens = 123456
                keep_tool_uses = 6
                clear_at_least_tokens = 4096
                clear_tool_inputs = true
            "#,
        )
        .expect("valid harness config");

        assert!(parsed.tool_result_clearing.enabled);
        assert_eq!(parsed.tool_result_clearing.trigger_tokens, 123_456);
        assert_eq!(parsed.tool_result_clearing.keep_tool_uses, 6);
        assert_eq!(parsed.tool_result_clearing.clear_at_least_tokens, 4_096);
        assert!(parsed.tool_result_clearing.clear_tool_inputs);
        assert!(parsed.tool_result_clearing.validate().is_ok());
    }

    #[test]
    fn test_tool_result_clearing_rejects_zero_values() {
        let clearing = ToolResultClearingConfig {
            trigger_tokens: 0,
            ..ToolResultClearingConfig::default()
        };
        assert!(clearing.validate().is_err());

        let clearing = ToolResultClearingConfig {
            keep_tool_uses: 0,
            ..ToolResultClearingConfig::default()
        };
        assert!(clearing.validate().is_err());

        let clearing = ToolResultClearingConfig {
            clear_at_least_tokens: 0,
            ..ToolResultClearingConfig::default()
        };
        assert!(clearing.validate().is_err());
    }

    #[test]
    fn test_structured_reasoning_is_opt_in_for_every_prompt_mode() {
        let default_prompt_config = AgentConfig {
            system_prompt_mode: SystemPromptMode::Default,
            ..Default::default()
        };
        assert!(!default_prompt_config.should_include_structured_reasoning_tags());

        let specialized_mode = AgentConfig {
            system_prompt_mode: SystemPromptMode::Specialized,
            ..Default::default()
        };
        assert!(!specialized_mode.should_include_structured_reasoning_tags());

        let minimal_mode = AgentConfig {
            system_prompt_mode: SystemPromptMode::Minimal,
            ..Default::default()
        };
        assert!(!minimal_mode.should_include_structured_reasoning_tags());

        let lightweight_mode = AgentConfig {
            system_prompt_mode: SystemPromptMode::Lightweight,
            ..Default::default()
        };
        assert!(!lightweight_mode.should_include_structured_reasoning_tags());
    }

    #[test]
    fn test_structured_reasoning_explicit_override() {
        let mut config = AgentConfig {
            system_prompt_mode: SystemPromptMode::Minimal,
            include_structured_reasoning_tags: Some(true),
            ..AgentConfig::default()
        };
        assert!(config.should_include_structured_reasoning_tags());

        config.include_structured_reasoning_tags = Some(false);
        assert!(!config.should_include_structured_reasoning_tags());
    }
}
