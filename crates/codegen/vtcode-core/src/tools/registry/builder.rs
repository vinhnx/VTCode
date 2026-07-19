//! ToolRegistry construction helpers.

use std::future::Future;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::RwLock;

use parking_lot::Mutex;

use crate::config::PtyConfig;
use crate::core::memory_pool::MemoryPool;
use crate::tool_policy::ToolPolicyManager;
use crate::tools::handlers::PlanningWorkflowState;
use crate::tools::output_spooler::{SpoolerConfig, ToolOutputSpooler};
use crate::tools::safety_gateway::SafetyGateway;
use vtcode_config::DynamicContextConfig;
use vtcode_config::loader::ConfigManager;

use super::ToolRegistry;
use super::assembly::ToolAssembly;
use super::circuit_breaker;
use super::distributed::ToolConfigSnapshot;
use super::execution_history::ToolExecutionHistory;
use super::harness::HarnessContext;
use super::inventory::ToolInventory;
use super::pack::register_builtin_packs;
use super::policy::ToolPolicyGateway;
use super::pty;
use super::resiliency::ResiliencyContext;
use super::shell_policy::ShellPolicyChecker;
use super::timeout::ToolTimeoutPolicy;

fn spooler_config_from_dynamic_context(config: &DynamicContextConfig) -> SpoolerConfig {
    SpoolerConfig {
        enabled: config.enabled,
        threshold_bytes: config.tool_output_threshold,
        max_files: config.max_spooled_files,
        max_age_secs: config.spool_max_age_secs,
        include_file_reference: true,
    }
}

fn load_workspace_spooler_config(workspace_root: &Path) -> SpoolerConfig {
    match ConfigManager::load_from_workspace(workspace_root) {
        Ok(manager) => spooler_config_from_dynamic_context(&manager.config().context.dynamic),
        Err(err) => {
            tracing::warn!(
                workspace = %workspace_root.display(),
                error = %err,
                "Failed to load workspace config for output spooler; using defaults"
            );
            SpoolerConfig::default()
        }
    }
}

/// Load the per-tool config bits (`[web_search]`, `[web_fetch]`) from the
/// workspace `vtcode.toml`. Falls back to defaults on any load/parse error
/// so a malformed config never blocks tool registration.
fn load_tool_config(workspace_root: &Path) -> ToolConfigSnapshot {
    match ConfigManager::load_from_workspace(workspace_root) {
        Ok(manager) => ToolConfigSnapshot {
            web_search: manager.config().tools.web_search.clone(),
            web_fetch: manager.config().tools.web_fetch.clone(),
        },
        Err(err) => {
            tracing::debug!(
                workspace = %workspace_root.display(),
                error = %err,
                "No workspace vtcode.toml found; using default web tool config"
            );
            ToolConfigSnapshot::default()
        }
    }
}

impl ToolRegistry {
    pub fn new(workspace_root: PathBuf) -> impl Future<Output = Self> {
        Self::build(workspace_root, PtyConfig::default())
    }

    pub fn new_with_config(workspace_root: PathBuf, pty_config: PtyConfig) -> impl Future<Output = Self> {
        Self::build(workspace_root, pty_config)
    }

    pub fn new_with_custom_policy(
        workspace_root: PathBuf,
        policy_manager: ToolPolicyManager,
    ) -> impl Future<Output = Self> {
        Self::build_with_policy(workspace_root, PtyConfig::default(), Some(policy_manager))
    }

    pub fn new_with_custom_policy_and_config(
        workspace_root: PathBuf,
        pty_config: PtyConfig,
        policy_manager: ToolPolicyManager,
    ) -> impl Future<Output = Self> {
        Self::build_with_policy(workspace_root, pty_config, Some(policy_manager))
    }

    async fn build(workspace_root: PathBuf, pty_config: PtyConfig) -> Self {
        Self::build_with_policy(workspace_root, pty_config, None).await
    }

    async fn build_with_policy(
        workspace_root: PathBuf,
        pty_config: PtyConfig,
        policy_manager: Option<ToolPolicyManager>,
    ) -> Self {
        // Install the user-config snapshot *before* constructing the
        // inventory so `WebFetchTool`/`WebSearchTool` are built with the
        // user's allow/block lists, cooldown, and session cap rather than
        // defaulting out.
        let tool_config = load_tool_config(&workspace_root);
        if let Err(error) = super::distributed::install_tool_config(tool_config) {
            tracing::warn!(error = %error, "tool config reinstall failed; using existing snapshot");
        }

        let edited_file_monitor = Arc::new(crate::tools::edited_file_monitor::EditedFileMonitor::new());
        let inventory = ToolInventory::new(workspace_root.clone(), Arc::clone(&edited_file_monitor));
        let planning_workflow_state = PlanningWorkflowState::new(workspace_root.clone());

        register_builtin_packs(&inventory, &planning_workflow_state).await;

        let pty_sessions = pty::PtySessionManager::new(workspace_root.clone(), pty_config);
        let exec_sessions =
            crate::tools::exec_session::ExecSessionManager::new(workspace_root.clone(), pty_sessions.clone());

        let policy_gateway = match policy_manager {
            Some(pm) => ToolPolicyGateway::with_policy_manager(pm),
            None => ToolPolicyGateway::new(&workspace_root).await,
        };

        let optimization_config = vtcode_config::OptimizationConfig::default();
        let metrics = Arc::new(crate::metrics::MetricsCollector::new());
        let hot_cache_size = std::num::NonZeroUsize::new(optimization_config.tool_registry.hot_cache_size)
            .unwrap_or(std::num::NonZeroUsize::MIN);
        let output_spooler =
            Arc::new(ToolOutputSpooler::with_config(&workspace_root, load_workspace_spooler_config(&workspace_root)));

        // Pre-allocate FxHashMaps with expected capacity for typical MCP tool sets.
        // Most sessions register 10-50 MCP tools; start with room for 32 to
        // avoid rehashing during initial discovery without wasting memory.
        let mcp_tool_index = rustc_hash::FxHashMap::with_capacity_and_hasher(32, rustc_hash::FxBuildHasher);
        let mcp_reverse_index = rustc_hash::FxHashMap::with_capacity_and_hasher(32, rustc_hash::FxBuildHasher);

        let registry = Self {
            inventory,
            edited_file_monitor,
            policy_gateway: Arc::new(tokio::sync::Mutex::new(policy_gateway)),
            pty_sessions,
            exec_sessions,
            mcp_client: Arc::new(parking_lot::RwLock::new(None)),
            mcp_tool_index: Arc::new(tokio::sync::RwLock::new(mcp_tool_index)),
            mcp_reverse_index: Arc::new(tokio::sync::RwLock::new(mcp_reverse_index)),
            timeout_policy: Arc::new(parking_lot::RwLock::new(ToolTimeoutPolicy::default())),
            execution_history: ToolExecutionHistory::with_workspace_root(100, workspace_root.clone()),
            harness_context: HarnessContext::default(),
            resiliency: Arc::new(Mutex::new(ResiliencyContext::default())),
            mcp_circuit_breaker: Arc::new(circuit_breaker::McpCircuitBreaker::with_metrics(metrics.clone())),
            shared_circuit_breaker: Arc::new(RwLock::new(None)),
            initialized: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            tool_call_counter: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            pty_poll_counter: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            metrics,
            shell_policy: Arc::new(RwLock::new(ShellPolicyChecker::new())),
            runtime_sandbox_config: Arc::new(RwLock::new(super::sandbox_facade::runtime_sandbox_config_default())),
            agent_type: Arc::new(RwLock::new("unknown".to_owned())),
            cached_available_tools: Arc::new(parking_lot::RwLock::new(None)),
            active_tool_profile: Arc::new(RwLock::new(crate::config::ToolProfile::default())),
            progress_callback: Arc::new(RwLock::new(None)),
            active_pty_sessions: Arc::new(RwLock::new(None)),

            memory_pool: Arc::new(MemoryPool::from_config(&optimization_config.memory_pool)),
            hot_tool_cache: Arc::new(parking_lot::RwLock::new(lru::LruCache::new(hot_cache_size))),
            optimization_config,
            middleware: crate::tools::tool_middleware::MiddlewareChain::new(),

            output_spooler,

            planning_workflow_state,
            safety_gateway: Arc::new(SafetyGateway::default()),
            cgp_runtime_mode: Arc::new(RwLock::new(None)),
            tool_assembly: Arc::new(RwLock::new(ToolAssembly::empty())),
            tool_catalog_state: Arc::new(super::tool_catalog_facade::SessionToolCatalogState::new()),
            subagent_controller: Arc::new(RwLock::new(None)),
            session_scheduler: Arc::new(tokio::sync::Mutex::new(crate::scheduler::SessionScheduler::new())),
            session_model_tools: Arc::new(RwLock::new(None)),
            self_ref: Arc::new(RwLock::new(None)),
        };

        registry.rebuild_tool_assembly().await;
        registry.sync_policy_catalog().await;
        registry.initialize_resiliency_trackers();
        registry
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn tool_registry_uses_workspace_dynamic_context_for_output_spooler() {
        let temp = tempdir().unwrap();
        std::fs::write(
            temp.path().join("vtcode.toml"),
            r#"[context.dynamic]
enabled = true
tool_output_threshold = 4096
max_spooled_files = 7
spool_max_age_secs = 12
"#,
        )
        .unwrap();

        let registry = ToolRegistry::new(temp.path().to_path_buf()).await;
        let config = registry.output_spooler().config();

        assert!(config.enabled);
        assert_eq!(config.threshold_bytes, 4096);
        assert_eq!(config.max_files, 7);
        assert_eq!(config.max_age_secs, 12);
    }

    #[tokio::test]
    async fn tool_registry_falls_back_to_default_spooler_config_on_invalid_workspace_config() {
        let temp = tempdir().unwrap();
        std::fs::write(
            temp.path().join("vtcode.toml"),
            r#"[context.dynamic]
tool_output_threshold = "oops"
"#,
        )
        .unwrap();

        let registry = ToolRegistry::new(temp.path().to_path_buf()).await;
        let config = registry.output_spooler().config();

        assert!(config.enabled);
        assert_eq!(config.threshold_bytes, DynamicContextConfig::default().tool_output_threshold);
        assert_eq!(config.max_files, SpoolerConfig::default().max_files);
        assert_eq!(config.max_age_secs, SpoolerConfig::default().max_age_secs);
    }
}
