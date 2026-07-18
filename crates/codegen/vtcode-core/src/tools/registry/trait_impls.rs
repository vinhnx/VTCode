//! Trait implementations for ToolRegistry.
//!
//! Each impl delegates to the existing facade methods, adding no new logic.
//! This file exists to consolidate the interface boundary in one place
//! so that the delegation chain is auditable and the subsystem decomposition
//! in Phase 2 can replace these with direct subsystem calls.

use anyhow::Result;
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use crate::config::types::CapabilityLevel;
use crate::llm::provider::ToolDefinition;
use crate::llm::providers::gemini::wire::FunctionDeclaration;
use crate::mcp::{McpClient, McpToolInfo};
use crate::tool_policy::ToolPolicy;
use crate::tools::exec_session::ExecSessionManager;
use crate::tools::handlers::{SessionSurface, SessionToolsConfig, ToolCallError, ToolSchemaEntry};
use crate::tools::registry::{ToolExecutionRecord, ToolPermissionDecision, ToolRegistration, ToolTimeoutCategory};

use super::ToolRegistry;
use super::interfaces::{McpBridge, PtySessionControl, ToolCatalog, ToolMetrics, ToolResilience, ToolSecurity};

// ============================================================================
// ToolSecurity
// ============================================================================

#[async_trait::async_trait]
impl ToolSecurity for ToolRegistry {
    async fn evaluate_tool_policy(&self, tool_name: &str) -> Result<ToolPermissionDecision> {
        self.evaluate_tool_policy(tool_name).await
    }

    async fn get_tool_policy(&self, tool_name: &str) -> ToolPolicy {
        self.get_tool_policy(tool_name).await
    }

    async fn set_tool_policy(&self, tool_name: &str, policy: ToolPolicy) -> Result<()> {
        self.set_tool_policy(tool_name, policy).await
    }

    async fn mark_tool_preapproved(&self, tool_name: &str) {
        self.mark_tool_preapproved(tool_name).await;
    }

    async fn enable_full_auto_permission(&self, allowed_tools: &[String]) {
        self.enable_full_auto_permission(allowed_tools).await;
    }

    async fn disable_full_auto_permission(&self) {
        self.disable_full_auto_permission().await;
    }

    async fn is_allowed_in_full_auto(&self, tool_name: &str) -> bool {
        self.is_allowed_in_full_auto(tool_name).await
    }

    async fn apply_config_policies(&self, tools_config: &crate::config::ToolsConfig) -> Result<()> {
        self.apply_config_policies(tools_config).await
    }

    fn sandbox_config(&self) -> vtcode_config::SandboxConfig {
        self.sandbox_config()
    }

    fn apply_sandbox_config(&self, config: &vtcode_config::SandboxConfig) {
        self.apply_sandbox_config(config);
    }

    async fn persist_approval_cache_key(&self, key: &str) -> Result<()> {
        self.persist_approval_cache_key(key).await
    }

    async fn has_persisted_approval(&self, key: &str) -> bool {
        self.has_persisted_approval(key).await
    }
}

// ============================================================================
// PtySessionControl
// ============================================================================

#[async_trait::async_trait]
impl PtySessionControl for ToolRegistry {
    fn can_start_session(&self) -> bool {
        self.can_start_pty_session()
    }

    fn active_session_count(&self) -> usize {
        self.active_pty_sessions()
    }

    async fn terminate_all_sessions(&self) -> Result<()> {
        self.terminate_all_pty_sessions_async().await
    }

    fn exec_session_manager(&self) -> ExecSessionManager {
        self.exec_session_manager()
    }
}

// ============================================================================
// McpBridge
// ============================================================================

#[async_trait::async_trait]
impl McpBridge for ToolRegistry {
    async fn set_mcp_client(&self, client: Arc<McpClient>) {
        self.set_mcp_client(client).await;
    }

    async fn clear_mcp_client(&self) {
        self.clear_mcp_client().await;
    }

    fn mcp_client(&self) -> Option<Arc<McpClient>> {
        self.mcp_client()
    }

    async fn list_mcp_tools(&self) -> Result<Vec<McpToolInfo>> {
        self.list_mcp_tools().await
    }

    async fn has_mcp_tool(&self, tool_name: &str) -> bool {
        self.has_mcp_tool(tool_name).await
    }

    async fn execute_mcp_tool(&self, tool_name: &str, args: Value) -> Result<Value> {
        self.execute_mcp_tool(tool_name, args).await
    }

    async fn refresh_mcp_tools(&self) -> Result<()> {
        self.refresh_mcp_tools().await
    }
}

// ============================================================================
// ToolResilience
// ============================================================================

impl ToolResilience for ToolRegistry {
    fn effective_timeout(&self, category: ToolTimeoutCategory) -> Option<Duration> {
        self.effective_timeout(category)
    }

    fn record_failure(&self, category: ToolTimeoutCategory) -> bool {
        self.record_tool_failure(category)
    }

    fn reset_failure(&self, category: ToolTimeoutCategory) {
        self.reset_tool_failure(category);
    }

    fn record_latency(&self, category: ToolTimeoutCategory, duration: Duration) {
        self.record_tool_latency(category, duration);
    }

    fn should_circuit_break(&self, category: ToolTimeoutCategory) -> Option<Duration> {
        self.should_circuit_break(category)
    }

    fn decay_adaptive_timeout(&self, category: ToolTimeoutCategory) {
        self.decay_adaptive_timeout(category);
    }
}

// ============================================================================
// ToolCatalog
// ============================================================================

#[async_trait::async_trait]
impl ToolCatalog for ToolRegistry {
    async fn register_tool(&self, registration: ToolRegistration) -> Result<()> {
        self.register_tool(registration).await
    }

    async fn unregister_tool(&self, name: &str) -> Result<bool> {
        self.unregister_tool(name).await
    }

    fn get_tool(&self, name: &str) -> Option<Arc<dyn crate::tools::traits::Tool>> {
        self.get_tool(name)
    }

    fn workspace_root(&self) -> PathBuf {
        self.workspace_root_owned()
    }

    async fn public_tool_names(&self, surface: SessionSurface, capability_level: CapabilityLevel) -> Vec<String> {
        self.public_tool_names(surface, capability_level).await
    }

    async fn schema_entries(&self, config: SessionToolsConfig) -> Vec<ToolSchemaEntry> {
        self.schema_entries(config).await
    }

    async fn function_declarations(&self, config: SessionToolsConfig) -> Vec<FunctionDeclaration> {
        self.function_declarations(config).await
    }

    async fn model_tools(&self, config: SessionToolsConfig) -> Vec<ToolDefinition> {
        self.model_tools(config).await
    }

    fn resolve_tool_name(&self, name: &str) -> Result<String, ToolCallError> {
        self.resolve_public_tool_name(name)
    }
}

// ============================================================================
// ToolMetrics
// ============================================================================

impl ToolMetrics for ToolRegistry {
    fn record_execution(&self, _record: ToolExecutionRecord) {
        // ToolExecutionHistory is managed internally by the execution facade.
        // This hook is available for external metric sinks in Phase 2.
    }

    fn call_count(&self) -> u64 {
        self.tool_call_count()
    }

    fn pty_poll_count(&self) -> u64 {
        self.pty_poll_count()
    }

    fn metrics_collector(&self) -> Arc<crate::metrics::MetricsCollector> {
        self.metrics_collector()
    }
}
