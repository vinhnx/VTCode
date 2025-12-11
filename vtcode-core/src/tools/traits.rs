//! Core traits for the composable tool system

use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;
use std::borrow::Cow;
use std::path::PathBuf;
use std::sync::Arc;

use crate::tool_policy::ToolPolicy;

/// Core trait for all agent tools
#[async_trait]
pub trait Tool: Send + Sync {
    /// Execute the tool with given arguments
    async fn execute(&self, args: Value) -> Result<Value>;

    /// Get the tool's name
    fn name(&self) -> &'static str;

    /// Get the tool's description
    fn description(&self) -> &'static str;

    /// Validate arguments before execution
    fn validate_args(&self, _args: &Value) -> Result<()> {
        // Default implementation - tools can override for specific validation
        Ok(())
    }

    /// Optional JSON schema for the tool's parameters, if available.
    fn parameter_schema(&self) -> Option<Value> {
        None
    }

    /// Optional JSON schema for the tool's configuration, if available.
    fn config_schema(&self) -> Option<Value> {
        None
    }

    /// Optional JSON schema describing state persisted by the tool, if any.
    fn state_schema(&self) -> Option<Value> {
        None
    }

    /// Optional prompt path metadata (e.g., for loading companion prompts).
    fn prompt_path(&self) -> Option<Cow<'static, str>> {
        None
    }

    /// Default execution policy for this tool.
    fn default_permission(&self) -> ToolPolicy {
        ToolPolicy::Prompt
    }

    /// Optional allowlist patterns the tool considers pre-approved.
    fn allow_patterns(&self) -> Option<&'static [&'static str]> {
        None
    }

    /// Optional denylist patterns the tool considers blocked.
    fn deny_patterns(&self) -> Option<&'static [&'static str]> {
        None
    }
}

/// Trait for tools that operate on files
#[async_trait]
pub trait FileTool: Tool {
    /// Get the workspace root
    fn workspace_root(&self) -> &PathBuf;

    /// Check if a path should be excluded
    async fn should_exclude(&self, path: &std::path::Path) -> bool;
}

/// Trait for tools that support multiple execution modes
#[async_trait]
pub trait ModeTool: Tool {
    /// Get supported modes
    fn supported_modes(&self) -> Vec<&'static str>;

    /// Execute with specific mode
    async fn execute_mode(&self, mode: &str, args: Value) -> Result<Value>;
}

/// Trait for caching tool results
#[async_trait]
pub trait CacheableTool: Tool {
    /// Generate cache key for given arguments
    fn cache_key(&self, args: &Value) -> String;

    /// Check if result should be cached
    fn should_cache(&self, _args: &Value) -> bool {
        true // Default: cache everything
    }

    /// Get cache TTL in seconds
    fn cache_ttl(&self) -> u64 {
        300 // Default: 5 minutes
    }
}

/// Main tool executor that coordinates all tools
#[async_trait]
pub trait ToolExecutor: Send + Sync {
    /// Execute a tool by name
    async fn execute_tool(&self, name: &str, args: Value) -> Result<Value>;

    /// Execute a tool with a reference to arguments to avoid cloning when caller
    /// already holds a reference.
    async fn execute_tool_ref(&self, name: &str, args: &Value) -> Result<Value> {
        self.execute_tool(name, args.clone()).await
    }

    /// Execute a tool and return a shared result (Arc) to avoid cloning results
    /// for callers that want to keep a shared reference.
    async fn execute_shared(&self, name: &str, args: Arc<Value>) -> Result<Arc<Value>> {
        let res = self
            .execute_tool(
                name,
                Arc::try_unwrap(args).unwrap_or_else(|arc| (*arc).clone()),
            )
            .await?;
        Ok(Arc::new(res))
    }

    /// List available tools
    fn available_tools(&self) -> Vec<String>;

    /// Check if a tool exists
    fn has_tool(&self, name: &str) -> bool;
}
