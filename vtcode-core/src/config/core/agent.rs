use crate::config::constants::{defaults, instructions, project_doc};
use crate::config::types::{ReasoningEffortLevel, UiSurfacePreference};
use crate::core::agent::snapshots::{
    DEFAULT_CHECKPOINTS_ENABLED, DEFAULT_MAX_AGE_DAYS, DEFAULT_MAX_SNAPSHOTS,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Agent-wide configuration
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

    /// Reasoning effort level for models that support it (low, medium, high)
    /// Applies to: Claude, GPT-5, Gemini, Qwen3, DeepSeek with reasoning capability
    #[serde(default = "default_reasoning_effort")]
    pub reasoning_effort: ReasoningEffortLevel,

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
            enable_self_review: default_enable_self_review(),
            max_review_passes: default_max_review_passes(),
            refine_prompts_enabled: default_refine_prompts_enabled(),
            refine_prompts_max_passes: default_refine_max_passes(),
            refine_prompts_model: String::new(),
            onboarding: AgentOnboardingConfig::default(),
            project_doc_max_bytes: default_project_doc_max_bytes(),
            instruction_max_bytes: default_instruction_max_bytes(),
            instruction_files: Vec::new(),
            custom_api_keys: BTreeMap::new(),
            checkpointing: AgentCheckpointingConfig::default(),
        }
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
