//! Unified Safety Gateway
//!
//! Consolidates all safety checking mechanisms into a single gateway:
//! - Rate limiting (from runloop's tool_call_safety)
//! - Destructive tool detection
//! - Command policy enforcement
//! - Plan mode restrictions
//!
//! This provides consistent safety decisions across all tool execution paths.

use hashbrown::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;
use tokio::sync::Mutex;

use crate::config::CommandsConfig;
use crate::config::constants::tools;
use crate::dotfile_protection::{
    AccessContext, AccessType, DotfileGuardian, ProtectionDecision, get_global_guardian,
};
use crate::tools::apply_patch::{Patch, PatchOperation, decode_apply_patch_input};
use crate::tools::command_policy::CommandPolicyEvaluator;
use crate::tools::invocation::ToolInvocationId;
use crate::tools::registry::{
    RiskLevel, ToolRiskContext, ToolRiskScorer, ToolSource, WorkspaceTrust,
};
use crate::tools::tool_intent::classify_tool_intent;
use vtcode_config::core::DotfileProtectionConfig;

/// Trust level used by the safety gateway for approval bypass decisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SafetyTrustLevel {
    Untrusted,
    #[default]
    Standard,
    Elevated,
    Full,
}

impl SafetyTrustLevel {
    #[inline]
    pub const fn can_bypass_approval(self) -> bool {
        matches!(self, Self::Elevated | Self::Full)
    }
}

/// Minimal execution context required for safety decisions.
#[derive(Debug, Clone)]
pub struct SafetyContext {
    pub session_id: String,
    pub trust_level: SafetyTrustLevel,
}

impl SafetyContext {
    #[must_use]
    pub fn new(session_id: impl Into<String>) -> Self {
        Self {
            session_id: session_id.into(),
            trust_level: SafetyTrustLevel::default(),
        }
    }
}

/// Safety decision for a tool invocation
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SafetyDecision {
    /// Tool execution is allowed without user approval
    Allow,
    /// Tool execution is denied with a reason
    Deny(String),
    /// Tool execution requires user approval with justification
    NeedsApproval(String),
}

impl SafetyDecision {
    /// Whether execution can proceed (Allow only)
    #[inline]
    pub fn is_allowed(&self) -> bool {
        matches!(self, SafetyDecision::Allow)
    }

    /// Whether execution is blocked (Deny)
    #[inline]
    pub fn is_denied(&self) -> bool {
        matches!(self, SafetyDecision::Deny(_))
    }

    /// Whether user approval is needed
    #[inline]
    pub fn needs_approval(&self) -> bool {
        matches!(self, SafetyDecision::NeedsApproval(_))
    }

    /// Get the reason/justification if present
    pub fn reason(&self) -> Option<&str> {
        match self {
            SafetyDecision::Allow => None,
            SafetyDecision::Deny(reason) | SafetyDecision::NeedsApproval(reason) => Some(reason),
        }
    }
}

/// Errors from safety checks
#[derive(Debug, Error, Clone)]
pub enum SafetyError {
    #[error("Rate limit exceeded: {current} calls in {window} (max: {max})")]
    RateLimitExceeded {
        current: usize,
        max: usize,
        window: &'static str,
    },
    #[error("Per-turn tool limit reached (max: {max})")]
    TurnLimitReached { max: usize },
    #[error("Session tool limit reached (max: {max})")]
    SessionLimitReached { max: usize },
    #[error("Plan mode violation: {0}")]
    PlanModeViolation(String),
    #[error("Command policy denied: {0}")]
    CommandPolicyDenied(String),
    #[error("Dotfile protection violation: {0}")]
    DotfileProtectionViolation(String),
}

/// Result of a safety check with optional retry hint metadata.
#[derive(Debug, Clone)]
pub struct SafetyCheckResult {
    /// Final decision for this invocation.
    pub decision: SafetyDecision,
    /// Suggested delay before retrying if the decision is a rate-limit denial.
    pub retry_after: Option<Duration>,
    /// Structured error when denial is produced by a safety limit.
    pub violation: Option<SafetyError>,
}

/// Configuration for the safety gateway
#[derive(Debug, Clone)]
pub struct SafetyGatewayConfig {
    /// Maximum tool calls per turn
    pub max_per_turn: usize,
    /// Maximum tool calls per session
    pub max_per_session: usize,
    /// Rate limit: calls per second
    pub rate_limit_per_second: usize,
    /// Rate limit: calls per minute (optional burst protection)
    pub rate_limit_per_minute: Option<usize>,
    /// Whether plan mode is active (read-only)
    pub plan_mode_active: bool,
    /// Workspace trust level
    pub workspace_trust: WorkspaceTrust,
    /// Risk threshold for requiring approval
    pub approval_risk_threshold: RiskLevel,
    /// Enforce short-window rate limiting (per-second/per-minute).
    /// Turn/session limits are always enforced.
    pub enforce_rate_limits: bool,
}

impl Default for SafetyGatewayConfig {
    fn default() -> Self {
        let rate_limit_per_second = std::env::var("VTCODE_TOOL_RATE_LIMIT_PER_SECOND")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .filter(|v| *v > 0)
            .unwrap_or(5);

        let rate_limit_per_minute = std::env::var("VTCODE_TOOL_CALLS_PER_MIN")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .filter(|v| *v > 0);

        Self {
            max_per_turn: 10,
            max_per_session: 100,
            rate_limit_per_second,
            rate_limit_per_minute,
            plan_mode_active: false,
            workspace_trust: WorkspaceTrust::Trusted,
            approval_risk_threshold: RiskLevel::Medium,
            enforce_rate_limits: true,
        }
    }
}

/// Rate limiter state (shared across async contexts)
#[derive(Debug, Default)]
struct RateLimiterState {
    calls_per_second: std::collections::VecDeque<Instant>,
    calls_per_minute: std::collections::VecDeque<Instant>,
    current_turn_count: usize,
    session_count: usize,
}

/// Unified Safety Gateway
///
/// Consolidates rate limiting, destructive tool detection, command policy
/// enforcement, plan mode restrictions, and dotfile protection into a single
/// safety decision point.
pub struct SafetyGateway {
    /// Configuration
    config: SafetyGatewayConfig,
    /// Command policy evaluator (optional, for shell commands)
    command_policy: Option<CommandPolicyEvaluator>,
    /// Rate limiter state
    rate_state: Arc<Mutex<RateLimiterState>>,
    /// Preapproved tools for this session
    preapproved: Arc<Mutex<HashSet<String>>>,
    /// Dotfile guardian for protected file access
    dotfile_guardian: Option<Arc<DotfileGuardian>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct FileAccessTarget {
    path: PathBuf,
    access_type: AccessType,
}

fn primary_path_arg(args: &Value) -> Option<&str> {
    args.get("path")
        .and_then(|value| value.as_str())
        .or_else(|| args.get("file_path").and_then(|value| value.as_str()))
        .or_else(|| args.get("filepath").and_then(|value| value.as_str()))
        .or_else(|| args.get("target_path").and_then(|value| value.as_str()))
}

fn destination_path_arg(args: &Value) -> Option<&str> {
    args.get("destination").and_then(|value| value.as_str())
}

fn push_file_access_target(
    targets: &mut Vec<FileAccessTarget>,
    path: &str,
    access_type: AccessType,
) {
    let path_str = path.trim();
    if path_str.is_empty() {
        return;
    }

    let path = PathBuf::from(path_str);
    // For small number of targets, linear search is faster than HashSet.
    // In large patches, we'll use a local HashSet in patch_file_access_targets.
    if targets
        .iter()
        .any(|existing| existing.path == path && existing.access_type == access_type)
    {
        return;
    }

    targets.push(FileAccessTarget { path, access_type });
}

fn command_text_for_tool(tool_name: &str, args: &Value) -> Option<String> {
    match tool_name {
        "shell" | tools::RUN_PTY_CMD => crate::tools::command_args::command_text(args)
            .ok()
            .flatten(),
        tools::SEND_PTY_INPUT => {
            crate::tools::command_args::interactive_input_text(args).map(str::to_owned)
        }
        tools::UNIFIED_EXEC => match crate::tools::tool_intent::unified_exec_action(args) {
            Some(action) if action.eq_ignore_ascii_case("run") => {
                crate::tools::command_args::command_text(args)
                    .ok()
                    .flatten()
            }
            Some(action)
                if action.eq_ignore_ascii_case("write")
                    || action.eq_ignore_ascii_case("continue") =>
            {
                crate::tools::command_args::interactive_input_text(args).map(str::to_owned)
            }
            _ => None,
        },
        _ => None,
    }
}

fn patch_file_access_targets(args: &Value) -> Vec<FileAccessTarget> {
    let Ok(Some(patch_input)) = decode_apply_patch_input(args) else {
        return Vec::new();
    };
    let Ok(patch) = Patch::parse(&patch_input.text) else {
        return Vec::new();
    };

    let mut targets = Vec::new();
    for operation in patch.operations() {
        match operation {
            PatchOperation::AddFile { path, .. } => {
                push_file_access_target(&mut targets, path, AccessType::Write);
            }
            PatchOperation::DeleteFile { path } => {
                push_file_access_target(&mut targets, path, AccessType::Delete);
            }
            PatchOperation::UpdateFile { path, new_path, .. } => {
                push_file_access_target(&mut targets, path, AccessType::Modify);
                if let Some(destination) =
                    new_path.as_deref().filter(|candidate| *candidate != path)
                {
                    push_file_access_target(&mut targets, destination, AccessType::Write);
                }
            }
        }
    }

    targets
}

fn file_access_targets(tool_name: &str, args: &Value) -> Vec<FileAccessTarget> {
    let mut targets = Vec::new();

    match tool_name {
        tools::WRITE_FILE | tools::CREATE_FILE => {
            if let Some(path) = primary_path_arg(args) {
                push_file_access_target(&mut targets, path, AccessType::Write);
            }
        }
        tools::EDIT_FILE | "search_replace" => {
            if let Some(path) = primary_path_arg(args) {
                push_file_access_target(&mut targets, path, AccessType::Modify);
            }
        }
        tools::DELETE_FILE => {
            if let Some(path) = primary_path_arg(args) {
                push_file_access_target(&mut targets, path, AccessType::Delete);
            }
        }
        tools::MOVE_FILE => {
            if let Some(path) = primary_path_arg(args) {
                push_file_access_target(&mut targets, path, AccessType::Modify);
            }
            if let Some(path) = destination_path_arg(args) {
                push_file_access_target(&mut targets, path, AccessType::Write);
            }
        }
        tools::COPY_FILE => {
            if let Some(path) = destination_path_arg(args) {
                push_file_access_target(&mut targets, path, AccessType::Write);
            }
        }
        tools::APPLY_PATCH => {
            targets.extend(patch_file_access_targets(args));
        }
        tools::UNIFIED_FILE => match crate::tools::tool_intent::unified_file_action(args) {
            Some(action) if action.eq_ignore_ascii_case("write") => {
                if let Some(path) = primary_path_arg(args) {
                    push_file_access_target(&mut targets, path, AccessType::Write);
                }
            }
            Some(action) if action.eq_ignore_ascii_case("edit") => {
                if let Some(path) = primary_path_arg(args) {
                    push_file_access_target(&mut targets, path, AccessType::Modify);
                }
            }
            Some(action) if action.eq_ignore_ascii_case("delete") => {
                if let Some(path) = primary_path_arg(args) {
                    push_file_access_target(&mut targets, path, AccessType::Delete);
                }
            }
            Some(action) if action.eq_ignore_ascii_case("move") => {
                if let Some(path) = primary_path_arg(args) {
                    push_file_access_target(&mut targets, path, AccessType::Modify);
                }
                if let Some(path) = destination_path_arg(args) {
                    push_file_access_target(&mut targets, path, AccessType::Write);
                }
            }
            Some(action) if action.eq_ignore_ascii_case("copy") => {
                if let Some(path) = destination_path_arg(args) {
                    push_file_access_target(&mut targets, path, AccessType::Write);
                }
            }
            Some(action) if action.eq_ignore_ascii_case("patch") => {
                targets.extend(patch_file_access_targets(args));
            }
            _ => {}
        },
        _ => {}
    }

    targets
}

fn proposed_changes_preview(args: &Value) -> String {
    const PREVIEW_LIMIT: usize = 500;

    let preview_text = |label: &str, text: &str| {
        let preview_len = text.len().min(PREVIEW_LIMIT);
        format!(
            "{label} ({} bytes):\n{}{}",
            text.len(),
            &text[..preview_len],
            if text.len() > preview_len { "..." } else { "" }
        )
    };

    if let Some(content) = args.get("content").and_then(|value| value.as_str()) {
        return preview_text("Content", content);
    }

    if let Some(old_str) = args.get("old_str").and_then(|value| value.as_str()) {
        let new_str = args
            .get("new_str")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        return format!("Replace:\n  '{}'\nWith:\n  '{}'", old_str, new_str);
    }

    if let Ok(Some(patch_input)) = decode_apply_patch_input(args) {
        return preview_text("Patch", &patch_input.text);
    }

    "No details provided".to_string()
}

impl SafetyGateway {
    /// Create a new safety gateway with default configuration
    pub fn new() -> Self {
        Self::with_config(SafetyGatewayConfig::default())
    }

    /// Create a new safety gateway with custom configuration
    pub fn with_config(config: SafetyGatewayConfig) -> Self {
        Self {
            config,
            command_policy: None,
            rate_state: Arc::new(Mutex::new(RateLimiterState::default())),
            preapproved: Arc::new(Mutex::new(HashSet::new())),
            dotfile_guardian: None,
        }
    }

    /// Set the dotfile guardian for protected file access
    pub fn with_dotfile_guardian(mut self, guardian: Arc<DotfileGuardian>) -> Self {
        self.dotfile_guardian = Some(guardian);
        self
    }

    /// Create and set a dotfile guardian from configuration
    pub async fn with_dotfile_protection(
        mut self,
        config: DotfileProtectionConfig,
    ) -> anyhow::Result<Self> {
        let guardian = DotfileGuardian::new(config).await?;
        self.dotfile_guardian = Some(Arc::new(guardian));
        Ok(self)
    }

    /// Set the command policy evaluator for shell command checks
    pub fn with_command_policy(mut self, policy: CommandPolicyEvaluator) -> Self {
        self.command_policy = Some(policy);
        self
    }

    /// Create from commands config
    pub fn with_commands_config(mut self, config: &CommandsConfig) -> Self {
        self.command_policy = Some(CommandPolicyEvaluator::from_config(config));
        self
    }

    /// Enable or disable plan mode
    pub fn set_plan_mode(&mut self, active: bool) {
        self.config.plan_mode_active = active;
    }

    /// Set workspace trust level
    pub fn set_workspace_trust(&mut self, trust: WorkspaceTrust) {
        self.config.workspace_trust = trust;
    }

    /// Update rate limits
    pub fn set_limits(&mut self, max_per_turn: usize, max_per_session: usize) {
        self.config.max_per_turn = max_per_turn;
        self.config.max_per_session = max_per_session;
    }

    /// Update rate-limiter thresholds.
    pub fn set_rate_limits(
        &mut self,
        rate_limit_per_second: usize,
        rate_limit_per_minute: Option<usize>,
    ) {
        if rate_limit_per_second > 0 {
            self.config.rate_limit_per_second = rate_limit_per_second;
        }
        self.config.rate_limit_per_minute = rate_limit_per_minute.filter(|v| *v > 0);
    }

    /// Enable or disable rate-limit enforcement while preserving counters.
    pub fn set_rate_limit_enforcement(&mut self, enabled: bool) {
        self.config.enforce_rate_limits = enabled;
    }

    /// Increase session limit dynamically
    pub fn increase_session_limit(&mut self, increment: usize) {
        let new_max = self.config.max_per_session.saturating_add(increment);
        self.config.max_per_session = new_max;
        tracing::info!("Session tool limit increased to {}", new_max);
    }

    /// Reset turn counters (call at start of new turn)
    pub async fn start_turn(&self) {
        let mut state = self.rate_state.lock().await;
        state.current_turn_count = 0;
        state.calls_per_second.clear();
        state.calls_per_minute.clear();
    }

    /// Preapprove a tool for this session
    pub async fn preapprove(&self, tool_name: &str) {
        let mut preapproved = self.preapproved.lock().await;
        preapproved.insert(tool_name.to_string());
    }

    /// Check if a tool is preapproved
    pub async fn is_preapproved(&self, tool_name: &str) -> bool {
        let preapproved = self.preapproved.lock().await;
        preapproved.contains(tool_name)
    }

    /// Check if a tool is destructive
    pub fn is_destructive(&self, tool_name: &str) -> bool {
        classify_tool_intent(tool_name, &Value::Object(Default::default())).destructive
    }

    /// Check if a tool is mutating
    pub fn is_mutating(&self, tool_name: &str) -> bool {
        classify_tool_intent(tool_name, &Value::Object(Default::default())).mutating
    }

    fn is_destructive_call(&self, tool_name: &str, args: &Value) -> bool {
        classify_tool_intent(tool_name, args).destructive
    }

    fn is_mutating_call(&self, tool_name: &str, args: &Value) -> bool {
        classify_tool_intent(tool_name, args).mutating
    }

    /// Main entry point: check safety for a tool invocation
    ///
    /// Returns a SafetyDecision indicating whether execution can proceed.
    pub async fn check_safety(
        &self,
        ctx: &SafetyContext,
        tool_name: &str,
        args: &Value,
    ) -> SafetyDecision {
        self.check_safety_with_id(ctx, tool_name, args, None).await
    }

    /// Check safety with explicit invocation ID for correlation
    pub async fn check_safety_with_id(
        &self,
        ctx: &SafetyContext,
        tool_name: &str,
        args: &Value,
        invocation_id: Option<ToolInvocationId>,
    ) -> SafetyDecision {
        let inv_id = invocation_id
            .map(|id| id.short())
            .unwrap_or_else(|| "unknown".to_string());

        tracing::trace!(
            invocation_id = %inv_id,
            tool = %tool_name,
            "SafetyGateway: checking safety"
        );

        if let Err(err) = self.check_rate_limits().await {
            tracing::warn!(
                invocation_id = %inv_id,
                error = %err,
                "SafetyGateway: rate limit exceeded"
            );
            return SafetyDecision::Deny(err.to_string());
        }

        self.evaluate_non_rate_decision(ctx, tool_name, args, &inv_id)
            .await
    }

    /// Check safety and atomically reserve a rate-limit slot on success.
    ///
    /// This avoids split check/record races by validating rate limits and recording
    /// execution under a single lock acquisition.
    pub async fn check_and_record(
        &self,
        ctx: &SafetyContext,
        tool_name: &str,
        args: &Value,
    ) -> SafetyCheckResult {
        self.check_and_record_with_id(ctx, tool_name, args, None)
            .await
    }

    /// Check safety with correlation ID and atomically reserve a rate-limit slot.
    pub async fn check_and_record_with_id(
        &self,
        ctx: &SafetyContext,
        tool_name: &str,
        args: &Value,
        invocation_id: Option<ToolInvocationId>,
    ) -> SafetyCheckResult {
        let inv_id = invocation_id
            .map(|id| id.short())
            .unwrap_or_else(|| "unknown".to_string());
        tracing::trace!(
            invocation_id = %inv_id,
            tool = %tool_name,
            "SafetyGateway: checking and recording safety"
        );

        let decision = self
            .evaluate_non_rate_decision(ctx, tool_name, args, &inv_id)
            .await;

        if decision.is_denied() {
            return SafetyCheckResult {
                decision,
                retry_after: None,
                violation: None,
            };
        }

        let now = Instant::now();
        let mut state = self.rate_state.lock().await;
        match self.check_rate_limits_locked(&mut state, now) {
            Ok(()) => {
                self.record_execution_locked(&mut state, now);
                SafetyCheckResult {
                    decision,
                    retry_after: None,
                    violation: None,
                }
            }
            Err(err) => {
                tracing::warn!(
                    invocation_id = %inv_id,
                    error = %err,
                    "SafetyGateway: rate limit exceeded during atomic reservation"
                );
                SafetyCheckResult {
                    decision: SafetyDecision::Deny(err.to_string()),
                    retry_after: self.retry_after_for_violation(&err, &state, now),
                    violation: Some(err),
                }
            }
        }
    }

    async fn evaluate_non_rate_decision(
        &self,
        ctx: &SafetyContext,
        tool_name: &str,
        args: &Value,
        inv_id: &str,
    ) -> SafetyDecision {
        if let Some(decision) = self
            .check_dotfile_protection(tool_name, args, &ctx.session_id)
            .await
        {
            tracing::info!(
                invocation_id = %inv_id,
                tool = %tool_name,
                "SafetyGateway: dotfile protection triggered"
            );
            return decision;
        }

        if self.config.plan_mode_active && self.is_mutating_call(tool_name, args) {
            let reason = format!(
                "Tool '{}' is blocked in plan mode (read-only). Switch to edit mode to execute.",
                tool_name
            );
            tracing::info!(
                invocation_id = %inv_id,
                tool = %tool_name,
                "SafetyGateway: plan mode violation"
            );
            return SafetyDecision::Deny(reason);
        }

        if let Some(ref policy) = self.command_policy
            && let Some(command) = command_text_for_tool(tool_name, args)
            && !policy.allows_text(&command)
        {
            let reason = format!("Command '{}' blocked by policy", command);
            tracing::info!(
                invocation_id = %inv_id,
                command = %command,
                "SafetyGateway: command policy denied"
            );
            return SafetyDecision::Deny(reason);
        }

        if self.is_preapproved(tool_name).await {
            tracing::trace!(
                invocation_id = %inv_id,
                tool = %tool_name,
                "SafetyGateway: tool preapproved"
            );
            return SafetyDecision::Allow;
        }

        if ctx.trust_level.can_bypass_approval() {
            tracing::trace!(
                invocation_id = %inv_id,
                tool = %tool_name,
                trust_level = ?ctx.trust_level,
                "SafetyGateway: trust level allows bypass"
            );
            return SafetyDecision::Allow;
        }

        let risk_ctx = self.build_risk_context(tool_name, args);
        let risk_level = ToolRiskScorer::calculate_risk(&risk_ctx);

        if ToolRiskScorer::requires_justification(risk_level, self.config.approval_risk_threshold) {
            let justification = self.build_approval_justification(tool_name, &risk_level, args);
            tracing::info!(
                invocation_id = %inv_id,
                tool = %tool_name,
                risk = %risk_level,
                "SafetyGateway: requires approval"
            );
            return SafetyDecision::NeedsApproval(justification);
        }

        if self.is_destructive_call(tool_name, args) {
            let justification = format!(
                "Tool '{}' is destructive and may modify files or execute commands.",
                tool_name
            );
            tracing::info!(
                invocation_id = %inv_id,
                tool = %tool_name,
                "SafetyGateway: destructive tool requires approval"
            );
            return SafetyDecision::NeedsApproval(justification);
        }

        SafetyDecision::Allow
    }

    /// Record that a tool call was executed (for rate limiting)
    pub async fn record_execution(&self) {
        let mut state = self.rate_state.lock().await;
        self.record_execution_locked(&mut state, Instant::now());
    }

    /// Check rate limits without recording
    async fn check_rate_limits(&self) -> Result<(), SafetyError> {
        let mut state = self.rate_state.lock().await;
        self.check_rate_limits_locked(&mut state, Instant::now())
    }

    fn check_rate_limits_locked(
        &self,
        state: &mut RateLimiterState,
        now: Instant,
    ) -> Result<(), SafetyError> {
        self.prune_rate_windows(state, now);

        if self.config.enforce_rate_limits {
            if state.calls_per_second.len() >= self.config.rate_limit_per_second {
                return Err(SafetyError::RateLimitExceeded {
                    current: state.calls_per_second.len(),
                    max: self.config.rate_limit_per_second,
                    window: "1s",
                });
            }

            if let Some(limit) = self.config.rate_limit_per_minute
                && state.calls_per_minute.len() >= limit
            {
                return Err(SafetyError::RateLimitExceeded {
                    current: state.calls_per_minute.len(),
                    max: limit,
                    window: "60s",
                });
            }
        }

        if state.current_turn_count >= self.config.max_per_turn {
            return Err(SafetyError::TurnLimitReached {
                max: self.config.max_per_turn,
            });
        }

        if state.session_count >= self.config.max_per_session {
            return Err(SafetyError::SessionLimitReached {
                max: self.config.max_per_session,
            });
        }

        Ok(())
    }

    fn record_execution_locked(&self, state: &mut RateLimiterState, now: Instant) {
        state.current_turn_count = state.current_turn_count.saturating_add(1);
        state.session_count = state.session_count.saturating_add(1);
        state.calls_per_second.push_back(now);
        state.calls_per_minute.push_back(now);
    }

    fn prune_rate_windows(&self, state: &mut RateLimiterState, now: Instant) {
        while let Some(front) = state.calls_per_second.front() {
            if now.duration_since(*front) > Duration::from_secs(1) {
                state.calls_per_second.pop_front();
            } else {
                break;
            }
        }
        while let Some(front) = state.calls_per_minute.front() {
            if now.duration_since(*front) > Duration::from_secs(60) {
                state.calls_per_minute.pop_front();
            } else {
                break;
            }
        }
    }

    fn retry_after_for_violation(
        &self,
        violation: &SafetyError,
        state: &RateLimiterState,
        now: Instant,
    ) -> Option<Duration> {
        match violation {
            SafetyError::RateLimitExceeded { window: "1s", .. } => state
                .calls_per_second
                .front()
                .map(|first| Duration::from_secs(1).saturating_sub(now.duration_since(*first))),
            SafetyError::RateLimitExceeded { window: "60s", .. } => state
                .calls_per_minute
                .front()
                .map(|first| Duration::from_secs(60).saturating_sub(now.duration_since(*first))),
            _ => None,
        }
    }

    /// Check dotfile protection for file operations.
    /// Returns Some(SafetyDecision) if dotfile protection applies, None otherwise.
    async fn check_dotfile_protection(
        &self,
        tool_name: &str,
        args: &Value,
        session_id: &str,
    ) -> Option<SafetyDecision> {
        // Use local guardian if set, otherwise try global guardian
        let guardian = match self.dotfile_guardian.as_ref() {
            Some(g) => g.clone(),
            None => get_global_guardian()?,
        };

        let file_targets = file_access_targets(tool_name, args);
        if file_targets.is_empty() {
            return None;
        }

        let proposed_changes = proposed_changes_preview(args);

        for target in file_targets {
            if !guardian.is_protected(Path::new(&target.path)) {
                continue;
            }

            let context =
                AccessContext::new(&target.path, target.access_type, tool_name, session_id)
                    .with_proposed_changes(&proposed_changes);

            match guardian.request_access(&context).await {
                Ok(ProtectionDecision::Allowed) => continue,
                Ok(ProtectionDecision::RequiresConfirmation(req)) => {
                    return Some(SafetyDecision::NeedsApproval(format!(
                        "DOTFILE PROTECTION\n\n\
                        File: {}\n\
                        Operation: {}\n\
                        Reason: {}\n\n\
                        Proposed changes:\n{}\n\n\
                        {}",
                        req.file_path,
                        req.access_type,
                        req.protection_reason,
                        req.proposed_changes,
                        req.warning
                    )));
                }
                Ok(ProtectionDecision::RequiresSecondaryAuth(req)) => {
                    return Some(SafetyDecision::NeedsApproval(format!(
                        "DOTFILE SECONDARY AUTHENTICATION REQUIRED\n\n\
                        File: {} (whitelisted)\n\
                        Operation: {}\n\
                        Reason: {}\n\n\
                        This file is on the whitelist but requires secondary authentication.\n\n\
                        Proposed changes:\n{}\n\n\
                        {}",
                        req.file_path,
                        req.access_type,
                        req.protection_reason,
                        req.proposed_changes,
                        req.warning
                    )));
                }
                Ok(ProtectionDecision::Blocked(violation)) => {
                    return Some(SafetyDecision::Deny(format!(
                        "DOTFILE MODIFICATION BLOCKED\n\n\
                            File: {}\n\
                            Reason: {}\n\n\
                            Suggestion: {}",
                        violation.file_path, violation.reason, violation.suggestion
                    )));
                }
                Ok(ProtectionDecision::Denied(violation)) => {
                    return Some(SafetyDecision::Deny(format!(
                        "DOTFILE ACCESS DENIED\n\n\
                            File: {}\n\
                            Reason: {}\n\n\
                            Suggestion: {}",
                        violation.file_path, violation.reason, violation.suggestion
                    )));
                }
                Err(e) => {
                    tracing::error!("Dotfile protection check failed: {}", e);
                    return Some(SafetyDecision::Deny(format!(
                        "Dotfile protection check failed: {}",
                        e
                    )));
                }
            }
        }

        None
    }

    /// Get the dotfile guardian (if configured)
    pub fn dotfile_guardian(&self) -> Option<&Arc<DotfileGuardian>> {
        self.dotfile_guardian.as_ref()
    }

    /// Build risk context from tool name and arguments
    fn build_risk_context(&self, tool_name: &str, args: &Value) -> ToolRiskContext {
        let source = if tool_name.starts_with("mcp_") {
            ToolSource::Mcp
        } else if tool_name.starts_with("acp_") {
            ToolSource::Acp
        } else {
            ToolSource::Internal
        };

        let action = args.get("action").and_then(|v| v.as_str());
        let risk_tool_name = if tool_name == tools::UNIFIED_SEARCH && action == Some("web") {
            "unified_search:web"
        } else {
            tool_name
        };

        let mut ctx = ToolRiskContext::new(
            risk_tool_name.to_string(),
            source,
            self.config.workspace_trust,
        );

        // Set flags based on tool type
        if self.is_mutating_call(tool_name, args) {
            ctx = ctx.as_write();
        }
        if self.is_destructive_call(tool_name, args) {
            ctx = ctx.as_destructive();
        }

        // Check for network access
        if tool_name == "web_search"
            || tool_name == "fetch_url"
            || (tool_name == tools::UNIFIED_SEARCH && action == Some("web"))
        {
            ctx = ctx.accesses_network();
        }

        // Extract command args for shell tools
        if let Some(command) = command_text_for_tool(tool_name, args) {
            ctx = ctx.with_args(command.split_whitespace().map(String::from).collect());
        }

        ctx
    }

    /// Build justification message for approval prompt
    fn build_approval_justification(
        &self,
        tool_name: &str,
        risk_level: &RiskLevel,
        args: &Value,
    ) -> String {
        let mut parts = Vec::new();

        parts.push(format!("Tool: {}", tool_name));
        parts.push(format!("Risk level: {}", risk_level));

        if self.is_destructive_call(tool_name, args) {
            parts.push("This tool may modify or delete files.".to_string());
        }

        if let Some(command) = command_text_for_tool(tool_name, args) {
            parts.push(format!("Command: {}", command));
        }

        let file_targets = file_access_targets(tool_name, args);
        if let Some(target) = file_targets.first() {
            parts.push(format!("Path: {}", target.path.display()));
            if file_targets.len() > 1 {
                parts.push(format!("Additional targets: {}", file_targets.len() - 1));
            }
        }

        parts.join("\n")
    }

    /// Get current session statistics
    pub async fn get_stats(&self) -> SafetyStats {
        let state = self.rate_state.lock().await;
        let preapproved = self.preapproved.lock().await;

        SafetyStats {
            turn_count: state.current_turn_count,
            session_count: state.session_count,
            max_per_turn: self.config.max_per_turn,
            max_per_session: self.config.max_per_session,
            plan_mode_active: self.config.plan_mode_active,
            preapproved_count: preapproved.len(),
        }
    }
}

impl Default for SafetyGateway {
    fn default() -> Self {
        Self::new()
    }
}

/// Statistics from the safety gateway
#[derive(Debug, Clone)]
pub struct SafetyStats {
    pub turn_count: usize,
    pub session_count: usize,
    pub max_per_turn: usize,
    pub max_per_session: usize,
    pub plan_mode_active: bool,
    pub preapproved_count: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use vtcode_config::core::DotfileProtectionConfig;

    fn make_ctx() -> SafetyContext {
        SafetyContext::new("test-session")
    }

    #[tokio::test]
    async fn test_allow_read_only_tools() {
        let gateway = SafetyGateway::new();
        let ctx = make_ctx();

        let decision = gateway
            .check_safety(&ctx, "read_file", &serde_json::json!({"path": "/tmp/test"}))
            .await;

        assert!(decision.is_allowed());
    }

    #[tokio::test]
    async fn test_destructive_tool_needs_approval() {
        let gateway = SafetyGateway::new();
        let ctx = make_ctx();

        let decision = gateway
            .check_safety(
                &ctx,
                "delete_file",
                &serde_json::json!({"path": "/tmp/test"}),
            )
            .await;

        assert!(decision.needs_approval());
    }

    #[tokio::test]
    async fn test_plan_mode_blocks_mutating() {
        let mut gateway = SafetyGateway::new();
        gateway.set_plan_mode(true);
        let ctx = make_ctx();

        let decision = gateway
            .check_safety(
                &ctx,
                "write_file",
                &serde_json::json!({"path": "/tmp/test"}),
            )
            .await;

        assert!(decision.is_denied());
        assert!(decision.reason().unwrap().contains("plan mode"));
    }

    #[tokio::test]
    async fn test_preapproved_tools_allowed() {
        let gateway = SafetyGateway::new();
        gateway.preapprove("delete_file").await;
        let ctx = make_ctx();

        let decision = gateway
            .check_safety(
                &ctx,
                "delete_file",
                &serde_json::json!({"path": "/tmp/test"}),
            )
            .await;

        assert!(decision.is_allowed());
    }

    #[tokio::test]
    async fn test_trust_level_bypass() {
        let gateway = SafetyGateway::new();
        let mut ctx = make_ctx();
        ctx.trust_level = SafetyTrustLevel::Full;

        let decision = gateway
            .check_safety(
                &ctx,
                "delete_file",
                &serde_json::json!({"path": "/tmp/test"}),
            )
            .await;

        assert!(decision.is_allowed());
    }

    #[tokio::test]
    async fn test_rate_limiting() {
        let config = SafetyGatewayConfig {
            rate_limit_per_second: 2,
            ..Default::default()
        };
        let gateway = SafetyGateway::with_config(config);
        let ctx = make_ctx();

        // First two calls should succeed
        gateway.record_execution().await;
        gateway.record_execution().await;

        // Third call should be denied
        let decision = gateway
            .check_safety(&ctx, "read_file", &serde_json::json!({}))
            .await;

        assert!(decision.is_denied());
        assert!(decision.reason().unwrap().contains("Rate limit"));
    }

    #[tokio::test]
    async fn test_atomic_check_and_record_rate_limited() {
        let config = SafetyGatewayConfig {
            rate_limit_per_second: 2,
            ..Default::default()
        };
        let gateway = SafetyGateway::with_config(config);
        let ctx = make_ctx();

        let first = gateway
            .check_and_record(&ctx, "read_file", &serde_json::json!({}))
            .await;
        assert!(first.decision.is_allowed());

        let second = gateway
            .check_and_record(&ctx, "read_file", &serde_json::json!({}))
            .await;
        assert!(second.decision.is_allowed());

        let third = gateway
            .check_and_record(&ctx, "read_file", &serde_json::json!({}))
            .await;
        assert!(third.decision.is_denied());
        assert!(third.retry_after.is_some());
        assert!(matches!(
            third.violation,
            Some(SafetyError::RateLimitExceeded { .. })
        ));
    }

    #[tokio::test]
    async fn test_command_policy_enforcement() {
        let mut commands_config = CommandsConfig::default();
        commands_config.deny_list.push("rm".to_string());

        let gateway = SafetyGateway::new().with_commands_config(&commands_config);
        let ctx = make_ctx();

        let decision = gateway
            .check_safety(&ctx, "shell", &serde_json::json!({"command": "rm -rf /"}))
            .await;

        assert!(decision.is_denied());
    }

    #[tokio::test]
    async fn test_unified_exec_command_policy_enforcement_with_indexed_args() {
        let mut commands_config = CommandsConfig::default();
        commands_config.deny_list.push("rm".to_string());

        let gateway = SafetyGateway::new().with_commands_config(&commands_config);
        let ctx = make_ctx();

        let decision = gateway
            .check_safety(
                &ctx,
                tools::UNIFIED_EXEC,
                &serde_json::json!({
                    "command.0": "rm",
                    "command.1": "-rf",
                    "command.2": "/"
                }),
            )
            .await;

        assert!(decision.is_denied());
    }

    #[tokio::test]
    async fn test_unified_exec_continue_command_policy_enforcement_with_input() {
        let mut commands_config = CommandsConfig::default();
        commands_config.deny_list.push("rm".to_string());

        let gateway = SafetyGateway::new().with_commands_config(&commands_config);
        let ctx = make_ctx();

        let decision = gateway
            .check_safety(
                &ctx,
                tools::UNIFIED_EXEC,
                &serde_json::json!({
                    "action": "continue",
                    "session_id": "run-123",
                    "input": "rm -rf /\n"
                }),
            )
            .await;

        assert!(decision.is_denied());
    }

    #[tokio::test]
    async fn test_send_pty_input_command_policy_enforcement_with_input() {
        let mut commands_config = CommandsConfig::default();
        commands_config.deny_list.push("rm".to_string());

        let gateway = SafetyGateway::new().with_commands_config(&commands_config);
        let ctx = make_ctx();

        let decision = gateway
            .check_safety(
                &ctx,
                tools::SEND_PTY_INPUT,
                &serde_json::json!({
                    "session_id": "run-123",
                    "input": "rm -rf /\n"
                }),
            )
            .await;

        assert!(decision.is_denied());
    }

    #[tokio::test]
    async fn test_apply_patch_dotfile_protection_requires_approval() {
        let guardian = Arc::new(
            DotfileGuardian::new(DotfileProtectionConfig {
                audit_logging_enabled: false,
                create_backups: false,
                ..Default::default()
            })
            .await
            .expect("guardian should initialize"),
        );
        let gateway = SafetyGateway::new().with_dotfile_guardian(guardian);
        let ctx = make_ctx();

        let decision = gateway
            .check_safety(
                &ctx,
                tools::APPLY_PATCH,
                &serde_json::json!({
                    "input": "*** Begin Patch\n*** Update File: .gitignore\n@@\n-old\n+new\n*** End Patch\n"
                }),
            )
            .await;

        assert!(decision.needs_approval());
        assert!(
            decision
                .reason()
                .is_some_and(|reason| reason.contains(".gitignore"))
        );
    }

    #[test]
    fn test_patch_file_access_targets_preserve_patch_order() {
        let targets = patch_file_access_targets(&serde_json::json!({
            "input": "*** Begin Patch\n*** Update File: src/main.rs\n@@\n-old\n+new\n*** Update File: .gitignore\n@@\n-old\n+new\n*** End Patch\n"
        }));

        assert_eq!(targets.len(), 2);
        assert_eq!(targets[0].path, PathBuf::from("src/main.rs"));
        assert_eq!(targets[0].access_type, AccessType::Modify);
        assert_eq!(targets[1].path, PathBuf::from(".gitignore"));
        assert_eq!(targets[1].access_type, AccessType::Modify);
    }

    #[tokio::test]
    async fn test_unified_file_patch_dotfile_protection_requires_approval() {
        let guardian = Arc::new(
            DotfileGuardian::new(DotfileProtectionConfig {
                audit_logging_enabled: false,
                create_backups: false,
                ..Default::default()
            })
            .await
            .expect("guardian should initialize"),
        );
        let gateway = SafetyGateway::new().with_dotfile_guardian(guardian);
        let ctx = make_ctx();

        let decision = gateway
            .check_safety(
                &ctx,
                tools::UNIFIED_FILE,
                &serde_json::json!({
                    "action": "patch",
                    "patch": "*** Begin Patch\n*** Update File: .gitignore\n@@\n-old\n+new\n*** End Patch\n"
                }),
            )
            .await;

        assert!(decision.needs_approval());
        assert!(
            decision
                .reason()
                .is_some_and(|reason| reason.contains(".gitignore"))
        );
    }

    #[tokio::test]
    async fn test_stats_tracking() {
        let gateway = SafetyGateway::new();
        gateway.preapprove("test_tool").await;
        gateway.record_execution().await;
        gateway.record_execution().await;

        let stats = gateway.get_stats().await;
        assert_eq!(stats.turn_count, 2);
        assert_eq!(stats.session_count, 2);
        assert_eq!(stats.preapproved_count, 1);
    }

    #[tokio::test]
    async fn test_start_turn_resets_counters() {
        let gateway = SafetyGateway::new();

        gateway.record_execution().await;
        gateway.record_execution().await;

        let stats_before = gateway.get_stats().await;
        assert_eq!(stats_before.turn_count, 2);

        gateway.start_turn().await;

        let stats_after = gateway.get_stats().await;
        assert_eq!(stats_after.turn_count, 0);
        assert_eq!(stats_after.session_count, 2); // Session count preserved
    }

    #[tokio::test]
    async fn test_increase_session_limit_updates_limit() {
        let mut gateway = SafetyGateway::new();
        gateway.set_limits(10, 1);
        let ctx = make_ctx();

        let first = gateway
            .check_and_record(&ctx, "read_file", &serde_json::json!({}))
            .await;
        assert!(first.decision.is_allowed());

        let second = gateway
            .check_and_record(&ctx, "read_file", &serde_json::json!({}))
            .await;
        assert!(second.decision.is_denied());

        gateway.increase_session_limit(1);

        let third = gateway
            .check_and_record(&ctx, "read_file", &serde_json::json!({}))
            .await;
        assert!(third.decision.is_allowed());
    }
}
