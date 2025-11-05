#[cfg(feature = "bootstrap")]
pub mod bootstrap;

use crate::acp::AgentClientProtocolConfig;
use crate::context::ContextFeaturesConfig;
use crate::core::{
    AgentConfig, AutomationConfig, CommandsConfig, PromptCachingConfig, SecurityConfig, ToolsConfig,
};
use crate::defaults::{self, ConfigDefaultsProvider, SyntaxHighlightingDefaults};
use crate::hooks::HooksConfig;
use crate::mcp::McpClientConfig;
use crate::root::{PtyConfig, UiConfig};
use crate::router::RouterConfig;
use crate::telemetry::TelemetryConfig;
use crate::timeouts::TimeoutsConfig;
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// Syntax highlighting configuration
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SyntaxHighlightingConfig {
    /// Enable syntax highlighting for tool output
    #[serde(default = "defaults::syntax_highlighting::enabled")]
    pub enabled: bool,

    /// Theme to use for syntax highlighting
    #[serde(default = "defaults::syntax_highlighting::theme")]
    pub theme: String,

    /// Enable theme caching for better performance
    #[serde(default = "defaults::syntax_highlighting::cache_themes")]
    pub cache_themes: bool,

    /// Maximum file size for syntax highlighting (in MB)
    #[serde(default = "defaults::syntax_highlighting::max_file_size_mb")]
    pub max_file_size_mb: usize,

    /// Languages to enable syntax highlighting for
    #[serde(default = "defaults::syntax_highlighting::enabled_languages")]
    pub enabled_languages: Vec<String>,

    /// Performance settings - highlight timeout in milliseconds
    #[serde(default = "defaults::syntax_highlighting::highlight_timeout_ms")]
    pub highlight_timeout_ms: u64,
}

impl Default for SyntaxHighlightingConfig {
    fn default() -> Self {
        Self {
            enabled: defaults::syntax_highlighting::enabled(),
            theme: defaults::syntax_highlighting::theme(),
            cache_themes: defaults::syntax_highlighting::cache_themes(),
            max_file_size_mb: defaults::syntax_highlighting::max_file_size_mb(),
            enabled_languages: defaults::syntax_highlighting::enabled_languages(),
            highlight_timeout_ms: defaults::syntax_highlighting::highlight_timeout_ms(),
        }
    }
}

impl SyntaxHighlightingConfig {
    pub fn validate(&self) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }

        ensure!(
            self.max_file_size_mb >= SyntaxHighlightingDefaults::min_file_size_mb(),
            "Syntax highlighting max_file_size_mb must be at least {} MB",
            SyntaxHighlightingDefaults::min_file_size_mb()
        );

        ensure!(
            self.highlight_timeout_ms >= SyntaxHighlightingDefaults::min_highlight_timeout_ms(),
            "Syntax highlighting highlight_timeout_ms must be at least {} ms",
            SyntaxHighlightingDefaults::min_highlight_timeout_ms()
        );

        ensure!(
            !self.theme.trim().is_empty(),
            "Syntax highlighting theme must not be empty"
        );

        ensure!(
            self.enabled_languages
                .iter()
                .all(|lang| !lang.trim().is_empty()),
            "Syntax highlighting languages must not contain empty entries"
        );

        Ok(())
    }
}

/// Main configuration structure for VTCode
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct VTCodeConfig {
    /// Agent-wide settings
    #[serde(default)]
    pub agent: AgentConfig,

    /// Tool execution policies
    #[serde(default)]
    pub tools: ToolsConfig,

    /// Unix command permissions
    #[serde(default)]
    pub commands: CommandsConfig,

    /// Security settings
    #[serde(default)]
    pub security: SecurityConfig,

    /// UI settings
    #[serde(default)]
    pub ui: UiConfig,

    /// PTY settings
    #[serde(default)]
    pub pty: PtyConfig,

    /// Context features (e.g., Decision Ledger)
    #[serde(default)]
    pub context: ContextFeaturesConfig,

    /// Router configuration (dynamic model + engine selection)
    #[serde(default)]
    pub router: RouterConfig,

    /// Telemetry configuration (logging, trajectory)
    #[serde(default)]
    pub telemetry: TelemetryConfig,

    /// Syntax highlighting configuration
    #[serde(default)]
    pub syntax_highlighting: SyntaxHighlightingConfig,

    /// Timeout ceilings and UI warning thresholds
    #[serde(default)]
    pub timeouts: TimeoutsConfig,

    /// Automation configuration
    #[serde(default)]
    pub automation: AutomationConfig,

    /// Prompt cache configuration (local + provider integration)
    #[serde(default)]
    pub prompt_cache: PromptCachingConfig,

    /// Model Context Protocol configuration
    #[serde(default)]
    pub mcp: McpClientConfig,

    /// Agent Client Protocol configuration
    #[serde(default)]
    pub acp: AgentClientProtocolConfig,

    /// Lifecycle hooks configuration
    #[serde(default)]
    pub hooks: HooksConfig,
}

impl VTCodeConfig {
    pub fn validate(&self) -> Result<()> {
        self.syntax_highlighting
            .validate()
            .context("Invalid syntax_highlighting configuration")?;

        self.context
            .validate()
            .context("Invalid context configuration")?;

        self.router
            .validate()
            .context("Invalid router configuration")?;

        self.hooks
            .validate()
            .context("Invalid hooks configuration")?;

        self.timeouts
            .validate()
            .context("Invalid timeouts configuration")?;

        Ok(())
    }

    #[cfg(feature = "bootstrap")]
    /// Bootstrap project with config + gitignore
    pub fn bootstrap_project<P: AsRef<Path>>(workspace: P, force: bool) -> Result<Vec<String>> {
        Self::bootstrap_project_with_options(workspace, force, false)
    }

    #[cfg(feature = "bootstrap")]
    /// Bootstrap project with config + gitignore, with option to create in home directory
    pub fn bootstrap_project_with_options<P: AsRef<Path>>(
        workspace: P,
        force: bool,
        use_home_dir: bool,
    ) -> Result<Vec<String>> {
        let workspace = workspace.as_ref().to_path_buf();
        defaults::with_config_defaults(|provider| {
            Self::bootstrap_project_with_provider(&workspace, force, use_home_dir, provider)
        })
    }

    #[cfg(feature = "bootstrap")]
    /// Bootstrap project files using the supplied [`ConfigDefaultsProvider`].
    pub fn bootstrap_project_with_provider<P: AsRef<Path>>(
        workspace: P,
        force: bool,
        use_home_dir: bool,
        defaults_provider: &dyn ConfigDefaultsProvider,
    ) -> Result<Vec<String>> {
        let workspace = workspace.as_ref();
        let config_file_name = defaults_provider.config_file_name().to_string();
        let (config_path, gitignore_path) = bootstrap::determine_bootstrap_targets(
            workspace,
            use_home_dir,
            &config_file_name,
            defaults_provider,
        )?;

        bootstrap::ensure_parent_dir(&config_path)?;
        bootstrap::ensure_parent_dir(&gitignore_path)?;

        let mut created_files = Vec::new();

        if !config_path.exists() || force {
            let config_content = Self::default_vtcode_toml_template();

            fs::write(&config_path, config_content).with_context(|| {
                format!("Failed to write config file: {}", config_path.display())
            })?;

            if let Some(file_name) = config_path.file_name().and_then(|name| name.to_str()) {
                created_files.push(file_name.to_string());
            }
        }

        if !gitignore_path.exists() || force {
            let gitignore_content = Self::default_vtcode_gitignore();
            fs::write(&gitignore_path, gitignore_content).with_context(|| {
                format!(
                    "Failed to write gitignore file: {}",
                    gitignore_path.display()
                )
            })?;

            if let Some(file_name) = gitignore_path.file_name().and_then(|name| name.to_str()) {
                created_files.push(file_name.to_string());
            }
        }

        Ok(created_files)
    }

    #[cfg(feature = "bootstrap")]
    /// Generate the default `vtcode.toml` template used by bootstrap helpers.
    fn default_vtcode_toml_template() -> String {
        r#"# VTCode Configuration File (Example)
# Getting-started reference; see docs/config/CONFIGURATION_PRECEDENCE.md for override order.
# Copy this file to vtcode.toml and customize as needed.

# Core agent behavior; see docs/config/CONFIGURATION_PRECEDENCE.md.
[agent]
# Primary LLM provider to use (e.g., "openai", "gemini", "anthropic", "openrouter")
provider = "openai"

# Environment variable containing the API key for the provider
api_key_env = "OPENAI_API_KEY"

# Default model to use when no specific model is specified
default_model = "gpt-5-nano"

# Visual theme for the terminal interface
theme = "ciapre-dark"

# Enable TODO planning helper mode for structured task management
todo_planning_mode = true

# UI surface to use ("auto", "alternate", "inline")
ui_surface = "auto"

# Maximum number of conversation turns before rotating context (affects memory usage)
# Lower values reduce memory footprint but may lose context; higher values preserve context but use more memory
max_conversation_turns = 50

# Reasoning effort level ("low", "medium", "high") - affects model usage and response speed
reasoning_effort = "low"

# Enable self-review loop to check and improve responses (increases API calls)
enable_self_review = false

# Maximum number of review passes when self-review is enabled
max_review_passes = 1

# Enable prompt refinement loop for improved prompt quality (increases processing time)
refine_prompts_enabled = false

# Maximum passes for prompt refinement when enabled
refine_prompts_max_passes = 1

# Optional alternate model for refinement (leave empty to use default)
refine_prompts_model = ""

# Maximum size of project documentation to include in context (in bytes)
project_doc_max_bytes = 16384

# Maximum size of instruction files to process (in bytes)
instruction_max_bytes = 16384

# List of additional instruction files to include in context
instruction_files = []

# Onboarding configuration - Customize the startup experience
[agent.onboarding]
# Enable the onboarding welcome message on startup
enabled = true

# Custom introduction text shown on startup
intro_text = "Let's get oriented. I preloaded workspace context so we can move fast."

# Include project overview information in welcome
include_project_overview = true

# Include language summary information in welcome
include_language_summary = false

# Include key guideline highlights from AGENTS.md
include_guideline_highlights = true

# Include usage tips in the welcome message
include_usage_tips_in_welcome = false

# Include recommended actions in the welcome message
include_recommended_actions_in_welcome = false

# Maximum number of guideline highlights to show
guideline_highlight_limit = 3

# List of usage tips shown during onboarding
usage_tips = [
    "Describe your current coding goal or ask for a quick status overview.",
    "Reference AGENTS.md guidelines when proposing changes.",
    "Draft or refresh your TODO list with update_plan before coding.",
    "Prefer asking for targeted file reads or diffs before editing.",
]

# List of recommended actions shown during onboarding
recommended_actions = [
    "Start the session by outlining a 3–6 step TODO plan via update_plan.",
    "Review the highlighted guidelines and share the task you want to tackle.",
    "Ask for a workspace tour if you need more context.",
]

# Custom prompts configuration - Define personal assistant commands
[agent.custom_prompts]
# Enable the custom prompts feature with /prompt:<name> syntax
enabled = true

# Directory where custom prompt files are stored
directory = "~/.vtcode/prompts"

# Additional directories to search for custom prompts
extra_directories = []

# Maximum file size for custom prompts (in kilobytes)
max_file_size_kb = 64

# Custom API keys for specific providers
[agent.custom_api_keys]
# Moonshot AI API key (for specific provider access)
moonshot = "sk-sDj3JUXDbfARCYKNL4q7iGWRtWuhL1M4O6zzgtDpN3Yxt9EA"

# Checkpointing configuration for session persistence
[agent.checkpointing]
# Enable automatic session checkpointing
enabled = false

# Maximum number of checkpoints to keep on disk
max_snapshots = 50

# Maximum age of checkpoints to keep (in days)
max_age_days = 30

# Tool security configuration
[tools]
# Default policy when no specific policy is defined ("allow", "prompt", "deny")
# "allow" - Execute without confirmation
# "prompt" - Ask for confirmation
# "deny" - Block the tool
default_policy = "prompt"

# Maximum number of tool loops allowed per turn (prevents infinite loops)
# Higher values allow more complex operations but risk performance issues
# Recommended: 20 for most tasks, 50 for complex multi-step workflows
max_tool_loops = 20

# Maximum number of repeated identical tool calls (prevents stuck loops)
max_repeated_tool_calls = 2

# Specific tool policies - Override default policy for individual tools
[tools.policies]
apply_patch = "prompt"            # Apply code patches (requires confirmation)
ast_grep_search = "allow"         # AST-based code search (no confirmation needed)
bash = "prompt"                   # Execute bash commands (requires confirmation)
close_pty_session = "allow"        # Close PTY sessions (no confirmation needed)
create_pty_session = "allow"       # Create PTY sessions (no confirmation needed)
curl = "prompt"                   # HTTP requests (requires confirmation)
edit_file = "allow"               # Edit files directly (no confirmation needed)
git_diff = "allow"                # Git diff operations (no confirmation needed)
grep_file = "allow"               # Sole content-search tool (ripgrep-backed)
list_files = "allow"              # List directory contents (no confirmation needed)
list_pty_sessions = "allow"       # List PTY sessions (no confirmation needed)
read_file = "allow"               # Read files (no confirmation needed)
read_pty_session = "allow"        # Read PTY session output (no no confirmation needed)
resize_pty_session = "allow"      # Resize PTY sessions (no confirmation needed)
run_pty_cmd = "prompt"            # Run commands in PTY (requires confirmation)
run_terminal_cmd = "prompt"       # Run terminal commands (requires confirmation)
send_pty_input = "prompt"         # Send input to PTY (requires confirmation)
update_plan = "allow"             # Update task plans (no confirmation needed)
write_file = "allow"              # Write files (no confirmation needed)

# Command security - Define safe and dangerous command patterns
[commands]
# Commands that are always allowed without confirmation
allow_list = [
    "ls",           # List directory contents
    "pwd",          # Print working directory
    "git status",   # Show git status
    "git diff",     # Show git differences
    "cargo check",  # Check Rust code
    "echo",         # Print text
]

# Commands that are never allowed
deny_list = [
    "rm -rf /",        # Delete root directory (dangerous)
    "rm -rf ~",        # Delete home directory (dangerous)
    "shutdown",        # Shut down system (dangerous)
    "reboot",          # Reboot system (dangerous)
    "sudo *",          # Any sudo command (dangerous)
    ":(){ :|:& };:",   # Fork bomb (dangerous)
]

# Command patterns that are allowed (supports glob patterns)
allow_glob = [
    "git *",        # All git commands
    "cargo *",      # All cargo commands
    "python -m *",  # Python module commands
]

# Command patterns that are denied (supports glob patterns)
deny_glob = [
    "rm *",         # All rm commands
    "sudo *",       # All sudo commands
    "chmod *",      # All chmod commands
    "chown *",      # All chown commands
    "kubectl *",    # All kubectl commands (admin access)
]

# Regular expression patterns for allowed commands (if needed)
allow_regex = []

# Regular expression patterns for denied commands (if needed)
deny_regex = []

# Security configuration - Safety settings for automated operations
[security]
# Require human confirmation for potentially dangerous actions
human_in_the_loop = true

# Require explicit write tool usage for claims about file modifications
require_write_tool_for_claims = true

# Auto-apply patches without prompting (DANGEROUS - disable for safety)
auto_apply_detected_patches = false

# UI configuration - Terminal and display settings
[ui]
# Tool output display mode
# "compact" - Concise tool output
# "full" - Detailed tool output
tool_output_mode = "compact"

# Maximum number of lines to display in tool output (prevents transcript flooding)
# Lines beyond this limit are truncated to a tail preview
tool_output_max_lines = 600

# Maximum bytes threshold for spooling tool output to disk
# Output exceeding this size is written to .vtcode/tool-output/*.log
tool_output_spool_bytes = 200000

# Optional custom directory for spooled tool output logs
# If not set, defaults to .vtcode/tool-output/
# tool_output_spool_dir = "/path/to/custom/spool/dir"

# Allow ANSI escape sequences in tool output (enables colors but may cause layout issues)
allow_tool_ansi = false

# Number of rows to allocate for inline UI viewport
inline_viewport_rows = 16

# Show timeline navigation panel
show_timeline_pane = false

# Status line configuration
[ui.status_line]
# Status line mode ("auto", "command", "hidden")
mode = "auto"

# How often to refresh status line (milliseconds)
refresh_interval_ms = 2000

# Timeout for command execution in status line (milliseconds)
command_timeout_ms = 200

# PTY (Pseudo Terminal) configuration - For interactive command execution
[pty]
# Enable PTY support for interactive commands
enabled = true

# Default number of terminal rows for PTY sessions
default_rows = 24

# Default number of terminal columns for PTY sessions
default_cols = 80

# Maximum number of concurrent PTY sessions
max_sessions = 10

# Command timeout in seconds (prevents hanging commands)
command_timeout_seconds = 300

# Number of recent lines to show in PTY output
stdout_tail_lines = 20

# Total lines to keep in PTY scrollback buffer
scrollback_lines = 400

# Context management configuration - Controls conversation memory
[context]
# Maximum number of tokens to keep in context (affects model cost and performance)
# Higher values preserve more context but cost more and may hit token limits
max_context_tokens = 90000

# Percentage to trim context to when it gets too large
trim_to_percent = 60

# Number of recent conversation turns to always preserve
preserve_recent_turns = 6

# Decision ledger configuration - Track important decisions
[context.ledger]
# Enable decision tracking and persistence
enabled = true

# Maximum number of decisions to keep in ledger
max_entries = 12

# Include ledger summary in model prompts
include_in_prompt = true

# Preserve ledger during context compression
preserve_in_compression = true

# Token budget management - Track and limit token usage
[context.token_budget]
# Enable token usage tracking and budget enforcement
enabled = false

# Model to use for token counting (must match your actual model)
model = "gpt-5-nano"

# Percentage threshold to warn about token usage (0.75 = 75%)
warning_threshold = 0.75

# Percentage threshold to trigger context compaction (0.85 = 85%)
compaction_threshold = 0.85

# Enable detailed component-level token tracking (increases overhead)
detailed_tracking = false

# Context curation - Intelligent context management
[context.curation]
# Enable automatic context curation (filters and optimizes context)
enabled = false

# Maximum tokens to allow per turn after curation
max_tokens_per_turn = 50000

# Number of recent messages to always preserve
preserve_recent_messages = 5

# Maximum number of tool descriptions to keep in context
max_tool_descriptions = 10

# Include decision ledger in curation
include_ledger = true

# Maximum ledger entries to include in curation
ledger_max_entries = 12

# Include recent error messages in context
include_recent_errors = true

# Maximum recent errors to include
max_recent_errors = 3

# AI model routing - Intelligent model selection
[router]
# Enable intelligent model routing
enabled = true

# Enable heuristic-based model selection
heuristic_classification = true

# Optional override model for routing decisions (empty = use default)
llm_router_model = ""

# Model mapping for different task types
[router.models]
# Model for simple queries
simple = "gpt-5-nano"
# Model for standard tasks
standard = "gpt-5-nano"
# Model for complex tasks
complex = "gpt-5-nano"
# Model for code generation heavy tasks
codegen_heavy = "gpt-5-nano"
# Model for information retrieval heavy tasks
retrieval_heavy = "gpt-5-nano"

# Router budget settings (if applicable)
[router.budgets]

# Router heuristic patterns for task classification
[router.heuristics]
# Maximum characters for short requests
short_request_max_chars = 120
# Minimum characters for long requests
long_request_min_chars = 1200

# Text patterns that indicate code patch operations
code_patch_markers = [
    "```",
    "diff --git",
    "apply_patch",
    "unified diff",
    "patch",
    "edit_file",
    "create_file",
]

# Text patterns that indicate information retrieval
retrieval_markers = [
    "search",
    "web",
    "google",
    "docs",
    "cite",
    "source",
    "up-to-date",
]

# Text patterns that indicate complex multi-step tasks
complex_markers = [
    "plan",
    "multi-step",
    "decompose",
    "orchestrate",
    "architecture",
    "benchmark",
    "implement end-to-end",
    "design api",
    "refactor module",
    "evaluate",
    "tests suite",
]

# Telemetry and analytics
[telemetry]
# Enable trajectory logging for usage analysis
trajectory_enabled = true

# Syntax highlighting configuration
[syntax_highlighting]
# Enable syntax highlighting for code in tool output
enabled = true

# Theme for syntax highlighting
theme = "base16-ocean.dark"

# Cache syntax highlighting themes for performance
cache_themes = true

# Maximum file size for syntax highlighting (in MB)
max_file_size_mb = 10

# Programming languages to enable syntax highlighting for
enabled_languages = [
    "rust",
    "python",
    "javascript",
    "typescript",
    "go",
    "java",
]

# Timeout for syntax highlighting operations (milliseconds)
highlight_timeout_ms = 1000

# Automation features - Full-auto mode settings
[automation.full_auto]
# Enable full automation mode (DANGEROUS - requires careful oversight)
enabled = false

# Maximum number of turns before asking for human input
max_turns = 30

# Tools allowed in full automation mode
allowed_tools = [
    "write_file",
    "read_file",
    "list_files",
    "grep_file",
]

# Require profile acknowledgment before using full auto
require_profile_ack = true

# Path to full auto profile configuration
profile_path = "automation/full_auto_profile.toml"

# Prompt caching - Cache model responses for efficiency
[prompt_cache]
# Enable prompt caching (reduces API calls for repeated prompts)
enabled = false

# Directory for cache storage
cache_dir = "~/.vtcode/cache/prompts"

# Maximum number of cache entries to keep
max_entries = 1000

# Maximum age of cache entries (in days)
max_age_days = 30

# Enable automatic cache cleanup
enable_auto_cleanup = true

# Minimum quality threshold to keep cache entries
min_quality_threshold = 0.7

# Prompt cache configuration for OpenAI
[prompt_cache.providers.openai]
enabled = true
min_prefix_tokens = 1024
idle_expiration_seconds = 3600
surface_metrics = true

# Prompt cache configuration for Anthropic
[prompt_cache.providers.anthropic]
enabled = true
default_ttl_seconds = 300
extended_ttl_seconds = 3600
max_breakpoints = 4
cache_system_messages = true
cache_user_messages = true

# Prompt cache configuration for Gemini
[prompt_cache.providers.gemini]
enabled = true
mode = "implicit"
min_prefix_tokens = 1024
explicit_ttl_seconds = 3600

# Prompt cache configuration for OpenRouter
[prompt_cache.providers.openrouter]
enabled = true
propagate_provider_capabilities = true
report_savings = true

# Prompt cache configuration for Moonshot
[prompt_cache.providers.moonshot]
enabled = true

# Prompt cache configuration for xAI
[prompt_cache.providers.xai]
enabled = true

# Prompt cache configuration for DeepSeek
[prompt_cache.providers.deepseek]
enabled = true
surface_metrics = true

# Prompt cache configuration for Z.AI
[prompt_cache.providers.zai]
enabled = false

# Model Context Protocol (MCP) - Connect external tools and services
[mcp]
# Enable Model Context Protocol (may impact startup time if services unavailable)
enabled = true
max_concurrent_connections = 5
request_timeout_seconds = 30
retry_attempts = 3

# MCP UI configuration
[mcp.ui]
mode = "compact"
max_events = 50
show_provider_names = true

# MCP renderer profiles for different services
[mcp.ui.renderers]
sequential-thinking = "sequential-thinking"
context7 = "context7"

# MCP provider configuration - External services that connect via MCP
[[mcp.providers]]
name = "time"
command = "uvx"
args = ["mcp-server-time"]
enabled = true
max_concurrent_requests = 3
[mcp.providers.env]

# Agent Client Protocol (ACP) - IDE integration
[acp]
enabled = true

[acp.zed]
enabled = true
transport = "stdio"
workspace_trust = "full_auto"

[acp.zed.tools]
read_file = true
list_files = true"#.to_string()
    }

    #[cfg(feature = "bootstrap")]
    fn default_vtcode_gitignore() -> String {
        r#"# Security-focused exclusions
.env, .env.local, secrets/, .aws/, .ssh/

# Development artifacts
target/, build/, dist/, node_modules/, vendor/

# Database files
*.db, *.sqlite, *.sqlite3

# Binary files
*.exe, *.dll, *.so, *.dylib, *.bin

# IDE files (comprehensive)
.vscode/, .idea/, *.swp, *.swo
"#
        .to_string()
    }

    #[cfg(feature = "bootstrap")]
    /// Create sample configuration file
    pub fn create_sample_config<P: AsRef<Path>>(output: P) -> Result<()> {
        let output = output.as_ref();
        let config_content = Self::default_vtcode_toml_template();

        fs::write(output, config_content)
            .with_context(|| format!("Failed to write config file: {}", output.display()))?;

        Ok(())
    }
}

/// Configuration manager for loading and validating configurations
#[derive(Clone)]
pub struct ConfigManager {
    config: VTCodeConfig,
    config_path: Option<PathBuf>,
    workspace_root: Option<PathBuf>,
    config_file_name: String,
}

impl ConfigManager {
    /// Load configuration from the default locations
    pub fn load() -> Result<Self> {
        Self::load_from_workspace(std::env::current_dir()?)
    }

    /// Load configuration from a specific workspace
    pub fn load_from_workspace(workspace: impl AsRef<Path>) -> Result<Self> {
        let workspace = workspace.as_ref();
        let defaults_provider = defaults::current_config_defaults();
        let workspace_paths = defaults_provider.workspace_paths_for(workspace);
        let workspace_root = workspace_paths.workspace_root().to_path_buf();
        let config_dir = workspace_paths.config_dir();
        let config_file_name = defaults_provider.config_file_name().to_string();

        // Try configuration file in workspace root first
        let config_path = workspace_root.join(&config_file_name);
        if config_path.exists() {
            let mut manager = Self::load_from_file(&config_path)?;
            manager.workspace_root = Some(workspace_root.clone());
            manager.config_file_name = config_file_name.clone();
            return Ok(manager);
        }

        // Try config directory fallback (e.g., .vtcode/vtcode.toml)
        let fallback_path = config_dir.join(&config_file_name);
        if fallback_path.exists() {
            let mut manager = Self::load_from_file(&fallback_path)?;
            manager.workspace_root = Some(workspace_root.clone());
            manager.config_file_name = config_file_name.clone();
            return Ok(manager);
        }

        // Try ~/.vtcode/vtcode.toml in user home directory
        for home_config_path in defaults_provider.home_config_paths(&config_file_name) {
            if home_config_path.exists() {
                let mut manager = Self::load_from_file(&home_config_path)?;
                manager.workspace_root = Some(workspace_root.clone());
                manager.config_file_name = config_file_name.clone();
                return Ok(manager);
            }
        }

        // Try project-specific configuration within the workspace config directory
        if let Some(project_config_path) =
            Self::project_config_path(&config_dir, &workspace_root, &config_file_name)
        {
            let mut manager = Self::load_from_file(&project_config_path)?;
            manager.workspace_root = Some(workspace_root.clone());
            manager.config_file_name = config_file_name.clone();
            return Ok(manager);
        }

        // Use default configuration if no file found
        let config = VTCodeConfig::default();
        config
            .validate()
            .context("Default configuration failed validation")?;

        Ok(Self {
            config,
            config_path: None,
            workspace_root: Some(workspace_root),
            config_file_name,
        })
    }

    /// Load configuration from a specific file
    pub fn load_from_file(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read config file: {}", path.display()))?;

        let config: VTCodeConfig = toml::from_str(&content)
            .with_context(|| format!("Failed to parse config file: {}", path.display()))?;

        config
            .validate()
            .with_context(|| format!("Failed to validate config file: {}", path.display()))?;

        let config_file_name = path
            .file_name()
            .and_then(|name| name.to_str().map(ToOwned::to_owned))
            .unwrap_or_else(|| {
                defaults::current_config_defaults()
                    .config_file_name()
                    .to_string()
            });

        Ok(Self {
            config,
            config_path: Some(path.to_path_buf()),
            workspace_root: path.parent().map(Path::to_path_buf),
            config_file_name,
        })
    }

    /// Get the loaded configuration
    pub fn config(&self) -> &VTCodeConfig {
        &self.config
    }

    /// Get the configuration file path (if loaded from file)
    pub fn config_path(&self) -> Option<&Path> {
        self.config_path.as_deref()
    }

    /// Get session duration from agent config
    pub fn session_duration(&self) -> std::time::Duration {
        std::time::Duration::from_secs(60 * 60) // Default 1 hour
    }

    /// Persist configuration to a specific path, preserving comments
    pub fn save_config_to_path(path: impl AsRef<Path>, config: &VTCodeConfig) -> Result<()> {
        let path = path.as_ref();

        // If file exists, preserve comments by using toml_edit
        if path.exists() {
            let original_content = fs::read_to_string(path)
                .with_context(|| format!("Failed to read existing config: {}", path.display()))?;

            let mut doc = original_content
                .parse::<toml_edit::DocumentMut>()
                .with_context(|| format!("Failed to parse existing config: {}", path.display()))?;

            // Serialize new config to TOML value
            let new_value =
                toml::to_string_pretty(config).context("Failed to serialize configuration")?;
            let new_doc: toml_edit::DocumentMut = new_value
                .parse()
                .context("Failed to parse serialized configuration")?;

            // Update values while preserving structure and comments
            Self::merge_toml_documents(&mut doc, &new_doc);

            fs::write(path, doc.to_string())
                .with_context(|| format!("Failed to write config file: {}", path.display()))?;
        } else {
            // New file, just write normally
            let content =
                toml::to_string_pretty(config).context("Failed to serialize configuration")?;
            fs::write(path, content)
                .with_context(|| format!("Failed to write config file: {}", path.display()))?;
        }

        Ok(())
    }

    /// Merge TOML documents, preserving comments and structure from original
    fn merge_toml_documents(original: &mut toml_edit::DocumentMut, new: &toml_edit::DocumentMut) {
        for (key, new_value) in new.iter() {
            if let Some(original_value) = original.get_mut(key) {
                Self::merge_toml_items(original_value, new_value);
            } else {
                original[key] = new_value.clone();
            }
        }
    }

    /// Recursively merge TOML items
    fn merge_toml_items(original: &mut toml_edit::Item, new: &toml_edit::Item) {
        match (original, new) {
            (toml_edit::Item::Table(orig_table), toml_edit::Item::Table(new_table)) => {
                for (key, new_value) in new_table.iter() {
                    if let Some(orig_value) = orig_table.get_mut(key) {
                        Self::merge_toml_items(orig_value, new_value);
                    } else {
                        orig_table[key] = new_value.clone();
                    }
                }
            }
            (orig, new) => {
                *orig = new.clone();
            }
        }
    }

    fn project_config_path(
        config_dir: &Path,
        workspace_root: &Path,
        config_file_name: &str,
    ) -> Option<PathBuf> {
        let project_name = Self::identify_current_project(workspace_root)?;
        let project_config_path = config_dir
            .join("projects")
            .join(project_name)
            .join("config")
            .join(config_file_name);

        if project_config_path.exists() {
            Some(project_config_path)
        } else {
            None
        }
    }

    fn identify_current_project(workspace_root: &Path) -> Option<String> {
        let project_file = workspace_root.join(".vtcode-project");
        if let Ok(contents) = fs::read_to_string(&project_file) {
            let name = contents.trim();
            if !name.is_empty() {
                return Some(name.to_string());
            }
        }

        workspace_root
            .file_name()
            .and_then(|name| name.to_str())
            .map(|name| name.to_string())
    }

    /// Persist configuration to the manager's associated path or workspace
    pub fn save_config(&self, config: &VTCodeConfig) -> Result<()> {
        if let Some(path) = &self.config_path {
            return Self::save_config_to_path(path, config);
        }

        if let Some(workspace_root) = &self.workspace_root {
            let path = workspace_root.join(&self.config_file_name);
            return Self::save_config_to_path(path, config);
        }

        let cwd = std::env::current_dir().context("Failed to resolve current directory")?;
        let path = cwd.join(&self.config_file_name);
        Self::save_config_to_path(path, config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::defaults::WorkspacePathsDefaults;
    use std::io::Write;
    use std::path::PathBuf;
    use std::sync::Arc;
    use tempfile::NamedTempFile;
    use vtcode_commons::reference::StaticWorkspacePaths;

    #[test]
    fn syntax_highlighting_defaults_are_valid() {
        let config = SyntaxHighlightingConfig::default();
        config
            .validate()
            .expect("default syntax highlighting config should be valid");
    }

    #[test]
    fn vtcode_config_validation_fails_for_invalid_highlight_timeout() {
        let mut config = VTCodeConfig::default();
        config.syntax_highlighting.highlight_timeout_ms = 0;
        let error = config
            .validate()
            .expect_err("validation should fail for zero highlight timeout");
        assert!(
            error.to_string().contains("highlight timeout"),
            "expected error to mention highlight timeout, got: {}",
            error
        );
    }

    #[test]
    fn load_from_file_rejects_invalid_syntax_highlighting() {
        let mut temp_file = NamedTempFile::new().expect("failed to create temp file");
        writeln!(
            temp_file,
            "[syntax_highlighting]\nhighlight_timeout_ms = 0\n"
        )
        .expect("failed to write temp config");

        let result = ConfigManager::load_from_file(temp_file.path());
        assert!(result.is_err(), "expected validation error");
        let error = format!("{:?}", result.err().unwrap());
        assert!(
            error.contains("validate"),
            "expected validation context in error, got: {}",
            error
        );
    }

    #[test]
    fn save_config_preserves_comments() {
        use std::io::Write;

        let mut temp_file = NamedTempFile::new().expect("failed to create temp file");
        let config_with_comments = r#"# This is a test comment
[agent]
# Provider comment
provider = "openai"
default_model = "gpt-5-nano"

# Tools section comment
[tools]
default_policy = "prompt"
"#;

        write!(temp_file, "{}", config_with_comments).expect("failed to write temp config");
        temp_file.flush().expect("failed to flush");

        // Load config
        let manager =
            ConfigManager::load_from_file(temp_file.path()).expect("failed to load config");

        // Modify and save
        let mut modified_config = manager.config().clone();
        modified_config.agent.default_model = "gpt-5".to_string();

        ConfigManager::save_config_to_path(temp_file.path(), &modified_config)
            .expect("failed to save config");

        // Read back and verify comments are preserved
        let saved_content =
            fs::read_to_string(temp_file.path()).expect("failed to read saved config");

        assert!(
            saved_content.contains("# This is a test comment"),
            "top-level comment should be preserved"
        );
        assert!(
            saved_content.contains("# Provider comment"),
            "inline comment should be preserved"
        );
        assert!(
            saved_content.contains("# Tools section comment"),
            "section comment should be preserved"
        );
        assert!(
            saved_content.contains("gpt-5"),
            "modified value should be present"
        );
    }

    #[test]
    fn config_defaults_provider_overrides_paths_and_theme() {
        let workspace = assert_fs::TempDir::new().expect("failed to create workspace");
        let workspace_root = workspace.path();
        let config_dir = workspace_root.join("config-root");
        fs::create_dir_all(&config_dir).expect("failed to create config directory");

        let config_file_name = "custom-config.toml";
        let config_path = config_dir.join(config_file_name);
        let serialized =
            toml::to_string(&VTCodeConfig::default()).expect("failed to serialize default config");
        fs::write(&config_path, serialized).expect("failed to write config file");

        let static_paths = StaticWorkspacePaths::new(workspace_root, &config_dir);
        let provider = WorkspacePathsDefaults::new(Arc::new(static_paths))
            .with_config_file_name(config_file_name)
            .with_home_paths(Vec::new())
            .with_syntax_theme("custom-theme")
            .with_syntax_languages(vec!["zig".to_string()]);

        defaults::provider::with_config_defaults_provider_for_test(Arc::new(provider), || {
            let manager = ConfigManager::load_from_workspace(workspace_root)
                .expect("failed to load workspace config");

            let resolved_path = manager
                .config_path()
                .expect("config path should be resolved");
            assert_eq!(resolved_path, config_path);

            assert_eq!(SyntaxHighlightingDefaults::theme(), "custom-theme");
            assert_eq!(
                SyntaxHighlightingDefaults::enabled_languages(),
                vec!["zig".to_string()]
            );
        });
    }
}
