use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

use crate::acp::AgentClientProtocolConfig;
use crate::agent_teams::AgentTeamsConfig;
use crate::context::ContextFeaturesConfig;
use crate::core::{
    AgentConfig, AnthropicConfig, AuthConfig, AutomationConfig, CommandsConfig,
    DotfileProtectionConfig, ModelConfig, PermissionsConfig, PromptCachingConfig, SandboxConfig,
    SecurityConfig, SkillsConfig, ToolsConfig,
};
use crate::debug::DebugConfig;
use crate::defaults::{self, ConfigDefaultsProvider};
use crate::hooks::HooksConfig;
use crate::mcp::McpClientConfig;
use crate::optimization::OptimizationConfig;
use crate::output_styles::OutputStyleConfig;
use crate::root::{ChatConfig, PtyConfig, UiConfig};
use crate::subagent::SubagentsConfig;
use crate::telemetry::TelemetryConfig;
use crate::timeouts::TimeoutsConfig;

use crate::loader::syntax_highlighting::SyntaxHighlightingConfig;

/// Provider-specific configuration
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct ProviderConfig {
    /// Anthropic provider configuration
    #[serde(default)]
    pub anthropic: AnthropicConfig,
}

/// Main configuration structure for VT Code
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct VTCodeConfig {
    /// Agent-wide settings
    #[serde(default)]
    pub agent: AgentConfig,

    /// Authentication configuration for OAuth flows
    #[serde(default)]
    pub auth: AuthConfig,

    /// Tool execution policies
    #[serde(default)]
    pub tools: ToolsConfig,

    /// Unix command permissions
    #[serde(default)]
    pub commands: CommandsConfig,

    /// Permission system settings (resolution, audit logging, caching)
    #[serde(default)]
    pub permissions: PermissionsConfig,

    /// Security settings
    #[serde(default)]
    pub security: SecurityConfig,

    /// Sandbox settings for command execution isolation
    #[serde(default)]
    pub sandbox: SandboxConfig,

    /// UI settings
    #[serde(default)]
    pub ui: UiConfig,

    /// Chat settings
    #[serde(default)]
    pub chat: ChatConfig,

    /// PTY settings
    #[serde(default)]
    pub pty: PtyConfig,

    /// Debug and tracing settings
    #[serde(default)]
    pub debug: DebugConfig,

    /// Context features (e.g., Decision Ledger)
    #[serde(default)]
    pub context: ContextFeaturesConfig,

    /// Telemetry configuration (logging, trajectory)
    #[serde(default)]
    pub telemetry: TelemetryConfig,

    /// Performance optimization settings
    #[serde(default)]
    pub optimization: OptimizationConfig,

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

    /// Model-specific behavior configuration
    #[serde(default)]
    pub model: ModelConfig,

    /// Provider-specific configuration
    #[serde(default)]
    pub provider: ProviderConfig,

    /// Skills system configuration (Agent Skills spec)
    #[serde(default)]
    pub skills: SkillsConfig,

    /// Subagent system configuration
    #[serde(default)]
    pub subagents: SubagentsConfig,

    /// Agent teams configuration (experimental)
    #[serde(default)]
    pub agent_teams: AgentTeamsConfig,

    /// Output style configuration
    #[serde(default)]
    pub output_style: OutputStyleConfig,

    /// Dotfile protection configuration
    #[serde(default)]
    pub dotfile_protection: DotfileProtectionConfig,
}

impl VTCodeConfig {
    pub fn validate(&self) -> Result<()> {
        self.syntax_highlighting
            .validate()
            .context("Invalid syntax_highlighting configuration")?;

        self.context
            .validate()
            .context("Invalid context configuration")?;

        self.hooks
            .validate()
            .context("Invalid hooks configuration")?;

        self.timeouts
            .validate()
            .context("Invalid timeouts configuration")?;

        self.prompt_cache
            .validate()
            .context("Invalid prompt_cache configuration")?;

        self.ui
            .keyboard_protocol
            .validate()
            .context("Invalid keyboard_protocol configuration")?;

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
        let (config_path, gitignore_path) = crate::loader::bootstrap::determine_bootstrap_targets(
            workspace,
            use_home_dir,
            &config_file_name,
            defaults_provider,
        )?;

        crate::loader::bootstrap::ensure_parent_dir(&config_path)?;
        crate::loader::bootstrap::ensure_parent_dir(&gitignore_path)?;

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
        r#"# VT Code Configuration File (Example)
# Getting-started reference; see docs/config/CONFIGURATION_PRECEDENCE.md for override order.
# Copy this file to vtcode.toml and customize as needed.

# Core agent behavior; see docs/config/CONFIGURATION_PRECEDENCE.md.
[agent]
# Primary LLM provider to use (e.g., "openai", "gemini", "anthropic", "openrouter")
provider = "anthropic"

# Environment variable containing the API key for the provider
api_key_env = "ANTHROPIC_API_KEY"

# Default model to use when no specific model is specified
default_model = "claude-sonnet-4-5"

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

# Default editing mode on startup: "edit" or "plan"
# "edit" - Full tool access for file modifications and command execution (default)
# "plan" - Read-only mode that produces implementation plans without making changes
# Toggle during session with Shift+Tab or /plan command
default_editing_mode = "edit"

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
    "Prefer asking for targeted file reads or diffs before editing.",
]

# List of recommended actions shown during onboarding
recommended_actions = [
    "Review the highlighted guidelines and share the task you want to tackle.",
    "Ask for a workspace tour if you need more context.",
]

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

# Subagent system (opt-in)
[subagents]
# Enable subagents (default: false)
enabled = false

# Maximum concurrent subagents
# max_concurrent = 3

# Default timeout for subagent execution (seconds)
# default_timeout_seconds = 300

# Default model for subagents (override per-agent model if set)
# default_model = ""

# Agent teams (experimental)
[agent_teams]
# Enable agent teams (default: false)
enabled = false

# Maximum number of teammates per team
# max_teammates = 4

# Default model for agent team subagents
# default_model = ""

# Teammate display mode (auto, tmux, in_process)
# teammate_mode = "auto"

# Optional storage directory override for team state
# storage_dir = "~/.vtcode"

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

# Maximum consecutive blocked tool calls before force-breaking the turn
# Helps prevent high-CPU churn when calls are repeatedly denied/blocked
max_consecutive_blocked_tool_calls_per_turn = 8

# Maximum sequential spool-chunk reads per turn before nudging targeted extraction/summarization
max_sequential_spool_chunk_reads = 6

# Specific tool policies - Override default policy for individual tools
[tools.policies]
apply_patch = "prompt"            # Apply code patches (requires confirmation)
close_pty_session = "allow"        # Close PTY sessions (no confirmation needed)
create_pty_session = "allow"       # Create PTY sessions (no confirmation needed)
edit_file = "allow"               # Edit files directly (no confirmation needed)
grep_file = "allow"               # Sole content-search tool (ripgrep-backed)
list_files = "allow"              # List directory contents (no confirmation needed)
list_pty_sessions = "allow"       # List PTY sessions (no confirmation needed)
read_file = "allow"               # Read files (no confirmation needed)
read_pty_session = "allow"        # Read PTY session output (no no confirmation needed)
resize_pty_session = "allow"      # Resize PTY sessions (no confirmation needed)
run_pty_cmd = "prompt"            # Run commands in PTY (requires confirmation)
exec_command = "prompt"           # Execute command in unified session (requires confirmation)
write_stdin = "prompt"            # Write to stdin in unified session (requires confirmation)

send_pty_input = "prompt"         # Send input to PTY (requires confirmation)
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

# Runtime notification preferences
[ui.notifications]
# Master toggle for terminal/desktop notifications
enabled = true

# Delivery mode: "terminal", "hybrid", or "desktop"
delivery_mode = "hybrid"

# Suppress notifications while terminal is focused
suppress_when_focused = true

# High-signal event toggles
tool_failure = true
error = true
completion = true
hitl = true

# Success notifications for tool call results
tool_success = false

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

# AI model routing - Intelligent model selection
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
    "bash",
    "sh",
    "shell",
    "zsh",
    "markdown",
    "md",
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
    # Routing key strategy for OpenAI prompt cache locality.
    # "session" creates one stable key per VT Code conversation.
    prompt_cache_key_mode = "session"
    # Optional: server-side prompt cache retention for OpenAI Responses API
    # Example: "24h" (leave commented out for default behavior)
    # prompt_cache_retention = "24h"

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
# workspace_trust controls ACP trust mode: "tools_policy" (prompts) or "full_auto" (no prompts)
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
