use crate::constants::{defaults, instructions, llm_generation, project_doc, prompts};
use crate::types::{ReasoningEffortLevel, UiSurfacePreference, VerbosityLevel};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

const DEFAULT_CHECKPOINTS_ENABLED: bool = true;
const DEFAULT_MAX_SNAPSHOTS: usize = 50;
const DEFAULT_MAX_AGE_DAYS: u64 = 30;

/// Agent-wide configuration
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AgentConfig {
    /// AI provider for single agent mode (gemini, openai, anthropic, openrouter, xai, zai)
    #[serde(default = "default_provider")]
    pub provider: String,

    /// Environment variable that stores the API key for the active provider
    #[serde(default = "default_api_key_env")]
    pub api_key_env: String,

    /// Default model to use
    #[serde(default = "default_model")]
    pub default_model: String,

    /// UI theme identifier controlling ANSI styling
    #[serde(default = "default_theme")]
    pub theme: String,

    /// Enable TODO planning workflow integrations (update_plan tool, onboarding hints)
    #[serde(default = "default_todo_planning_mode")]
    pub todo_planning_mode: bool,

    /// Preferred rendering surface for the interactive chat UI (auto, alternate, inline)
    #[serde(default)]
    pub ui_surface: UiSurfacePreference,

    /// Maximum number of conversation turns before auto-termination
    #[serde(default = "default_max_conversation_turns")]
    pub max_conversation_turns: usize,

    /// Reasoning effort level for models that support it (none, low, medium, high)
    /// Applies to: Claude, GPT-5, GPT-5.1, Gemini, Qwen3, DeepSeek with reasoning capability
    #[serde(default = "default_reasoning_effort")]
    pub reasoning_effort: ReasoningEffortLevel,

    /// Verbosity level for output text (low, medium, high)
    /// Applies to: GPT-5.1 and other models that support verbosity control
    #[serde(default = "default_verbosity")]
    pub verbosity: VerbosityLevel,

    /// Temperature for main LLM responses (0.0-1.0)
    /// Lower values = more deterministic, higher values = more creative
    /// Recommended: 0.7 for balanced creativity and consistency
    /// Range: 0.0 (deterministic) to 1.0 (maximum randomness)
    #[serde(default = "default_temperature")]
    pub temperature: f32,

    /// Maximum tokens for main LLM generation responses (default: 2000)
    /// Adjust based on model context window size:
    /// - 2000 for standard tasks
    /// - 16384 for models with 128k context
    /// - 32768 for models with 256k context
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,

    /// Temperature for prompt refinement (0.0-1.0, default: 0.3)
    /// Lower values ensure prompt refinement is more deterministic/consistent
    /// Keep lower than main temperature for stable prompt improvement
    #[serde(default = "default_refine_temperature")]
    pub refine_temperature: f32,

    /// Maximum tokens for prompt refinement (default: 800)
    /// Prompts are typically shorter, so 800 tokens is usually sufficient
    #[serde(default = "default_refine_max_tokens")]
    pub refine_max_tokens: u32,

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
    /// Typically 70-80% cheaper than main model; ~50% of Claude Code's calls use this tier
    #[serde(default)]
    pub small_model: AgentSmallModelConfig,

    /// Session onboarding and welcome message configuration
    #[serde(default)]
    pub onboarding: AgentOnboardingConfig,

    /// Maximum bytes of AGENTS.md content to load from project hierarchy
    #[serde(default = "default_project_doc_max_bytes")]
    pub project_doc_max_bytes: usize,

    /// Maximum bytes of instruction content to load from AGENTS.md hierarchy
    #[serde(
        default = "default_instruction_max_bytes",
        alias = "rule_doc_max_bytes"
    )]
    pub instruction_max_bytes: usize,

    /// Additional instruction files or globs to merge into the hierarchy
    #[serde(default, alias = "instruction_paths", alias = "instructions")]
    pub instruction_files: Vec<String>,

    /// Custom prompt configuration for slash command shortcuts
    #[serde(default)]
    pub custom_prompts: AgentCustomPromptsConfig,

    /// Provider-specific API keys captured from interactive configuration flows
    #[serde(default)]
    pub custom_api_keys: BTreeMap<String, String>,

    /// Checkpointing configuration for automatic turn snapshots
    #[serde(default)]
    pub checkpointing: AgentCheckpointingConfig,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            provider: default_provider(),
            api_key_env: default_api_key_env(),
            default_model: default_model(),
            theme: default_theme(),
            todo_planning_mode: default_todo_planning_mode(),
            ui_surface: UiSurfacePreference::default(),
            max_conversation_turns: default_max_conversation_turns(),
            reasoning_effort: default_reasoning_effort(),
            verbosity: default_verbosity(),
            temperature: default_temperature(),
            max_tokens: default_max_tokens(),
            refine_temperature: default_refine_temperature(),
            refine_max_tokens: default_refine_max_tokens(),
            enable_self_review: default_enable_self_review(),
            max_review_passes: default_max_review_passes(),
            refine_prompts_enabled: default_refine_prompts_enabled(),
            refine_prompts_max_passes: default_refine_max_passes(),
            refine_prompts_model: String::new(),
            small_model: AgentSmallModelConfig::default(),
            onboarding: AgentOnboardingConfig::default(),
            project_doc_max_bytes: default_project_doc_max_bytes(),
            instruction_max_bytes: default_instruction_max_bytes(),
            instruction_files: Vec::new(),
            custom_prompts: AgentCustomPromptsConfig::default(),
            custom_api_keys: BTreeMap::new(),
            checkpointing: AgentCheckpointingConfig::default(),
        }
    }
}

impl AgentConfig {
    /// Validate LLM generation parameters
    pub fn validate_llm_params(&self) -> Result<(), String> {
        if !(0.0..=1.0).contains(&self.temperature) {
            return Err(format!(
                "temperature must be between 0.0 and 1.0, got {}",
                self.temperature
            ));
        }

        if !(0.0..=1.0).contains(&self.refine_temperature) {
            return Err(format!(
                "refine_temperature must be between 0.0 and 1.0, got {}",
                self.refine_temperature
            ));
        }

        if self.max_tokens == 0 {
            return Err("max_tokens must be greater than 0".to_string());
        }

        if self.refine_max_tokens == 0 {
            return Err("refine_max_tokens must be greater than 0".to_string());
        }

        Ok(())
    }
}

fn default_provider() -> String {
    defaults::DEFAULT_PROVIDER.to_string()
}

fn default_api_key_env() -> String {
    defaults::DEFAULT_API_KEY_ENV.to_string()
}
fn default_model() -> String {
    defaults::DEFAULT_MODEL.to_string()
}
fn default_theme() -> String {
    defaults::DEFAULT_THEME.to_string()
}

fn default_todo_planning_mode() -> bool {
    true
}
fn default_max_conversation_turns() -> usize {
    150
}
fn default_reasoning_effort() -> ReasoningEffortLevel {
    ReasoningEffortLevel::default()
}

fn default_verbosity() -> VerbosityLevel {
    VerbosityLevel::default()
}

fn default_temperature() -> f32 {
    llm_generation::DEFAULT_TEMPERATURE
}

fn default_max_tokens() -> u32 {
    llm_generation::DEFAULT_MAX_TOKENS
}

fn default_refine_temperature() -> f32 {
    llm_generation::DEFAULT_REFINE_TEMPERATURE
}

fn default_refine_max_tokens() -> u32 {
    llm_generation::DEFAULT_REFINE_MAX_TOKENS
}

fn default_enable_self_review() -> bool {
    false
}

fn default_max_review_passes() -> usize {
    1
}

fn default_refine_prompts_enabled() -> bool {
    false
}

fn default_refine_max_passes() -> usize {
    1
}

fn default_project_doc_max_bytes() -> usize {
    project_doc::DEFAULT_MAX_BYTES
}

fn default_instruction_max_bytes() -> usize {
    instructions::DEFAULT_MAX_BYTES
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AgentCustomPromptsConfig {
    /// Master switch for custom prompt support
    #[serde(default = "default_custom_prompts_enabled")]
    pub enabled: bool,

    /// Primary directory for prompt markdown files
    #[serde(default = "default_custom_prompts_directory")]
    pub directory: String,

    /// Additional directories to search for prompts
    #[serde(default)]
    pub extra_directories: Vec<String>,

    /// Maximum file size (KB) to load for a single prompt
    #[serde(default = "default_custom_prompts_max_file_size_kb")]
    pub max_file_size_kb: usize,
}

impl Default for AgentCustomPromptsConfig {
    fn default() -> Self {
        Self {
            enabled: default_custom_prompts_enabled(),
            directory: default_custom_prompts_directory(),
            extra_directories: Vec::new(),
            max_file_size_kb: default_custom_prompts_max_file_size_kb(),
        }
    }
}

fn default_custom_prompts_enabled() -> bool {
    true
}

fn default_custom_prompts_directory() -> String {
    prompts::DEFAULT_CUSTOM_PROMPTS_DIR.to_string()
}

fn default_custom_prompts_max_file_size_kb() -> usize {
    prompts::DEFAULT_CUSTOM_PROMPT_MAX_FILE_SIZE_KB
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

fn default_checkpointing_enabled() -> bool {
    DEFAULT_CHECKPOINTS_ENABLED
}

fn default_checkpointing_max_snapshots() -> usize {
    DEFAULT_MAX_SNAPSHOTS
}

fn default_checkpointing_max_age_days() -> Option<u64> {
    Some(DEFAULT_MAX_AGE_DAYS)
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AgentOnboardingConfig {
    /// Toggle onboarding message rendering
    #[serde(default = "default_onboarding_enabled")]
    pub enabled: bool,

    /// Introductory text shown at session start
    #[serde(default = "default_intro_text")]
    pub intro_text: String,

    /// Whether to include project overview in onboarding message
    #[serde(default = "default_show_project_overview")]
    pub include_project_overview: bool,

    /// Whether to include language summary in onboarding message
    #[serde(default = "default_show_language_summary")]
    pub include_language_summary: bool,

    /// Whether to include AGENTS.md highlights in onboarding message
    #[serde(default = "default_show_guideline_highlights")]
    pub include_guideline_highlights: bool,

    /// Whether to surface usage tips inside the welcome text banner
    #[serde(default = "default_show_usage_tips_in_welcome")]
    pub include_usage_tips_in_welcome: bool,

    /// Whether to surface suggested actions inside the welcome text banner
    #[serde(default = "default_show_recommended_actions_in_welcome")]
    pub include_recommended_actions_in_welcome: bool,

    /// Maximum number of guideline bullets to surface
    #[serde(default = "default_guideline_highlight_limit")]
    pub guideline_highlight_limit: usize,

    /// Tips for collaborating with the agent effectively
    #[serde(default = "default_usage_tips")]
    pub usage_tips: Vec<String>,

    /// Recommended follow-up actions to display
    #[serde(default = "default_recommended_actions")]
    pub recommended_actions: Vec<String>,

    /// Placeholder suggestion for the chat input bar
    #[serde(default)]
    pub chat_placeholder: Option<String>,
}

impl Default for AgentOnboardingConfig {
    fn default() -> Self {
        Self {
            enabled: default_onboarding_enabled(),
            intro_text: default_intro_text(),
            include_project_overview: default_show_project_overview(),
            include_language_summary: default_show_language_summary(),
            include_guideline_highlights: default_show_guideline_highlights(),
            include_usage_tips_in_welcome: default_show_usage_tips_in_welcome(),
            include_recommended_actions_in_welcome: default_show_recommended_actions_in_welcome(),
            guideline_highlight_limit: default_guideline_highlight_limit(),
            usage_tips: default_usage_tips(),
            recommended_actions: default_recommended_actions(),
            chat_placeholder: None,
        }
    }
}

fn default_onboarding_enabled() -> bool {
    true
}

fn default_intro_text() -> String {
    "Let's get oriented. I preloaded workspace context so we can move fast.".to_string()
}

fn default_show_project_overview() -> bool {
    true
}

fn default_show_language_summary() -> bool {
    false
}

fn default_show_guideline_highlights() -> bool {
    true
}

fn default_show_usage_tips_in_welcome() -> bool {
    false
}

fn default_show_recommended_actions_in_welcome() -> bool {
    false
}

fn default_guideline_highlight_limit() -> usize {
    3
}

fn default_usage_tips() -> Vec<String> {
    vec![
        "Describe your current coding goal or ask for a quick status overview.".to_string(),
        "Reference AGENTS.md guidelines when proposing changes.".to_string(),
        "Draft or refresh your TODO list with update_plan before coding.".to_string(),
        "Prefer asking for targeted file reads or diffs before editing.".to_string(),
    ]
}

fn default_recommended_actions() -> Vec<String> {
    vec![
        "Start the session by outlining a 3–6 step TODO plan via update_plan.".to_string(),
        "Review the highlighted guidelines and share the task you want to tackle.".to_string(),
        "Ask for a workspace tour if you need more context.".to_string(),
    ]
}

/// Small/lightweight model configuration for efficient operations
///
/// Following Claude Code's pattern, use a smaller model (e.g., Haiku, GPT-4 Mini) for 50%+ of calls:
/// - Large file reads and parsing (>50KB)
/// - Web page summarization and analysis
/// - Git history and commit message processing
/// - One-word processing labels and simple classifications
///
/// Typically 70-80% cheaper than the main model while maintaining quality for these tasks.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AgentSmallModelConfig {
    /// Enable small model tier for efficient operations
    #[serde(default = "default_small_model_enabled")]
    pub enabled: bool,

    /// Small model to use (e.g., "claude-3-5-haiku", "gpt-4o-mini", "gemini-2.0-flash")
    /// Leave empty to auto-select a lightweight sibling of the main model
    #[serde(default)]
    pub model: String,

    /// Maximum tokens for small model responses
    #[serde(default = "default_small_model_max_tokens")]
    pub max_tokens: u32,

    /// Temperature for small model responses
    #[serde(default = "default_small_model_temperature")]
    pub temperature: f32,

    /// Enable small model for large file reads (>50KB)
    #[serde(default = "default_small_model_for_large_reads")]
    pub use_for_large_reads: bool,

    /// Enable small model for web content summarization
    #[serde(default = "default_small_model_for_web_summary")]
    pub use_for_web_summary: bool,

    /// Enable small model for git history processing
    #[serde(default = "default_small_model_for_git_history")]
    pub use_for_git_history: bool,
}

impl Default for AgentSmallModelConfig {
    fn default() -> Self {
        Self {
            enabled: default_small_model_enabled(),
            model: String::new(),
            max_tokens: default_small_model_max_tokens(),
            temperature: default_small_model_temperature(),
            use_for_large_reads: default_small_model_for_large_reads(),
            use_for_web_summary: default_small_model_for_web_summary(),
            use_for_git_history: default_small_model_for_git_history(),
        }
    }
}

fn default_small_model_enabled() -> bool {
    true // Enable by default following Claude Code pattern
}

fn default_small_model_max_tokens() -> u32 {
    1000 // Smaller responses for summary/parse operations
}

fn default_small_model_temperature() -> f32 {
    0.3 // More deterministic for parsing/summarization
}

fn default_small_model_for_large_reads() -> bool {
    true
}

fn default_small_model_for_web_summary() -> bool {
    true
}

fn default_small_model_for_git_history() -> bool {
    true
}
