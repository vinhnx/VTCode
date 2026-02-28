//! CLI argument parsing and configuration

use crate::config::models::ModelId;
use clap::{ArgAction, ColorChoice, Parser, Subcommand, ValueEnum, ValueHint};
use colorchoice_clap::Color as ColorSelection;
use std::path::PathBuf;
use vtcode_config::agent_teams::TeammateMode;

#[derive(Clone, Debug, ValueEnum)]
pub enum TeammateModeArg {
    Auto,
    Tmux,
    InProcess,
}

impl From<TeammateModeArg> for TeammateMode {
    fn from(value: TeammateModeArg) -> Self {
        match value {
            TeammateModeArg::Auto => TeammateMode::Auto,
            TeammateModeArg::Tmux => TeammateMode::Tmux,
            TeammateModeArg::InProcess => TeammateMode::InProcess,
        }
    }
}

#[derive(Clone, Debug, ValueEnum)]
pub enum TeamRoleArg {
    Lead,
    Teammate,
}

/// Get the long version information following Ratatui recipe pattern
///
/// Displays version, authors, and directory information following the
/// XDG Base Directory Specification for organized file storage.
/// See: https://ratatui.rs/recipes/apps/config-directories/
///
/// This function is called at runtime to provide dynamic version info
/// that includes actual resolved directory paths.
pub fn long_version() -> String {
    use crate::config::defaults::{get_config_dir, get_data_dir};

    let config_dir = get_config_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "~/.vtcode/".to_string());

    let data_dir = get_data_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "~/.vtcode/cache/".to_string());

    format!(
        "{}\n\nAuthors: {}\nConfig directory: {}\nData directory: {}\n\nEnvironment variables:\n  VTCODE_CONFIG - Override config directory\n  VTCODE_DATA - Override data directory",
        env!("CARGO_PKG_VERSION"),
        env!("CARGO_PKG_AUTHORS"),
        config_dir,
        data_dir
    )
}

/// Main CLI structure for vtcode with advanced features
#[derive(Parser, Debug, Clone)]
#[command(
    name = "vtcode",
    version,
    about = "VT Code - AI coding assistant",
    color = ColorChoice::Auto
)]
pub struct Cli {
    /// Color output selection (auto, always, never)
    #[command(flatten)]
    pub color: ColorSelection,

    /// Optional positional path to run vtcode against a different workspace
    #[arg(
        value_name = "WORKSPACE",
        value_hint = ValueHint::DirPath,
        global = true
    )]
    pub workspace_path: Option<PathBuf>,

    /// LLM Model ID (e.g., gpt-5, claude-sonnet-4-5, gemini-3-flash-preview)
    #[arg(long, global = true)]
    pub model: Option<String>,

    /// LLM Provider (gemini, openai, anthropic, deepseek, openrouter, zai, moonshot, minimax, ollama, lmstudio)
    #[arg(long, global = true)]
    pub provider: Option<String>,

    /// API key environment variable (auto-detects GEMINI_API_KEY, OPENAI_API_KEY, etc.)
    #[arg(long, global = true, default_value = crate::config::constants::defaults::DEFAULT_API_KEY_ENV)]
    pub api_key_env: String,

    /// Workspace root directory (default: current directory)
    #[arg(
        long,
        global = true,
        alias = "workspace-dir",
        value_name = "PATH",
        value_hint = ValueHint::DirPath
    )]
    pub workspace: Option<PathBuf>,

    /// Team name to join or lead
    #[arg(long, global = true, value_name = "TEAM")]
    pub team: Option<String>,

    /// Teammate name (join as teammate)
    #[arg(long, global = true, value_name = "NAME")]
    pub teammate: Option<String>,

    /// Team role override (lead or teammate)
    #[arg(long, global = true, value_enum)]
    pub team_role: Option<TeamRoleArg>,

    /// Teammate mode override (auto, tmux, in_process)
    #[arg(long, global = true, value_enum)]
    pub teammate_mode: Option<TeammateModeArg>,

    /// Enable research-preview features
    #[arg(long, global = true)]
    pub research_preview: bool,

    /// Security level for tool execution (strict, moderate, permissive)
    #[arg(long, global = true, default_value = "moderate")]
    pub security_level: String,

    /// Show diffs for file changes in chat interface
    #[arg(long, global = true)]
    pub show_file_diffs: bool,

    /// Maximum concurrent async operations
    #[arg(long, global = true, default_value_t = 5)]
    pub max_concurrent_ops: usize,

    /// Maximum API requests per minute
    #[arg(long, global = true, default_value_t = 30)]
    pub api_rate_limit: usize,

    /// Maximum tool calls per session
    #[arg(long, global = true, default_value_t = 10)]
    pub max_tool_calls: usize,

    /// Enable debug output for troubleshooting
    #[arg(long, global = true)]
    pub debug: bool,

    /// Enable verbose logging
    #[arg(long, global = true)]
    pub verbose: bool,

    /// Suppress all non-essential output (for scripting, CI/CD)
    #[arg(short, long, global = true)]
    pub quiet: bool,

    /// Configuration overrides or file path (KEY=VALUE or PATH)
    #[arg(
        short = 'c',
        long = "config",
        value_name = "KEY=VALUE|PATH",
        action = ArgAction::Append,
        global = true
    )]
    pub config: Vec<String>,

    /// Log level (error, warn, info, debug, trace)
    #[arg(long, global = true, default_value = "info")]
    pub log_level: String,

    /// Disable color output (for log files, CI/CD)
    #[arg(long, global = true)]
    pub no_color: bool,

    /// Select UI theme (e.g., ciapre-dark, ciapre-blue)
    #[arg(long, global = true, value_name = "THEME")]
    pub theme: Option<String>,

    /// App tick rate in milliseconds (default: 250)
    #[arg(short = 't', long, default_value_t = 250)]
    pub tick_rate: u64,

    /// Frame rate in FPS (default: 60)
    #[arg(short = 'f', long, default_value_t = 60)]
    pub frame_rate: u64,

    /// Enable skills system
    #[arg(long, global = true)]
    pub enable_skills: bool,

    /// Enable Chrome browser integration for web automation
    #[arg(long, global = true)]
    pub chrome: bool,

    /// Disable Chrome browser integration
    #[arg(long = "no-chrome", global = true, conflicts_with = "chrome")]
    pub no_chrome: bool,

    /// Skip safety confirmations (use with caution)
    #[arg(long, global = true)]
    pub skip_confirmations: bool,

    /// Print response without launching the interactive TUI
    #[arg(
        short = 'p',
        long = "print",
        value_name = "PROMPT",
        value_hint = ValueHint::Other,
        num_args = 0..=1,
        default_missing_value = "",
        global = true,
        conflicts_with_all = ["full_auto"]
    )]
    pub print: Option<String>,

    /// Enable full-auto mode (no interaction) or run a headless task
    #[arg(
        long = "full-auto",
        visible_alias = "auto",
        global = true,
        value_name = "PROMPT",
        num_args = 0..=1,
        default_missing_value = "",
        value_hint = ValueHint::Other
    )]
    pub full_auto: Option<String>,

    /// Resume a previous conversation (use without ID for interactive picker)
    #[arg(
        short = 'r',
        long = "resume",
        global = true,
        value_name = "SESSION_ID",
        num_args = 0..=1,
        default_missing_value = "__interactive__",
        conflicts_with_all = ["continue_latest", "full_auto"]
    )]
    pub resume_session: Option<String>,

    /// Continue the most recent conversation automatically
    #[arg(
        long = "continue",
        visible_alias = "continue-session",
        global = true,
        conflicts_with_all = ["resume_session", "full_auto"]
    )]
    pub continue_latest: bool,

    /// Fork an existing session with a new session ID
    #[arg(
        long = "fork-session",
        global = true,
        value_name = "SESSION_ID",
        conflicts_with_all = ["resume_session", "continue_latest", "full_auto"]
    )]
    pub fork_session: Option<String>,

    /// Custom suffix for session identifier (alphanumeric, dash, underscore only, max 64 chars)
    #[arg(long = "session-id", global = true, value_name = "CUSTOM_SUFFIX")]
    pub session_id: Option<String>,

    /// Override the default agent model for this session
    #[arg(long, global = true, value_name = "AGENT")]
    pub agent: Option<String>,

    /// Add additional working directories for the agent to access
    #[arg(long = "add-dir", global = true, value_name = "PATH", value_hint = ValueHint::DirPath)]
    pub additional_dirs: Vec<PathBuf>,

    /// Tools that execute without prompting (comma-separated, supports patterns like "Bash(git:*)")
    #[arg(long = "allowed-tools", global = true, value_name = "TOOLS", action = ArgAction::Append)]
    pub allowed_tools: Vec<String>,

    /// Tools that cannot be used by the agent
    #[arg(long = "disallowed-tools", global = true, value_name = "TOOLS", action = ArgAction::Append)]
    pub disallowed_tools: Vec<String>,

    /// Skip all permission prompts (reduces security - use with caution)
    #[arg(long = "dangerously-skip-permissions", global = true)]
    pub dangerously_skip_permissions: bool,

    /// Explicitly connect to IDE on startup (auto-detects available IDEs)
    #[arg(long, global = true)]
    pub ide: bool,

    /// Begin in a specified permission mode (ask, suggest, auto-approved, full-auto, plan)
    #[arg(long, global = true, value_name = "MODE")]
    pub permission_mode: Option<String>,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

/// Options for the `ask` command
#[derive(Debug, Default, Clone)]
pub struct AskCommandOptions {
    pub output_format: Option<AskOutputFormat>,
    pub allowed_tools: Vec<String>,
    pub disallowed_tools: Vec<String>,
    pub skip_confirmations: bool,
}

/// Output format options for the `ask` subcommand.
#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
pub enum AskOutputFormat {
    /// Emit the response as a structured JSON document.
    Json,
}

/// Available commands
#[derive(Subcommand, Debug, Clone)]
pub enum Commands {
    /// Start Agent Client Protocol bridge for IDE integrations
    #[command(name = "acp")]
    AgentClientProtocol {
        /// Client to connect over ACP
        #[arg(value_enum, default_value_t = AgentClientProtocolTarget::Zed)]
        target: AgentClientProtocolTarget,
    },

    /// Interactive AI coding assistant
    Chat,

    /// Single prompt mode - prints model reply without tools
    Ask {
        /// Prompt to ask. Use `-` to force reading from stdin.
        #[arg(value_name = "PROMPT")]
        prompt: Option<String>,
        /// Format the response using a structured representation.
        #[arg(long = "output-format", value_enum, value_name = "FORMAT")]
        output_format: Option<AskOutputFormat>,
    },
    /// Headless execution mode
    Exec {
        /// Emit structured JSON events to stdout (one per line)
        #[arg(long)]
        json: bool,
        /// Optional path to write the JSONL transcript
        #[arg(long, value_name = "PATH", value_hint = ValueHint::FilePath)]
        events: Option<PathBuf>,
        /// Write the last agent message to this file
        #[arg(long, value_name = "PATH", value_hint = ValueHint::FilePath)]
        last_message_file: Option<PathBuf>,
        /// Prompt to execute. Use `-` to force reading from stdin.
        #[arg(value_name = "PROMPT")]
        prompt: Option<String>,
    },

    /// Verbose interactive chat with debug output
    ChatVerbose,

    /// Analyze workspace (structure, security, performance)
    Analyze {
        /// Type of analysis to perform
        #[arg(value_name = "TYPE", default_value = "full")]
        analysis_type: String,
    },

    /// Pretty-print trajectory logs
    #[command(name = "trajectory")]
    Trajectory {
        /// Optional path to trajectory JSONL file
        #[arg(long)]
        file: Option<std::path::PathBuf>,
        /// Number of top entries to show
        #[arg(long, default_value_t = 10)]
        top: usize,
    },

    /// Benchmark against SWE-bench evaluation framework
    Benchmark {
        /// Path to a JSON benchmark specification
        #[arg(long, value_name = "PATH", value_hint = ValueHint::FilePath)]
        task_file: Option<PathBuf>,
        /// Inline JSON specification for quick experiments
        #[arg(long, value_name = "JSON")]
        task: Option<String>,
        /// Optional path to write the structured benchmark report
        #[arg(long, value_name = "PATH", value_hint = ValueHint::FilePath)]
        output: Option<PathBuf>,
        /// Limit the number of tasks executed
        #[arg(long, value_name = "COUNT")]
        max_tasks: Option<usize>,
    },

    /// Create complete Rust project
    CreateProject { name: String, features: Vec<String> },

    /// Revert agent to a previous snapshot
    Revert {
        /// Turn number to revert to
        #[arg(short, long)]
        turn: usize,
        /// Scope of revert operation: conversation, code, full
        #[arg(short, long)]
        partial: Option<String>,
    },

    /// List all available snapshots
    Snapshots,

    /// Clean up old snapshots
    ///
    /// Features:
    ///   • Remove snapshots beyond limit
    ///   • Configurable retention policy
    ///   • Safe deletion with confirmation
    ///
    /// Examples:
    ///   vtcode cleanup-snapshots
    ///   vtcode cleanup-snapshots --max 20
    #[command(name = "cleanup-snapshots")]
    CleanupSnapshots {
        /// Maximum number of snapshots to keep
        ///
        /// Default: 50
        /// Example: --max 20
        #[arg(short, long, default_value_t = 50)]
        max: usize,
    },

    /// Initialize project with dot-folder structure
    Init,

    /// Initialize project in ~/.vtcode/projects/
    #[command(name = "init-project")]
    InitProject {
        /// Project name - defaults to current directory name
        #[arg(long)]
        name: Option<String>,
        /// Force initialization - overwrite existing project structure
        #[arg(long)]
        force: bool,
        /// Migrate existing files - move existing config/cache files to new structure
        #[arg(long)]
        migrate: bool,
    },

    /// Generate configuration file
    Config {
        /// Output file path
        #[arg(long)]
        output: Option<std::path::PathBuf>,
        /// Create in user home directory (~/.vtcode/vtcode.toml)
        #[arg(long)]
        global: bool,
    },

    /// Manage tool execution policies
    #[command(name = "tool-policy")]
    ToolPolicy {
        #[command(subcommand)]
        command: crate::cli::tool_policy_commands::ToolPolicyCommands,
    },

    /// Manage Model Context Protocol providers
    #[command(name = "mcp")]
    Mcp {
        #[command(subcommand)]
        command: crate::mcp::cli::McpCommands,
    },

    /// Agent2Agent (A2A) Protocol
    #[command(name = "a2a")]
    A2a {
        #[command(subcommand)]
        command: super::super::a2a::cli::A2aCommands,
    },

    /// Manage models and providers
    Models {
        #[command(subcommand)]
        command: ModelCommands,
    },

    /// Security and safety management
    Security,

    /// Generate or display man pages
    Man {
        /// Command name to generate man page for (optional)
        command: Option<String>,
        /// Output file path to save man page
        #[arg(short, long)]
        output: Option<std::path::PathBuf>,
    },

    /// Display token budget information
    Tokens {
        #[command(subcommand)]
        command: TokenCommands,
    },

    /// Manage Agent Skills
    #[command(subcommand)]
    Skills(SkillsSubcommand),

    /// List available skills (alias for `vtcode skills list`)
    #[command(name = "list-skills")]
    ListSkills {},

    /// Check for and install binary updates from GitHub Releases
    #[command(name = "update")]
    Update {
        /// Check for updates without installing
        #[arg(long)]
        check: bool,
        /// Force update even if on latest version
        #[arg(long)]
        force: bool,
    },

    /// Start Anthropic API compatibility server
    #[command(name = "anthropic-api")]
    AnthropicApi {
        /// Port to run the server on
        #[arg(long, default_value = "11434")]
        port: u16,
        /// Host address to bind to
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
    },
}

/// Token-related subcommands
#[derive(Subcommand, Debug, Clone)]
pub enum TokenCommands {
    /// Show current token budget status and usage
    Status,

    /// Show recent token usage history
    History,

    /// Show summary of token usage patterns
    Summary,
}

/// Supported Agent Client Protocol clients
#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum AgentClientProtocolTarget {
    /// Agent Client Protocol client (legacy Zed identifier)
    Zed,
    /// Standard Agent Client Protocol client
    Standard,
}

/// Model management commands with concise, actionable help
#[derive(Subcommand, Debug, Clone)]
pub enum ModelCommands {
    /// List all providers and models with status indicators
    List,

    /// Set default provider (gemini, openai, anthropic, deepseek)
    #[command(name = "set-provider")]
    SetProvider {
        /// Provider name to set as default
        provider: String,
    },

    /// Set default model (e.g., deepseek-reasoner, gpt-5, claude-sonnet-4-5)
    #[command(name = "set-model")]
    SetModel {
        /// Model name to set as default
        model: String,
    },

    /// Configure provider settings (API keys, base URLs, models)
    Config {
        /// Provider name to configure
        provider: String,

        /// API key for the provider
        #[arg(long)]
        api_key: Option<String>,

        /// Base URL for local providers
        #[arg(long)]
        base_url: Option<String>,

        /// Default model for this provider
        #[arg(long)]
        model: Option<String>,
    },

    /// Test provider connectivity and validate configuration
    Test {
        /// Provider name to test
        provider: String,
    },

    /// Compare model performance across providers (coming soon)
    Compare,

    /// Show detailed model information and specifications
    Info {
        /// Model name to get information about
        model: String,
    },
}

/// Skills subcommands
#[derive(Debug, Subcommand, Clone)]
pub enum SkillsSubcommand {
    /// List available skills
    #[command(name = "list")]
    List {
        /// Show all skills including system skills
        #[arg(long)]
        all: bool,
    },

    /// Load a skill for use in agent session
    #[command(name = "load")]
    Load {
        /// Skill name to load
        name: String,
        /// Optional path to skill directory
        #[arg(long)]
        path: Option<PathBuf>,
    },

    /// Unload a skill from session
    #[command(name = "unload")]
    Unload {
        /// Skill name to unload
        name: String,
    },

    /// Show skill details and instructions
    #[command(name = "info")]
    Info {
        /// Skill name to get information about
        name: String,
    },

    /// Create a new skill from template
    #[command(name = "create")]
    Create {
        /// Path for new skill directory
        path: PathBuf,
        /// Optional template to use
        #[arg(long)]
        template: Option<String>,
    },

    /// Validate SKILL.md manifest
    #[command(name = "validate")]
    Validate {
        /// Path to skill directory or SKILL.md file
        path: PathBuf,
    },

    /// Validate all skills for container skills compatibility
    #[command(name = "check-compatibility")]
    CheckCompatibility,

    /// Show skill configuration and search paths
    #[command(name = "config")]
    Config,

    /// Regenerate skills index file
    #[command(name = "regenerate-index")]
    RegenerateIndex,

    /// skills-ref compatible commands (agentskills.io spec)
    #[command(name = "skills-ref", subcommand)]
    SkillsRef(SkillsRefSubcommand),
}

/// skills-ref compatible subcommands per agentskills.io specification
#[derive(Debug, Subcommand, Clone)]
pub enum SkillsRefSubcommand {
    /// Validate a skill directory
    #[command(name = "validate")]
    Validate {
        /// Path to skill directory
        path: PathBuf,
    },

    /// Generate <available_skills> XML for agent prompts
    #[command(name = "to-prompt")]
    ToPrompt {
        /// Paths to skill directories
        paths: Vec<PathBuf>,
    },

    /// List discovered skills
    #[command(name = "list")]
    List {
        /// Optional path to search (defaults to current directory)
        path: Option<PathBuf>,
    },
}

/// Configuration file structure with latest features
#[derive(Debug)]
pub struct ConfigFile {
    pub model: Option<String>,
    pub provider: Option<String>,
    pub api_key_env: Option<String>,
    pub verbose: Option<bool>,
    pub log_level: Option<String>,
    pub workspace: Option<PathBuf>,
    pub tools: Option<ToolConfig>,
    pub context: Option<ContextConfig>,
    pub logging: Option<LoggingConfig>,
    pub performance: Option<PerformanceConfig>,
    pub security: Option<SecurityConfig>,
}

/// Tool configuration from config file
#[derive(Debug, serde::Deserialize)]
pub struct ToolConfig {
    pub enable_validation: Option<bool>,
    pub max_execution_time_seconds: Option<u64>,
    pub allow_file_creation: Option<bool>,
    pub allow_file_deletion: Option<bool>,
}

/// Context management configuration
#[derive(Debug, serde::Deserialize)]
pub struct ContextConfig {
    pub max_context_length: Option<usize>,
}

/// Logging configuration
#[derive(Debug, serde::Deserialize)]
pub struct LoggingConfig {
    pub file_logging: Option<bool>,
    pub log_directory: Option<String>,
    pub max_log_files: Option<usize>,
    pub max_log_size_mb: Option<usize>,
}

/// Performance monitoring configuration
#[derive(Debug, serde::Deserialize)]
pub struct PerformanceConfig {
    pub enabled: Option<bool>,
    pub track_token_usage: Option<bool>,
    pub track_api_costs: Option<bool>,
    pub track_response_times: Option<bool>,
    pub enable_benchmarking: Option<bool>,
    pub metrics_retention_days: Option<usize>,
}

/// Security configuration
#[derive(Debug, serde::Deserialize)]
pub struct SecurityConfig {
    pub level: Option<String>,
    pub enable_audit_logging: Option<bool>,
    pub enable_vulnerability_scanning: Option<bool>,
    pub allow_external_urls: Option<bool>,
    pub max_file_access_depth: Option<usize>,
}

impl Default for Cli {
    fn default() -> Self {
        Self {
            color: ColorSelection {
                color: ColorChoice::Auto,
            },
            workspace_path: None,
            model: Some(ModelId::default().to_string()),
            provider: Some("gemini".to_owned()),
            api_key_env: "GEMINI_API_KEY".to_owned(),
            workspace: None,
            team: None,
            teammate: None,
            team_role: None,
            teammate_mode: None,
            research_preview: false,
            security_level: "moderate".to_owned(),
            show_file_diffs: false,
            max_concurrent_ops: 5,
            api_rate_limit: 30,
            max_tool_calls: 10,
            verbose: false,
            quiet: false,
            config: Vec::new(),
            log_level: "info".to_owned(),
            no_color: false,
            theme: None,
            skip_confirmations: false,
            print: None,
            full_auto: None,
            resume_session: None,
            continue_latest: false,
            fork_session: None,
            session_id: None,
            debug: false,
            enable_skills: false,                // Skills disabled by default
            tick_rate: 250,                      // Default tick rate: 250ms
            frame_rate: 60,                      // Default frame rate: 60 FPS
            agent: None,                         // No agent override by default
            additional_dirs: Vec::new(),         // No additional directories by default
            allowed_tools: Vec::new(),           // No tool restrictions by default
            disallowed_tools: Vec::new(),        // No tool restrictions by default
            dangerously_skip_permissions: false, // Safety confirmations enabled by default
            ide: false,                          // No auto IDE connection by default
            permission_mode: None,               // Use config permission mode by default
            chrome: false,                       // Chrome integration disabled by default
            no_chrome: false,                    // Chrome integration not explicitly disabled
            command: Some(Commands::Chat),
        }
    }
}

impl Cli {
    /// Get the model to use, with fallback to default
    pub fn get_model(&self) -> String {
        self.model
            .clone()
            .unwrap_or_else(|| ModelId::default().to_string())
    }

    /// Load configuration from a simple TOML-like file without external deps
    ///
    /// Supported keys (top-level): model, api_key_env, verbose, log_level, workspace
    /// Example:
    ///   model = "gemini-3-flash-preview"
    ///   api_key_env = "GEMINI_API_KEY"
    ///   verbose = true
    ///   log_level = "info"
    ///   workspace = "/path/to/workspace"
    pub async fn load_config(&self) -> Result<ConfigFile, Box<dyn std::error::Error>> {
        use std::path::Path;
        use tokio::fs;

        // Resolve candidate path
        let explicit_path = self.config.iter().find_map(|entry| {
            let trimmed = entry.trim();
            if trimmed.contains('=') || trimmed.is_empty() {
                None
            } else {
                Some(PathBuf::from(trimmed))
            }
        });

        let path = if let Some(p) = explicit_path {
            p
        } else {
            let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            let primary = cwd.join("vtcode.toml");
            let secondary = cwd.join(".vtcode.toml");
            if fs::try_exists(&primary).await.unwrap_or(false) {
                primary
            } else if fs::try_exists(&secondary).await.unwrap_or(false) {
                secondary
            } else {
                // No config file; return empty config
                return Ok(ConfigFile {
                    model: None,
                    provider: None,
                    api_key_env: None,
                    verbose: None,
                    log_level: None,
                    workspace: None,
                    tools: None,
                    context: None,
                    logging: None,
                    performance: None,
                    security: None,
                });
            }
        };

        let text = fs::read_to_string(&path).await?;

        // Very small parser: key = value, supports quoted strings, booleans, and plain paths
        let mut cfg = ConfigFile {
            model: None,
            provider: None,
            api_key_env: None,
            verbose: None,
            log_level: None,
            workspace: None,
            tools: None,
            context: None,
            logging: None,
            performance: None,
            security: None,
        };

        for raw_line in text.lines() {
            let line = raw_line.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with("//") {
                continue;
            }
            // Strip inline comments after '#'
            let line = match line.find('#') {
                Some(idx) => &line[..idx],
                None => line,
            }
            .trim();

            // Expect key = value
            let mut parts = line.splitn(2, '=');
            let key = parts.next().map(|s| s.trim()).unwrap_or("");
            let val = parts.next().map(|s| s.trim()).unwrap_or("");
            if key.is_empty() || val.is_empty() {
                continue;
            }

            // Remove surrounding quotes if present
            let unquote = |s: &str| -> String {
                let s = s.trim();
                if (s.starts_with('"') && s.ends_with('"'))
                    || (s.starts_with('\'') && s.ends_with('\''))
                {
                    s[1..s.len() - 1].to_owned()
                } else {
                    s.to_owned()
                }
            };

            match key {
                "model" => cfg.model = Some(unquote(val)),
                "api_key_env" => cfg.api_key_env = Some(unquote(val)),
                "verbose" => {
                    let v = unquote(val).to_lowercase();
                    cfg.verbose = Some(matches!(v.as_str(), "true" | "1" | "yes"));
                }
                "log_level" => cfg.log_level = Some(unquote(val)),
                "workspace" => {
                    let v = unquote(val);
                    let p = if Path::new(&v).is_absolute() {
                        PathBuf::from(v)
                    } else {
                        // Resolve relative to config file directory
                        let base = path.parent().unwrap_or(Path::new("."));
                        base.join(v)
                    };
                    cfg.workspace = Some(p);
                }
                _ => {
                    // Ignore unknown keys in this minimal parser
                }
            }
        }

        Ok(cfg)
    }

    /// Get the effective workspace path
    pub fn get_workspace(&self) -> std::path::PathBuf {
        self.workspace
            .clone()
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
    }

    /// Get the effective API key environment variable
    ///
    /// Automatically infers the API key environment variable based on the provider
    /// when the current value matches the default or is not explicitly set.
    pub fn get_api_key_env(&self) -> String {
        // If api_key_env is the default or empty, infer from provider
        if self.api_key_env == crate::config::constants::defaults::DEFAULT_API_KEY_ENV
            || self.api_key_env.is_empty()
        {
            let provider = self
                .provider
                .as_deref()
                .unwrap_or(crate::config::constants::defaults::DEFAULT_PROVIDER);
            let provider_key = provider.to_ascii_lowercase();

            match provider_key.as_str() {
                "openai" => "OPENAI_API_KEY".to_owned(),
                "anthropic" => "ANTHROPIC_API_KEY".to_owned(),
                "gemini" => "GEMINI_API_KEY".to_owned(),
                "deepseek" => "DEEPSEEK_API_KEY".to_owned(),
                "openrouter" => "OPENROUTER_API_KEY".to_owned(),
                "zai" => "ZAI_API_KEY".to_owned(),
                "minimax" => "ANTHROPIC_API_KEY".to_owned(),
                _ => crate::config::constants::defaults::DEFAULT_API_KEY_ENV.to_owned(),
            }
        } else {
            self.api_key_env.clone()
        }
    }

    /// Check if verbose mode is enabled
    pub fn is_verbose(&self) -> bool {
        self.verbose
    }

    /// Check if performance monitoring is enabled
    /// Check if research-preview features are enabled
    pub fn is_research_preview_enabled(&self) -> bool {
        self.research_preview
    }

    /// Get the security level
    pub fn get_security_level(&self) -> &str {
        &self.security_level
    }

    /// Check if debug mode is enabled (includes verbose)
    pub fn is_debug_mode(&self) -> bool {
        self.debug || self.verbose
    }
}
