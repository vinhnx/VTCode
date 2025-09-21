#![doc = r#"
# VTCode Core - Research-Preview Rust Coding Agent Library

A sophisticated terminal-based coding agent library that implements state-of-the-art agent
architecture patterns, inspired by Anthropic's SWE-bench breakthroughs.

## Features

### Core Capabilities

- **Single-Agent Reliability**: Streamlined, linear agent with robust context engineering
- **Decision Ledger**: Structured record of key decisions injected each turn for consistency
- **Multi-Provider LLM Support**: Gemini, OpenAI, Anthropic, DeepSeek integration
- **Advanced Code Analysis**: Tree-sitter parsers for Rust, Python, JavaScript, TypeScript, Go, Java
- **Intelligent Tool Suite**: File operations, search, terminal commands, and PTY integration
- **Configuration Management**: TOML-based configuration with comprehensive policies
- **Safety & Security**: Path validation, command policies, and human-in-the-loop controls
- **Workspace-First Automation**: Reads, writes, indexing, and shell execution anchored to `WORKSPACE_DIR`

### Advanced Features

- **Context Engineering**: Full conversation history with intelligent management
- **Performance Monitoring**: Real-time metrics and benchmarking capabilities
- **Prompt Caching**: Strategic caching for improved response times
- **Conversation Summarization**: Automatic compression for long sessions
- **Tool Policy Management**: Configurable tool execution policies
- **PTY Integration**: Full terminal emulation for interactive commands
- **Project Indexing**: Intelligent workspace analysis and file discovery

## Quick Start

```rust,no_run
use vtcode_core::{Agent, VTCodeConfig};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Load configuration
    let config = VTCodeConfig::load()?;

    // Create agent
    let agent = Agent::new(config).await?;

    // Start interactive session
    agent.run().await?;

    Ok(())
}
```

## Architecture

The agent follows proven patterns for reliable, long-running coding assistance:

- **Model-Driven Control**: Maximum autonomy given to language models
- **Decision Transparency**: Complete audit trail of all agent actions
- **Error Recovery**: Intelligent error handling with context preservation
- **Conversation Summarization**: Automatic compression for long sessions
- **Tool Integration**: Modular tool system with policy-based execution

## Core Components

### Agent System
- [`Agent`] - Main agent implementation with conversation management
- [`ConversationSummarizer`] - Automatic conversation compression
- [`ContextCompressor`] - Intelligent context management

### Tool System
- [`ToolRegistry`] - Central tool registration and execution
- [`ToolPolicy`] - Configurable tool execution policies
- [`ToolPolicyManager`] - Policy enforcement and validation

### LLM Integration
- [`AnyClient`] - Unified interface for multiple LLM providers
- [`make_client`] - Factory function for creating LLM clients
- Gemini, OpenAI, Anthropic, DeepSeek provider implementations

### Configuration
- [`VTCodeConfig`] - Main configuration structure
- [`AgentConfig`] - Agent-specific configuration
- TOML-based configuration with comprehensive policies

## Examples

### Basic Agent Usage

```rust,no_run
use vtcode_core::{Agent, VTCodeConfig};
use std::path::PathBuf;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Load configuration
    let config = VTCodeConfig::load()?;

    // Create agent with custom workspace
    let workspace = PathBuf::from("/path/to/project");
    let agent = Agent::new_with_workspace(config, workspace).await?;

    // Process a coding task
    let task = "Add error handling to the user authentication function";
    let result = agent.process_task(task).await?;

    println!("Task completed: {}", result);

    Ok(())
}
```

### Tool Registry Usage

```rust,no_run
use vtcode_core::tools::{ToolRegistry, ToolRegistration};
use serde_json::Value;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let workspace = std::env::current_dir()?;
    let mut registry = ToolRegistry::new(workspace);

    // Register a custom tool
    let custom_tool = ToolRegistration {
        name: "analyze_code".to_string(),
        description: "Analyze code for potential issues".to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "file_path": {"type": "string", "description": "Path to file to analyze"},
                "analysis_type": {"type": "string", "enum": ["security", "performance", "style"]}
            },
            "required": ["file_path"]
        }),
        handler: |args: Value| async move {
            let file_path = args["file_path"].as_str().unwrap_or("");
            let analysis_type = args["analysis_type"].as_str().unwrap_or("general");

            // Perform analysis
            let result = format!("Analysis of {} for {} completed", file_path, analysis_type);

            Ok(serde_json::json!({
                "success": true,
                "analysis": result,
                "issues_found": 0
            }))
        },
    };

    registry.register_tool(custom_tool).await?;

    // Execute the tool
    let args = serde_json::json!({
        "file_path": "src/main.rs",
        "analysis_type": "security"
    });

    let result = registry.execute_tool("analyze_code", args).await?;
    println!("Tool result: {}", result);

    Ok(())
}
```

### Configuration Management

```rust,no_run
use vtcode_core::{VTCodeConfig, AgentConfig};
use vtcode_core::config::types::{ToolConfig, LoggingConfig};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create custom configuration
    let config = VTCodeConfig {
        agent: AgentConfig {
            max_iterations: 50,
            timeout_seconds: 300,
            ..Default::default()
        },
        tools: ToolConfig {
            max_tool_loops: 25,
            default_policy: "prompt".to_string(),
            ..Default::default()
        },
        logging: LoggingConfig {
            level: "info".to_string(),
            file_path: Some("vtcode.log".to_string()),
            ..Default::default()
        },
        ..Default::default()
    };

    // Save configuration
    config.save()?;

    // Load and verify
    let loaded = VTCodeConfig::load()?;
    assert_eq!(loaded.agent.max_iterations, 50);

    Ok(())
}
```

### LLM Provider Integration

```rust,no_run
use vtcode_core::llm::{AnyClient, make_client};
use vtcode_core::config::types::ProviderConfigs;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Configure providers
    let providers = ProviderConfigs {
        gemini: Some(vtcode_core::utils::dot_config::ProviderConfig {
            api_key: std::env::var("GEMINI_API_KEY")?,
            model: "gemini-2.5-flash-exp".to_string(),
            ..Default::default()
        }),
        ..Default::default()
    };

    // Create LLM client
    let client = make_client(&providers, "gemini")?;

    // Make a request
    let messages = vec![
        vtcode_core::llm::types::Message {
            role: "user".to_string(),
            content: "Hello, can you help me with Rust code?".to_string(),
        }
    ];

    let response = client.chat(&messages, None).await?;
    println!("LLM Response: {}", response.content);

    Ok(())
}
```

## Safety & Security

VTCode implements multiple layers of security:

- **Path Validation**: All file operations check workspace boundaries
- **Command Policies**: Configurable allow/deny lists for terminal commands
- **Tool Policies**: Granular control over tool execution
- **Human-in-the-Loop**: Optional approval for sensitive operations
- **Audit Logging**: Complete trail of all agent actions

## Performance

- **Prompt Caching**: Reduces API calls for repeated prompts
- **Context Compression**: Efficient memory usage for long conversations
- **Parallel Processing**: Concurrent tool execution where appropriate
- **Resource Limits**: Configurable timeouts and size limits

## Distribution

- **Cargo**: `cargo install vtcode-core`
- **GitHub**: Source code and releases
- **Documentation**: Available on [docs.rs](https://docs.rs/vtcode-core)

## Contributing

Contributions are welcome! Please see the main VTCode repository for contribution guidelines.

## License

Licensed under the MIT License.
"#]

//!
//! A sophisticated terminal-based coding agent that implements state-of-the-art agent
//! architecture patterns, inspired by Anthropic's SWE-bench breakthroughs.
//!
//! ## Architecture
//!
//! The agent follows proven patterns for reliable, long-running coding assistance:
//! - **Model-Driven Control**: Maximum autonomy given to language models
//! - **Decision Transparency**: Complete audit trail of all agent actions
//! - **Error Recovery**: Intelligent error handling with context preservation
//! - **Conversation Summarization**: Automatic compression for long sessions

//! VTCode Core Library
//!
//! This crate provides the core functionality for the VTCode agent,
//! including tool implementations, LLM integration, and utility functions.

// Public modules
pub mod bash_runner;
pub mod cli;
pub mod code;
pub mod commands;
pub mod config;
pub mod constants;
pub mod core;
pub mod gemini;
pub mod llm;
pub mod markdown_storage;
pub mod models;
pub mod project;
pub mod prompts;
pub mod safety;
pub mod simple_indexer;
pub mod tool_policy;
pub mod tools;
pub mod types;
pub mod ui;
pub mod utils;

// Re-exports for convenience
pub use bash_runner::BashRunner;
pub use cli::args::{Cli, Commands};
pub use code::code_completion::{CompletionEngine, CompletionSuggestion};
pub use commands::stats::handle_stats_command;
pub use config::types::{
    AnalysisDepth, CapabilityLevel, CommandResult, CompressionLevel, ContextConfig, LoggingConfig,
    OutputFormat, PerformanceMetrics, SessionInfo, ToolConfig,
};
pub use config::{AgentConfig, VTCodeConfig};
pub use core::agent::core::Agent;
pub use core::context_compression::{
    CompressedContext, ContextCompressionConfig, ContextCompressor,
};
pub use core::conversation_summarizer::ConversationSummarizer;
pub use core::performance_profiler::PerformanceProfiler;
pub use core::prompt_caching::{CacheStats, PromptCache, PromptCacheConfig, PromptOptimizer};
pub use core::timeout_detector::TimeoutDetector;
pub use gemini::{Content, FunctionDeclaration, Part};
pub use llm::{AnyClient, make_client};
pub use markdown_storage::{MarkdownStorage, ProjectData, ProjectStorage, SimpleKVStorage};
pub use project::{SimpleCache, SimpleProjectManager};
pub use prompts::{
    generate_lightweight_instruction, generate_specialized_instruction, generate_system_instruction,
};
pub use simple_indexer::SimpleIndexer;
pub use tool_policy::{ToolPolicy, ToolPolicyManager};
pub use tools::advanced_search::{AdvancedSearchTool, SearchOptions};
pub use tools::grep_search::GrepSearchManager;
pub use tools::tree_sitter::TreeSitterAnalyzer;
pub use tools::{
    ToolRegistration, ToolRegistry, build_function_declarations,
    build_function_declarations_for_level,
};
pub use ui::diff_renderer::DiffRenderer;
pub use utils::dot_config::{
    CacheConfig, DotConfig, DotManager, ProviderConfigs, UiConfig, UserPreferences,
    WorkspaceTrustLevel, WorkspaceTrustRecord, WorkspaceTrustStore, initialize_dot_folder,
    load_user_config, save_user_config, update_theme_preference,
};
pub use utils::vtcodegitignore::initialize_vtcode_gitignore;

#[cfg(test)]
mod tests {
    use super::*;

    use tempfile::TempDir;

    #[test]
    fn test_library_exports() {
        // Test that all public exports are accessible
        let _cache = PromptCache::new();
    }

    #[test]
    fn test_module_structure() {
        // Test that all modules can be imported
        // This is a compile-time test that ensures module structure is correct
    }

    #[test]
    fn test_version_consistency() {
        // Test that version information is consistent across modules
        // This would be more meaningful with actual version checking
    }

    #[tokio::test]
    async fn test_tool_registry_integration() {
        use crate::config::constants::tools;

        let temp_dir = TempDir::new().unwrap();
        std::env::set_current_dir(&temp_dir).unwrap();

        let mut registry = ToolRegistry::new(temp_dir.path().to_path_buf());

        // Test that we can execute basic tools
        let list_args = serde_json::json!({
            "path": "."
        });

        let result = registry.execute_tool(tools::LIST_FILES, list_args).await;
        assert!(result.is_ok());

        let response: serde_json::Value = result.unwrap();
        assert!(response["files"].is_array());
    }

    #[tokio::test]
    async fn test_pty_basic_command() {
        let temp_dir = TempDir::new().unwrap();
        let workspace = temp_dir.path().to_path_buf();
        let mut registry = ToolRegistry::new(workspace.clone());

        // Test a simple PTY command
        let args = serde_json::json!({
            "command": "echo",
            "args": ["Hello, PTY!"]
        });

        let result = registry.execute_tool("run_pty_cmd", args).await;
        assert!(result.is_ok());
        let response: serde_json::Value = result.unwrap();
        assert_eq!(response["success"], true);
        assert_eq!(response["code"], 0);
        assert!(response["output"].as_str().unwrap().contains("Hello, PTY!"));
    }

    #[tokio::test]
    async fn test_pty_session_management() {
        let temp_dir = TempDir::new().unwrap();
        let workspace = temp_dir.path().to_path_buf();
        let mut registry = ToolRegistry::new(workspace.clone());

        // Test creating a PTY session
        let args = serde_json::json!({
            "session_id": "test_session",
            "command": "bash"
        });

        let result = registry.execute_tool("create_pty_session", args).await;
        assert!(result.is_ok());
        let response: serde_json::Value = result.unwrap();
        assert_eq!(response["success"], true);
        assert_eq!(response["session_id"], "test_session");

        // Test listing PTY sessions
        let args = serde_json::json!({});
        let result = registry.execute_tool("list_pty_sessions", args).await;
        assert!(result.is_ok());
        let response: serde_json::Value = result.unwrap();
        assert!(
            response["sessions"]
                .as_array()
                .unwrap()
                .contains(&"test_session".into())
        );

        // Test closing a PTY session
        let args = serde_json::json!({
            "session_id": "test_session"
        });

        let result = registry.execute_tool("close_pty_session", args).await;
        assert!(result.is_ok());
        let response: serde_json::Value = result.unwrap();
        assert_eq!(response["success"], true);
        assert_eq!(response["session_id"], "test_session");
    }
}
