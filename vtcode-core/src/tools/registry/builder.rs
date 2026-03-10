//! ToolRegistry construction helpers.

use std::borrow::Cow;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::RwLock;

use parking_lot::Mutex;

use crate::config::PtyConfig;
use crate::core::memory_pool::MemoryPool;
use crate::tool_policy::ToolPolicyManager;
use crate::tools::handlers::PlanModeState;
use crate::tools::output_spooler::ToolOutputSpooler;

use super::ToolRegistry;
use super::assembly::ToolAssembly;
use super::builtins::register_builtin_tools;
use super::circuit_breaker;
use super::execution_history::ToolExecutionHistory;
use super::harness::HarnessContext;
use super::inventory::ToolInventory;
use super::policy::ToolPolicyGateway;
use super::pty;
use super::resiliency::ResiliencyContext;
use super::shell_policy::ShellPolicyChecker;
use super::timeout::ToolTimeoutPolicy;

impl ToolRegistry {
    pub async fn new(workspace_root: PathBuf) -> Self {
        Self::build(workspace_root, PtyConfig::default()).await
    }

    pub async fn new_with_config(workspace_root: PathBuf, pty_config: PtyConfig) -> Self {
        Self::build(workspace_root, pty_config).await
    }

    pub async fn new_with_custom_policy(
        workspace_root: PathBuf,
        policy_manager: ToolPolicyManager,
    ) -> Self {
        Self::build_with_policy(workspace_root, PtyConfig::default(), Some(policy_manager)).await
    }

    pub async fn new_with_custom_policy_and_config(
        workspace_root: PathBuf,
        pty_config: PtyConfig,
        policy_manager: ToolPolicyManager,
    ) -> Self {
        Self::build_with_policy(workspace_root, pty_config, Some(policy_manager)).await
    }

    async fn build(workspace_root: PathBuf, pty_config: PtyConfig) -> Self {
        Self::build_with_policy(workspace_root, pty_config, None).await
    }

    async fn build_with_policy(
        workspace_root: PathBuf,
        pty_config: PtyConfig,
        policy_manager: Option<ToolPolicyManager>,
    ) -> Self {
        let edited_file_monitor =
            Arc::new(crate::tools::edited_file_monitor::EditedFileMonitor::new());
        let inventory = ToolInventory::new(
            workspace_root.clone(),
            Arc::clone(&edited_file_monitor),
        );
        let plan_mode_state = PlanModeState::new(workspace_root.clone());

        register_builtin_tools(&inventory, &plan_mode_state);

        let pty_sessions = pty::PtySessionManager::new(workspace_root.clone(), pty_config);
        let exec_sessions = crate::tools::exec_session::ExecSessionManager::new(
            workspace_root.clone(),
            pty_sessions.clone(),
        );

        let policy_gateway = match policy_manager {
            Some(pm) => ToolPolicyGateway::with_policy_manager(pm),
            None => ToolPolicyGateway::new(&workspace_root).await,
        };

        let optimization_config = vtcode_config::OptimizationConfig::default();
        let metrics = Arc::new(crate::metrics::MetricsCollector::new());
        let hot_cache_size =
            std::num::NonZeroUsize::new(optimization_config.tool_registry.hot_cache_size)
                .unwrap_or(std::num::NonZeroUsize::MIN);

        let registry = Self {
            inventory,
            edited_file_monitor,
            policy_gateway: Arc::new(tokio::sync::RwLock::new(policy_gateway)),
            pty_sessions,
            exec_sessions,
            mcp_client: Arc::new(RwLock::new(None)),
            mcp_tool_index: Arc::new(tokio::sync::RwLock::new(rustc_hash::FxHashMap::default())),
            mcp_reverse_index: Arc::new(tokio::sync::RwLock::new(rustc_hash::FxHashMap::default())),
            timeout_policy: Arc::new(RwLock::new(ToolTimeoutPolicy::default())),
            execution_history: ToolExecutionHistory::new(100),
            harness_context: HarnessContext::default(),
            resiliency: Arc::new(Mutex::new(ResiliencyContext::default())),
            mcp_circuit_breaker: Arc::new(circuit_breaker::McpCircuitBreaker::with_metrics(
                metrics.clone(),
            )),
            shared_circuit_breaker: Arc::new(RwLock::new(None)),
            initialized: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            tool_call_counter: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            pty_poll_counter: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            metrics,
            shell_policy: Arc::new(RwLock::new(ShellPolicyChecker::new())),
            runtime_sandbox_config: Arc::new(RwLock::new(
                super::sandbox_facade::runtime_sandbox_config_default(),
            )),
            agent_type: Arc::new(RwLock::new(Cow::Borrowed("unknown"))),
            cached_available_tools: Arc::new(RwLock::new(None)),
            progress_callback: Arc::new(RwLock::new(None)),
            active_pty_sessions: Arc::new(RwLock::new(None)),

            memory_pool: Arc::new(MemoryPool::from_config(&optimization_config.memory_pool)),
            hot_tool_cache: Arc::new(parking_lot::RwLock::new(lru::LruCache::new(hot_cache_size))),
            optimization_config,

            output_spooler: Arc::new(ToolOutputSpooler::new(&workspace_root)),

            plan_read_only_mode: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            plan_mode_state,
            tool_assembly: Arc::new(RwLock::new(ToolAssembly::empty())),
        };

        registry.rebuild_tool_assembly().await;
        registry.sync_policy_catalog().await;
        registry.initialize_resiliency_trackers();
        registry
    }
}
