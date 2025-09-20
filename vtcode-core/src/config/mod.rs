//! # Configuration Management System
//!
//! This module provides a comprehensive configuration management system for VTCode,
//! handling TOML-based configuration files with support for policies, security settings,
//! and runtime customization.
//!
//! ## Architecture Overview
//!
//! The configuration system is built around several key components:
//!
//! - **TOML Configuration**: Human-readable configuration files
//! - **Layered Defaults**: Sensible defaults with user overrides
//! - **Runtime Validation**: Configuration validation and error handling
//! - **Hot Reloading**: Configuration changes without restart (where applicable)
//! - **Security Controls**: Policy-based access control and restrictions
//!
//! ## Configuration Structure
//!
//! ```toml
//! [agent]
//! max_iterations = 50
//! timeout_seconds = 300
//! enable_decision_ledger = true
//!
//! [tools]
//! max_tool_loops = 25
//! default_policy = "prompt"
//!
//! [llm.providers.gemini]
//! api_key = "your-key"
//! model = "gemini-2.5-flash"
//!
//! [security]
//! workspace_root = "/path/to/project"
//! allow_network_access = false
//! ```
//!
//! ## Basic Usage
//!
//! ```rust,no_run
//! use vtcode_core::{VTCodeConfig, AgentConfig};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // Load configuration from vtcode.toml
//!     let config = VTCodeConfig::load()?;
//!
//!     // Access specific sections
//!     println!("Max iterations: {}", config.agent.max_iterations);
//!     println!("Default tool policy: {}", config.tools.default_policy);
//!
//!     // Create agent with configuration
//!     let agent = vtcode_core::core::agent::core::Agent::new(config).await?;
//!
//!     Ok(())
//! }
//! ```
//!
//! ## Configuration Sections
//!
//! ### Agent Configuration
//! ```rust,no_run
//! use vtcode_core::config::core::AgentConfig;
//!
//! let agent_config = AgentConfig {
//!     max_iterations: 100,
//!     timeout_seconds: 600,
//!     enable_decision_ledger: true,
//!     enable_conversation_summarization: true,
//!     ..Default::default()
//! };
//! ```
//!
//! ### Tool Configuration
//! ```rust,no_run
//! use vtcode_core::config::core::{ToolsConfig, ToolPolicy};
//!
//! let tools_config = ToolsConfig {
//!     max_tool_loops: 50,
//!     default_policy: ToolPolicy::Prompt,
//!     enable_file_operations: true,
//!     enable_terminal_commands: true,
//!     ..Default::default()
//! };
//! ```
//!
//! ### Security Configuration
//! ```rust,no_run
//! use vtcode_core::config::core::SecurityConfig;
//!
//! let security_config = SecurityConfig {
//!     workspace_root: "/path/to/secure/workspace".into(),
//!     allow_network_access: false,
//!     command_allowlist: vec!["git".to_string(), "cargo".to_string()],
//!     path_restrictions: vec!["*.secret".to_string()],
//!     ..Default::default()
//! };
//! ```
//!
//! ## Runtime Configuration Management
//!
//! ```rust,no_run
//! use vtcode_core::config::loader::ConfigManager;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let mut config_manager = ConfigManager::new()?;
//!
//!     // Load configuration
//!     let config = config_manager.load_config().await?;
//!
//!     // Modify configuration at runtime
//!     config.agent.max_iterations = 75;
//!
//!     // Save changes
//!     config_manager.save_config(&config).await?;
//!
//!     Ok(())
//! }
//! ```
//!
//! ## Environment Variables
//!
//! VTCode supports configuration through environment variables:
//!
//! ```bash
//! # API Keys
//! export GEMINI_API_KEY="your-gemini-key"
//! export OPENAI_API_KEY="your-openai-key"
//! export ANTHROPIC_API_KEY="your-anthropic-key"
//!
//! # Configuration
//! export VTCode_WORKSPACE_DIR="/path/to/project"
//! export VTCode_CONFIG_PATH="/path/to/vtcode.toml"
//! ```
//!
//! ## Validation and Error Handling
//!
//! The configuration system provides comprehensive validation:
//!
//! ```rust,no_run
//! use vtcode_core::VTCodeConfig;
//!
//! match VTCodeConfig::load() {
//!     Ok(config) => {
//!         // Configuration loaded successfully
//!         println!("Configuration valid");
//!     }
//!     Err(e) => {
//!         // Handle configuration errors
//!         eprintln!("Configuration error: {}", e);
//!         // Provide helpful error messages
//!         if e.to_string().contains("missing field") {
//!             eprintln!("Hint: Check your vtcode.toml file for required fields");
//!         }
//!     }
//! }
//! ```
//!
//! ## Security Best Practices
//!
//! - **Never commit API keys** to version control
//! - **Use environment variables** for sensitive configuration
//! - **Validate workspace paths** to prevent directory traversal
//! - **Restrict command execution** to approved commands only
//! - **Enable audit logging** for security monitoring

//! VTCode Configuration Module
//!
//! This module handles loading and managing configuration from vtcode.toml files.
//! It provides a centralized way to manage agent policies, tool permissions, and
//! command allow lists.

pub mod api_keys;
pub mod constants;
pub mod context;
pub mod core;
pub mod defaults;
pub mod loader;
pub mod models;
pub mod router;
pub mod telemetry;
pub mod types;

// Re-export main types for backward compatibility
pub use context::{ContextFeaturesConfig, LedgerConfig};
pub use core::{
    AgentConfig, AutomationConfig, CommandsConfig, FullAutoConfig, SecurityConfig, ToolPolicy,
    ToolsConfig,
};
pub use defaults::{ContextStoreDefaults, PerformanceDefaults, ScenarioDefaults};
pub use loader::{ConfigManager, VTCodeConfig};
pub use router::{ComplexityModelMap, ResourceBudget, RouterConfig};
pub use telemetry::TelemetryConfig;
pub use types::ReasoningEffortLevel;

use serde::{Deserialize, Serialize};

/// PTY configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PtyConfig {
    /// Enable PTY functionality
    #[serde(default = "default_pty_enabled")]
    pub enabled: bool,

    /// Default terminal rows
    #[serde(default = "default_pty_rows")]
    pub default_rows: u16,

    /// Default terminal columns
    #[serde(default = "default_pty_cols")]
    pub default_cols: u16,

    /// Maximum number of concurrent PTY sessions
    #[serde(default = "default_max_pty_sessions")]
    pub max_sessions: usize,

    /// Command timeout in seconds
    #[serde(default = "default_pty_timeout")]
    pub command_timeout_seconds: u64,
}

impl Default for PtyConfig {
    fn default() -> Self {
        Self {
            enabled: default_pty_enabled(),
            default_rows: default_pty_rows(),
            default_cols: default_pty_cols(),
            max_sessions: default_max_pty_sessions(),
            command_timeout_seconds: default_pty_timeout(),
        }
    }
}

fn default_pty_enabled() -> bool {
    true
}
fn default_pty_rows() -> u16 {
    24
}
fn default_pty_cols() -> u16 {
    80
}
fn default_max_pty_sessions() -> usize {
    10
}
fn default_pty_timeout() -> u64 {
    300
}
