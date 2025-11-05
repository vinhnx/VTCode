//! Man page generation for VTCode CLI using roff-rs
//!
//! This module provides functionality to generate Unix man pages for VTCode
//! commands and subcommands using the roff-rs library.

use anyhow::{Context, Result, bail};
use roff::{Roff, bold, italic, roman};
use std::path::Path;
use tokio::fs;

/// Man page generator for VTCode CLI
pub struct ManPageGenerator;

impl ManPageGenerator {
    /// Get current date in YYYY-MM-DD format
    fn current_date() -> String {
        use chrono::Utc;
        Utc::now().format("%Y-%m-%d").to_string()
    }

    /// Generate man page for the main VTCode command
    pub fn generate_main_man_page() -> Result<String> {
        let current_date = Self::current_date();
        let page = Roff::new()
            .control("TH", ["VTCODE", "1", &current_date, "VTCode", "User Commands"])
            .control("SH", ["NAME"])
            .text([roman("vtcode - Advanced coding agent with Decision Ledger")])
            .control("SH", ["SYNOPSIS"])
            .text([
                bold("vtcode"),
                roman(" ["),
                bold("OPTIONS"),
                roman("] ["),
                bold("COMMAND"),
                roman("] ["),
                bold("ARGS"),
                roman("]"),
            ])
            .control("SH", ["DESCRIPTION"])
            .text([
                roman("VTCode is an advanced coding agent with single-agent architecture and Decision Ledger that provides"),
                roman(" intelligent code generation, analysis, and modification capabilities. It supports"),
                roman(" multiple LLM providers including Gemini, OpenAI, Anthropic, DeepSeek, xAI, Z.AI,"),
                roman(" Moonshot AI, OpenRouter, and Ollama, and includes tree-sitter powered code analysis for"),
                roman(" Rust, Python, JavaScript, TypeScript, Go, and Java."),
            ])
            .control("SH", ["OPTIONS"])
            .control("TP", [])
            .text([bold("-m"), roman(", "), bold("--model"), roman(" "), italic("MODEL")])
            .text([roman("Specify the LLM model to use (default: gemini-2.5-flash-preview-05-20)")])
            .control("TP", [])
            .text([bold("-p"), roman(", "), bold("--provider"), roman(" "), italic("PROVIDER")])
            .text([
                roman(
                    "Specify the LLM provider (gemini, openai, anthropic, deepseek, xai, zai, moonshot, openrouter, ollama, lmstudio)",
                ),
            ])
            .control("TP", [])
            .text([bold("--workspace"), roman(" "), italic("PATH")])
            .text([roman("Set the workspace root directory for file operations")])
            .control("TP", [])
            .text([bold("--enable-tree-sitter")])
            .text([roman("Enable tree-sitter code analysis")])
            .control("TP", [])
            .text([bold("--performance-monitoring")])
            .text([roman("Enable performance monitoring and metrics")])
            .control("TP", [])
            .text([bold("--research-preview")])
            .text([roman("Enable research-preview features including context compression")])
            .control("TP", [])
            .text([bold("--debug")])
            .text([roman("Enable debug output")])
            .control("TP", [])
            .text([bold("--verbose")])
            .text([roman("Enable verbose logging")])
            .control("TP", [])
            .text([bold("-h"), roman(", "), bold("--help")])
            .text([roman("Display help information")])
            .control("TP", [])
            .text([bold("-V"), roman(", "), bold("--version")])
            .text([roman("Display version information")])
            .control("SH", ["COMMANDS"])
            .control("TP", [])
            .text([bold("chat")])
            .text([roman("Start interactive AI coding assistant")])
            .control("TP", [])
            .text([bold("ask"), roman(" "), italic("PROMPT")])
            .text([roman("Single prompt mode without tools")])
            .control("TP", [])
            .text([bold("analyze")])
            .text([roman("Analyze workspace with tree-sitter integration")])
            .control("TP", [])
            .text([bold("performance")])
            .text([roman("Display performance metrics and system status")])
            .control("TP", [])
            .text([bold("benchmark")])
            .text([roman("Run SWE-bench evaluation framework")])
            .control("TP", [])
            .text([bold("create-project"), roman(" "), italic("NAME"), roman(" "), italic("FEATURES")])
            .text([roman("Create complete Rust project with features")])
            .control("TP", [])
            .text([bold("init")])
            .text([roman("Initialize project with enhanced structure")])
            .control("TP", [])
            .text([bold("man"), roman(" "), italic("COMMAND")])
            .text([roman("Generate or display man pages for commands")])
            .control("TP", [])
            .text([bold("acp")])
            .text([roman("Start Agent Client Protocol bridge for IDE integrations")])
            .control("TP", [])
            .text([bold("chat-verbose")])
            .text([roman("Verbose interactive chat with enhanced transparency")])
            .control("TP", [])
            .text([bold("analyze")])
            .text([roman("Analyze workspace with tree-sitter integration")])
            .control("TP", [])
            .text([bold("performance")])
            .text([roman("Display performance metrics and system status")])
            .control("TP", [])
            .text([bold("trajectory")])
            .text([roman("Pretty-print trajectory logs and show basic analytics")])
            .control("TP", [])
            .text([bold("benchmark")])
            .text([roman("Benchmark against SWE-bench evaluation framework")])
            .control("TP", [])
            .text([bold("create-project"), roman(" "), italic("name"), roman(" "), italic("features")])
            .text([roman("Create complete Rust project with advanced features")])
            .control("TP", [])
            .text([bold("compress-context")])
            .text([roman("Compress conversation context for long-running sessions")])
            .control("TP", [])
            .text([bold("revert"), roman(" "), italic("turn")])
            .text([roman("Revert agent to a previous snapshot")])
            .control("TP", [])
            .text([bold("snapshots")])
            .text([roman("List all available snapshots")])
            .control("TP", [])
            .text([bold("cleanup-snapshots")])
            .text([roman("Clean up old snapshots")])
            .control("TP", [])
            .text([bold("init")])
            .text([roman("Initialize project with enhanced dot-folder structure")])
            .control("TP", [])
            .text([bold("init-project")])
            .text([roman("Initialize project with dot-folder structure")])
            .control("TP", [])
            .text([bold("config")])
            .text([roman("Generate configuration file")])
            .control("TP", [])
            .text([bold("tool-policy")])
            .text([roman("Manage tool execution policies")])
            .control("TP", [])
            .text([bold("mcp")])
            .text([roman("Manage Model Context Protocol providers")])
            .control("TP", [])
            .text([bold("models")])
            .text([roman("Manage models and providers")])
            .control("TP", [])
            .text([bold("security")])
            .text([roman("Security and safety management")])
            .control("TP", [])
            .text([bold("tree-sitter")])
            .text([roman("Tree-sitter code analysis tools")])
            .control("SH", ["EXAMPLES"])
            .text([roman("Start interactive chat:")])
            .text([bold("  vtcode chat")])
            .text([roman("Ask a question:")])
            .text([bold("  vtcode ask \"Explain Rust ownership\"")])
            .text([roman("Create a web project:")])
            .text([bold("  vtcode create-project myapp web,auth,db")])
            .text([roman("Generate man page:")])
            .text([bold("  vtcode man chat")])
            .control("SH", ["ENVIRONMENT"])
            .control("TP", [])
            .text([bold("GEMINI_API_KEY")])
            .text([roman("API key for Google Gemini (default provider)")])
            .control("TP", [])
            .text([bold("OPENAI_API_KEY")])
            .text([roman("API key for OpenAI GPT models")])
            .control("TP", [])
            .text([bold("ANTHROPIC_API_KEY")])
            .text([roman("API key for Anthropic Claude models")])
            .control("TP", [])
            .text([bold("DEEPSEEK_API_KEY")])
            .text([roman("API key for DeepSeek models")])
            .control("TP", [])
            .text([bold("XAI_API_KEY")])
            .text([roman("API key for xAI Grok models")])
            .control("TP", [])
            .text([bold("ZAI_API_KEY")])
            .text([roman("API key for Z.AI GLM models")])
            .control("TP", [])
            .text([bold("MOONSHOT_API_KEY")])
            .text([roman("API key for Moonshot AI Kimi models")])
            .control("TP", [])
            .text([bold("OPENROUTER_API_KEY")])
            .text([roman("API key for OpenRouter models")])
            .control("SH", ["FILES"])
            .control("TP", [])
            .text([bold("vtcode.toml")])
            .text([roman("Configuration file (current directory or ~/.vtcode/)")])
            .control("TP", [])
            .text([bold(".vtcode/")])
            .text([roman("Project cache and context directory")])
            .control("SH", ["SAFETY"])
            .control("TP", [])
            .text([roman(
                "apply_patch: reserve for reviewed diffs or small batches. For large refactors or critical files, stage local backups and prefer edit_file/write_file to avoid partial rewrites if a patch fails.",
            )])
            .control("TP", [])
            .text([roman(
                "Timeout governance: tune [timeouts] in vtcode.toml to clamp tool duration. VTCode warns once execution passes the configured warning threshold so you can cancel runaway commands.",
            )])
            .control("SH", ["SEE ALSO"])
            .text([roman("Full documentation: https://github.com/vinhnx/vtcode")])
            .text([roman("Related commands: cargo(1), rustc(1), git(1)")])
            .render();

        Ok(page)
    }

    /// Generate man page for a specific command
    pub fn generate_command_man_page(command: &str) -> Result<String> {
        match command {
            "chat" => Self::generate_chat_man_page(),
            "ask" => Self::generate_ask_man_page(),
            "analyze" => Self::generate_analyze_man_page(),
            "performance" => Self::generate_performance_man_page(),
            "benchmark" => Self::generate_benchmark_man_page(),
            "create-project" => Self::generate_create_project_man_page(),
            "init" => Self::generate_init_man_page(),
            "man" => Self::generate_man_man_page(),
            _ => bail!("Unknown command: {}", command),
        }
    }

    /// Generate man page for the chat command
    fn generate_chat_man_page() -> Result<String> {
        let current_date = Self::current_date();
        let page = Roff::new()
            .control("TH", ["VTCODE-CHAT", "1", &current_date, "VTCode", "User Commands"])
            .control("SH", ["NAME"])
            .text([roman("vtcode-chat - Interactive AI coding assistant")])
            .control("SH", ["SYNOPSIS"])
            .text([
                bold("vtcode"),
                roman(" ["),
                bold("OPTIONS"),
                roman("] "),
                bold("chat"),
            ])
            .control("SH", ["DESCRIPTION"])
            .text([
                roman("Start an interactive AI coding assistant session."),
                roman(" The chat command provides intelligent code generation, analysis, and modification"),
                roman(" with support for multiple LLM providers and tree-sitter powered code analysis."),
            ])
            .control("SH", ["OPTIONS"])
            .text([roman("All global options are supported. See "), bold("vtcode(1)"), roman(" for details.")])
            .control("SH", ["EXAMPLES"])
            .text([roman("Start basic chat session:")])
            .text([bold("  vtcode chat")])
            .text([roman("Start with specific model:")])
            .text([bold("  vtcode --model gemini-2.5-pro chat")])
            .control("SH", ["SEE ALSO"])
            .text([bold("vtcode(1)"), roman(", "), bold("vtcode-ask(1)"), roman(", "), bold("vtcode-analyze(1)")])
            .render();

        Ok(page)
    }

    /// Generate man page for the ask command
    fn generate_ask_man_page() -> Result<String> {
        let current_date = Self::current_date();
        let page = Roff::new()
            .control("TH", ["VTCODE-ASK", "1", &current_date, "VTCode", "User Commands"])
            .control("SH", ["NAME"])
            .text([roman("vtcode-ask - Single prompt mode without tools")])
            .control("SH", ["SYNOPSIS"])
            .text([
                bold("vtcode"),
                roman(" ["),
                bold("OPTIONS"),
                roman("] "),
                bold("ask"),
                roman(" "),
                italic("PROMPT"),
            ])
            .control("SH", ["DESCRIPTION"])
            .text([
                roman("Execute a single prompt without tool usage. This is perfect for quick questions,"),
                roman(" code explanations, and simple queries that don't require file operations or"),
                roman(" complex tool interactions."),
            ])
            .control("SH", ["EXAMPLES"])
            .text([roman("Ask about Rust ownership:")])
            .text([bold("  vtcode ask \"Explain Rust ownership\"")])
            .text([roman("Get code explanation:")])
            .text([bold("  vtcode ask \"What does this regex do: \\w+@\\w+\\.\\w+\"")])
            .control("SH", ["SEE ALSO"])
            .text([bold("vtcode(1)"), roman(", "), bold("vtcode-chat(1)")])
            .render();

        Ok(page)
    }

    /// Generate man page for the analyze command
    fn generate_analyze_man_page() -> Result<String> {
        let current_date = Self::current_date();
        let page = Roff::new()
            .control(
                "TH",
                [
                    "VTCODE-ANALYZE",
                    "1",
                    &current_date,
                    "VTCode",
                    "User Commands",
                ],
            )
            .control("SH", ["NAME"])
            .text([roman(
                "vtcode-analyze - Analyze workspace with tree-sitter integration",
            )])
            .control("SH", ["SYNOPSIS"])
            .text([
                bold("vtcode"),
                roman(" ["),
                bold("OPTIONS"),
                roman("] "),
                bold("analyze"),
            ])
            .control("SH", ["DESCRIPTION"])
            .text([
                roman(
                    "Analyze the current workspace using tree-sitter integration. Provides project",
                ),
                roman(
                    " structure analysis, language detection, code complexity metrics, dependency",
                ),
                roman(" insights, and symbol extraction for supported languages."),
            ])
            .control("SH", ["SUPPORTED LANGUAGES"])
            .text([roman(
                "• Rust • Python • JavaScript • TypeScript • Go • Java",
            )])
            .control("SH", ["FEATURES"])
            .control("TP", [])
            .text([bold("Project Structure")])
            .text([roman("Directory tree and file organization analysis")])
            .control("TP", [])
            .text([bold("Language Detection")])
            .text([roman("Automatic detection of programming languages used")])
            .control("TP", [])
            .text([bold("Code Metrics")])
            .text([roman("Complexity analysis and code quality metrics")])
            .control("TP", [])
            .text([bold("Symbol Extraction")])
            .text([roman("Functions, classes, and other code symbols")])
            .control("SH", ["EXAMPLES"])
            .text([roman("Analyze current workspace:")])
            .text([bold("  vtcode analyze")])
            .control("SH", ["SEE ALSO"])
            .text([bold("vtcode(1)"), roman(", "), bold("vtcode-chat(1)")])
            .render();

        Ok(page)
    }

    /// Generate man page for the performance command
    fn generate_performance_man_page() -> Result<String> {
        let current_date = Self::current_date();
        let page = Roff::new()
            .control(
                "TH",
                [
                    "VTCODE-PERFORMANCE",
                    "1",
                    &current_date,
                    "VTCode",
                    "User Commands",
                ],
            )
            .control("SH", ["NAME"])
            .text([roman(
                "vtcode-performance - Display performance metrics and system status",
            )])
            .control("SH", ["SYNOPSIS"])
            .text([
                bold("vtcode"),
                roman(" ["),
                bold("OPTIONS"),
                roman("] "),
                bold("performance"),
            ])
            .control("SH", ["DESCRIPTION"])
            .text([
                roman("Display comprehensive performance metrics and system status information."),
                roman(" Shows token usage, API costs, response times, tool execution statistics,"),
                roman(" memory usage patterns, and agent performance metrics."),
            ])
            .control("SH", ["METRICS DISPLAYED"])
            .control("TP", [])
            .text([bold("Token Usage")])
            .text([roman("Input/output token counts and API costs")])
            .control("TP", [])
            .text([bold("Response Times")])
            .text([roman("API response latency and processing times")])
            .control("TP", [])
            .text([bold("Tool Execution")])
            .text([roman("Tool call statistics and execution times")])
            .control("TP", [])
            .text([bold("Memory Usage")])
            .text([roman("Memory consumption patterns")])
            .control("TP", [])
            .text([bold("Agent Performance")])
            .text([roman("Single-agent execution metrics")])
            .control("SH", ["EXAMPLES"])
            .text([roman("Show performance metrics:")])
            .text([bold("  vtcode performance")])
            .control("SH", ["SEE ALSO"])
            .text([bold("vtcode(1)"), roman(", "), bold("vtcode-benchmark(1)")])
            .render();

        Ok(page)
    }

    /// Generate man page for the benchmark command
    fn generate_benchmark_man_page() -> Result<String> {
        let current_date = Self::current_date();
        let page = Roff::new()
            .control(
                "TH",
                [
                    "VTCODE-BENCHMARK",
                    "1",
                    &current_date,
                    "VTCode",
                    "User Commands",
                ],
            )
            .control("SH", ["NAME"])
            .text([roman(
                "vtcode-benchmark - Run SWE-bench evaluation framework",
            )])
            .control("SH", ["SYNOPSIS"])
            .text([
                bold("vtcode"),
                roman(" ["),
                bold("OPTIONS"),
                roman("] "),
                bold("benchmark"),
            ])
            .control("SH", ["DESCRIPTION"])
            .text([
                roman(
                    "Run automated performance testing against the SWE-bench evaluation framework.",
                ),
                roman(" Provides comparative analysis across different models, benchmark scoring,"),
                roman(" and optimization insights for coding tasks."),
            ])
            .control("SH", ["FEATURES"])
            .control("TP", [])
            .text([bold("Automated Testing")])
            .text([roman("Run standardized coding tasks and challenges")])
            .control("TP", [])
            .text([bold("Comparative Analysis")])
            .text([roman("Compare performance across different models")])
            .control("TP", [])
            .text([bold("Benchmark Scoring")])
            .text([roman("Quantitative performance metrics and scores")])
            .control("TP", [])
            .text([bold("Optimization Insights")])
            .text([roman("Recommendations for performance improvements")])
            .control("SH", ["EXAMPLES"])
            .text([roman("Run benchmark suite:")])
            .text([bold("  vtcode benchmark")])
            .control("SH", ["SEE ALSO"])
            .text([
                bold("vtcode(1)"),
                roman(", "),
                bold("vtcode-performance(1)"),
            ])
            .render();

        Ok(page)
    }

    /// Generate man page for the create-project command
    fn generate_create_project_man_page() -> Result<String> {
        let current_date = Self::current_date();
        let page = Roff::new()
            .control(
                "TH",
                [
                    "VTCODE-CREATE-PROJECT",
                    "1",
                    &current_date,
                    "VTCode",
                    "User Commands",
                ],
            )
            .control("SH", ["NAME"])
            .text([roman(
                "vtcode-create-project - Create complete Rust project with features",
            )])
            .control("SH", ["SYNOPSIS"])
            .text([
                bold("vtcode"),
                roman(" ["),
                bold("OPTIONS"),
                roman("] "),
                bold("create-project"),
                roman(" "),
                italic("NAME"),
                roman(" "),
                italic("FEATURES"),
            ])
            .control("SH", ["DESCRIPTION"])
            .text([
                roman("Create a complete Rust project with advanced features and integrations."),
                roman(" Supports web frameworks, database integration, authentication systems,"),
                roman(" testing setup, and tree-sitter integration."),
            ])
            .control("SH", ["AVAILABLE FEATURES"])
            .text([roman("• web - Web framework (Axum, Rocket, Warp)")])
            .text([roman("• auth - Authentication system")])
            .text([roman("• db - Database integration")])
            .text([roman("• test - Testing setup")])
            .text([roman("• tree-sitter - Code analysis integration")])
            .control("SH", ["EXAMPLES"])
            .text([roman("Create web app with auth and database:")])
            .text([bold("  vtcode create-project myapp web,auth,db")])
            .text([roman("Create basic project:")])
            .text([bold("  vtcode create-project simple_app")])
            .control("SH", ["SEE ALSO"])
            .text([bold("vtcode(1)"), roman(", "), bold("vtcode-init(1)")])
            .render();

        Ok(page)
    }

    /// Generate man page for the init command
    fn generate_init_man_page() -> Result<String> {
        let current_date = Self::current_date();
        let page = Roff::new()
            .control(
                "TH",
                ["VTCODE-INIT", "1", &current_date, "VTCode", "User Commands"],
            )
            .control("SH", ["NAME"])
            .text([roman(
                "vtcode-init - Initialize project with enhanced structure",
            )])
            .control("SH", ["SYNOPSIS"])
            .text([
                bold("vtcode"),
                roman(" ["),
                bold("OPTIONS"),
                roman("] "),
                bold("init"),
            ])
            .control("SH", ["DESCRIPTION"])
            .text([
                roman("Initialize a project with enhanced dot-folder structure for VTCode."),
                roman(" Creates project directory structure, config files, cache directories,"),
                roman(" embeddings storage, and tree-sitter parser setup."),
            ])
            .control("SH", ["DIRECTORY STRUCTURE"])
            .text([roman(
                "• .vtcode/ - Main project cache and context directory",
            )])
            .text([roman("• .vtcode/config/ - Configuration files")])
            .text([roman("• .vtcode/cache/ - File and analysis cache")])
            .text([roman("• .vtcode/embeddings/ - Code embeddings storage")])
            .text([roman("• .vtcode/parsers/ - Tree-sitter parsers")])
            .text([roman("• .vtcode/context/ - Agent context stores")])
            .control("SH", ["EXAMPLES"])
            .text([roman("Initialize current directory:")])
            .text([bold("  vtcode init")])
            .control("SH", ["SEE ALSO"])
            .text([
                bold("vtcode(1)"),
                roman(", "),
                bold("vtcode-create-project(1)"),
            ])
            .render();

        Ok(page)
    }

    /// Generate man page for the man command itself
    fn generate_man_man_page() -> Result<String> {
        let current_date = Self::current_date();
        let page = Roff::new()
            .control("TH", ["VTCODE-MAN", "1", &current_date, "VTCode", "User Commands"])
            .control("SH", ["NAME"])
            .text([roman("vtcode-man - Generate or display man pages for VTCode commands")])
            .control("SH", ["SYNOPSIS"])
            .text([
                bold("vtcode"),
                roman(" ["),
                bold("OPTIONS"),
                roman("] "),
                bold("man"),
                roman(" ["),
                italic("COMMAND"),
                roman("] ["),
                bold("--output"),
                roman(" "),
                italic("FILE"),
                roman("]"),
            ])
            .control("SH", ["DESCRIPTION"])
            .text([
                roman("Generate or display Unix man pages for VTCode commands. Man pages provide"),
                roman(" detailed documentation for all VTCode functionality including usage examples,"),
                roman(" option descriptions, and feature explanations."),
            ])
            .control("SH", ["OPTIONS"])
            .control("TP", [])
            .text([bold("--output"), roman(" "), italic("FILE")])
            .text([roman("Write man page to specified file instead of displaying")])
            .control("SH", ["AVAILABLE COMMANDS"])
            .text([roman("• chat - Interactive AI coding assistant")])
            .text([roman("• ask - Single prompt mode")])
            .text([roman("• analyze - Workspace analysis")])
            .text([roman("• performance - Performance metrics")])
            .text([roman("• trajectory - Pretty-print trajectory logs and analytics")])
            .text([roman("• benchmark - SWE-bench evaluation framework")])
            .text([roman("• create-project - Create complete Rust project with features")])
            .text([roman("• compress-context - Compress conversation context")])
            .text([roman("• revert - Revert agent to a previous snapshot")])
            .text([roman("• snapshots - List available snapshots")])
            .text([roman("• cleanup-snapshots - Clean up old snapshots")])
            .text([roman("• init - Initialize project with enhanced structure")])
            .text([roman("• init-project - Initialize project with dot-folder structure")])
            .text([roman("• config - Generate configuration file")])
            .text([roman("• tool-policy - Manage tool execution policies")])
            .text([roman("• mcp - Manage Model Context Protocol providers")])
            .text([roman("• models - Manage models and providers")])
            .text([roman("• security - Security and safety management")])
            .text([roman("• tree-sitter - Tree-sitter code analysis tools")])
            .text([roman("• acp - Agent Client Protocol bridge for IDE integrations")])
            .text([roman("• chat-verbose - Verbose interactive chat with transparency")])
            .text([roman("• man - Man page generation (this command)")])
            .control("SH", ["EXAMPLES"])
            .text([roman("Display main VTCode man page:")])
            .text([bold("  vtcode man")])
            .text([roman("Display chat command man page:")])
            .text([bold("  vtcode man chat")])
            .text([roman("Save man page to file:")])
            .text([bold("  vtcode man chat --output chat.1")])
            .control("SH", ["SEE ALSO"])
            .text([bold("vtcode(1)"), roman(", "), bold("man(1)")])
            .render();

        Ok(page)
    }

    /// Save man page to file
    pub async fn save_man_page(content: &str, filename: &Path) -> Result<()> {
        fs::write(filename, content)
            .await
            .with_context(|| format!("Failed to write man page to {}", filename.display()))?;
        Ok(())
    }

    /// Get list of available commands for man page generation
    pub fn available_commands() -> Vec<&'static str> {
        vec![
            "chat",
            "ask",
            "analyze",
            "performance",
            "benchmark",
            "create-project",
            "init",
            "man",
        ]
    }
}
