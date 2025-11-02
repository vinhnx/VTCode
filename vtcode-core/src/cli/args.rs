//! CLI argument parsing and configuration

use crate::config::models::ModelId;
use clap::{ArgAction, ColorChoice, Parser, Subcommand, ValueEnum, ValueHint};
use colorchoice_clap::Color as ColorSelection;
use std::path::PathBuf;

/// Main CLI structure for vtcode with advanced features
#[derive(Parser, Debug)]
#[command(
    name = "vtcode",
    version,
    about = "Advanced coding agent with Decision Ledger\n\nFeatures:\n• Single-agent architecture with Decision Ledger for reliable task execution\n• Tree-sitter powered code analysis (Rust, Python, JavaScript, TypeScript, Go, Java)\n• Multi-provider LLM support (Gemini, OpenAI, Anthropic, DeepSeek, xAI, Z.AI, Moonshot AI, OpenRouter, Ollama)
• Prompt caching and token-efficient context management\n• Real-time performance monitoring and benchmarking\n• Enhanced security with tool policies and sandboxing\n• Research-preview context management and conversation compression\n\nQuick Start:\n  export GEMINI_API_KEY=\"your_key\"\n  vtcode chat",
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

    /// LLM Model ID with latest model support
    ///
    /// Available providers & models:
    ///   • gemini-2.5-flash-preview-05-20 - Latest fast Gemini model (default)
    ///   • gemini-2.5-pro - Latest, most capable Gemini model
    ///   • gpt-5 - OpenAI's latest
    ///   • gpt-5-codex - OpenAI's latest coding-focused model
    ///   • claude-sonnet-4-5 - Anthropic's latest Sonnet model
    ///   • claude-opus-4-1-20250805 - Anthropic's latest Opus model
    ///   • deepseek-reasoner - DeepSeek reasoning model
    ///   • deepseek-chat - DeepSeek chat model
    ///   • grok-4 - xAI Grok flagship model
    ///   • grok-4-code - xAI Grok coding-specific model
    ///   • glm-4.6 - Z.AI GLM 4.6 model
    ///   • kimi-k2-turbo-preview - Moonshot AI Kimi K2 Turbo
    ///   • x-ai/grok-code-fast-1 - OpenRouter Grok fast coding model
    ///   • x-ai/grok-4 - OpenRouter xAI Grok 4
    ///   • qwen/qwen3-coder - OpenRouter Qwen3 Coder optimized for IDE usage
    ///   • qwen/qwen3-coder-plus - OpenRouter Qwen3 Coder Plus
    ///   • qwen/qwen3-235b-a22b - OpenRouter Qwen3 235B model
    ///   • openai/gpt-oss-20b - OpenRouter OSS 20B model
    ///   • z-ai/glm-4.6 - OpenRouter Z.AI GLM 4.6
    ///   • moonshotai/kimi-k2-0905 - OpenRouter MoonshotAI Kimi K2
    ///   • gpt-oss:20b - Ollama local model
    #[arg(long, global = true)]
    pub model: Option<String>,

    /// **LLM Provider** with expanded support
    ///
    /// Available providers:
    ///   • gemini - Google Gemini (default)
    ///   • openai - OpenAI GPT models
    ///   • anthropic - Anthropic Claude models
    ///   • deepseek - DeepSeek models
    ///   • openrouter - OpenRouter marketplace models
    ///   • xai - xAI Grok models
    ///   • zai - Z.AI GLM models
    ///   • moonshot - Moonshot AI Kimi models
    ///   • ollama - Local Ollama server (default gpt-oss:20b)
    ///   • lmstudio - Local LM Studio server (OpenAI-compatible)
    ///
    /// Example: --provider deepseek
    #[arg(long, global = true)]
    pub provider: Option<String>,

    /// **API key environment variable**\n\n**Auto-detects based on provider:**\n• Gemini: `GEMINI_API_KEY`\n• OpenAI: `OPENAI_API_KEY`\n• Anthropic: `ANTHROPIC_API_KEY`\n• DeepSeek: `DEEPSEEK_API_KEY`\n• OpenRouter: `OPENROUTER_API_KEY`\n• xAI: `XAI_API_KEY`\n• Z.AI: `ZAI_API_KEY`\n• Ollama: (not required)\n• LM Studio: (optional `LMSTUDIO_API_KEY`)\n\n**Override:** --api-key-env CUSTOM_KEY
    #[arg(long, global = true, default_value = crate::config::constants::defaults::DEFAULT_API_KEY_ENV)]
    pub api_key_env: String,

    /// **Workspace root directory for file operations**
    ///
    /// Security: All file operations restricted to this path
    /// Default: Current directory
    #[arg(
        long,
        global = true,
        alias = "workspace-dir",
        value_name = "PATH",
        value_hint = ValueHint::DirPath
    )]
    pub workspace: Option<PathBuf>,

    /// **Enable tree-sitter code analysis**
    ///
    /// Features:
    ///   • AST-based code parsing
    ///   • Symbol extraction and navigation
    ///   • Intelligent refactoring suggestions
    ///   • Multi-language support (Rust, Python, JS, TS, Go, Java)
    #[arg(long, global = true)]
    pub enable_tree_sitter: bool,

    /// **Enable research-preview features**
    ///
    /// Includes:
    ///   • Advanced context compression
    ///   • Conversation summarization
    ///   • Enhanced error recovery
    ///   • Decision transparency tracking
    #[arg(long, global = true)]
    pub research_preview: bool,

    /// **Security level** for tool execution
    ///
    /// Options:
    ///   • strict - Maximum security, prompt for all tools
    ///   • moderate - Balance security and usability
    ///   • permissive - Minimal restrictions (not recommended)
    #[arg(long, global = true, default_value = "moderate")]
    pub security_level: String,

    /// **Show diffs for file changes in chat interface**
    ///
    /// Features:
    ///   • Real-time diff rendering
    ///   • Syntax highlighting
    ///   • Line-by-line changes
    ///   • Before/after comparison
    #[arg(long, global = true)]
    pub show_file_diffs: bool,

    /// **Maximum concurrent async operations**
    ///
    /// Default: 5
    /// Higher values: Better performance but more resource usage
    #[arg(long, global = true, default_value_t = 5)]
    pub max_concurrent_ops: usize,

    /// **Maximum API requests per minute**
    ///
    /// Default: 30
    /// Purpose: Prevents rate limiting
    #[arg(long, global = true, default_value_t = 30)]
    pub api_rate_limit: usize,

    /// **Maximum tool calls per session**
    ///
    /// Default: 10
    /// Purpose: Prevents runaway execution
    #[arg(long, global = true, default_value_t = 10)]
    pub max_tool_calls: usize,

    /// **Enable debug output for troubleshooting**
    ///
    /// Shows:
    ///   • Tool call details
    ///   • API request/response
    ///   • Internal agent state
    ///   • Performance metrics
    #[arg(long, global = true)]
    pub debug: bool,

    /// **Enable verbose logging**
    ///
    /// Includes:
    ///   • Detailed operation logs
    ///   • Context management info
    ///   • Agent coordination details
    #[arg(long, global = true)]
    pub verbose: bool,

    /// **Configuration overrides or file path**
    ///
    /// Mirrors Codex CLI's `--config key=value` behaviour while still accepting
    /// an explicit config path.
    /// Use repeated flags to apply multiple overrides (highest precedence).
    #[arg(
        short = 'c',
        long = "config",
        value_name = "KEY=VALUE|PATH",
        action = ArgAction::Append,
        global = true
    )]
    pub config: Vec<String>,

    /// Log level (error, warn, info, debug, trace)
    ///
    /// Default: info
    #[arg(long, global = true, default_value = "info")]
    pub log_level: String,

    /// Disable color output
    ///
    /// Useful for: Log files, CI/CD pipelines
    #[arg(long, global = true)]
    pub no_color: bool,

    /// Select UI theme for ANSI styling (e.g., ciapre-dark, ciapre-blue)
    #[arg(long, global = true, value_name = "THEME")]
    pub theme: Option<String>,

    /// **Skip safety confirmations**
    ///
    /// Warning: Reduces security, use with caution
    #[arg(long, global = true)]
    pub skip_confirmations: bool,

    /// **Print response without launching the interactive TUI**
    ///
    /// Equivalent to `claude -p` style single prompt mode.
    ///
    /// Behaviour:
    ///   • Provide a prompt inline: `vtcode -p "Explain this function"`
    ///   • Pipe content and add a question: `cat file.rs | vtcode -p "Summarize"`
    ///   • Pipe content only: `cat plan.md | vtcode -p`
    ///
    /// When both piped content and an inline prompt are supplied, the piped
    /// content is prepended to the inline prompt separated by a blank line.
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

    /// **Enable full-auto mode (no interaction) or run a headless task**
    ///
    /// Use without a value to launch the interactive UI in full-auto mode.
    /// Provide a prompt value to execute a single autonomous task headlessly.
    /// The alias `--auto` is provided for ergonomic scripting.
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

    /// **Resume a previous conversation**
    ///
    /// Use without an ID to open an interactive picker of recent sessions.
    /// Provide a session identifier (from `/sessions` or `vtcode --resume`) to resume directly.
    /// Shortcut: `-r`.
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

    /// **Continue the most recent conversation automatically**
    ///
    /// Equivalent to `--resume` with the newest session identifier.
    #[arg(
        long = "continue",
        visible_alias = "continue-session",
        global = true,
        conflicts_with_all = ["resume_session", "full_auto"]
    )]
    pub continue_latest: bool,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

/// Output format options for the `ask` subcommand.
#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
pub enum AskOutputFormat {
    /// Emit the response as a structured JSON document.
    Json,
}

/// Available commands with comprehensive features
#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Start Agent Client Protocol bridge for IDE integrations
    #[command(name = "acp")]
    AgentClientProtocol {
        /// Client to connect over ACP
        #[arg(value_enum, default_value_t = AgentClientProtocolTarget::Zed)]
        target: AgentClientProtocolTarget,
    },

    /// **Interactive AI coding assistant** with advanced capabilities
    ///
    /// Features:
    ///   • Real-time code generation and editing
    ///   • Tree-sitter powered analysis
    ///   • Research-preview context management
    ///
    /// Usage: vtcode chat
    Chat,

    /// **Single prompt mode** - prints model reply without tools
    ///
    /// Perfect for:
    ///   • Quick questions
    ///   • Code explanations
    ///   • Simple queries
    ///
    /// Example: vtcode ask "Explain Rust ownership"
    Ask {
        prompt: String,
        /// Format the response using a structured representation.
        #[arg(long = "output-format", value_enum, value_name = "FORMAT")]
        output_format: Option<AskOutputFormat>,
    },
    /// **Headless execution mode** mirroring Codex exec semantics
    ///
    /// Features:
    ///   • Autonomous run using workspace automation settings
    ///   • Optional JSONL output stream for tool integrations
    ///   • Support for writing the final message to disk
    ///
    /// Prompt handling:
    ///   • Positional argument or `-` to read from stdin
    ///   • When omitted and stdin is a TTY, the command exits with an error
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

    /// **Verbose interactive chat** with enhanced transparency
    ///
    /// Shows:
    ///   • Tool execution details
    ///   • API request/response
    ///   • Performance metrics
    ///
    /// Usage: vtcode chat-verbose
    ChatVerbose,

    /// **Analyze workspace** with tree-sitter integration
    ///
    /// Provides:
    ///   • Project structure analysis
    ///   • Language detection
    ///   • Code complexity metrics
    ///   • Dependency insights
    ///   • Symbol extraction
    ///
    /// Usage: vtcode analyze
    Analyze,

    /// Pretty-print trajectory logs and show basic analytics
    ///
    /// Sources:
    ///   • .vtcode/logs/trajectory.jsonl (default)
    /// Options:
    ///   • --file to specify an alternate path
    ///   • --top to limit report rows (default: 10)
    ///
    /// Shows:
    ///   • Class distribution with percentages
    ///   • Model usage statistics
    ///   • Tool success rates with status indicators
    ///   • Time range of logged activity
    #[command(name = "trajectory")]
    Trajectory {
        /// Optional path to trajectory JSONL file
        #[arg(long)]
        file: Option<std::path::PathBuf>,
        /// Number of top entries to show for each section
        #[arg(long, default_value_t = 10)]
        top: usize,
    },

    /// **Benchmark against SWE-bench evaluation framework**
    ///
    /// Features:
    ///   • Automated performance testing
    ///   • Comparative analysis across models
    ///   • Benchmark scoring and metrics
    ///   • Optimization insights
    ///
    /// Usage: vtcode benchmark --task-file swe_task.json --output reports/result.json
    Benchmark {
        /// Path to a JSON benchmark specification. Falls back to STDIN when omitted.
        #[arg(long, value_name = "PATH", value_hint = ValueHint::FilePath)]
        task_file: Option<PathBuf>,
        /// Inline JSON specification for quick experiments.
        #[arg(long, value_name = "JSON")]
        task: Option<String>,
        /// Optional path to write the structured benchmark report.
        #[arg(long, value_name = "PATH", value_hint = ValueHint::FilePath)]
        output: Option<PathBuf>,
        /// Limit the number of tasks executed from the specification.
        #[arg(long, value_name = "COUNT")]
        max_tasks: Option<usize>,
    },

    /// **Create complete Rust project with advanced features**
    ///
    /// Features:
    ///   • Web frameworks (Axum, Rocket, Warp)
    ///   • Database integration
    ///   • Authentication systems
    ///   • Testing setup
    ///   • Tree-sitter integration
    ///
    /// Example: vtcode create-project myapp web,auth,db
    CreateProject { name: String, features: Vec<String> },

    /// **Compress conversation context** for long-running sessions
    ///
    /// Benefits:
    ///   • Reduced token usage
    ///   • Faster responses
    ///   • Memory optimization
    ///   • Context preservation
    ///
    /// Usage: vtcode compress-context
    CompressContext,

    /// **Revert agent to a previous snapshot
    ///
    /// Features:
    ///   • Revert to any previous turn
    ///   • Partial reverts (conversation, code, full)
    ///   • Safe rollback with validation
    ///
    /// Examples:
    ///   vtcode revert --turn 5
    ///   vtcode revert --turn 3 --partial memory
    Revert {
        /// Turn number to revert to
        ///
        /// Required: Yes
        /// Example: 5
        #[arg(short, long)]
        turn: usize,

        /// Scope of revert operation
        ///
        /// Options: conversation, code, full
        /// Default: full
        /// Examples:
        ///   --partial conversation (revert chat history only)
        ///   --partial code (revert code changes only)
        #[arg(short, long)]
        partial: Option<String>,
    },

    /// **List all available snapshots**
    ///
    /// Shows:
    ///   • Snapshot ID and turn number
    ///   • Creation timestamp
    ///   • Description
    ///   • File size and compression status
    ///
    /// Usage: vtcode snapshots
    Snapshots,

    /// **Clean up old snapshots**
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

    /// **Initialize project** with enhanced dot-folder structure
    ///
    /// Features:
    ///   • Creates project directory structure
    ///   • Sets up config, cache, embeddings directories
    ///   • Creates .project metadata file
    ///   • Tree-sitter parser setup
    ///
    /// Usage: vtcode init
    Init,

    /// **Initialize project with dot-folder structure** - sets up `~/.vtcode/projects/<project-name>` structure
    ///
    /// Features:
    ///   • Creates project directory structure in ~/.vtcode/projects/
    ///   • Sets up config, cache, embeddings, and retrieval directories
    ///   • Creates .project metadata file
    ///   • Migrates existing config/cache files with user confirmation
    ///
    /// Examples:
    ///   vtcode init-project
    ///   vtcode init-project --name my-project
    ///   vtcode init-project --force
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

    /// **Generate configuration file - creates a vtcode.toml configuration file
    ///
    /// Features:
    ///   • Generate default configuration
    ///   • Support for global (home directory) and local configuration
    ///   • TOML format with comprehensive settings
    ///   • Tree-sitter and performance monitoring settings
    ///
    /// Examples:
    ///   vtcode config
    ///   vtcode config --output ./custom-config.toml
    ///   vtcode config --global
    Config {
        /// Output file path - where to save the configuration file
        #[arg(long)]
        output: Option<std::path::PathBuf>,

        /// Create in user home directory - creates ~/.vtcode/vtcode.toml
        #[arg(long)]
        global: bool,
    },

    /// **Manage tool execution policies** - control which tools the agent can use
    ///
    /// Features:
    ///   • Granular tool permissions
    ///   • Security level presets
    ///   • Audit logging
    ///   • Safe tool execution
    ///
    /// Examples:
    ///   vtcode tool-policy status
    ///   vtcode tool-policy allow file-write
    ///   vtcode tool-policy deny shell-exec
    #[command(name = "tool-policy")]
    ToolPolicy {
        #[command(subcommand)]
        command: crate::cli::tool_policy_commands::ToolPolicyCommands,
    },

    /// **Manage Model Context Protocol providers**
    ///
    /// Features:
    ///   • Add stdio or HTTP MCP providers
    ///   • Inspect configuration details
    ///   • Remove providers from the global vtcode config
    ///
    /// Examples:
    ///   vtcode mcp list --json
    ///   vtcode mcp add context7 --url https://example
    #[command(name = "mcp")]
    Mcp {
        #[command(subcommand)]
        command: crate::cli::mcp_commands::McpCommands,
    },

    /// **Manage models and providers** - configure and switch between LLM providers\n\n**Features:**\n• Support for latest models (DeepSeek, etc.)\n• Provider configuration and testing\n• Model performance comparison\n• API key management\n\n**Examples:**\n  vtcode models list\n  vtcode models set-provider deepseek\n  vtcode models set-model deepseek-reasoner
    Models {
        #[command(subcommand)]
        command: ModelCommands,
    },

    /// **Security and safety management**\n\n**Features:**\n• Security scanning and vulnerability detection\n• Audit logging and monitoring\n• Access control management\n• Privacy protection settings\n\n**Usage:** vtcode security
    Security,

    /// **Tree-sitter code analysis tools**\n\n**Features:**\n• AST-based code parsing\n• Symbol extraction and navigation\n• Code complexity analysis\n• Multi-language refactoring\n\n**Usage:** vtcode tree-sitter
    #[command(name = "tree-sitter")]
    TreeSitter,

    /// **Generate or display man pages** for VTCode commands\n\n**Features:**\n• Generate Unix man pages for all commands\n• Display detailed command documentation\n• Save man pages to files\n• Comprehensive help for all VTCode features\n\n**Examples:**\n  vtcode man\n  vtcode man chat\n  vtcode man chat --output chat.1
    Man {
        /// **Command name** to generate man page for (optional)\n\n**Available commands:**\n• chat, ask, analyze, performance, benchmark\n• create-project, init, man\n\n**If not specified, shows main VTCode man page**
        command: Option<String>,

        /// **Output file path** to save man page\n\n**Format:** Standard Unix man page format (.1, .8, etc.)\n**Default:** Display to stdout
        #[arg(short, long)]
        output: Option<std::path::PathBuf>,
    },
}

/// Supported Agent Client Protocol clients
#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum AgentClientProtocolTarget {
    /// Agent Client Protocol client
    Zed,
}

/// Model management commands with concise, actionable help
#[derive(Subcommand, Debug)]
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
    pub tree_sitter: Option<TreeSitterConfig>,
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
    pub compression_threshold: Option<usize>,
    pub summarization_interval: Option<usize>,
}

/// Logging configuration
#[derive(Debug, serde::Deserialize)]
pub struct LoggingConfig {
    pub file_logging: Option<bool>,
    pub log_directory: Option<String>,
    pub max_log_files: Option<usize>,
    pub max_log_size_mb: Option<usize>,
}

/// Tree-sitter configuration
#[derive(Debug, serde::Deserialize)]
pub struct TreeSitterConfig {
    pub enabled: Option<bool>,
    pub supported_languages: Option<Vec<String>>,
    pub max_file_size_kb: Option<usize>,
    pub enable_symbol_extraction: Option<bool>,
    pub enable_complexity_analysis: Option<bool>,
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
            model: Some(ModelId::default().as_str().to_string()),
            provider: Some("gemini".to_string()),
            api_key_env: "GEMINI_API_KEY".to_string(),
            workspace: None,
            enable_tree_sitter: false,
            research_preview: false,
            security_level: "moderate".to_string(),
            show_file_diffs: false,
            max_concurrent_ops: 5,
            api_rate_limit: 30,
            max_tool_calls: 10,
            verbose: false,
            config: Vec::new(),
            log_level: "info".to_string(),
            no_color: false,
            theme: None,
            skip_confirmations: false,
            print: None,
            full_auto: None,
            resume_session: None,
            continue_latest: false,
            debug: false,
            command: Some(Commands::Chat),
        }
    }
}

impl Cli {
    /// Get the model to use, with fallback to default
    pub fn get_model(&self) -> String {
        self.model
            .clone()
            .unwrap_or_else(|| ModelId::default().as_str().to_string())
    }

    /// Load configuration from a simple TOML-like file without external deps
    ///
    /// Supported keys (top-level): model, api_key_env, verbose, log_level, workspace
    /// Example:
    ///   model = "gemini-2.5-flash-preview-05-20"
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
            if trimmed.contains('=') {
                None
            } else if trimmed.is_empty() {
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
                    tree_sitter: None,
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
            tree_sitter: None,
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
                    s[1..s.len() - 1].to_string()
                } else {
                    s.to_string()
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
                "openai" => "OPENAI_API_KEY".to_string(),
                "anthropic" => "ANTHROPIC_API_KEY".to_string(),
                "gemini" => "GEMINI_API_KEY".to_string(),
                "deepseek" => "DEEPSEEK_API_KEY".to_string(),
                "openrouter" => "OPENROUTER_API_KEY".to_string(),
                "xai" => "XAI_API_KEY".to_string(),
                "zai" => "ZAI_API_KEY".to_string(),
                "minimax" => "ANTHROPIC_API_KEY".to_string(),
                _ => crate::config::constants::defaults::DEFAULT_API_KEY_ENV.to_string(),
            }
        } else {
            self.api_key_env.clone()
        }
    }

    /// Check if verbose mode is enabled
    pub fn is_verbose(&self) -> bool {
        self.verbose
    }

    /// Check if tree-sitter analysis is enabled
    pub fn is_tree_sitter_enabled(&self) -> bool {
        self.enable_tree_sitter
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
