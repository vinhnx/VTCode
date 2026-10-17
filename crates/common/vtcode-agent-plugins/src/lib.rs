#![allow(
    missing_docs,
    clippy::expect_used,
    dead_code,
    unused_imports,
    reason = "Intentional compatibility, platform, or test-only suppression."
)]
//! Agent Plugins loader for VT Code.
//!
//! Portable plugin format: a directory with a root `plugin.json` manifest,
//! optional `skills/*/SKILL.md` Agent Skills, and optional `mcp.json` MCP
//! server configuration. Clients discover, validate, and load components
//! according to the [Agent Plugins specification](https://agent-plugins.org/specification).

pub mod discovery;
pub mod errors;
pub mod expansion;
pub mod loader;
pub mod manifest;
pub mod mcp;

pub use discovery::{DiscoveredSkill, LoadedPlugin};
pub use errors::PluginError;
pub use expansion::{
    default_plugin_data_dir, expand_placeholders, is_reserved_env_key, resolve_cwd, validate_command_token,
    validate_cwd_form, validate_plugin_relative,
};
pub use loader::{FileSystemPluginInstaller, FileSystemPluginLoader, PluginInstaller, PluginLoader, plugin_roots_for};
pub use manifest::{PluginAuthor, PluginManifest};
pub use mcp::{HttpServerConfig, McpConfig, ServerConfig, SseServerConfig, StdioServerConfig};
