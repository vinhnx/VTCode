//! Tool registry and function declarations

mod approval_recorder;
mod builtins;
mod cache;
mod declarations;
mod error;
mod executors;
mod file_helpers;
mod inventory;
mod justification;
mod justification_extractor;
mod policy;
mod pty;
mod registration;
mod risk_scorer;
mod shell_policy;
mod telemetry;
mod utils;

use std::borrow::Cow;
use std::env;

pub use approval_recorder::ApprovalRecorder;
pub use declarations::{
    build_function_declarations, build_function_declarations_for_level,
    build_function_declarations_with_mode,
};
pub use error::{ToolErrorType, ToolExecutionError, classify_error};
pub use justification::{ApprovalPattern, JustificationManager, ToolJustification};
pub use justification_extractor::JustificationExtractor;
pub use pty::{PtySessionGuard, PtySessionManager};
pub use registration::{ToolExecutorFn, ToolHandler, ToolRegistration};
pub use risk_scorer::{RiskLevel, ToolRiskContext, ToolRiskScorer, ToolSource, WorkspaceTrust};
pub use shell_policy::ShellPolicyChecker;
pub use telemetry::ToolTelemetryEvent;

use builtins::register_builtin_tools;
use inventory::ToolInventory;
use policy::ToolPolicyGateway;
use utils::normalize_tool_output;

use crate::config::constants::defaults;
use crate::config::constants::tools;
use crate::config::{CommandsConfig, PtyConfig, TimeoutsConfig, ToolsConfig};
use crate::tool_policy::{ToolPolicy, ToolPolicyManager};
use crate::tools::file_ops::FileOpsTool;
use crate::tools::grep_file::GrepSearchManager;
use crate::tools::mcp::build_mcp_registration;
use crate::tools::names::canonical_tool_name;
use crate::tools::pty::PtyManager;
use crate::tools::{PlanSummary, TaskPlan};
use anyhow::Result;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{debug, trace, warn};

use super::plan::PlanManager;
use crate::mcp::{McpClient, McpToolExecutor, McpToolInfo};
use std::collections::VecDeque;
use std::sync::RwLock;
use std::time::SystemTime;

#[cfg(test)]
use super::traits::Tool;
#[cfg(test)]
use crate::config::types::CapabilityLevel;

/// Record of a tool execution for diagnostics
#[derive(Debug, Clone)]
pub struct HarnessContextSnapshot {
    pub session_id: String,
    pub task_id: Option<String>,
    pub plan_version: u64,
    pub plan_summary: PlanSummary,
}

impl HarnessContextSnapshot {
    pub fn new(
        session_id: String,
        task_id: Option<String>,
        plan_version: u64,
        plan_summary: PlanSummary,
    ) -> Self {
        Self {
            session_id,
            task_id,
            plan_version,
            plan_summary,
        }
    }

    /// Serialize snapshot for middleware/telemetry consumers without cloning callers
    pub fn to_json(&self) -> Value {
        json!({
            "session_id": self.session_id,
            "task_id": self.task_id,
            "plan_version": self.plan_version,
            "plan_summary": {
                "status": self.plan_summary.status.label(),
                "total_steps": self.plan_summary.total_steps,
                "completed_steps": self.plan_summary.completed_steps,
            }
        })
    }
}

#[derive(Debug, Clone)]
pub struct ToolExecutionRecord {
    pub tool_name: String,
    pub requested_name: String,
    pub is_mcp: bool,
    pub mcp_provider: Option<String>,
    pub args: Value,
    pub result: Result<Value, String>, // Ok(result) or Err(error_message)
    pub timestamp: SystemTime,
    pub success: bool,
    pub context: HarnessContextSnapshot,
    pub timeout_category: Option<String>,
    pub base_timeout_ms: Option<u64>,
    pub adaptive_timeout_ms: Option<u64>,
    pub effective_timeout_ms: Option<u64>,
    pub circuit_breaker: bool,
}

impl ToolExecutionRecord {
    /// Create a new failed execution record
    #[allow(clippy::too_many_arguments)]
    #[inline]
    pub fn failure(
        tool_name: String,
        requested_name: String,
        is_mcp: bool,
        mcp_provider: Option<String>,
        args: Value,
        error_msg: String,
        context: HarnessContextSnapshot,
        timeout_category: Option<String>,
        base_timeout_ms: Option<u64>,
        adaptive_timeout_ms: Option<u64>,
        effective_timeout_ms: Option<u64>,
        circuit_breaker: bool,
    ) -> Self {
        Self {
            tool_name,
            requested_name,
            is_mcp,
            mcp_provider,
            args,
            result: Err(error_msg),
            timestamp: SystemTime::now(),
            success: false,
            context,
            timeout_category,
            base_timeout_ms,
            adaptive_timeout_ms,
            effective_timeout_ms,
            circuit_breaker,
        }
    }

    /// Create a new successful execution record
    #[allow(clippy::too_many_arguments)]
    #[inline]
    pub fn success(
        tool_name: String,
        requested_name: String,
        is_mcp: bool,
        mcp_provider: Option<String>,
        args: Value,
        result: Value,
        context: HarnessContextSnapshot,
        timeout_category: Option<String>,
        base_timeout_ms: Option<u64>,
        adaptive_timeout_ms: Option<u64>,
        effective_timeout_ms: Option<u64>,
        circuit_breaker: bool,
    ) -> Self {
        Self {
            tool_name,
            requested_name,
            is_mcp,
            mcp_provider,
            args,
            result: Ok(result),
            timestamp: SystemTime::now(),
            success: true,
            context,
            timeout_category,
            base_timeout_ms,
            adaptive_timeout_ms,
            effective_timeout_ms,
            circuit_breaker,
        }
    }
}

/// Thread-safe execution history for recording tool executions
const DEFAULT_LOOP_DETECT_WINDOW: usize = 5;
const MIN_READONLY_IDENTICAL_LIMIT: usize = 5;
#[derive(Clone)]
pub struct ToolExecutionHistory {
    records: Arc<RwLock<VecDeque<ToolExecutionRecord>>>,
    max_records: usize,
    detect_window: usize,
    identical_limit: usize,
    rate_limit_per_minute: Option<usize>,
}

impl ToolExecutionHistory {
    pub fn new(max_records: usize) -> Self {
        Self {
            records: Arc::new(RwLock::new(VecDeque::new())),
            max_records,
            detect_window: DEFAULT_LOOP_DETECT_WINDOW,
            identical_limit: defaults::DEFAULT_MAX_REPEATED_TOOL_CALLS,
            rate_limit_per_minute: tool_rate_limit_from_env(),
        }
    }

    pub fn add_record(&self, record: ToolExecutionRecord) {
        let mut records = self.records.write().unwrap();
        records.push_back(record);
        while records.len() > self.max_records {
            records.pop_front();
        }
    }

    pub fn set_loop_detection_limits(&mut self, detect_window: usize, identical_limit: usize) {
        self.detect_window = detect_window.max(1);
        self.identical_limit = identical_limit;
    }

    pub fn set_rate_limit_per_minute(&mut self, limit: Option<usize>) {
        self.rate_limit_per_minute = limit.filter(|value| *value > 0);
    }

    pub fn get_recent_records(&self, count: usize) -> Vec<ToolExecutionRecord> {
        let records = self.records.read().unwrap();
        let records_len = records.len();
        let start = records_len.saturating_sub(count);
        records.iter().skip(start).cloned().collect()
    }

    pub fn get_recent_failures(&self, count: usize) -> Vec<ToolExecutionRecord> {
        let records = self.records.read().unwrap();
        // Collect in reverse order and reverse at the end for chronological order
        let mut failures: Vec<ToolExecutionRecord> = records
            .iter()
            .rev() // Go from newest to oldest
            .filter(|r| !r.success)
            .take(count)
            .cloned()
            .collect();
        // Reverse to get chronological order (oldest to newest)
        failures.reverse();
        failures
    }

    pub fn clear(&self) {
        let mut records = self.records.write().unwrap();
        records.clear();
    }

    pub fn loop_limit(&self) -> usize {
        self.identical_limit
    }

    pub fn loop_limit_for(&self, tool_name: &str) -> usize {
        self.effective_identical_limit_for_tool(tool_name)
    }

    pub fn rate_limit_per_minute(&self) -> Option<usize> {
        self.rate_limit_per_minute
    }

    fn effective_identical_limit_for_tool(&self, tool_name: &str) -> usize {
        match tool_name {
            tools::READ_FILE | tools::GREP_FILE | tools::LIST_FILES => {
                self.identical_limit.max(MIN_READONLY_IDENTICAL_LIMIT)
            }
            _ => self.identical_limit,
        }
    }

    pub fn calls_in_window(&self, window: Duration) -> usize {
        let cutoff = SystemTime::now()
            .checked_sub(window)
            .unwrap_or(SystemTime::UNIX_EPOCH);

        let records = self.records.read().unwrap();
        records
            .iter()
            .rev()
            .take_while(|record| record.timestamp >= cutoff)
            .count()
    }

    /// Detect if the agent is stuck in a loop (calling the same tool repeatedly with identical params)
    ///
    /// Returns (is_loop, repeat_count, tool_name) if a loop is detected
    pub fn detect_loop(&self, tool_name: &str, args: &Value) -> (bool, usize, String) {
        let limit = self.effective_identical_limit_for_tool(tool_name);
        if limit == 0 {
            return (false, 0, String::new());
        }

        let window = self.detect_window.max(limit.saturating_mul(2)).max(1);

        let records = self.records.read().unwrap();

        // Look at the recent calls within the configured window
        let recent: Vec<&ToolExecutionRecord> = records.iter().rev().take(window).collect();

        if recent.is_empty() {
            return (false, 0, String::new());
        }

        // Count how many of the recent calls match this exact tool + args combo
        // CRITICAL FIX: Only count SUCCESSFUL calls to avoid cascade blocking
        // When a call fails due to loop detection, it shouldn't count toward future loop detection
        let mut identical_count = 0;
        for record in &recent {
            if record.tool_name == tool_name && record.args == *args && record.success {
                identical_count += 1;
            }
        }

        // If we've called this exact combination at or above the configured limit, it's a loop
        let is_loop = identical_count >= limit;

        (is_loop, identical_count, tool_name.to_string())
    }
}

fn tool_rate_limit_from_env() -> Option<usize> {
    env::var("VTCODE_TOOL_CALLS_PER_MIN")
        .ok()
        .and_then(|raw| raw.trim().parse::<usize>().ok())
        .filter(|value| *value > 0)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ToolTimeoutCategory {
    Default,
    Pty,
    Mcp,
}

impl ToolTimeoutCategory {
    pub fn label(&self) -> &'static str {
        match self {
            ToolTimeoutCategory::Default => "standard",
            ToolTimeoutCategory::Pty => "PTY",
            ToolTimeoutCategory::Mcp => "MCP",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ToolTimeoutPolicy {
    default_ceiling: Option<Duration>,
    pty_ceiling: Option<Duration>,
    mcp_ceiling: Option<Duration>,
    warning_fraction: f32,
}

impl Default for ToolTimeoutPolicy {
    fn default() -> Self {
        Self {
            default_ceiling: Some(Duration::from_secs(180)),
            pty_ceiling: Some(Duration::from_secs(300)),
            mcp_ceiling: Some(Duration::from_secs(120)),
            warning_fraction: 0.8,
        }
    }
}

impl ToolTimeoutPolicy {
    pub fn from_config(config: &TimeoutsConfig) -> Self {
        Self {
            default_ceiling: config.ceiling_duration(config.default_ceiling_seconds),
            pty_ceiling: config.ceiling_duration(config.pty_ceiling_seconds),
            mcp_ceiling: config.ceiling_duration(config.mcp_ceiling_seconds),
            warning_fraction: config.warning_threshold_fraction().clamp(0.0, 0.99),
        }
    }

    /// Validate a single ceiling duration against bounds
    #[inline]
    fn validate_ceiling(ceiling: Option<Duration>, name: &str) -> anyhow::Result<()> {
        if let Some(ceiling) = ceiling {
            if ceiling < Duration::from_secs(1) {
                anyhow::bail!(
                    "{} must be at least 1 second (got {}s)",
                    name,
                    ceiling.as_secs()
                );
            }
            if ceiling > Duration::from_secs(3600) {
                anyhow::bail!(
                    "{} must not exceed 3600 seconds/1 hour (got {}s)",
                    name,
                    ceiling.as_secs()
                );
            }
        }
        Ok(())
    }

    /// Validate the timeout policy configuration
    ///
    /// Ensures that:
    /// - Ceiling values are within reasonable bounds (1s - 3600s)
    /// - Warning fraction is between 0.0 and 1.0
    /// - No ceiling is configured as 0 seconds
    pub fn validate(&self) -> anyhow::Result<()> {
        Self::validate_ceiling(self.default_ceiling, "default_ceiling_seconds")?;
        Self::validate_ceiling(self.pty_ceiling, "pty_ceiling_seconds")?;
        Self::validate_ceiling(self.mcp_ceiling, "mcp_ceiling_seconds")?;

        // Validate warning fraction
        if self.warning_fraction <= 0.0 {
            anyhow::bail!(
                "warning_threshold_percent must be greater than 0 (got {})",
                self.warning_fraction * 100.0
            );
        }
        if self.warning_fraction >= 1.0 {
            anyhow::bail!(
                "warning_threshold_percent must be less than 100 (got {})",
                self.warning_fraction * 100.0
            );
        }

        Ok(())
    }

    pub fn ceiling_for(&self, category: ToolTimeoutCategory) -> Option<Duration> {
        match category {
            ToolTimeoutCategory::Default => self.default_ceiling,
            ToolTimeoutCategory::Pty => self.pty_ceiling.or(self.default_ceiling),
            ToolTimeoutCategory::Mcp => self.mcp_ceiling.or(self.default_ceiling),
        }
    }

    pub fn warning_fraction(&self) -> f32 {
        self.warning_fraction
    }
}

#[derive(Debug, Clone, Default)]
struct ToolFailureTracker {
    consecutive_failures: u32,
}

impl ToolFailureTracker {
    fn record_failure(&mut self) {
        self.consecutive_failures = self.consecutive_failures.saturating_add(1);
    }

    fn reset(&mut self) {
        self.consecutive_failures = 0;
    }

    fn should_circuit_break(&self) -> bool {
        self.consecutive_failures >= 3
    }

    fn backoff_duration(&self) -> Duration {
        let base_ms = 500;
        let max_ms = 10_000;
        let backoff_ms = base_ms * 2_u64.pow(self.consecutive_failures.saturating_sub(1).min(8));
        Duration::from_millis(backoff_ms.min(max_ms))
    }
}

#[derive(Debug, Clone, Default)]
struct ToolLatencyStats {
    samples: VecDeque<Duration>,
    max_samples: usize,
}

#[derive(Debug, Clone, Copy)]
struct AdaptiveTimeoutTuning {
    decay_ratio: f64,
    success_streak: u32,
    min_floor_ms: u64,
}

impl Default for AdaptiveTimeoutTuning {
    fn default() -> Self {
        Self {
            decay_ratio: 0.875,  // relax toward ceiling by 12.5%
            success_streak: 5,   // decay after 5 consecutive successes
            min_floor_ms: 1_000, // never clamp below 1s
        }
    }
}

fn load_adaptive_tuning_from_config(
    timeouts: &crate::config::TimeoutsConfig,
) -> AdaptiveTimeoutTuning {
    AdaptiveTimeoutTuning {
        decay_ratio: timeouts.adaptive_decay_ratio,
        success_streak: timeouts.adaptive_success_streak,
        min_floor_ms: timeouts.adaptive_min_floor_ms,
    }
}

impl ToolLatencyStats {
    fn new(max_samples: usize) -> Self {
        Self {
            samples: VecDeque::with_capacity(max_samples),
            max_samples,
        }
    }

    fn record(&mut self, duration: Duration) {
        if self.samples.len() >= self.max_samples {
            self.samples.pop_front();
        }
        self.samples.push_back(duration);
    }

    fn percentile(&self, pct: f64) -> Option<Duration> {
        if self.samples.is_empty() {
            return None;
        }
        let mut sorted: Vec<Duration> = self.samples.iter().copied().collect();
        sorted.sort_unstable();
        let idx =
            ((pct.clamp(0.0, 1.0)) * (sorted.len().saturating_sub(1) as f64)).round() as usize;
        sorted.get(idx).copied()
    }
}

#[derive(Debug, Clone)]
pub struct HarnessContext {
    session_id: String,
    task_id: Option<String>,
}

impl Default for HarnessContext {
    fn default() -> Self {
        let session_id = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| format!("session-{}", d.as_millis()))
            .unwrap_or_else(|_| "session-unknown".to_string());

        Self {
            session_id,
            task_id: None,
        }
    }
}

impl HarnessContext {
    pub fn with_session(session_id: impl Into<String>) -> Self {
        Self {
            session_id: session_id.into(),
            task_id: None,
        }
    }

    pub fn set_session_id(&mut self, session_id: impl Into<String>) {
        self.session_id = session_id.into();
    }

    pub fn set_task_id(&mut self, task_id: Option<String>) {
        self.task_id = task_id;
    }

    fn snapshot_with_plan(&self, plan: &TaskPlan) -> HarnessContextSnapshot {
        HarnessContextSnapshot {
            session_id: self.session_id.clone(),
            task_id: self.task_id.clone(),
            plan_version: plan.version,
            plan_summary: plan.summary.clone(),
        }
    }
}

#[derive(Clone)]
pub struct ToolRegistry {
    inventory: ToolInventory,
    policy_gateway: ToolPolicyGateway,
    pty_sessions: PtySessionManager,
    mcp_client: Option<Arc<McpClient>>,
    mcp_tool_index: HashMap<String, Vec<String>>,
    mcp_tool_presence: HashMap<String, bool>,
    timeout_policy: ToolTimeoutPolicy,
    execution_history: ToolExecutionHistory,
    harness_context: HarnessContext,
    adaptive_timeout_ceiling: HashMap<ToolTimeoutCategory, Duration>,
    failure_trackers: HashMap<ToolTimeoutCategory, ToolFailureTracker>,
    success_trackers: HashMap<ToolTimeoutCategory, u32>,
    latency_stats: HashMap<ToolTimeoutCategory, ToolLatencyStats>,
    adaptive_tuning: AdaptiveTimeoutTuning,
    mcp_healthy: bool,
    mcp_last_failed: Option<SystemTime>,
    initialized: bool,
    // Security & Identity
    shell_policy: Arc<RwLock<ShellPolicyChecker>>,
    agent_type: Cow<'static, str>,
    // Caching
    cached_available_tools: Arc<RwLock<Option<Vec<String>>>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolPermissionDecision {
    Allow,
    Deny,
    Prompt,
}

impl ToolRegistry {
    pub async fn new(workspace_root: PathBuf) -> Self {
        Self::build(workspace_root, PtyConfig::default(), true).await
    }

    pub async fn new_with_config(workspace_root: PathBuf, pty_config: PtyConfig) -> Self {
        Self::build(workspace_root, pty_config, true).await
    }

    pub async fn new_with_features(workspace_root: PathBuf, todo_planning_enabled: bool) -> Self {
        Self::build(workspace_root, PtyConfig::default(), todo_planning_enabled).await
    }

    pub async fn new_with_config_and_features(
        workspace_root: PathBuf,
        pty_config: PtyConfig,
        todo_planning_enabled: bool,
    ) -> Self {
        Self::build(workspace_root, pty_config, todo_planning_enabled).await
    }

    pub async fn new_with_custom_policy(
        workspace_root: PathBuf,
        policy_manager: ToolPolicyManager,
    ) -> Self {
        Self::build_with_policy(
            workspace_root,
            PtyConfig::default(),
            true,
            Some(policy_manager),
        )
        .await
    }

    pub async fn new_with_custom_policy_and_config(
        workspace_root: PathBuf,
        pty_config: PtyConfig,
        todo_planning_enabled: bool,
        policy_manager: ToolPolicyManager,
    ) -> Self {
        Self::build_with_policy(
            workspace_root,
            pty_config,
            todo_planning_enabled,
            Some(policy_manager),
        )
        .await
    }

    async fn build(
        workspace_root: PathBuf,
        pty_config: PtyConfig,
        todo_planning_enabled: bool,
    ) -> Self {
        Self::build_with_policy(workspace_root, pty_config, todo_planning_enabled, None).await
    }

    async fn build_with_policy(
        workspace_root: PathBuf,
        pty_config: PtyConfig,
        todo_planning_enabled: bool,
        policy_manager: Option<ToolPolicyManager>,
    ) -> Self {
        let mut inventory = ToolInventory::new(workspace_root.clone());
        register_builtin_tools(&mut inventory, todo_planning_enabled);

        let pty_sessions = pty::PtySessionManager::new(workspace_root.clone(), pty_config);

        let policy_gateway = match policy_manager {
            Some(pm) => ToolPolicyGateway::with_policy_manager(pm),
            None => ToolPolicyGateway::new(&workspace_root).await,
        };

        let mut registry = Self {
            inventory,
            policy_gateway,
            pty_sessions,
            mcp_client: None,
            mcp_tool_index: HashMap::new(),
            mcp_tool_presence: HashMap::new(),
            timeout_policy: ToolTimeoutPolicy::default(),
            execution_history: ToolExecutionHistory::new(100), // Keep last 100 executions
            harness_context: HarnessContext::default(),
            adaptive_timeout_ceiling: HashMap::new(),
            failure_trackers: HashMap::new(),
            success_trackers: HashMap::new(),
            latency_stats: HashMap::new(),
            adaptive_tuning: AdaptiveTimeoutTuning::default(),
            mcp_healthy: true,
            mcp_last_failed: None,
            initialized: false,
            shell_policy: Arc::new(RwLock::new(ShellPolicyChecker::new())),
            agent_type: Cow::Borrowed("unknown"),
            cached_available_tools: Arc::new(RwLock::new(None)),
        };

        registry.sync_policy_catalog().await;
        registry.initialize_resiliency_trackers();
        registry
    }

    async fn sync_policy_catalog(&mut self) {
        // Include aliases so policy prompts stay in sync with exposed names
        let available = self.available_tools().await;
        let mcp_keys = self.mcp_policy_keys();
        self.policy_gateway
            .sync_available_tools(available, &mcp_keys)
            .await;

        // Seed default permissions from tool metadata when policy manager is present
        let registrations = self.inventory.registration_metadata();
        if let Ok(policy) = self.policy_manager_mut() {
            let mut seeded = 0usize;
            for (name, metadata) in registrations {
                if let Some(default_policy) = metadata.default_permission() {
                    let current = policy.get_policy(&name);
                    if matches!(current, ToolPolicy::Prompt) {
                        if let Err(err) = policy.set_policy(&name, default_policy.clone()).await {
                            warn!(
                                tool = %name,
                                error = %err,
                                "Failed to seed default policy from tool metadata"
                            );
                        } else {
                            seeded += 1;
                            // Apply same default to aliases so they behave consistently
                            for alias in metadata.aliases() {
                                if let Err(err) =
                                    policy.set_policy(alias, default_policy.clone()).await
                                {
                                    warn!(
                                        tool = %name,
                                        alias = %alias,
                                        error = %err,
                                        "Failed to seed default policy for alias"
                                    );
                                }
                            }
                        }
                    }
                }
            }

            if seeded > 0 {
                debug!(seeded, "Seeded default tool policies from registrations");
            }
        }
    }

    /// Register a new tool with the registry
    ///
    /// # Arguments
    /// * `registration` - The tool registration to add
    ///
    /// # Returns
    /// `Result<()>` indicating success or an error if the tool is already registered
    pub fn register_tool(&mut self, registration: ToolRegistration) -> Result<()> {
        self.inventory.register_tool(registration)?;
        // Invalidate cache
        if let Ok(mut cache) = self.cached_available_tools.write() {
            *cache = None;
        }
        Ok(())
    }

    /// Get a list of all available tools, including MCP tools
    ///
    /// # Returns
    /// A `Vec<String>` containing the names of all available tools
    pub async fn available_tools(&self) -> Vec<String> {
        // Use try_read to avoid blocking on contested locks
        match self.cached_available_tools.try_read() {
            Ok(cache) if cache.is_some() => return cache.as_ref().unwrap().clone(),
            _ => {} // Continue with computation if cache miss or lock contested
        }

        let mut tools = self.inventory.available_tools();
        tools.extend(self.inventory.registered_aliases());

        // Add MCP tools if available
        if let Some(mcp_client) = &self.mcp_client {
            match mcp_client.list_mcp_tools().await {
                Ok(mcp_tools) => {
                    tools.reserve(mcp_tools.len());
                    for tool in mcp_tools {
                        tools.push(format!("mcp_{}", tool.name));
                    }
                }
                Err(e) => {
                    tracing::debug!("Failed to list MCP tools: {}", e);
                }
            }
        }

        tools.sort_unstable();

        // Update cache with try_write to avoid blocking
        if let Ok(mut cache) = self.cached_available_tools.try_write() {
            *cache = Some(tools.clone());
        }

        tools
    }

    fn mcp_policy_keys(&self) -> Vec<String> {
        // Pre-calculate capacity
        let capacity: usize = self.mcp_tool_index.values().map(|tools| tools.len()).sum();
        let mut keys = Vec::with_capacity(capacity);
        for (provider, tools) in &self.mcp_tool_index {
            for tool in tools {
                keys.push(format!("mcp::{}::{}", provider, tool));
            }
        }
        keys
    }

    fn find_mcp_provider(&self, tool_name: &str) -> Option<String> {
        for (provider, tools) in &self.mcp_tool_index {
            if tools.iter().any(|candidate| candidate == tool_name) {
                return Some(provider.clone());
            }
        }
        None
    }

    pub async fn enable_full_auto_mode(&mut self, allowed_tools: &[String]) {
        let available = self.available_tools().await;
        self.policy_gateway
            .enable_full_auto_mode(allowed_tools, &available);
    }

    pub fn set_agent_type(&mut self, agent_type: impl Into<Cow<'static, str>>) {
        self.agent_type = agent_type.into();
    }

    pub fn check_shell_policy(
        &self,
        command: &str,
        deny_regex_patterns: &[String],
        deny_glob_patterns: &[String],
    ) -> Result<()> {
        let agent_type = self.agent_type.clone();
        let mut checker = self.shell_policy.write().unwrap();
        checker.check_command(
            command,
            &agent_type,
            deny_regex_patterns,
            deny_glob_patterns,
        )
    }

    pub fn disable_full_auto_mode(&mut self) {
        self.policy_gateway.disable_full_auto_mode();
    }

    pub fn current_full_auto_allowlist(&self) -> Option<Vec<String>> {
        self.policy_gateway.current_full_auto_allowlist()
    }

    /// Check if a tool with the given name is registered
    ///
    /// # Arguments
    /// * `name` - The name of the tool to check
    ///
    /// # Returns
    /// `bool` indicating whether the tool exists (including aliases)
    pub async fn has_tool(&self, name: &str) -> bool {
        // First check the main tool registry
        if self.inventory.has_tool(name) {
            return true;
        }

        // If not found, check if it's an MCP tool
        if let Some(tool_name) = name.strip_prefix("mcp_") {
            if self.find_mcp_provider(tool_name).is_some() {
                return true;
            }

            if let Some(mcp_client) = &self.mcp_client {
                if let Ok(true) = mcp_client.has_mcp_tool(tool_name).await {
                    return true;
                }
                // Check if it's an alias
                if let Some(resolved_name) = self.resolve_mcp_tool_alias(tool_name).await
                    && resolved_name != tool_name
                {
                    return true;
                }
            }
        }

        false
    }

    pub fn workspace_root(&self) -> &PathBuf {
        self.inventory.workspace_root()
    }

    /// Get workspace root as Cow<str> to avoid allocations when possible
    pub(crate) fn workspace_root_str(&self) -> Cow<'_, str> {
        self.workspace_root().to_string_lossy()
    }

    /// Get workspace root as PathBuf (clones only when needed)
    pub(crate) fn workspace_root_owned(&self) -> PathBuf {
        self.inventory.workspace_root().clone()
    }

    pub fn file_ops_tool(&self) -> &FileOpsTool {
        self.inventory.file_ops_tool()
    }

    pub fn grep_file_manager(&self) -> Arc<GrepSearchManager> {
        self.inventory.grep_file_manager()
    }

    pub fn pty_manager(&self) -> &PtyManager {
        self.pty_sessions.manager()
    }

    pub fn pty_config(&self) -> &PtyConfig {
        self.pty_sessions.config()
    }

    pub fn can_start_pty_session(&self) -> bool {
        self.pty_sessions.can_start_session()
    }

    pub fn start_pty_session(&self) -> Result<pty::PtySessionGuard> {
        self.pty_sessions.start_session()
    }

    pub fn end_pty_session(&self) {
        self.pty_sessions.end_session();
    }

    pub fn active_pty_sessions(&self) -> usize {
        self.pty_sessions.active_sessions()
    }

    pub fn plan_manager(&self) -> PlanManager {
        self.inventory.plan_manager()
    }

    pub fn current_plan(&self) -> crate::tools::TaskPlan {
        self.inventory.plan_manager().snapshot()
    }

    /// Update harness session identifier used for structured tool telemetry
    pub fn set_harness_session(&mut self, session_id: impl Into<String>) {
        self.harness_context.set_session_id(session_id);
    }

    /// Update current task identifier used for structured tool telemetry
    pub fn set_harness_task(&mut self, task_id: Option<String>) {
        self.harness_context.set_task_id(task_id);
    }

    /// Snapshot harness context including current plan metadata
    pub fn harness_context_snapshot(&self) -> HarnessContextSnapshot {
        let plan = self.current_plan();
        self.harness_context.snapshot_with_plan(&plan)
    }

    pub fn policy_manager_mut(&mut self) -> Result<&mut ToolPolicyManager> {
        self.policy_gateway.policy_manager_mut()
    }

    pub fn policy_manager(&self) -> Result<&ToolPolicyManager> {
        self.policy_gateway.policy_manager()
    }

    pub async fn set_policy_manager(&mut self, manager: ToolPolicyManager) {
        self.policy_gateway.set_policy_manager(manager);
        self.sync_policy_catalog().await;
    }

    pub async fn set_tool_policy(&mut self, tool_name: &str, policy: ToolPolicy) -> Result<()> {
        self.policy_gateway.set_tool_policy(tool_name, policy).await
    }

    pub fn get_tool_policy(&self, tool_name: &str) -> ToolPolicy {
        self.policy_gateway.get_tool_policy(tool_name)
    }

    pub async fn reset_tool_policies(&mut self) -> Result<()> {
        self.policy_gateway.reset_tool_policies().await
    }

    pub async fn allow_all_tools(&mut self) -> Result<()> {
        self.policy_gateway.allow_all_tools().await
    }

    pub async fn deny_all_tools(&mut self) -> Result<()> {
        self.policy_gateway.deny_all_tools().await
    }

    pub fn print_policy_status(&self) {
        self.policy_gateway.print_policy_status();
    }

    pub async fn initialize_async(&mut self) -> Result<()> {
        if self.initialized && (self.mcp_client.is_none() || !self.mcp_tool_index.is_empty()) {
            return Ok(());
        }

        if self.mcp_client.is_some()
            && self.mcp_tool_index.is_empty()
            && let Err(err) = self.refresh_mcp_tools().await
        {
            warn!(
                error = %err,
                "Failed to refresh MCP tools during registry initialization"
            );
        }

        self.sync_policy_catalog().await;
        self.initialized = true;

        Ok(())
    }

    pub async fn apply_config_policies(&mut self, tools_config: &ToolsConfig) -> Result<()> {
        if let Ok(policy_manager) = self.policy_manager_mut() {
            policy_manager.apply_tools_config(tools_config).await?;
        }

        let detect_window = DEFAULT_LOOP_DETECT_WINDOW
            .max(tools_config.max_repeated_tool_calls.saturating_mul(2))
            .max(1);
        self.execution_history
            .set_loop_detection_limits(detect_window, tools_config.max_repeated_tool_calls);
        self.execution_history
            .set_rate_limit_per_minute(tool_rate_limit_from_env());

        Ok(())
    }

    pub fn apply_commands_config(&mut self, commands_config: &CommandsConfig) {
        self.inventory
            .command_tool_mut()
            .update_commands_config(commands_config);
        self.pty_sessions
            .manager()
            .apply_commands_config(commands_config);
    }

    pub fn apply_timeout_policy(&mut self, timeouts: &TimeoutsConfig) {
        let policy = ToolTimeoutPolicy::from_config(timeouts);

        // Validate the policy before applying
        if let Err(e) = policy.validate() {
            warn!(
                error = %e,
                "Invalid timeout configuration detected, using defaults"
            );
            self.timeout_policy = ToolTimeoutPolicy::default();
        } else {
            self.timeout_policy = policy;
        }

        self.adaptive_tuning = load_adaptive_tuning_from_config(timeouts);
    }

    pub fn timeout_policy(&self) -> &ToolTimeoutPolicy {
        &self.timeout_policy
    }

    pub fn rate_limit_per_minute(&self) -> Option<usize> {
        self.execution_history.rate_limit_per_minute()
    }

    fn initialize_resiliency_trackers(&mut self) {
        let categories = [
            ToolTimeoutCategory::Default,
            ToolTimeoutCategory::Pty,
            ToolTimeoutCategory::Mcp,
        ];
        for category in categories {
            self.failure_trackers.entry(category).or_default();
            self.success_trackers.entry(category).or_insert(0);
            self.latency_stats
                .entry(category)
                .or_insert_with(|| ToolLatencyStats::new(50));
            self.adaptive_timeout_ceiling
                .entry(category)
                .or_insert_with(|| Duration::from_secs(0));
        }
    }

    fn effective_timeout(&self, category: ToolTimeoutCategory) -> Option<Duration> {
        let base = self.timeout_policy.ceiling_for(category);
        let adaptive = self.adaptive_timeout_ceiling.get(&category).copied();

        match (base, adaptive) {
            (Some(b), Some(a)) if a.as_millis() > 0 => Some(std::cmp::min(b, a)),
            (Some(b), _) => Some(b),
            (None, Some(a)) if a.as_millis() > 0 => Some(a),
            _ => None,
        }
    }

    fn decay_adaptive_timeout(&mut self, category: ToolTimeoutCategory) {
        if let Some(adaptive) = self.adaptive_timeout_ceiling.get_mut(&category) {
            if adaptive.as_millis() == 0 {
                return;
            }
            let before = *adaptive;
            if let Some(base) = self.timeout_policy.ceiling_for(category) {
                if *adaptive < base {
                    let relaxed_ms = ((*adaptive).as_millis() as f64
                        * (1.0 / self.adaptive_tuning.decay_ratio))
                        as u128;
                    let relaxed = Duration::from_millis(relaxed_ms as u64);
                    *adaptive = std::cmp::min(relaxed, base);
                }
            } else {
                // If no base, relax upward modestly
                let relaxed = Duration::from_millis(
                    ((*adaptive).as_millis() as f64 * (1.0 / self.adaptive_tuning.decay_ratio))
                        as u64,
                );
                *adaptive = relaxed;
            }

            let floor = Duration::from_millis(self.adaptive_tuning.min_floor_ms);
            if *adaptive < floor {
                *adaptive = floor;
            }

            if *adaptive != before {
                debug!(
                    category = %category.label(),
                    previous_ms = %before.as_millis(),
                    new_ms = %adaptive.as_millis(),
                    decay_ratio = %self.adaptive_tuning.decay_ratio,
                    "Adaptive timeout relaxed after success streak"
                );
            }
        }
    }

    fn record_tool_failure(&mut self, category: ToolTimeoutCategory) -> bool {
        let tracker = self.failure_trackers.entry(category).or_default();
        tracker.record_failure();
        self.success_trackers.insert(category, 0);
        tracker.should_circuit_break()
    }

    fn reset_tool_failure(&mut self, category: ToolTimeoutCategory) {
        if let Some(tracker) = self.failure_trackers.get_mut(&category) {
            tracker.reset();
        }
        self.success_trackers.insert(category, 0);
    }

    fn record_tool_latency(&mut self, category: ToolTimeoutCategory, duration: Duration) {
        let stats = self
            .latency_stats
            .entry(category)
            .or_insert_with(|| ToolLatencyStats::new(50));
        stats.record(duration);

        if let Some(p95) = stats.percentile(0.95) {
            if let Some(ceiling) = self.timeout_policy.ceiling_for(category) {
                if p95 > ceiling {
                    warn!(
                        category = %category.label(),
                        p95_ms = %p95.as_millis(),
                        ceiling_ms = %ceiling.as_millis(),
                        "Observed p95 tool latency exceeds configured ceiling; consider adjusting timeouts"
                    );
                    let adjusted = std::cmp::min(
                        ceiling,
                        std::cmp::max(
                            Duration::from_millis(self.adaptive_tuning.min_floor_ms),
                            Self::scale_duration(p95, 11, 10),
                        ),
                    );
                    self.adaptive_timeout_ceiling.insert(category, adjusted);
                    debug!(
                        category = %category.label(),
                        new_ceiling_ms = %adjusted.as_millis(),
                        "Adaptive timeout ceiling applied from p95 latency"
                    );
                }
            } else {
                // No ceiling configured; derive one from p95 with headroom
                let derived = std::cmp::max(
                    Duration::from_millis(self.adaptive_tuning.min_floor_ms),
                    Self::scale_duration(p95, 12, 10),
                );
                self.adaptive_timeout_ceiling.insert(category, derived);
                debug!(
                    category = %category.label(),
                    new_ceiling_ms = %derived.as_millis(),
                    "Adaptive timeout ceiling derived from p95 latency without static ceiling"
                );
            }
        }
    }

    fn should_circuit_break(&self, category: ToolTimeoutCategory) -> Option<Duration> {
        self.failure_trackers
            .get(&category)
            .filter(|tracker| tracker.should_circuit_break())
            .map(|tracker| tracker.backoff_duration())
    }

    fn sanitize_tool_output(value: Value, is_mcp: bool) -> Value {
        let (entry_fuse, depth_fuse, token_fuse, byte_fuse) = Self::fuse_limits();

        let trimmed = Self::clamp_value_recursive(&value, entry_fuse, depth_fuse);

        let serialized = trimmed.to_string();
        let approx_tokens = serialized.len() / 4;
        if serialized.len() > byte_fuse || approx_tokens > token_fuse {
            let truncated = serialized.chars().take(byte_fuse).collect::<String>();
            return json!({
                "content": truncated,
                "truncated": true,
                "note": if is_mcp {
                    "MCP tool result truncated to protect context budget"
                } else {
                    "Tool result truncated to protect context budget"
                },
                "approx_tokens": approx_tokens,
                "byte_fuse": byte_fuse
            });
        }
        trimmed
    }

    fn clamp_value_recursive(value: &Value, entry_fuse: usize, depth: usize) -> Value {
        if depth == 0 {
            return value.clone();
        }
        match value {
            Value::Array(arr) => {
                if arr.is_empty() {
                    return Value::Array(Vec::new());
                }
                let overflow = arr.len().saturating_sub(entry_fuse);
                let trimmed: Vec<Value> = arr
                    .iter()
                    .take(entry_fuse)
                    .map(|v| Self::clamp_value_recursive(v, entry_fuse, depth - 1))
                    .collect();
                if overflow > 0 {
                    let approx_tokens = trimmed
                        .iter()
                        .map(|v| v.to_string().len() / 4)
                        .sum::<usize>();
                    json!({
                        "truncated": true,
                        "note": "Array truncated to protect context budget",
                        "total_entries": arr.len(),
                        "entries": trimmed,
                        "overflow": overflow,
                        "approx_tokens": approx_tokens
                    })
                } else {
                    Value::Array(trimmed)
                }
            }
            Value::Object(map) => {
                if map.is_empty() {
                    return Value::Object(serde_json::Map::new());
                }
                let overflow = map.len().saturating_sub(entry_fuse);
                let mut head = serde_json::Map::new();
                for (k, v) in map.iter().take(entry_fuse) {
                    head.insert(
                        k.clone(),
                        Self::clamp_value_recursive(v, entry_fuse, depth - 1),
                    );
                }
                if overflow > 0 {
                    let approx_tokens = head
                        .iter()
                        .map(|(k, v)| (k.len() + v.to_string().len()) / 4)
                        .sum::<usize>();
                    json!({
                        "truncated": true,
                        "note": "Object truncated to protect context budget",
                        "total_entries": map.len(),
                        "entries": head,
                        "overflow": overflow,
                        "approx_tokens": approx_tokens
                    })
                } else {
                    Value::Object(head)
                }
            }
            _ => value.clone(),
        }
    }

    fn fuse_limits() -> (usize, usize, usize, usize) {
        let entry_fuse = std::env::var("VTCODE_FUSE_ENTRY")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .filter(|v| *v >= 10)
            .unwrap_or(200);
        let depth_fuse = std::env::var("VTCODE_FUSE_DEPTH")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .filter(|v| *v >= 1)
            .unwrap_or(3);
        let token_fuse = std::env::var("VTCODE_FUSE_TOKEN")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .filter(|v| *v >= 1_000)
            .unwrap_or(50_000);
        let byte_fuse = std::env::var("VTCODE_FUSE_BYTES")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .filter(|v| *v >= 10_000)
            .unwrap_or(200_000);
        (entry_fuse, depth_fuse, token_fuse, byte_fuse)
    }

    fn scale_duration(duration: Duration, num: u32, denom: u32) -> Duration {
        if denom == 0 {
            return duration;
        }
        let millis = duration.as_millis();
        let scaled = millis
            .saturating_mul(num as u128)
            .saturating_div(denom as u128);
        Duration::from_millis(scaled as u64)
    }

    pub async fn timeout_category_for(&mut self, name: &str) -> ToolTimeoutCategory {
        let canonical_name = canonical_tool_name(name);
        let tool_name = canonical_name.as_ref();

        if let Some(registration) = self.inventory.registration_for(tool_name) {
            return if registration.uses_pty() {
                ToolTimeoutCategory::Pty
            } else {
                ToolTimeoutCategory::Default
            };
        }

        if let Some(stripped) = name.strip_prefix("mcp_") {
            if self.has_mcp_tool(stripped).await {
                return ToolTimeoutCategory::Mcp;
            }
        } else if self.find_mcp_provider(tool_name).is_some() || self.has_mcp_tool(tool_name).await
        {
            return ToolTimeoutCategory::Mcp;
        }

        ToolTimeoutCategory::Default
    }

    pub async fn execute_tool(&mut self, name: &str, args: Value) -> Result<Value> {
        self.execute_tool_ref(name, &args).await
    }

    /// Reference-taking version of execute_tool to avoid cloning by callers
    /// that already have access to an existing `Value`.
    pub async fn execute_tool_ref(&mut self, name: &str, args: &Value) -> Result<Value> {
        let canonical_name = canonical_tool_name(name);
        let tool_name = canonical_name.as_ref();
        // Cache tool_name as owned String once for all record usage
        let tool_name_owned = tool_name.to_string();
        let requested_name = name.to_string();
        let display_name = if tool_name == name {
            tool_name_owned.clone()
        } else {
            format!("{} (alias for {})", name, tool_name)
        };

        // Clone args once at the start for error recording paths (clone only here)
        let args_for_recording = args.clone();
        // Capture harness context snapshot for structured telemetry and history
        let context_snapshot = self.harness_context_snapshot();
        let context_payload = context_snapshot.to_json();

        // Validate arguments against schema if available
        if let Some(registration) = self.inventory.registration_for(tool_name)
            && let Some(schema) = registration.parameter_schema()
                && let Err(errors) = jsonschema::validate(schema, args) {
                    return Err(anyhow::anyhow!(
                        "Invalid arguments for tool '{}': {}",
                        tool_name,
                        errors
                    ));
                }

        let timeout_category = self.timeout_category_for(tool_name).await;

        if let Some(backoff) = self.should_circuit_break(timeout_category) {
            warn!(
                tool = %tool_name,
                category = %timeout_category.label(),
                delay_ms = %backoff.as_millis(),
                "Circuit breaker active for tool category; backing off before execution"
            );
            tokio::time::sleep(backoff).await;
        }

        let execution_span = tracing::debug_span!(
            "tool_execution",
            tool = %tool_name,
            requested = %name,
            session_id = %context_snapshot.session_id,
            task_id = %context_snapshot.task_id.as_deref().unwrap_or(""),
            plan_version = context_snapshot.plan_version,
            plan_status = %context_snapshot.plan_summary.status.label(),
            plan_total_steps = context_snapshot.plan_summary.total_steps,
            plan_completed_steps = context_snapshot.plan_summary.completed_steps
        );
        let _span_guard = execution_span.enter();

        debug!(
            tool = %tool_name,
            session_id = %context_snapshot.session_id,
            task_id = %context_snapshot.task_id.as_deref().unwrap_or(""),
            plan_version = context_snapshot.plan_version,
            plan_status = ?context_snapshot.plan_summary.status,
            "Executing tool with harness context"
        );

        if tool_name != name {
            trace!(
                requested = %name,
                canonical = %tool_name,
                "Resolved tool alias to canonical name"
            );
        }

        let base_timeout_ms = self
            .timeout_policy
            .ceiling_for(timeout_category)
            .map(|d| d.as_millis() as u64);
        let adaptive_timeout_ms = self
            .adaptive_timeout_ceiling
            .get(&timeout_category)
            .filter(|d| d.as_millis() > 0)
            .map(|d| d.as_millis() as u64);
        let timeout_category_label = Some(timeout_category.label().to_string());

        if let Some(rate_limit) = self.execution_history.rate_limit_per_minute() {
            let calls_last_minute = self
                .execution_history
                .calls_in_window(Duration::from_secs(60));
            if calls_last_minute >= rate_limit {
                let error = ToolExecutionError::new(
                    tool_name_owned.clone(),
                    ToolErrorType::PolicyViolation,
                    format!(
                        "Tool '{}' skipped: rate limit reached ({} calls/min)",
                        display_name, rate_limit
                    ),
                );
                let payload = json!({
                    "error": error.to_json_value(),
                    "rate_limited": true,
                    "limit_per_minute": rate_limit,
                    "recent_calls": calls_last_minute,
                });

                self.execution_history
                    .add_record(ToolExecutionRecord::failure(
                        tool_name_owned,
                        requested_name,
                        false,
                        None,
                        args_for_recording,
                        "Tool rate limit reached".to_string(),
                        context_snapshot.clone(),
                        timeout_category_label.clone(),
                        base_timeout_ms,
                        adaptive_timeout_ms,
                        None,
                        false,
                    ));

                return Ok(payload);
            }
        }

        // LOOP DETECTION: Check if we're calling the same tool repeatedly with identical params
        let loop_limit = self.execution_history.loop_limit_for(tool_name);
        let (is_loop, repeat_count, _) = self.execution_history.detect_loop(tool_name, args);
        if loop_limit > 0 && is_loop {
            warn!(
                tool = %tool_name,
                repeats = repeat_count,
                "Loop detected: agent calling same tool with identical parameters {} times",
                repeat_count
            );
            if repeat_count >= loop_limit {
                let delay_ms = (75 * repeat_count as u64).min(500);
                if delay_ms > 0 {
                    tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                }

                let error = ToolExecutionError::new(
                    tool_name_owned.clone(),
                    ToolErrorType::PolicyViolation,
                    format!(
                        "LOOP DETECTION: Tool '{}' has been called {} times with identical parameters and is now blocked.\n\n\
                        ACTION REQUIRED: DO NOT retry this tool call. The tool execution has been prevented to avoid infinite loops.\n\n\
                        If you need the result from this tool:\n\
                        1. Check if you already have the result from a previous successful call in your conversation history\n\
                        2. If not available, use a different approach or modify your request",
                        display_name, repeat_count
                    ),
                );
                let mut payload = error.to_json_value();
                if let Some(obj) = payload.as_object_mut() {
                    obj.insert("loop_detected".into(), json!(true));
                    obj.insert("repeat_count".into(), json!(repeat_count));
                    obj.insert("limit".into(), json!(loop_limit));
                    obj.insert("tool".into(), json!(display_name));
                }

                self.execution_history
                    .add_record(ToolExecutionRecord::failure(
                        tool_name_owned,
                        requested_name.clone(),
                        false,
                        None,
                        args_for_recording,
                        "Tool call blocked due to repeated identical invocations".to_string(),
                        context_snapshot.clone(),
                        timeout_category_label.clone(),
                        base_timeout_ms,
                        adaptive_timeout_ms,
                        None,
                        false,
                    ));

                return Ok(payload);
            }
        }

        if self.policy_gateway.has_full_auto_allowlist()
            && !self.policy_gateway.is_allowed_in_full_auto(tool_name)
        {
            let error = ToolExecutionError::new(
                tool_name_owned.clone(),
                ToolErrorType::PolicyViolation,
                format!(
                    "Tool '{}' is not permitted while full-auto mode is active",
                    display_name
                ),
            );

            self.execution_history
                .add_record(ToolExecutionRecord::failure(
                    tool_name_owned,
                    requested_name.clone(),
                    false,
                    None,
                    args_for_recording,
                    "Tool execution denied by policy".to_string(),
                    context_snapshot.clone(),
                    timeout_category_label.clone(),
                    base_timeout_ms,
                    adaptive_timeout_ms,
                    None,
                    false,
                ));

            return Ok(error.to_json_value());
        }

        let skip_policy_prompt = self.policy_gateway.take_preapproved(tool_name);

        if !skip_policy_prompt && !self.policy_gateway.should_execute_tool(tool_name).await? {
            let error = ToolExecutionError::new(
                tool_name_owned.clone(),
                ToolErrorType::PolicyViolation,
                format!("Tool '{}' execution denied by policy", display_name),
            );

            self.execution_history
                .add_record(ToolExecutionRecord::failure(
                    tool_name_owned,
                    requested_name.clone(),
                    false,
                    None,
                    args_for_recording,
                    "Tool execution denied by policy".to_string(),
                    context_snapshot.clone(),
                    timeout_category_label.clone(),
                    base_timeout_ms,
                    adaptive_timeout_ms,
                    None,
                    false,
                ));

            return Ok(error.to_json_value());
        }

        let args = match self
            .policy_gateway
            .apply_policy_constraints(tool_name, args)
        {
            Ok(processed_args) => processed_args,
            Err(err) => {
                let error = ToolExecutionError::with_original_error(
                    tool_name_owned.clone(),
                    ToolErrorType::InvalidParameters,
                    "Failed to apply policy constraints".to_string(),
                    err.to_string(),
                );

                self.execution_history
                    .add_record(ToolExecutionRecord::failure(
                        tool_name_owned,
                        requested_name.clone(),
                        false,
                        None,
                        args_for_recording,
                        format!("Failed to apply policy constraints: {}", err),
                        context_snapshot.clone(),
                        timeout_category_label.clone(),
                        base_timeout_ms,
                        adaptive_timeout_ms,
                        None,
                        false,
                    ));

                return Ok(error.to_json_value());
            }
        };

        // First, check if we need a PTY session by checking if the tool exists and needs PTY
        let mut needs_pty = false;
        let mut tool_exists = false;
        let mut is_mcp_tool = false;
        let mut mcp_provider: Option<String> = None;
        let mut mcp_tool_name: Option<String> = None;
        let mut mcp_lookup_error: Option<anyhow::Error> = None;

        // Check if it's a standard tool first
        if let Some(registration) = self.inventory.registration_for(tool_name) {
            needs_pty = registration.uses_pty();
            tool_exists = true;
        }
        // If not a standard tool, check if it's an MCP tool
        else if let Some(mcp_client) = &self.mcp_client {
            let mut resolved_mcp_name = if let Some(stripped) = name.strip_prefix("mcp_") {
                stripped.to_string()
            } else {
                tool_name_owned.clone()
            };

            if let Some(alias_target) = self.resolve_mcp_tool_alias(&resolved_mcp_name).await
                && alias_target != resolved_mcp_name
            {
                trace!(
                    requested = %resolved_mcp_name,
                    resolved = %alias_target,
                    "Resolved MCP tool alias"
                );
                resolved_mcp_name = alias_target;
            }

            match mcp_client.has_mcp_tool(&resolved_mcp_name).await {
                Ok(true) => {
                    needs_pty = true;
                    tool_exists = true;
                    is_mcp_tool = true;
                    mcp_tool_name = Some(resolved_mcp_name);
                    mcp_provider = self.find_mcp_provider(mcp_tool_name.as_deref().unwrap());
                }
                Ok(false) => {
                    tool_exists = false;
                }
                Err(err) => {
                    warn!("Error checking MCP tool '{}': {}", resolved_mcp_name, err);
                    mcp_lookup_error = Some(err);
                }
            }
        }

        // If tool doesn't exist in either registry, return an error
        if !tool_exists {
            if let Some(err) = mcp_lookup_error {
                let error = ToolExecutionError::with_original_error(
                    tool_name_owned.clone(),
                    ToolErrorType::ExecutionError,
                    format!("Failed to resolve MCP tool '{}': {}", display_name, err),
                    err.to_string(),
                );

                self.execution_history
                    .add_record(ToolExecutionRecord::failure(
                        tool_name_owned,
                        requested_name.clone(),
                        is_mcp_tool,
                        mcp_provider.clone(),
                        args_for_recording,
                        format!("Failed to resolve MCP tool '{}': {}", display_name, err),
                        context_snapshot.clone(),
                        timeout_category_label.clone(),
                        base_timeout_ms,
                        adaptive_timeout_ms,
                        None,
                        false,
                    ));

                return Ok(error.to_json_value());
            }

            let error = ToolExecutionError::new(
                tool_name_owned.clone(),
                ToolErrorType::ToolNotFound,
                format!("Unknown tool: {}", display_name),
            );

            self.execution_history
                .add_record(ToolExecutionRecord::failure(
                    tool_name_owned,
                    requested_name.clone(),
                    is_mcp_tool,
                    mcp_provider.clone(),
                    args_for_recording,
                    format!("Unknown tool: {}", display_name),
                    context_snapshot.clone(),
                    timeout_category_label.clone(),
                    base_timeout_ms,
                    adaptive_timeout_ms,
                    None,
                    false,
                ));

            return Ok(error.to_json_value());
        }

        const MCP_COOLDOWN: Duration = Duration::from_secs(10);
        if is_mcp_tool
            && let Some(last_failed) = self.mcp_last_failed
            && let Ok(elapsed) = last_failed.elapsed()
            && elapsed < MCP_COOLDOWN
        {
            let error = ToolExecutionError::new(
                tool_name_owned.clone(),
                ToolErrorType::ExecutionError,
                "MCP provider cooling down after failure; skipping execution".to_string(),
            );
            let payload = json!({
                "error": error.to_json_value(),
                "mcp_unhealthy": true,
                "note": "MCP provider cooling down after failure; execution skipped",
                "last_failed_at": self.mcp_last_failed
                    .and_then(|ts| ts.duration_since(SystemTime::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs()),
                "cooldown_seconds": MCP_COOLDOWN.as_secs(),
                "mcp_provider": mcp_provider,
            });
            warn!(
                tool = %tool_name_owned,
                payload = %payload,
                "Skipping MCP tool execution due to cooldown"
            );
            self.execution_history
                .add_record(ToolExecutionRecord::failure(
                    tool_name_owned,
                    requested_name.clone(),
                    is_mcp_tool,
                    mcp_provider.clone(),
                    args_for_recording,
                    "MCP provider in cooldown; execution skipped".to_string(),
                    context_snapshot.clone(),
                    timeout_category_label.clone(),
                    base_timeout_ms,
                    adaptive_timeout_ms,
                    None,
                    false,
                ));
            return Ok(payload);
        }

        if is_mcp_tool && !self.mcp_healthy {
            let error = ToolExecutionError::new(
                tool_name_owned.clone(),
                ToolErrorType::ExecutionError,
                "MCP provider unhealthy; skipping tool execution until refresh succeeds"
                    .to_string(),
            );
            let payload = json!({
                "error": error.to_json_value(),
                "mcp_unhealthy": true,
                "note": "MCP provider unhealthy; execution skipped",
                "last_failed_at": self.mcp_last_failed
                    .and_then(|ts| ts.duration_since(SystemTime::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs()),
                "mcp_provider": mcp_provider,
            });
            warn!(
                tool = %tool_name_owned,
                payload = %payload,
                "Skipping MCP tool execution due to unhealthy provider"
            );
            self.execution_history
                .add_record(ToolExecutionRecord::failure(
                    tool_name_owned,
                    requested_name,
                    is_mcp_tool,
                    mcp_provider.clone(),
                    args_for_recording,
                    "MCP provider unhealthy; execution skipped".to_string(),
                    context_snapshot.clone(),
                    timeout_category_label.clone(),
                    base_timeout_ms,
                    adaptive_timeout_ms,
                    None,
                    false,
                ));
            return Ok(payload);
        }

        debug!(
            tool = %tool_name,
            requested = %name,
            is_mcp = is_mcp_tool,
            uses_pty = needs_pty,
            alias = %if tool_name == name { "" } else { name },
            mcp_provider = %mcp_provider.as_deref().unwrap_or(""),
            "Resolved tool route"
        );
        trace!(
            tool = %tool_name,
            requested = %name,
            mcp_provider = %mcp_provider.as_deref().unwrap_or(""),
            mcp_tool = %mcp_tool_name.as_deref().unwrap_or(""),
            context = %context_payload,
            "Tool execution context and routing finalized"
        );

        // Start PTY session if needed (using RAII guard for automatic cleanup)
        let _pty_guard = if needs_pty {
            match self.start_pty_session() {
                Ok(guard) => Some(guard),
                Err(err) => {
                    let error = ToolExecutionError::with_original_error(
                        tool_name_owned.clone(),
                        ToolErrorType::ExecutionError,
                        "Failed to start PTY session".to_string(),
                        err.to_string(),
                    );

                    self.execution_history
                        .add_record(ToolExecutionRecord::failure(
                            tool_name_owned,
                            requested_name.clone(),
                            is_mcp_tool,
                            mcp_provider.clone(),
                            args_for_recording,
                            "Failed to start PTY session".to_string(),
                            context_snapshot.clone(),
                            timeout_category_label.clone(),
                            base_timeout_ms,
                            adaptive_timeout_ms,
                            None,
                            false,
                        ));

                    return Ok(error.to_json_value());
                }
            }
        } else {
            None
        };

        // Execute the appropriate tool based on its type
        // The _pty_guard will automatically decrement the session count when dropped
        let execution_started_at = Instant::now();
        let effective_timeout = self.effective_timeout(timeout_category);
        let effective_timeout_ms = effective_timeout.map(|d| d.as_millis() as u64);
        let exec_future = async {
            if is_mcp_tool {
                let mcp_name =
                    mcp_tool_name.expect("mcp_tool_name should be set when is_mcp_tool is true");
                self.execute_mcp_tool(&mcp_name, args).await
            } else if let Some(registration) = self.inventory.registration_for(tool_name) {
                // Log deprecation warning if tool is deprecated
                if registration.is_deprecated() {
                    if let Some(msg) = registration.deprecation_message() {
                        warn!("Tool '{}' is deprecated: {}", tool_name, msg);
                    } else {
                        warn!(
                            "Tool '{}' is deprecated and may be removed in a future version",
                            tool_name
                        );
                    }
                }

                let handler = registration.handler();
                match handler {
                    ToolHandler::RegistryFn(executor) => executor(self, args).await,
                    ToolHandler::TraitObject(tool) => tool.execute(args).await,
                }
            } else {
                // This should theoretically never happen since we checked tool_exists above
                let error = ToolExecutionError::new(
                    tool_name_owned.clone(),
                    ToolErrorType::ToolNotFound,
                    "Tool not found in registry".to_string(),
                );

                self.execution_history
                    .add_record(ToolExecutionRecord::failure(
                        tool_name_owned.clone(),
                        requested_name.clone(),
                        is_mcp_tool,
                        mcp_provider.clone(),
                        args_for_recording.clone(),
                        "Tool not found in registry".to_string(),
                        context_snapshot.clone(),
                        timeout_category_label.clone(),
                        base_timeout_ms,
                        adaptive_timeout_ms,
                        effective_timeout_ms,
                        false,
                    ));

                Ok(error.to_json_value())
            }
        };

        let result = if let Some(limit) = effective_timeout {
            debug!(
                tool = %tool_name_owned,
                category = %timeout_category.label(),
                timeout_ms = %limit.as_millis(),
                "Executing tool with effective timeout"
            );
            match tokio::time::timeout(limit, exec_future).await {
                Ok(res) => res,
                Err(_) => {
                    let timeout_ms = limit.as_millis() as u64;
                    let timeout_payload = json!({
                        "error": {
                            "message": format!("Tool execution timed out after {:?} (category: {})", limit, timeout_category.label()),
                            "timeout_category": timeout_category.label(),
                            "timeout_ms": timeout_ms,
                            "circuit_breaker": false,
                        }
                    });
                    self.execution_history
                        .add_record(ToolExecutionRecord::failure(
                            tool_name_owned,
                            requested_name,
                            is_mcp_tool,
                            mcp_provider,
                            args_for_recording,
                            "Tool execution timed out".to_string(),
                            context_snapshot.clone(),
                            timeout_category_label.clone(),
                            base_timeout_ms,
                            adaptive_timeout_ms,
                            Some(timeout_ms),
                            false,
                        ));
                    return Ok(timeout_payload);
                }
            }
        } else {
            exec_future.await
        };

        // PTY session will be automatically cleaned up when _pty_guard is dropped

        // Handle the execution result and record it

        match result {
            Ok(value) => {
                self.reset_tool_failure(timeout_category);
                let should_decay =
                    if let Some(counter) = self.success_trackers.get_mut(&timeout_category) {
                        *counter = counter.saturating_add(1);
                        if *counter >= self.adaptive_tuning.success_streak {
                            *counter = 0;
                            true
                        } else {
                            false
                        }
                    } else {
                        false
                    };
                if should_decay {
                    self.decay_adaptive_timeout(timeout_category);
                }
                self.record_tool_latency(timeout_category, execution_started_at.elapsed());
                let sanitized_value = Self::sanitize_tool_output(value, is_mcp_tool);
                let normalized_value = normalize_tool_output(sanitized_value);

                self.execution_history
                    .add_record(ToolExecutionRecord::success(
                        tool_name_owned,
                        requested_name,
                        is_mcp_tool,
                        mcp_provider,
                        args_for_recording,
                        normalized_value.clone(),
                        context_snapshot.clone(),
                        timeout_category_label.clone(),
                        base_timeout_ms,
                        adaptive_timeout_ms,
                        effective_timeout_ms,
                        false,
                    ));

                Ok(normalized_value)
            }
            Err(err) => {
                let error_type = classify_error(&err);
                let error = ToolExecutionError::with_original_error(
                    tool_name_owned.clone(),
                    error_type,
                    format!("Tool execution failed: {}", err),
                    err.to_string(),
                );

                let tripped = self.record_tool_failure(timeout_category);
                if tripped {
                    warn!(
                        tool = %tool_name_owned,
                        category = %timeout_category.label(),
                        "Tool circuit breaker tripped after consecutive failures"
                    );
                }

                let mut payload = error.to_json_value();
                if let Some(obj) = payload.get_mut("error").and_then(|v| v.as_object_mut()) {
                    obj.insert(
                        "timeout_category".into(),
                        serde_json::Value::String(timeout_category.label().to_string()),
                    );
                    obj.insert(
                        "timeout_ms".into(),
                        serde_json::Value::from(effective_timeout_ms.unwrap_or(0)),
                    );
                    obj.insert("circuit_breaker".into(), serde_json::Value::Bool(tripped));
                }

                self.execution_history
                    .add_record(ToolExecutionRecord::failure(
                        tool_name_owned,
                        requested_name,
                        is_mcp_tool,
                        mcp_provider,
                        args_for_recording,
                        format!("Tool execution failed: {}", err),
                        context_snapshot.clone(),
                        timeout_category_label.clone(),
                        base_timeout_ms,
                        adaptive_timeout_ms,
                        effective_timeout_ms,
                        tripped,
                    ));

                Ok(payload)
            }
        }
    }

    /// Set the MCP client for this registry
    pub fn with_mcp_client(mut self, mcp_client: Arc<McpClient>) -> Self {
        self.mcp_client = Some(mcp_client);
        self.mcp_tool_index.clear();
        self.mcp_tool_presence.clear();
        self.initialized = false;
        self
    }

    /// Attach an MCP client without consuming the registry
    pub fn set_mcp_client(&mut self, mcp_client: Arc<McpClient>) {
        self.mcp_client = Some(mcp_client);
        self.mcp_tool_index.clear();
        self.mcp_tool_presence.clear();
        self.initialized = false;
    }

    /// Get the MCP client if available
    pub fn mcp_client(&self) -> Option<&Arc<McpClient>> {
        self.mcp_client.as_ref()
    }

    /// List all MCP tools
    pub async fn list_mcp_tools(&self) -> Result<Vec<McpToolInfo>> {
        if let Some(mcp_client) = &self.mcp_client {
            mcp_client.list_mcp_tools().await
        } else {
            Ok(Vec::new())
        }
    }

    /// Check if an MCP tool exists
    pub async fn has_mcp_tool(&mut self, tool_name: &str) -> bool {
        if self
            .mcp_tool_index
            .values()
            .any(|tools| tools.iter().any(|candidate| candidate == tool_name))
        {
            self.mcp_tool_presence.insert(tool_name.to_string(), true);
            return true;
        }

        if let Some(cached) = self.mcp_tool_presence.get(tool_name) {
            return *cached;
        }

        let Some(mcp_client) = &self.mcp_client else {
            self.mcp_tool_presence.insert(tool_name.to_string(), false);
            return false;
        };

        match mcp_client.has_mcp_tool(tool_name).await {
            Ok(result) => {
                self.mcp_tool_presence.insert(tool_name.to_string(), result);
                result
            }
            Err(err) => {
                warn!(
                    tool = tool_name,
                    error = %err,
                    "failed to query MCP tool presence"
                );
                self.mcp_tool_presence.insert(tool_name.to_string(), false);
                false
            }
        }
    }

    /// Execute an MCP tool
    pub async fn execute_mcp_tool(&self, tool_name: &str, args: Value) -> Result<Value> {
        if let Some(mcp_client) = &self.mcp_client {
            mcp_client.execute_mcp_tool(tool_name, &args).await
        } else {
            Err(anyhow::anyhow!("MCP client not available"))
        }
    }

    async fn resolve_mcp_tool_alias(&self, tool_name: &str) -> Option<String> {
        let Some(mcp_client) = &self.mcp_client else {
            return None;
        };

        let normalized = normalize_mcp_tool_identifier(tool_name);
        if normalized.is_empty() {
            return None;
        }

        let tools = match mcp_client.list_mcp_tools().await {
            Ok(list) => list,
            Err(err) => {
                warn!(
                    "Failed to list MCP tools while resolving alias '{}': {}",
                    tool_name, err
                );
                return None;
            }
        };

        for tool in tools {
            if normalize_mcp_tool_identifier(&tool.name) == normalized {
                return Some(tool.name);
            }
        }

        None
    }

    /// Refresh MCP tools (reconnect to providers and update tool lists)
    pub async fn refresh_mcp_tools(&mut self) -> Result<()> {
        if let Some(mcp_client) = &self.mcp_client {
            debug!(
                "Refreshing MCP tools for {} providers",
                mcp_client.get_status().provider_count
            );

            let mut tools: Option<Vec<McpToolInfo>> = None;
            let mut last_err: Option<anyhow::Error> = None;
            for attempt in 0..3 {
                match mcp_client.list_mcp_tools().await {
                    Ok(list) => {
                        tools = Some(list);
                        break;
                    }
                    Err(err) => {
                        last_err = Some(err);
                        let jitter = (attempt as u64 * 37) % 80;
                        let pow = 2_u64.saturating_pow(attempt.min(4) as u32); // cap exponent
                        let backoff =
                            Duration::from_millis(200 * pow + jitter).min(Duration::from_secs(3));
                        warn!(
                            attempt = attempt + 1,
                            delay_ms = %backoff.as_millis(),
                            "Failed to list MCP tools, retrying with backoff"
                        );
                        tokio::time::sleep(backoff).await;
                    }
                }
            }

            let tools = match tools {
                Some(list) => list,
                None => {
                    warn!(
                        error = %last_err.unwrap_or_else(|| anyhow::anyhow!("unknown MCP error")),
                        "Failed to refresh MCP tools after retries; keeping existing cache"
                    );
                    self.mcp_healthy = false;
                    self.mcp_last_failed = Some(SystemTime::now());
                    return Ok(());
                }
            };
            let mut provider_map: HashMap<String, Vec<String>> = HashMap::new();

            for tool in &tools {
                let registration =
                    build_mcp_registration(Arc::clone(mcp_client), &tool.provider, tool, None);

                if !self.inventory.has_tool(registration.name())
                    && let Err(err) = self.inventory.register_tool(registration)
                {
                    warn!(
                        tool = %tool.name,
                        provider = %tool.provider,
                        error = %err,
                        "failed to register MCP proxy tool"
                    );
                }
            }

            for tool in tools {
                provider_map
                    .entry(tool.provider.clone())
                    .or_default()
                    .push(tool.name.clone());
            }

            for tools in provider_map.values_mut() {
                tools.sort();
                tools.dedup();
            }

            self.mcp_tool_index = provider_map;
            self.mcp_tool_presence.clear();
            for tools in self.mcp_tool_index.values() {
                for tool in tools {
                    self.mcp_tool_presence.insert(tool.clone(), true);
                }
            }

            if let Some(allowlist) = self
                .policy_gateway
                .update_mcp_tools(&self.mcp_tool_index)
                .await?
            {
                mcp_client.update_allowlist(allowlist);
            }

            self.sync_policy_catalog().await;
            self.mcp_healthy = true;
            self.mcp_last_failed = None;
            Ok(())
        } else {
            debug!("No MCP client configured, nothing to refresh");
            Ok(())
        }
    }
}

impl ToolRegistry {
    /// Prompt for permission before starting long-running tool executions to avoid spinner conflicts
    pub async fn preflight_tool_permission(&mut self, name: &str) -> Result<bool> {
        match self.evaluate_tool_policy(name).await? {
            ToolPermissionDecision::Allow => Ok(true),
            ToolPermissionDecision::Deny => Ok(false),
            ToolPermissionDecision::Prompt => Ok(true),
        }
    }

    pub async fn evaluate_tool_policy(&mut self, name: &str) -> Result<ToolPermissionDecision> {
        if let Some(tool_name) = name.strip_prefix("mcp_") {
            return self.evaluate_mcp_tool_policy(name, tool_name).await;
        }

        let canonical = canonical_tool_name(name);
        let normalized = canonical.as_ref();

        if !self.policy_gateway.has_policy_manager()
            && let Some(registration) = self.inventory.registration_for(normalized)
            && let Some(permission) = registration.default_permission()
        {
            return Ok(match permission {
                ToolPolicy::Allow => ToolPermissionDecision::Allow,
                ToolPolicy::Deny => ToolPermissionDecision::Deny,
                ToolPolicy::Prompt => ToolPermissionDecision::Prompt,
            });
        }

        self.policy_gateway.evaluate_tool_policy(normalized).await
    }

    async fn evaluate_mcp_tool_policy(
        &mut self,
        full_name: &str,
        tool_name: &str,
    ) -> Result<ToolPermissionDecision> {
        let provider = match self.find_mcp_provider(tool_name) {
            Some(provider) => provider,
            None => {
                // Unknown provider for this tool; default to prompt for safety
                return Ok(ToolPermissionDecision::Prompt);
            }
        };

        if self.policy_gateway.has_full_auto_allowlist()
            && !self.policy_gateway.is_allowed_in_full_auto(full_name)
        {
            return Ok(ToolPermissionDecision::Deny);
        }

        if let Ok(policy_manager) = self.policy_manager_mut() {
            match policy_manager.get_mcp_tool_policy(&provider, tool_name) {
                ToolPolicy::Allow => {
                    self.policy_gateway.preapprove(full_name);
                    Ok(ToolPermissionDecision::Allow)
                }
                ToolPolicy::Deny => Ok(ToolPermissionDecision::Deny),
                ToolPolicy::Prompt => {
                    // Always prompt for explicit "prompt" policy, even in full-auto mode
                    // This ensures human-in-the-loop approval for sensitive operations
                    Ok(ToolPermissionDecision::Prompt)
                }
            }
        } else {
            // Policy manager not available - default to prompt for safety
            // instead of auto-approving
            Ok(ToolPermissionDecision::Prompt)
        }
    }

    /// Mark a tool as pre-approved, but only for specific internal diagnostic tools.
    /// This prevents accidental bypass of permission gates for arbitrary tools.
    pub fn mark_tool_preapproved(&mut self, name: &str) {
        // Allowlist of tools that can be preapproved (typically for slash commands)
        const PREAPPROVABLE_TOOLS: &[&str] = &["debug_agent", "analyze_agent"];

        if PREAPPROVABLE_TOOLS.contains(&name) {
            self.policy_gateway.preapprove(name);
        } else {
            tracing::warn!(
                tool = %name,
                "Attempted to preapprove non-whitelisted tool. Use permission pipeline instead."
            );
        }
    }

    pub async fn persist_mcp_tool_policy(&mut self, name: &str, policy: ToolPolicy) -> Result<()> {
        if !name.starts_with("mcp_") {
            return Ok(());
        }

        let Some(tool_name) = name.strip_prefix("mcp_") else {
            return Ok(());
        };

        let Some(provider) = self.find_mcp_provider(tool_name) else {
            return Ok(());
        };

        self.policy_gateway
            .persist_mcp_tool_policy(&provider, tool_name, policy)
            .await
    }

    /// Get recent tool execution records
    pub fn get_recent_tool_executions(&self, count: usize) -> Vec<ToolExecutionRecord> {
        self.execution_history.get_recent_records(count)
    }

    /// Get recent tool executions (successes and failures)
    pub fn get_recent_tool_records(&self, count: usize) -> Vec<ToolExecutionRecord> {
        self.execution_history.get_recent_records(count)
    }

    /// Get recent tool execution failures
    pub fn get_recent_tool_failures(&self, count: usize) -> Vec<ToolExecutionRecord> {
        self.execution_history.get_recent_failures(count)
    }

    /// Clear the execution history
    pub fn clear_execution_history(&self) {
        self.execution_history.clear();
    }
}

fn normalize_mcp_tool_identifier(value: &str) -> String {
    let mut normalized = String::new();
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            normalized.push(ch.to_ascii_lowercase());
        }
    }
    normalized
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::plan::{PlanCompletionState, PlanStep, StepStatus, UpdatePlanArgs};
    use async_trait::async_trait;
    use serde_json::json;
    use std::time::Duration;
    use tempfile::TempDir;

    const CUSTOM_TOOL_NAME: &str = "custom_test_tool";

    struct CustomEchoTool;

    #[async_trait]
    impl Tool for CustomEchoTool {
        async fn execute(&self, args: Value) -> Result<Value> {
            Ok(json!({
                "success": true,
                "args": args,
            }))
        }

        fn name(&self) -> &'static str {
            CUSTOM_TOOL_NAME
        }

        fn description(&self) -> &'static str {
            "Custom echo tool for testing"
        }
    }

    #[tokio::test]
    async fn registers_builtin_tools() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
        let available = registry.available_tools().await;

        assert!(available.contains(&tools::READ_FILE.to_string()));
        assert!(available.contains(&tools::RUN_PTY_CMD.to_string()));
        Ok(())
    }

    #[tokio::test]
    async fn allows_registering_custom_tools() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let mut registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

        registry.register_tool(ToolRegistration::from_tool_instance(
            CUSTOM_TOOL_NAME,
            CapabilityLevel::CodeSearch,
            CustomEchoTool,
        ))?;

        registry.allow_all_tools().await.ok();

        let available = registry.available_tools().await;
        assert!(available.contains(&CUSTOM_TOOL_NAME.to_string()));

        let response = registry
            .execute_tool(CUSTOM_TOOL_NAME, json!({"input": "value"}))
            .await?;
        assert!(response["success"].as_bool().unwrap_or(false));
        Ok(())
    }

    #[tokio::test]
    async fn harness_snapshot_tracks_session_task_and_plan() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let mut registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

        registry.set_harness_session("session-test");
        registry.set_harness_task(Some("task-123".to_owned()));

        let plan_manager = registry.plan_manager();
        let updated_plan = plan_manager.update_plan(UpdatePlanArgs {
            explanation: Some("ensure snapshot captures plan state".to_owned()),
            plan: vec![PlanStep {
                step: "do-a-thing".to_owned(),
                status: StepStatus::InProgress,
            }],
        })?;

        let snapshot = registry.harness_context_snapshot();

        assert_eq!(snapshot.session_id, "session-test");
        assert_eq!(snapshot.task_id.as_deref(), Some("task-123"));
        assert_eq!(snapshot.plan_version, updated_plan.version);
        assert_eq!(snapshot.plan_summary.total_steps, 1);
        assert_eq!(
            snapshot.plan_summary.status,
            PlanCompletionState::InProgress
        );

        Ok(())
    }

    #[tokio::test]
    async fn execution_history_records_harness_context() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let mut registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

        registry.set_harness_session("session-history");
        registry.set_harness_task(Some("task-history".to_owned()));

        registry.register_tool(ToolRegistration::from_tool_instance(
            CUSTOM_TOOL_NAME,
            CapabilityLevel::CodeSearch,
            CustomEchoTool,
        ))?;
        registry.allow_all_tools().await?;

        let args = json!({"input": "value"});
        let response = registry
            .execute_tool(CUSTOM_TOOL_NAME, args.clone())
            .await?;
        assert!(response["success"].as_bool().unwrap_or(false));

        let records = registry.get_recent_tool_records(1);
        let record = records.first().expect("execution record captured");
        assert_eq!(record.tool_name, CUSTOM_TOOL_NAME);
        assert_eq!(record.context.session_id, "session-history");
        assert_eq!(record.context.task_id.as_deref(), Some("task-history"));
        assert_eq!(record.args, args);
        assert!(record.success);

        let current_plan = registry.current_plan();
        assert_eq!(
            record.context.plan_summary.total_steps,
            current_plan.summary.total_steps
        );

        Ok(())
    }

    #[tokio::test]
    async fn full_auto_allowlist_enforced() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let mut registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

        registry
            .enable_full_auto_mode(&vec![tools::READ_FILE.to_string()])
            .await;

        assert!(registry.preflight_tool_permission(tools::READ_FILE).await?);
        assert!(
            !registry
                .preflight_tool_permission(tools::RUN_PTY_CMD)
                .await?
        );

        Ok(())
    }

    #[test]
    fn normalizes_mcp_tool_identifiers() {
        assert_eq!(
            normalize_mcp_tool_identifier("sequential-thinking"),
            "sequentialthinking"
        );
        assert_eq!(
            normalize_mcp_tool_identifier("Context7.Lookup"),
            "context7lookup"
        );
        assert_eq!(normalize_mcp_tool_identifier("alpha_beta"), "alphabeta");
    }

    #[test]
    fn timeout_policy_derives_from_config() {
        let mut config = TimeoutsConfig::default();
        config.default_ceiling_seconds = 0;
        config.pty_ceiling_seconds = 600;
        config.mcp_ceiling_seconds = 90;
        config.warning_threshold_percent = 75;

        let policy = ToolTimeoutPolicy::from_config(&config);
        assert_eq!(policy.ceiling_for(ToolTimeoutCategory::Default), None);
        assert_eq!(
            policy.ceiling_for(ToolTimeoutCategory::Pty),
            Some(Duration::from_secs(600))
        );
        assert_eq!(
            policy.ceiling_for(ToolTimeoutCategory::Mcp),
            Some(Duration::from_secs(90))
        );
        assert!((policy.warning_fraction() - 0.75).abs() < f32::EPSILON);
    }
}
