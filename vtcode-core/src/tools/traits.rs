//! Core traits for the composable tool system

use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;
use std::borrow::Cow;
use std::path::PathBuf;
use std::sync::Arc;

use crate::tool_policy::ToolPolicy;
use crate::tools::result::ToolResult as SplitToolResult;

/// Core trait for all agent tools
#[async_trait]
pub trait Tool: Send + Sync {
    /// Execute the tool with given arguments
    ///
    /// Returns a JSON Value for backward compatibility.
    /// For new tools, consider implementing `execute_dual()` instead.
    async fn execute(&self, args: Value) -> Result<Value>;

    /// Execute with dual-channel output (LLM summary + UI content)
    ///
    /// This method enables significant token savings by separating:
    /// - `llm_content`: Concise summary sent to LLM context (token-optimized)
    /// - `ui_content`: Rich output displayed to user (full details)
    ///
    /// Default implementation wraps single-channel `execute()` result for backward compatibility.
    /// Tools can override this to provide optimized dual output.
    ///
    /// # Example
    /// ```rust,no_run
    /// use vtcode_core::tools::result::ToolResult as SplitToolResult;
    /// use serde_json::Value;
    /// use anyhow::Result;
    ///
    /// async fn execute_dual(&self, args: Value) -> Result<SplitToolResult> {
    ///     let full_output = "127 matches across 2,500 tokens...";
    ///     let summary = "Found 127 matches in 15 files. Key: src/tools/grep.rs (3)";
    ///     Ok(SplitToolResult::new(self.name(), summary, full_output))
    /// }
    /// ```
    async fn execute_dual(&self, args: Value) -> Result<SplitToolResult> {
        // Default: wrap single-channel result for backward compatibility
        let result = self.execute(args).await?;

        // Convert JSON Value to string for dual output
        let content = if result.is_string() {
            result.as_str().unwrap_or("").to_string()
        } else {
            serde_json::to_string_pretty(&result).unwrap_or_else(|_| result.to_string())
        };

        Ok(SplitToolResult::simple(self.name(), content))
    }

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

    // ──────────────────────────────────────────────────────────────
    // Codex-inspired methods for execution policy and parallel safety
    // ──────────────────────────────────────────────────────────────

    /// Whether this tool mutates state (files, environment, etc).
    ///
    /// Mutating tools require more careful policy evaluation and typically
    /// cannot be run in parallel with other tools that touch the same resources.
    ///
    /// Default: true (conservative - assume mutation unless overridden)
    fn is_mutating(&self) -> bool {
        true
    }

    /// Whether this tool is safe to run in parallel with other tools.
    ///
    /// Non-mutating read-only tools can often run in parallel.
    /// Mutating tools should generally return false.
    ///
    /// Default: opposite of is_mutating()
    fn is_parallel_safe(&self) -> bool {
        !self.is_mutating()
    }

    /// Get the kind/category of this tool for matching against policies.
    ///
    /// Used by ExecPolicyManager to apply category-level rules.
    /// Common kinds: "shell", "file", "search", "network", "system"
    fn kind(&self) -> &'static str {
        "unknown"
    }

    /// Check if this tool matches a given kind pattern.
    ///
    /// Supports exact matches and wildcard patterns.
    fn matches_kind(&self, pattern: &str) -> bool {
        if pattern == "*" {
            return true;
        }
        if let Some(prefix) = pattern.strip_suffix('*') {
            return self.kind().starts_with(prefix);
        }
        self.kind() == pattern
    }

    /// Resources this tool might access (paths, URLs, etc).
    ///
    /// Used for conflict detection in parallel execution planning.
    fn resource_hints(&self, _args: &Value) -> Vec<String> {
        Vec::new()
    }

    /// Estimated execution cost (1-10 scale).
    ///
    /// Used for scheduling and resource management.
    /// 1 = instant, 5 = moderate, 10 = expensive/long-running
    fn execution_cost(&self) -> u8 {
        5
    }

    /// Resolve a path relative to workspace root and validate it is within bounds
    async fn resolve_and_validate_path(
        &self,
        workspace_root: &std::path::Path,
        path: &str,
    ) -> anyhow::Result<std::path::PathBuf> {
        crate::tools::validation::unified_path::validate_and_resolve_path(workspace_root, path)
            .await
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
    async fn available_tools(&self) -> Vec<String>;

    /// Check if a tool exists
    async fn has_tool(&self, name: &str) -> bool;

    /// Execute multiple tools in batch.
    ///
    /// The default implementation runs them sequentially.
    /// Implementors can override this to provide parallel execution.
    async fn execute_batch(&self, calls: Vec<(String, Value)>) -> Vec<Result<Value>> {
        let futures = calls
            .into_iter()
            .map(|(name, args)| async move { self.execute_tool(&name, args).await });
        futures::future::join_all(futures).await
    }
}
