//! Trait interfaces for ToolRegistry subsystems.
//!
//! Each trait isolates a single responsibility from the monolithic `ToolRegistry`,
//! enabling independent testing and substitutability. Implementors can be swapped
//! for test doubles without constructing the full 35-field registry.
//!
//! # Design Principles
//!
//! - **One trait per concern**: security, PTY, MCP, resilience, catalog, metrics.
//! - **Async where needed**: methods that touch I/O or locks are `async`.
//! - **Send + Sync bounds**: all traits require thread-safety for `Arc<dyn Trait>` usage.
//! - **Return owned types**: avoid lifetime entanglement with the implementor.

use anyhow::Result;
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use crate::config::types::CapabilityLevel;
use crate::llm::provider::ToolDefinition;
use crate::llm::providers::gemini::wire::FunctionDeclaration;
use crate::tool_policy::ToolPolicy;
use crate::tools::handlers::{SessionSurface, SessionToolsConfig, ToolCallError, ToolSchemaEntry};

use super::{ToolExecutionRecord, ToolPermissionDecision, ToolRegistration};

// ============================================================================
// Security & Policy
// ============================================================================

/// Security and policy enforcement for tool execution.
///
/// Encapsulates policy evaluation, sandbox configuration, approval management,
/// and shell policy checks. Implementors can provide different security
/// postures (e.g., auto-approve for CI, prompt-for-everything in interactive).
#[async_trait::async_trait]
pub trait ToolSecurity: Send + Sync {
    /// Evaluate the permission policy for a given tool.
    async fn evaluate_tool_policy(&self, tool_name: &str) -> Result<ToolPermissionDecision>;

    /// Get the current policy for a tool.
    async fn get_tool_policy(&self, tool_name: &str) -> ToolPolicy;

    /// Set the policy for a specific tool.
    async fn set_tool_policy(&self, tool_name: &str, policy: ToolPolicy) -> Result<()>;

    /// Mark a tool as pre-approved for a single execution.
    async fn mark_tool_preapproved(&self, tool_name: &str);

    /// Enable full-auto mode with an allowlist of tools.
    async fn enable_full_auto_permission(&self, allowed_tools: &[String]);

    /// Disable full-auto mode.
    async fn disable_full_auto_permission(&self);

    /// Check if a tool is allowed in full-auto mode.
    async fn is_allowed_in_full_auto(&self, tool_name: &str) -> bool;

    /// Apply tool policies from configuration.
    async fn apply_config_policies(&self, tools_config: &crate::config::ToolsConfig) -> Result<()>;

    /// Get the current sandbox configuration.
    fn sandbox_config(&self) -> vtcode_config::SandboxConfig;

    /// Apply a sandbox configuration.
    fn apply_sandbox_config(&self, config: &vtcode_config::SandboxConfig);

    /// Persist an approval cache key for future sessions.
    async fn persist_approval_cache_key(&self, key: &str) -> Result<()>;

    /// Check if an approval has been persisted.
    async fn has_persisted_approval(&self, key: &str) -> bool;
}

// ============================================================================
// PTY Session Management
// ============================================================================

/// PTY (pseudo-terminal) session lifecycle management.
///
/// Manages creation, tracking, and teardown of PTY sessions used for
/// interactive shell command execution.
#[async_trait::async_trait]
pub trait PtySessionControl: Send + Sync {
    /// Check whether a new PTY session can be started.
    fn can_start_session(&self) -> bool;

    /// Get the number of currently active PTY sessions.
    fn active_session_count(&self) -> usize;

    /// Terminate all active PTY sessions.
    async fn terminate_all_sessions(&self) -> Result<()>;

    /// Get the exec session manager for subprocess tracking.
    fn exec_session_manager(&self) -> crate::tools::exec_session::ExecSessionManager;
}

// ============================================================================
// MCP Bridge
// ============================================================================

/// Model Context Protocol (MCP) tool bridge.
///
/// Provides access to external tools exposed via MCP servers. Handles
/// client lifecycle, tool discovery, and execution delegation.
#[async_trait::async_trait]
pub trait McpBridge: Send + Sync {
    /// Set or replace the MCP client.
    async fn set_mcp_client(&self, client: Arc<crate::mcp::McpClient>);

    /// Clear the current MCP client and all cached tool indexes.
    async fn clear_mcp_client(&self);

    /// Get the current MCP client, if any.
    fn mcp_client(&self) -> Option<Arc<crate::mcp::McpClient>>;

    /// List all tools available via MCP.
    async fn list_mcp_tools(&self) -> Result<Vec<crate::mcp::McpToolInfo>>;

    /// Check if a tool name corresponds to an MCP-provided tool.
    async fn has_mcp_tool(&self, tool_name: &str) -> bool;

    /// Execute a tool via the MCP client.
    async fn execute_mcp_tool(&self, tool_name: &str, args: Value) -> Result<Value>;

    /// Refresh MCP tool registrations from all connected servers.
    async fn refresh_mcp_tools(&self) -> Result<()>;
}

// ============================================================================
// Resilience (Circuit Breakers, Timeouts, Retries)
// ============================================================================

/// Execution resilience: circuit breakers, adaptive timeouts, failure tracking.
///
/// Protects the system from cascading failures by tracking tool execution
/// health and applying backpressure when tools become unreliable.
pub trait ToolResilience: Send + Sync {
    /// Get the effective timeout for a tool category, considering adaptive tuning.
    fn effective_timeout(&self, category: super::ToolTimeoutCategory) -> Option<Duration>;

    /// Record a tool execution failure and check if circuit breaking should activate.
    /// Returns `true` if the circuit breaker tripped.
    fn record_failure(&self, category: super::ToolTimeoutCategory) -> bool;

    /// Reset failure tracking for a tool category (e.g., after a success streak).
    fn reset_failure(&self, category: super::ToolTimeoutCategory);

    /// Record a tool execution latency for adaptive timeout tuning.
    fn record_latency(&self, category: super::ToolTimeoutCategory, duration: Duration);

    /// Check if the circuit breaker is tripped for a category. Returns the
    /// recommended backoff duration if so.
    fn should_circuit_break(&self, category: super::ToolTimeoutCategory) -> Option<Duration>;

    /// Decay adaptive timeouts after a success streak (relax backpressure).
    fn decay_adaptive_timeout(&self, category: super::ToolTimeoutCategory);
}

// ============================================================================
// Tool Catalog (Registration, Lookup, Schema)
// ============================================================================

/// Tool catalog: registration, lookup, and schema access.
///
/// The catalog is the source of truth for which tools are available and how
/// to invoke them. It supports dynamic registration (e.g., MCP tools added
/// at runtime) and provides schema information for LLM tool-calling.
#[async_trait::async_trait]
pub trait ToolCatalog: Send + Sync {
    /// Register a tool. Replaces any existing registration with the same name.
    async fn register_tool(&self, registration: ToolRegistration) -> Result<()>;

    /// Unregister a tool by name. Returns `true` if the tool existed.
    async fn unregister_tool(&self, name: &str) -> Result<bool>;

    /// Get a tool by name (with hot-cache optimization).
    fn get_tool(&self, name: &str) -> Option<Arc<dyn crate::tools::traits::Tool>>;

    /// Get the workspace root path.
    fn workspace_root(&self) -> PathBuf;

    /// List public tool names for a given surface and capability level.
    async fn public_tool_names(
        &self,
        surface: SessionSurface,
        capability_level: CapabilityLevel,
    ) -> Vec<String>;

    /// Get schema entries for all available tools.
    async fn schema_entries(&self, config: SessionToolsConfig) -> Vec<ToolSchemaEntry>;

    /// Get Gemini-style function declarations for tool-calling.
    async fn function_declarations(&self, config: SessionToolsConfig) -> Vec<FunctionDeclaration>;

    /// Get OpenAI/Anthropic-style tool definitions.
    async fn model_tools(&self, config: SessionToolsConfig) -> Vec<ToolDefinition>;

    /// Resolve a public tool name to its canonical registration name.
    fn resolve_tool_name(&self, name: &str) -> Result<String, ToolCallError>;
}

// ============================================================================
// Metrics & Execution History
// ============================================================================

/// Tool execution metrics and history tracking.
///
/// Records execution outcomes for observability, debugging, and loop detection.
pub trait ToolMetrics: Send + Sync {
    /// Record a tool execution for history and metrics.
    fn record_execution(&self, record: ToolExecutionRecord);

    /// Get the total number of tool calls in this session.
    fn call_count(&self) -> u64;

    /// Get the total PTY poll iterations (for CPU monitoring).
    fn pty_poll_count(&self) -> u64;

    /// Get the shared metrics collector for external observability.
    fn metrics_collector(&self) -> Arc<crate::metrics::MetricsCollector>;
}

// ============================================================================
// Composite Supertrait
// ============================================================================

/// Full tool registry API: the union of all subsystem traits.
///
/// Use this as a bound when a consumer needs access to the complete registry
/// surface. For narrower needs, prefer the individual traits
/// (`ToolCatalog`, `ToolSecurity`, etc.) to reduce coupling.
///
/// # Migration Guide
///
/// Current code passes `Arc<ToolRegistry>` or `&ToolRegistry` directly.
/// To migrate a consumer to the trait-based interface:
///
/// 1. Replace `Arc<ToolRegistry>` with `Arc<dyn ToolRegistryApi>`.
/// 2. Replace `&ToolRegistry` with `&dyn ToolRegistryApi`.
/// 3. The consumer can now be tested with a mock that implements
///    only the traits it actually uses.
///
/// `ToolRegistry` implements this supertrait automatically.
pub trait ToolRegistryApi:
    ToolSecurity
    + PtySessionControl
    + McpBridge
    + ToolResilience
    + ToolCatalog
    + ToolMetrics
    + Send
    + Sync
    + 'static
{
}

/// Blanket impl: any type that implements all subsystem traits gets
/// `ToolRegistryApi` for free.
impl<T> ToolRegistryApi for T where
    T: ToolSecurity
        + PtySessionControl
        + McpBridge
        + ToolResilience
        + ToolCatalog
        + ToolMetrics
        + Send
        + Sync
        + 'static
{
}

/// Type alias for a shared, dynamically-dispatched tool registry.
///
/// Use this in struct fields and function signatures when the concrete
/// `ToolRegistry` type is not needed. This enables test doubles.
pub type SharedRegistry = Arc<dyn ToolRegistryApi>;
