//! VT Code plugin manifest implementation
//!
//! This module implements the VT Code plugin manifest format
//! with support for commands, agents, skills, hooks, MCP, and LSP servers.

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Author information for plugin manifests
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PluginAuthor {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

/// VT Code plugin manifest
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PluginManifest {
    /// Required: Unique identifier (kebab-case, no spaces)
    pub name: String,

    /// Semantic version
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,

    /// Brief description of plugin purpose
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Author information
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<PluginAuthor>,

    /// Documentation URL
    #[serde(skip_serializing_if = "Option::is_none")]
    pub homepage: Option<String>,

    /// Source code URL
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repository: Option<String>,

    /// License identifier
    #[serde(skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,

    /// Discovery tags
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keywords: Option<Vec<String>>,

    /// Additional command files/directories
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commands: Option<Vec<String>>,

    /// Additional agent files
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agents: Option<Vec<String>>,

    /// Additional skill directories
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skills: Option<Vec<String>>,

    /// Hook config path or inline config
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hooks: Option<HookConfig>,

    /// MCP config path or inline config
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mcp_servers: Option<McpServerConfig>,

    /// Output style files/directories
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_styles: Option<Vec<String>>,

    /// LSP server configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lsp_servers: Option<LspServerConfig>,
}

/// Hook configuration for event handling
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum HookConfig {
    /// Path to hook configuration file
    Path(String),
    /// Inline hook configuration
    Inline(HookConfiguration),
}

/// MCP server configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum McpServerConfig {
    /// Path to MCP configuration file
    Path(String),
    /// Inline MCP server configuration
    Inline(HashMap<String, McpServerDefinition>),
}

/// LSP server configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum LspServerConfig {
    /// Path to LSP configuration file
    Path(String),
    /// Inline LSP server configuration
    Inline(HashMap<String, LspServerDefinition>),
}

/// Hook configuration structure
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct HookConfiguration {
    pub hooks: HashMap<String, Vec<HookDefinition>>,
}

/// Individual hook definition
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct HookDefinition {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub matcher: Option<String>,
    pub hooks: Vec<HookAction>,
}

/// Hook action types
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type")]
pub enum HookAction {
    #[serde(rename = "command")]
    Command { command: String },
    #[serde(rename = "prompt")]
    Prompt { prompt: String },
    #[serde(rename = "agent")]
    Agent { agent: String },
}

/// MCP server definition
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct McpServerDefinition {
    pub command: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub args: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub env: Option<HashMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
}

/// LSP server definition
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LspServerDefinition {
    pub command: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub args: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub env: Option<HashMap<String, String>>,
    pub extension_to_language: HashMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transport: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub initialization_options: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub settings: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_folder: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub startup_timeout: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shutdown_timeout: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub restart_on_crash: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_restarts: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logging_config: Option<LspLoggingConfig>,
}

/// LSP logging configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LspLoggingConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub args: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub env: Option<HashMap<String, String>>,
}
