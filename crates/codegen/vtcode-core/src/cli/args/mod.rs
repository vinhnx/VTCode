use clap::{ArgAction, ColorChoice, Parser, Subcommand, ValueHint};
use colorchoice_clap::Color as ColorSelection;
use std::env;
use std::path::{Path, PathBuf};

use crate::config::models::ModelId;

mod acp;
mod ask;
mod background;
mod bench;
mod check;
mod config;
mod dependencies;
mod exec;
mod models;
mod pods;
mod review;
mod schedule;
mod schema;
mod secret;
mod session_store;
mod skills;

pub use acp::AgentClientProtocolTarget;
pub use ask::{AskCommandOptions, AskOutputFormat};
pub use background::BackgroundSubagentArgs;
pub use bench::BenchAllocatorArgs;
pub use check::CheckSubcommand;
pub use config::{ConfigFile, ContextConfig, LoggingConfig, PerformanceConfig, SecurityConfig, ToolConfig};
pub use dependencies::{DependenciesSubcommand, ManagedDependency};
pub use exec::{ExecEvalArgs, ExecResumeArgs, ExecSubcommand};
pub use models::ModelCommands;
pub use pods::PodsCommands;
pub use review::ReviewArgs;
pub use schedule::{ScheduleCreateArgs, ScheduleSubcommand};
pub use schema::{SchemaCommands, SchemaMode, SchemaOutputFormat};
pub use secret::{MigrateArgs, SecretArgs, SecretProvider, SecretSubcommand};
pub use session_store::SessionStoreCommand;
pub use skills::{SkillsRefSubcommand, SkillsSubcommand};

pub const PLANNING_WORKFLOW_READ_ONLY_HEADER: &str = "# PLANNING WORKFLOW (READ-ONLY)";

pub const PLANNING_WORKFLOW_READ_ONLY_NOTICE_LINE: &str = "Mutating file edits are blocked, including `apply_patch`. Use `exec_command.cmd` only for read-only repository inspection with the active shell profile's syntax; keep `task_tracker` current. Plan artifacts under `.vtcode/plans/` are allowed.";

pub const PLANNING_WORKFLOW_EXIT_INSTRUCTION_LINE: &str =
    "The user approved the plan. Stop planning and switch to implementation.";

pub const PLANNING_WORKFLOW_PLAN_QUALITY_LINE: &str = "Keep plans compact and spec-like. Emit ONE `<proposed_plan>` that fits ~1500 tokens: a 1-3 line Summary; a tight numbered step list where each step is `Action -> files/symbols -> verify:`; one Validation line (build/lint + test commands); Assumptions as short bullets. Prefer file:symbol references over prose, written as plain text or inline code (e.g. `src/main.rs:42`) — never as markdown links or editor/IDE URIs (no `[label](url)`, no `vscode-file://`/`file://` schemes). Ask only material blocking questions; unresolved: `Next open decision: ...`.";

pub const PLANNING_WORKFLOW_RESEARCH_SCOPE_LINE: &str = "Scale research to the request: for a narrow or simple ask, ~5-10 targeted reads/searches is usually enough before drafting `<proposed_plan>` — do not exhaustively enumerate the whole repository. For a broad or ambiguous ask, research proportionally more, but stop and draft as soon as scope/decomposition/verification decisions are closed.";

pub const PLANNING_WORKFLOW_INTERVIEW_POLICY_LINE: &str = "Use `request_user_input` for interview questions informed by repo context. Continue until scope/decomposition/verification decisions are closed before finalizing `<proposed_plan>`.";

pub const PLANNING_WORKFLOW_NO_REQUEST_USER_INPUT_POLICY_LINE: &str = "`request_user_input` unavailable here. Continue exploring read-only, finish unblocked planning, surface blockers in plain text.";

pub const PLANNING_WORKFLOW_NO_AUTO_EXIT_LINE: &str = "Do not auto-exit planning workflow. Present `<proposed_plan>` and wait for explicit user approval (\"implement\", \"looks good\", \"ship it\", etc.) before switching to implementation.";

pub const PLANNING_WORKFLOW_IMPLEMENTATION_PROMPT: &str = "Implement the approved plan.";

pub const PLANNING_WORKFLOW_HINT: &str = "Present the plan for approval; do not start implementing yet.";

pub const PLANNING_WORKFLOW_TASK_TRACKER_LINE: &str = "`task_tracker` remains available while planning.";

pub const PLANNING_WORKFLOW_IMPLEMENT_REMINDER: &str = "• Planning workflow is active with read-only permissions. Say “implement” to present the plan for user approval, or “stay in planning workflow” to revise. The plan will be shown to the user for approval; mutating tools stay disabled until the user approves the plan. If a write tool is unavailable because planning workflow is active, do not emit the full artifact content in the chat. Instead, summarize the blocker briefly and ask the user to save the content.";

#[derive(Parser, Debug, Clone)]
pub struct Cli {
    /// Color output selection (auto, always, never)
    #[command(flatten)]
    pub color: ColorSelection,

    /// Optional positional path to run vtcode against a different workspace
    #[arg(
        value_name = "WORKSPACE",
        value_hint = ValueHint::DirPath,
        value_parser = parse_workspace_directory,
        global = true
    )]
    pub workspace_path: Option<PathBuf>,

    /// LLM Model ID (e.g., gpt-5, claude-sonnet-4-6, gemini-3-flash-preview)
    #[arg(long, global = true)]
    pub model: Option<String>,

    /// LLM Provider (gemini, openai, anthropic, deepseek, openrouter, codex, zai, moonshot, minimax, ollama, lmstudio)
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
        value_hint = ValueHint::DirPath,
        value_parser = parse_workspace_directory
    )]
    pub workspace: Option<PathBuf>,

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

    /// Disable color output (equivalent to `--color never`)
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

    /// Enable experimental Codex app-server features for this run
    #[arg(long = "codex-experimental", global = true, conflicts_with = "no_codex_experimental")]
    pub codex_experimental: bool,

    /// Disable experimental Codex app-server features for this run
    #[arg(long = "no-codex-experimental", global = true, conflicts_with = "codex_experimental")]
    pub no_codex_experimental: bool,

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

    /// Run non-interactively with full-auto permission review
    #[arg(
        long = "full-auto",
        global = true,
        help = "Run non-interactively with full-auto permission review",
        long_help = r#"Run non-interactively on top of the active primary agent.

If no primary agent is explicitly selected or configured, VT Code selects the effective `auto` primary agent. Explicit choices, including `duck`, are honoured. Full-auto is an execution and permission layer, not a primary agent.

Full-auto does not override explicit denies or grant tools outside `[automation.full_auto].allowed_tools`. Tools outside that allow-list are denied. Promptable actions inside the allow-list are routed through automatic permission review after deny and policy checks instead of asking. The run fails fast if it needs the defaulted `auto` primary agent and no effective `auto` exists."#,
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

    /// Show archived sessions from every workspace when resuming or forking
    #[arg(long, global = true)]
    pub all: bool,

    /// Custom suffix for session identifier (alphanumeric, dash, underscore only, max 64 chars)
    #[arg(long = "session-id", global = true, value_name = "CUSTOM_SUFFIX")]
    pub session_id: Option<String>,

    /// Use summarized history when forking a session
    #[arg(long, global = true)]
    pub summarize: bool,

    /// Override the default agent model for this session
    #[arg(long, global = true, value_name = "AGENT")]
    pub agent: Option<String>,

    /// Tools that execute without prompting (comma-separated, supports patterns like "Bash(git:*)")
    #[arg(long = "allowed-tools", global = true, value_name = "TOOLS", action = ArgAction::Append)]
    pub allowed_tools: Vec<String>,

    /// Tools that cannot be used by the agent
    #[arg(long = "disallowed-tools", global = true, value_name = "TOOLS", action = ArgAction::Append)]
    pub disallowed_tools: Vec<String>,

    /// Auto-approve promptable actions while respecting denies and policy blocks
    #[arg(
        long = "dangerously-skip-permissions",
        global = true,
        help = "Auto-approve promptable actions while respecting denies and policy blocks",
        long_help = "Auto-approve promptable actions while still respecting explicit denies and policy blocks."
    )]
    pub dangerously_skip_permissions: bool,

    /// Explicitly connect to IDE on startup (auto-detects available IDEs)
    #[arg(long, global = true)]
    pub ide: bool,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand, Debug, Clone)]
pub enum Commands {
    /// Start Agent Client Protocol bridge for IDE integrations
    #[command(name = "acp")]
    AgentClientProtocol {
        /// Client to connect over ACP
        #[arg(value_enum, default_value_t = AgentClientProtocolTarget::Zed)]
        target: AgentClientProtocolTarget,
    },

    /// Unified per-session state store (single source of truth for state,
    /// context, and history). Consolidates the legacy `checkpoints/`, `logs/`,
    /// and `history/` stores.
    #[command(name = "session-store")]
    SessionStore {
        #[command(subcommand)]
        command: SessionStoreCommand,
    },

    /// Interactive AI coding assistant
    Chat,

    /// Resume the most recent conversation automatically
    ///
    /// Equivalent to `vtcode --continue`. Loads the latest archived session in
    /// the current workspace (or across all workspaces with `--all`) and resumes
    /// it without showing the interactive picker.
    ///
    /// Examples:
    ///   vtcode continue
    ///   vtcode continue --all
    ///   vtcode continue --session-id my-fork   # fork latest into a new session
    Continue,

    /// Single prompt mode - prints model reply without tools
    ///
    /// Send a single prompt to the model and print the response. No tools are
    /// invoked, no session is created, and the process exits after replying.
    ///
    /// Examples:
    ///   vtcode ask "what is a monad?"
    ///   echo "summarize this" | vtcode ask
    ///   vtcode ask --output-format json "explain ownership in Rust"
    Ask {
        /// Prompt to ask. Use `-` to force reading from stdin.
        #[arg(
            value_name = "PROMPT",
            long_help = "The prompt to send to the model.\n\nOmit to read from stdin (piped input).\nUse '-' to explicitly force reading from stdin."
        )]
        prompt: Option<String>,
        /// Format the response using a structured representation.
        #[arg(
            long = "output-format",
            value_enum,
            value_name = "FORMAT",
            long_help = "Output format for the response.\n\nCurrently supports:\n  json - Emit the response as a structured JSON document."
        )]
        output_format: Option<AskOutputFormat>,
    },
    /// Headless execution mode
    ///
    /// Run the agent in non-interactive mode. The agent executes the prompt,
    /// runs tools, and exits when done. Ideal for CI/CD, scripting, and
    /// agent-to-agent workflows.
    ///
    /// Examples:
    ///   vtcode exec "explain this codebase"
    ///   vtcode exec --json "fix the failing test"
    ///   vtcode exec --dry-run "refactor auth module"
    ///   cat file.rs | vtcode exec "review this code"
    ///   vtcode exec resume --last
    Exec {
        /// Emit structured JSON events to stdout (one per line)
        #[arg(
            long,
            long_help = "Stream newline-delimited JSON events to stdout.\nEach line is a JSON object representing an agent event (tool call, message, etc.).\nUseful for programmatic consumption and CI integration."
        )]
        json: bool,
        /// Run a read-only dry-run execution (blocks mutating tool calls)
        #[arg(
            long,
            long_help = "Simulate execution without making changes.\nThe agent plans tool calls but does not execute mutating operations (file writes, shell commands).\nUseful for previewing what the agent would do."
        )]
        dry_run: bool,
        /// Optional path to write the JSONL transcript
        #[arg(long, value_name = "PATH", value_hint = ValueHint::FilePath, long_help = "Write the full JSONL event transcript to this file.\nIncludes all agent events: tool calls, messages, errors, and metadata.")]
        events: Option<PathBuf>,
        /// Write the last agent message to this file
        #[arg(long, value_name = "PATH", value_hint = ValueHint::FilePath, long_help = "Write only the final agent message to this file.\nUseful for piping the agent's response into other tools.")]
        last_message_file: Option<PathBuf>,
        /// Optional exec subcommand
        #[command(subcommand)]
        command: Option<ExecSubcommand>,
        /// Prompt to execute. Use `-` to force reading from stdin.
        #[arg(
            value_name = "PROMPT",
            long_help = "The prompt to execute.\n\nOmit to read from stdin (piped input).\nUse '-' to explicitly force reading from stdin.\nQuote multi-word prompts: vtcode exec \"fix the bug in auth.rs\""
        )]
        prompt: Option<String>,
    },
    /// Manage durable scheduled tasks
    ///
    /// Create, list, and delete scheduled tasks that run on a recurring or
    /// one-shot basis. Tasks are stored persistently and survive restarts
    /// when paired with `vtcode schedule install-service`.
    ///
    /// Examples:
    ///   vtcode schedule create --name "daily-review" --cron "0 9 * * 1-5" --prompt "review recent changes"
    ///   vtcode schedule create --name "reminder" --reminder "standup in 10 minutes" --at "09:50"
    ///   vtcode schedule list
    ///   vtcode schedule delete `<task-id>`
    Schedule {
        #[command(subcommand)]
        command: ScheduleSubcommand,
    },

    /// Internal VT Code background subagent runner
    #[command(name = "background-subagent", hide = true)]
    BackgroundSubagent(BackgroundSubagentArgs),

    /// Headless code review for the current diff, selected files, or a custom git target
    #[command(
        long_about = "Run a non-interactive code review.\n\nExamples:\n  vtcode review\n  vtcode review --last-diff\n  vtcode review --target HEAD~1..HEAD\n  vtcode review --file src/main.rs --file crates/codegen/vtcode-core/src/lib.rs\n  vtcode review --style security"
    )]
    Review(ReviewArgs),

    /// Runtime schema introspection for built-in tools
    Schema {
        #[command(subcommand)]
        command: SchemaCommands,
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
        file: Option<PathBuf>,
        /// Number of top entries to show
        #[arg(long, default_value_t = 10)]
        top: usize,
    },

    /// Send a VT Code notification using the built-in notification system
    Notify {
        /// Optional notification title
        #[arg(long, value_name = "TITLE")]
        title: Option<String>,
        /// Notification message
        #[arg(value_name = "MESSAGE")]
        message: String,
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

    /// Measure allocator RSS behavior under a bursty/sparse Tokio workload
    ///
    /// Reproduces the mimalloc-vs-jemalloc analysis pattern: many short-lived
    /// tasks allocated across Tokio worker threads, with idle gaps between
    /// bursts. Reports the RSS trajectory so you can see whether the global
    /// allocator returns memory to the OS (jemalloc) or pins it (mimalloc/glibc).
    /// Build with `--features allocator-jemalloc` to compare allocators.
    #[command(name = "bench-allocator")]
    BenchAllocator(BenchAllocatorArgs),

    /// Create complete Rust project
    CreateProject {
        name: String,
        #[arg(long = "feature", value_name = "FEATURE", action = ArgAction::Append)]
        features: Vec<String>,
    },

    /// Revert agent to a previous snapshot
    Revert {
        /// Turn number to revert to
        #[arg(short, long)]
        turn: usize,
        /// Scope of revert operation: conversation, code, full
        #[arg(long)]
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

    /// Initialize project guidance and workspace scaffolding
    ///
    /// Bootstrap a workspace for use with VT Code. Creates vtcode.toml,
    /// AGENTS.md, and other scaffolding. Run this once per project.
    ///
    /// Examples:
    ///   vtcode init
    ///   vtcode init --force
    Init {
        /// Overwrite an existing AGENTS.md without prompting
        #[arg(
            long,
            short = 'f',
            long_help = "Overwrite AGENTS.md without confirmation.\nUse this in CI/CD or scripts where interactive prompts are not possible."
        )]
        force: bool,
    },

    /// Initialize project in ~/.vtcode/projects/
    ///
    /// Create a new project entry in the VT Code projects directory.
    /// This is separate from `vtcode init` which bootstraps a workspace.
    ///
    /// Examples:
    ///   vtcode init-project
    ///   vtcode init-project --name my-project
    ///   vtcode init-project --force --migrate
    #[command(name = "init-project")]
    InitProject {
        /// Project name - defaults to current directory name
        #[arg(
            long,
            long_help = "Name for the project.\nDefaults to the current directory name if not specified."
        )]
        name: Option<String>,
        /// Force initialization - overwrite existing project structure
        #[arg(long, long_help = "Overwrite existing project structure without confirmation.")]
        force: bool,
        /// Migrate existing files - move existing config/cache files to new structure
        #[arg(
            long,
            long_help = "Move existing config and cache files into the new project structure."
        )]
        migrate: bool,
    },

    /// Generate configuration file
    ///
    /// Create a vtcode.toml configuration file with default settings.
    /// Use --global to create in ~/.vtcode/ or specify an output path.
    ///
    /// Examples:
    ///   vtcode config
    ///   vtcode config --global
    ///   vtcode config --output ./my-vtcode.toml
    Config {
        /// Output file path
        #[arg(
            long,
            long_help = "Write the configuration to this path.\nDefaults to ./vtcode.toml in the current directory."
        )]
        output: Option<PathBuf>,
        /// Create in user home directory (~/.vtcode/vtcode.toml)
        #[arg(
            long,
            long_help = "Write the configuration to ~/.vtcode/vtcode.toml.\nThis sets global defaults for all workspaces."
        )]
        global: bool,
    },

    /// Authenticate with a supported provider
    ///
    /// Start an OAuth or API-key login flow for the given provider.
    /// Credentials are stored securely in the OS keychain.
    ///
    /// Examples:
    ///   vtcode login openai
    ///   vtcode login openrouter
    ///   vtcode login codex
    ///   vtcode login codex --device-code
    Login {
        /// Provider name (`openai`, `openrouter`, `copilot`, or `codex`)
        #[arg(long_help = "The provider to authenticate with.\nSupported: openai, openrouter, copilot, codex")]
        provider: String,
        /// Use device-code login when the provider supports it (currently `codex` only)
        #[arg(
            long,
            default_value_t = false,
            long_help = "Use the device-code OAuth flow.\nCurrently supported only for the `codex` provider.\nOpens a browser URL and asks you to enter a code."
        )]
        device_code: bool,
    },

    /// Clear stored authentication credentials for a provider
    ///
    /// Remove stored OAuth tokens or API keys for the given provider.
    ///
    /// Examples:
    ///   vtcode logout openai
    ///   vtcode logout openrouter
    Logout {
        /// Provider name (`openai`, `openrouter`, `copilot`, or `codex`)
        #[arg(long_help = "The provider to deauthenticate.\nSupported: openai, openrouter, copilot, codex")]
        provider: String,
    },

    /// Show authentication status for one provider or all supported providers
    ///
    /// Display whether each provider is authenticated, which credential type
    /// is in use, and token/session metadata when available.
    ///
    /// Examples:
    ///   vtcode auth
    ///   vtcode auth openai
    ///   vtcode auth openrouter
    Auth {
        /// Optional provider name (`openai`, `openrouter`, `copilot`, or `codex`)
        #[arg(long_help = "Show status for a single provider.\nOmit to show status for all supported providers.")]
        provider: Option<String>,
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

    /// Proxy to the official Codex app-server
    #[command(name = "app-server")]
    AppServer {
        /// Transport listen target passed through to `codex app-server`
        #[arg(long, default_value = "stdio://")]
        listen: String,
    },

    /// Manage models and providers
    Models {
        #[command(subcommand)]
        command: ModelCommands,
    },

    /// Manage GPU pod deployments
    #[command(name = "pods")]
    Pods {
        #[command(subcommand)]
        command: PodsCommands,
    },

    /// Generate or display man pages
    Man {
        /// Command name to generate man page for (optional)
        command: Option<String>,
        /// Output file path to save man page
        #[arg(short, long)]
        output: Option<PathBuf>,
    },

    /// Manage Agent Skills
    ///
    /// Skills are reusable instruction sets that extend the agent's capabilities.
    /// Each skill is a directory containing a SKILL.md manifest and optional scripts.
    ///
    /// Examples:
    ///   vtcode skills list
    ///   vtcode skills create my-skill
    ///   vtcode skills load my-skill
    ///   vtcode skills info my-skill
    ///   vtcode skills validate ./path/to/skill
    #[command(subcommand)]
    Skills(SkillsSubcommand),

    /// List available skills (alias for `vtcode skills list`)
    #[command(name = "list-skills", hide = true)]
    ListSkills {},

    /// Manage optional VT Code dependencies
    ///
    /// Install, update, or check the status of optional tools that VT Code
    /// can use (ripgrep, ast-grep, search-tools bundle).
    ///
    /// Examples:
    ///   vtcode dependencies status
    ///   vtcode dependencies install search-tools
    ///   vtcode deps install ripgrep
    #[command(name = "dependencies", visible_alias = "deps", subcommand)]
    Dependencies(DependenciesSubcommand),

    /// Manage API keys in secure storage (OS keyring or encrypted file)
    ///
    /// Store, inspect, and delete provider API keys without exposing them
    /// in shell history or workspace files.
    ///
    /// Examples:
    ///   vtcode secret
    ///   vtcode secret list
    ///   vtcode secret status openai
    ///   vtcode secret add openai
    ///   vtcode secret delete openai
    Secret(SecretArgs),

    /// Run built-in repository checks
    ///
    /// Execute repository-level checks such as ast-grep rule tests and scans.
    ///
    /// Examples:
    ///   vtcode check ast-grep
    Check {
        #[command(subcommand)]
        command: CheckSubcommand,
    },

    /// Check for and install binary updates from GitHub Releases
    ///
    /// Manage VT Code binary updates. By default checks for a new version
    /// and offers to install it. Use flags to customize behavior.
    ///
    /// Examples:
    ///   vtcode update
    ///   vtcode update --check
    ///   vtcode update --force
    ///   vtcode update --list
    ///   vtcode update --pin 0.120.0
    ///   vtcode update --unpin
    #[command(name = "update")]
    Update {
        /// Check for updates without installing
        #[arg(
            long,
            long_help = "Check whether a newer version is available without installing it."
        )]
        check: bool,
        /// Force update even if on latest version
        #[arg(
            long,
            long_help = "Reinstall or downgrade even if the current version is already the latest."
        )]
        force: bool,
        /// List available versions
        #[arg(long, long_help = "Print available release versions from GitHub and exit.")]
        list: bool,
        /// Number of versions to list (default: 10)
        #[arg(
            long,
            default_value = "10",
            long_help = "Maximum number of versions to display with --list."
        )]
        limit: usize,
        /// Pin to a specific version
        #[arg(
            long,
            value_name = "VERSION",
            long_help = "Pin the binary to a specific version.\nAuto-updates are disabled until --unpin is used."
        )]
        pin: Option<String>,
        /// Unpin version
        #[arg(long, long_help = "Remove a previously set version pin and resume auto-updates.")]
        unpin: bool,
        /// Set release channel (stable, beta, nightly)
        #[arg(
            long,
            value_name = "CHANNEL",
            long_help = "Switch the release channel.\nAccepted values: stable, beta, nightly."
        )]
        channel: Option<String>,
        /// Show current update configuration
        #[arg(
            long,
            long_help = "Display the current update configuration (channel, pin, intervals) and exit."
        )]
        show_config: bool,
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

impl Default for Cli {
    fn default() -> Self {
        Self {
            color: ColorSelection { color: ColorChoice::Auto },
            workspace_path: None,
            model: Some(ModelId::default().to_string()),
            provider: Some("gemini".to_owned()),
            api_key_env: "GEMINI_API_KEY".to_owned(),
            workspace: None,
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
            codex_experimental: false,
            no_codex_experimental: false,
            print: None,
            full_auto: None,
            resume_session: None,
            continue_latest: false,
            fork_session: None,
            all: false,
            session_id: None,
            summarize: false,
            debug: false,
            enable_skills: false,
            tick_rate: 250,
            frame_rate: 60,
            agent: None,
            allowed_tools: Vec::new(),
            disallowed_tools: Vec::new(),
            dangerously_skip_permissions: false,
            ide: false,
            chrome: false,
            no_chrome: false,
            command: Some(Commands::Chat),
        }
    }
}

impl Cli {
    /// Get the model to use, with fallback to default
    pub fn get_model(&self) -> String {
        self.model.clone().unwrap_or_else(|| ModelId::default().to_string())
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
    pub async fn load_config(&self) -> anyhow::Result<ConfigFile> {
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
            let cwd = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
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
                if (s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\'')) {
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
    pub fn get_workspace(&self) -> PathBuf {
        self.workspace
            .clone()
            .unwrap_or_else(|| env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
    }

    /// Get the effective API key environment variable
    ///
    /// Automatically infers the API key environment variable based on the provider
    /// when the current value matches the default or is not explicitly set.
    pub fn get_api_key_env(&self) -> String {
        crate::config::api_keys::resolve_api_key_env(
            self.provider
                .as_deref()
                .unwrap_or(crate::config::constants::defaults::DEFAULT_PROVIDER),
            &self.api_key_env,
        )
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

    pub fn codex_experimental_override(&self) -> Option<bool> {
        if self.codex_experimental {
            Some(true)
        } else if self.no_codex_experimental {
            Some(false)
        } else {
            None
        }
    }
}

fn parse_workspace_directory(raw: &str) -> Result<PathBuf, String> {
    let candidate = PathBuf::from(raw);
    if !candidate.exists() {
        return Err(format!(
            "'{}' is not a valid workspace path or subcommand.\n\
             Run `vtcode --help` to see available commands and options.",
            raw
        ));
    }

    Ok(candidate)
}

pub fn long_version() -> String {
    use crate::config::defaults::{get_config_dir, get_data_dir};

    let git_info = option_env!("VT_CODE_GIT_INFO").unwrap_or(env!("CARGO_PKG_VERSION"));

    let config_dir = get_config_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "~/.vtcode/".to_string());

    let data_dir = get_data_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "~/.vtcode/cache/".to_string());

    format!(
        "{}\n\nAuthors: {}\nConfig directory: {}\nData directory: {}\n\nEnvironment variables:\n  VTCODE_CONFIG - Override config directory\n  VTCODE_DATA - Override data directory",
        git_info,
        env!("CARGO_PKG_AUTHORS"),
        config_dir,
        data_dir
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn long_version_includes_expected_sections() {
        let text = long_version();
        assert!(text.contains("Authors:"));
        assert!(text.contains("Config directory:"));
        assert!(text.contains("Data directory:"));
        assert!(text.contains("VTCODE_CONFIG"));
        assert!(text.contains("VTCODE_DATA"));
    }

    #[test]
    fn long_version_starts_with_build_git_info() {
        let text = long_version();
        let expected = option_env!("VT_CODE_GIT_INFO").unwrap_or(env!("CARGO_PKG_VERSION"));
        assert!(text.starts_with(expected));
    }

    #[test]
    fn config_file_api_key_env_uses_provider_default() {
        let cli = Cli::parse_from(["vtcode", "--provider", "minimax"]);

        assert_eq!(cli.get_api_key_env(), "MINIMAX_API_KEY");
    }

    #[test]
    fn config_file_api_key_env_preserves_explicit_override() {
        let cli = Cli::parse_from(["vtcode", "--provider", "openai", "--api-key-env", "CUSTOM_OPENAI_KEY"]);

        assert_eq!(cli.get_api_key_env(), "CUSTOM_OPENAI_KEY");
    }

    #[test]
    fn parses_app_server_command_with_stdio_listen_target() {
        let cli = Cli::parse_from(["vtcode", "app-server", "--listen", "stdio://"]);

        assert!(matches!(
            cli.command,
            Some(Commands::AppServer { ref listen }) if listen == "stdio://"
        ));
    }

    #[test]
    fn parses_init_force_flag() {
        let cli = Cli::parse_from(["vtcode", "init", "--force"]);

        assert!(matches!(cli.command, Some(Commands::Init { force: true })));
    }

    #[test]
    fn parses_codex_login_device_code_flag() {
        let cli = Cli::parse_from(["vtcode", "login", "codex", "--device-code"]);

        assert!(matches!(
            cli.command,
            Some(Commands::Login {
                ref provider,
                device_code: true
            }) if provider == "codex"
        ));
    }

    #[test]
    fn parses_codex_experimental_flags() {
        let enabled = Cli::parse_from(["vtcode", "--codex-experimental"]);
        assert_eq!(enabled.codex_experimental_override(), Some(true));

        let disabled = Cli::parse_from(["vtcode", "--no-codex-experimental"]);
        assert_eq!(disabled.codex_experimental_override(), Some(false));
    }

    #[test]
    fn codex_experimental_flags_conflict() {
        let result = Cli::try_parse_from(["vtcode", "--codex-experimental", "--no-codex-experimental"]);

        result.unwrap_err();
    }

    #[test]
    fn parses_create_project_feature_flags() {
        let cli = Cli::parse_from([
            "vtcode",
            "create-project",
            "demo",
            "--feature",
            "web",
            "--feature",
            "db",
        ]);

        assert!(matches!(
            cli.command,
            Some(Commands::CreateProject { ref name, ref features })
                if name == "demo" && features == &vec!["web".to_string(), "db".to_string()]
        ));
    }

    #[test]
    fn parses_revert_partial_long_flag() {
        let cli = Cli::parse_from(["vtcode", "revert", "--turn", "3", "--partial", "code"]);

        assert!(matches!(
            cli.command,
            Some(Commands::Revert {
                turn: 3,
                partial: Some(ref scope)
            }) if scope == "code"
        ));
    }
}
