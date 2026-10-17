//! Unified Safety Gateway
//!
//! Consolidates all safety checking mechanisms into a single gateway:
//! - Rate limiting (from runloop's tool_call_safety)
//! - Destructive tool detection
//! - Command policy enforcement
//! - Planning workflow restrictions
//!
//! This provides consistent safety decisions across all tool execution paths.

use hashbrown::HashSet;
use parking_lot::{Mutex, RwLock};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use crate::config::CommandsConfig;
use crate::config::constants::tools;
use crate::dotfile_protection::{AccessContext, AccessType, DotfileGuardian, ProtectionDecision, get_global_guardian};
use crate::tools::apply_patch::{Patch, PatchOperation, decode_apply_patch_input};
use crate::tools::command_policy::CommandPolicyEvaluator;
use crate::tools::invocation::ToolInvocationId;
use crate::tools::rate_limit_config::{tool_calls_per_minute_from_env, tool_calls_per_second_from_env};
use crate::tools::registry::{RiskLevel, ToolRiskContext, ToolRiskScorer, ToolSource, WorkspaceTrust};
use crate::tools::tool_intent::{
    ToolIntent, action_qualified_policy_name, classify_tool_intent, command_session_action_in,
    command_session_action_is, file_operation_action_is,
};
use vtcode_config::constants::tool_limits::{
    DEFAULT_MAX_TOOL_CALLS_PER_TURN, DEFAULT_SAFETY_MAX_TOOL_CALLS_PER_SESSION, MAX_CONTROL_PLANE_TOOL_CALLS_PER_TURN,
};
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
    #[error("Planning workflow violation: {0}")]
    PlanningPolicyViolation(String),
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
    /// Whether planning workflow is active (read-only)
    pub planning_active: bool,
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
        let rate_limit_per_second = tool_calls_per_second_from_env().unwrap_or(5);

        Self {
            max_per_turn: DEFAULT_MAX_TOOL_CALLS_PER_TURN,
            max_per_session: DEFAULT_SAFETY_MAX_TOOL_CALLS_PER_SESSION,
            rate_limit_per_second,
            rate_limit_per_minute: tool_calls_per_minute_from_env(),
            planning_active: false,
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
    /// Count of control-plane calls (`wait`/`inspect`) recorded this turn.
    /// These are exempt from `current_turn_count` so a long build does not
    /// consume the budget its follow-up work needs, but they remain bounded
    /// by their own per-turn cap and the session fuse.
    control_plane_turn_count: usize,
}

/// Unified Safety Gateway
///
/// Consolidates rate limiting, destructive tool detection, command policy
/// enforcement, planning workflow restrictions, and dotfile protection into a single
/// safety decision point.
pub struct SafetyGateway {
    /// Configuration
    config: RwLock<SafetyGatewayConfig>,
    /// Command policy evaluator (optional, for shell commands)
    command_policy: Option<CommandPolicyEvaluator>,
    /// Rate limiter state
    rate_state: Mutex<RateLimiterState>,
    /// Preapproved tools for this session
    preapproved: Mutex<HashSet<String>>,
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

fn push_file_access_target(targets: &mut Vec<FileAccessTarget>, path: &str, access_type: AccessType) {
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
        tools::EXEC_COMMAND => crate::tools::command_args::command_text(args).ok().flatten(),
        tools::WRITE_STDIN => crate::tools::command_args::interactive_input_text(args).map(str::to_owned),
        tools::SHELL | tools::RUN_PTY_CMD => crate::tools::command_args::command_text(args).ok().flatten(),
        tools::SEND_PTY_INPUT => crate::tools::command_args::interactive_input_text(args).map(str::to_owned),
        tools::UNIFIED_EXEC if command_session_action_is(args, "run") => {
            crate::tools::command_args::command_text(args).ok().flatten()
        }
        tools::UNIFIED_EXEC if command_session_action_in(args, &["write", "continue"]) => {
            crate::tools::command_args::interactive_input_text(args).map(str::to_owned)
        }
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
                if let Some(destination) = new_path.as_deref().filter(|candidate| *candidate != path) {
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
        tools::UNIFIED_FILE => {
            // Single home of the unified-file mapping; mirrors the legacy
            // file-helper arms above. Keep them in sync.
            if file_operation_action_is(args, "write") {
                if let Some(path) = primary_path_arg(args) {
                    push_file_access_target(&mut targets, path, AccessType::Write);
                }
            } else if file_operation_action_is(args, "edit") {
                if let Some(path) = primary_path_arg(args) {
                    push_file_access_target(&mut targets, path, AccessType::Modify);
                }
            } else if file_operation_action_is(args, "delete") {
                if let Some(path) = primary_path_arg(args) {
                    push_file_access_target(&mut targets, path, AccessType::Delete);
                }
            } else if file_operation_action_is(args, "move") {
                if let Some(path) = primary_path_arg(args) {
                    push_file_access_target(&mut targets, path, AccessType::Modify);
                }
                if let Some(path) = destination_path_arg(args) {
                    push_file_access_target(&mut targets, path, AccessType::Write);
                }
            } else if file_operation_action_is(args, "copy") {
                if let Some(path) = destination_path_arg(args) {
                    push_file_access_target(&mut targets, path, AccessType::Write);
                }
            }
        }
        _ => {}
    }

    targets
}

fn proposed_changes_preview(args: &Value) -> String {
    const PREVIEW_LIMIT: usize = 500;

    let preview_text = |label: &str, text: &str| {
        let prefix = vtcode_commons::formatting::truncate_utf8_prefix(text, PREVIEW_LIMIT);
        format!("{label} ({} bytes):\n{}{}", text.len(), prefix, if prefix.len() < text.len() { "..." } else { "" })
    };

    if let Some(content) = args.get("content").and_then(|value| value.as_str()) {
        return preview_text("Content", content);
    }

    if let Some(old_str) = args.get("old_str").and_then(|value| value.as_str()) {
        let new_str = args.get("new_str").and_then(|value| value.as_str()).unwrap_or("");
        // Bound like the content/patch branches: an unbounded replacement
        // would bloat the approval prompt and audit log.
        return format!(
            "Replace:\n  '{}'\nWith:\n  '{}'",
            vtcode_commons::formatting::truncate_byte_budget(old_str, PREVIEW_LIMIT, "..."),
            vtcode_commons::formatting::truncate_byte_budget(new_str, PREVIEW_LIMIT, "...")
        );
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
            config: RwLock::new(config),
            command_policy: None,
            rate_state: Mutex::new(RateLimiterState::default()),
            preapproved: Mutex::new(HashSet::new()),
            dotfile_guardian: None,
        }
    }

    /// Set the dotfile guardian for protected file access
    pub fn with_dotfile_guardian(mut self, guardian: Arc<DotfileGuardian>) -> Self {
        self.dotfile_guardian = Some(guardian);
        self
    }

    /// Create and set a dotfile guardian from configuration
    pub async fn with_dotfile_protection(mut self, config: DotfileProtectionConfig) -> anyhow::Result<Self> {
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

    /// Enable or disable planning workflow
    pub fn set_planning_workflow(&self, active: bool) {
        self.config.write().planning_active = active;
    }

    /// Set workspace trust level
    pub fn set_workspace_trust(&self, trust: WorkspaceTrust) {
        self.config.write().workspace_trust = trust;
    }

    /// Update rate limits
    pub fn set_limits(&self, max_per_turn: usize, max_per_session: usize) {
        let mut config = self.config.write();
        config.max_per_turn = max_per_turn;
        config.max_per_session = max_per_session;
    }

    /// Update rate-limiter thresholds.
    pub fn set_rate_limits(&self, rate_limit_per_second: usize, rate_limit_per_minute: Option<usize>) {
        let mut config = self.config.write();
        if rate_limit_per_second > 0 {
            config.rate_limit_per_second = rate_limit_per_second;
        }
        config.rate_limit_per_minute = rate_limit_per_minute.filter(|v| *v > 0);
    }

    /// Enable or disable rate-limit enforcement while preserving counters.
    pub fn set_rate_limit_enforcement(&self, enabled: bool) {
        self.config.write().enforce_rate_limits = enabled;
    }

    /// Increase session limit dynamically
    pub fn increase_session_limit(&self, increment: usize) {
        let mut config = self.config.write();
        let new_max = config.max_per_session.saturating_add(increment);
        config.max_per_session = new_max;
        tracing::info!("Session tool limit increased to {}", new_max);
    }

    pub fn max_per_session(&self) -> usize {
        self.config.read().max_per_session
    }

    /// Reset turn counters (call at start of new turn)
    pub fn start_turn(&self) {
        let mut state = self.rate_state.lock();
        state.current_turn_count = 0;
        state.control_plane_turn_count = 0;
        state.calls_per_second.clear();
        state.calls_per_minute.clear();
    }

    /// Reset all transient tool-budget state for a newly reconstructed
    /// execution context and restore the limits supplied by the build mode.
    pub fn reset_for_fresh_execution(&self, max_per_turn: usize, max_per_session: usize) {
        self.set_limits(max_per_turn, max_per_session);
        let mut state = self.rate_state.lock();
        state.current_turn_count = 0;
        state.control_plane_turn_count = 0;
        state.session_count = 0;
        state.calls_per_second.clear();
        state.calls_per_minute.clear();
    }

    /// Preapprove a tool for this session
    pub fn preapprove(&self, tool_name: &str) {
        let mut preapproved = self.preapproved.lock();
        preapproved.insert(tool_name.to_string());
    }

    /// Check if a tool is preapproved
    pub fn is_preapproved(&self, tool_name: &str) -> bool {
        let preapproved = self.preapproved.lock();
        preapproved.contains(tool_name)
    }

    /// Main entry point: check safety for a tool invocation.
    ///
    /// Returns a [`SafetyDecision`] indicating whether execution can proceed.
    /// Inline-delegating wrapper that returns the inner future directly to
    /// avoid an extra coroutine state machine (audit section 16).
    pub fn check_safety<'a>(
        &'a self,
        ctx: &'a SafetyContext,
        tool_name: &'a str,
        args: &'a Value,
    ) -> impl Future<Output = SafetyDecision> + 'a {
        self.check_safety_with_id(ctx, tool_name, args, None)
    }

    /// Check safety with explicit invocation ID for correlation
    pub async fn check_safety_with_id(
        &self,
        ctx: &SafetyContext,
        tool_name: &str,
        args: &Value,
        invocation_id: Option<ToolInvocationId>,
    ) -> SafetyDecision {
        let inv_id = invocation_id.map(|id| id.short()).unwrap_or_else(|| "unknown".to_string());

        tracing::trace!(
            invocation_id = %inv_id,
            tool = %tool_name,
            "SafetyGateway: checking safety"
        );

        if let Err(err) = self.check_rate_limits(crate::tools::tool_intent::is_turn_budget_exempt_call(tool_name, args))
        {
            tracing::warn!(
                invocation_id = %inv_id,
                error = %err,
                "SafetyGateway: rate limit exceeded"
            );
            return SafetyDecision::Deny(err.to_string());
        }

        self.evaluate_non_rate_decision(ctx, tool_name, args, &inv_id).await
    }

    /// Check safety and atomically reserve a rate-limit slot on success.
    ///
    /// This avoids split check/record races by validating rate limits and recording
    /// execution under a single lock acquisition.
    pub async fn check_and_record(&self, ctx: &SafetyContext, tool_name: &str, args: &Value) -> SafetyCheckResult {
        self.check_and_record_with_id(ctx, tool_name, args, None).await
    }

    /// Check safety with correlation ID and atomically reserve a rate-limit slot.
    pub async fn check_and_record_with_id(
        &self,
        ctx: &SafetyContext,
        tool_name: &str,
        args: &Value,
        invocation_id: Option<ToolInvocationId>,
    ) -> SafetyCheckResult {
        let inv_id = invocation_id.map(|id| id.short()).unwrap_or_else(|| "unknown".to_string());
        tracing::trace!(
            invocation_id = %inv_id,
            tool = %tool_name,
            "SafetyGateway: checking and recording safety"
        );

        let decision = self.evaluate_non_rate_decision(ctx, tool_name, args, &inv_id).await;

        if decision.is_denied() {
            return SafetyCheckResult { decision, retry_after: None, violation: None };
        }

        let control_plane = crate::tools::tool_intent::is_turn_budget_exempt_call(tool_name, args);
        let now = Instant::now();
        let mut state = self.rate_state.lock();
        match self.check_rate_limits_locked(&mut state, now, control_plane) {
            Ok(()) => {
                self.record_execution_locked(&mut state, now, control_plane);
                SafetyCheckResult { decision, retry_after: None, violation: None }
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
        // Extract the command text once: the policy check, the risk context,
        // and the approval justification all read the same value, and each
        // extraction re-parses the command payload (shell_words split plus
        // action re-derivation).
        let command = command_text_for_tool(tool_name, args);

        if let Some(decision) = self.check_dotfile_protection(tool_name, args, &ctx.session_id).await {
            tracing::info!(
                invocation_id = %inv_id,
                tool = %tool_name,
                "SafetyGateway: dotfile protection triggered"
            );
            return decision;
        }

        if let Some(ref policy) = self.command_policy
            && let Some(command) = command.as_deref()
            && !policy.allows_text(command)
        {
            let reason = format!("Command '{command}' blocked by policy");
            tracing::info!(
                invocation_id = %inv_id,
                command = %command,
                "SafetyGateway: command policy denied"
            );
            return SafetyDecision::Deny(reason);
        }

        if self.is_preapproved(tool_name) {
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

        // Classify once for the risk context, the justification, and the
        // destructive gate below: all three read the same tool name and args,
        // and each re-classification re-parses command payloads.
        let intent = classify_tool_intent(tool_name, args);
        let command = command.as_deref();
        let risk_ctx = self.build_risk_context(tool_name, args, &intent, command);
        let risk_level = ToolRiskScorer::calculate_risk(&risk_ctx);

        if ToolRiskScorer::requires_justification(risk_level, self.config.read().approval_risk_threshold) {
            let justification = self.build_approval_justification(tool_name, &risk_level, args, &intent, command);
            tracing::info!(
                invocation_id = %inv_id,
                tool = %tool_name,
                risk = %risk_level,
                "SafetyGateway: requires approval"
            );
            return SafetyDecision::NeedsApproval(justification);
        }

        if intent.destructive {
            let justification = format!("Tool '{tool_name}' is destructive and may modify files or execute commands.");
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
    pub fn record_execution(&self) {
        let mut state = self.rate_state.lock();
        self.record_execution_locked(&mut state, Instant::now(), false);
    }

    /// Check rate limits without recording
    fn check_rate_limits(&self, control_plane: bool) -> Result<(), SafetyError> {
        let mut state = self.rate_state.lock();
        self.check_rate_limits_locked(&mut state, Instant::now(), control_plane)
    }

    fn check_rate_limits_locked(
        &self,
        state: &mut RateLimiterState,
        now: Instant,
        control_plane: bool,
    ) -> Result<(), SafetyError> {
        let config = self.config.read();
        self.prune_rate_windows(state, now);

        if config.enforce_rate_limits {
            if state.calls_per_second.len() >= config.rate_limit_per_second {
                return Err(SafetyError::RateLimitExceeded {
                    current: state.calls_per_second.len(),
                    max: config.rate_limit_per_second,
                    window: "1s",
                });
            }

            if let Some(limit) = config.rate_limit_per_minute
                && state.calls_per_minute.len() >= limit
            {
                return Err(SafetyError::RateLimitExceeded {
                    current: state.calls_per_minute.len(),
                    max: limit,
                    window: "60s",
                });
            }
        }

        if control_plane {
            // Control-plane calls bypass the per-turn work budget but keep
            // their own bounded cap so the exemption cannot become the
            // dominant spend path.
            if state.control_plane_turn_count >= MAX_CONTROL_PLANE_TOOL_CALLS_PER_TURN {
                return Err(SafetyError::TurnLimitReached { max: MAX_CONTROL_PLANE_TOOL_CALLS_PER_TURN });
            }
        } else if state.current_turn_count >= config.max_per_turn {
            return Err(SafetyError::TurnLimitReached { max: config.max_per_turn });
        }

        if state.session_count >= config.max_per_session {
            return Err(SafetyError::SessionLimitReached { max: config.max_per_session });
        }

        Ok(())
    }

    fn record_execution_locked(&self, state: &mut RateLimiterState, now: Instant, control_plane: bool) {
        state.calls_per_second.push_back(now);
        state.calls_per_minute.push_back(now);
        state.session_count = state.session_count.saturating_add(1);
        if control_plane {
            state.control_plane_turn_count = state.control_plane_turn_count.saturating_add(1);
        } else {
            state.current_turn_count = state.current_turn_count.saturating_add(1);
        }
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

            let context = AccessContext::new(&target.path, target.access_type, tool_name, session_id)
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
                        req.file_path, req.access_type, req.protection_reason, req.proposed_changes, req.warning
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
                        req.file_path, req.access_type, req.protection_reason, req.proposed_changes, req.warning
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
                    return Some(SafetyDecision::Deny(format!("Dotfile protection check failed: {e}")));
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
    ///
    /// `command` is the command extracted once by the caller
    /// ([`Self::evaluate_non_rate_decision`]); re-deriving it here would
    /// re-parse the command payload for every risk evaluation.
    fn build_risk_context(
        &self,
        tool_name: &str,
        args: &Value,
        intent: &ToolIntent,
        command: Option<&str>,
    ) -> ToolRiskContext {
        let source = if tool_name.starts_with("mcp_") {
            ToolSource::Mcp
        } else if tool_name.starts_with("acp_") {
            ToolSource::Acp
        } else {
            ToolSource::Internal
        };

        let risk_tool_name = action_qualified_policy_name(tool_name, Some(args)).unwrap_or(tool_name);

        let mut ctx = ToolRiskContext::new(risk_tool_name.to_string(), source, self.config.read().workspace_trust);

        // Set flags based on tool type
        if intent.mutating {
            ctx = ctx.as_write();
        }
        if intent.destructive {
            ctx = ctx.as_destructive();
        }

        if ToolRiskScorer::is_network_tool(risk_tool_name) {
            ctx = ctx.accesses_network();
        }

        // Extract command args for shell tools
        if let Some(command) = command {
            ctx = ctx.with_args(command.split_whitespace().map(String::from).collect());
        }

        ctx
    }

    /// Build justification message for approval prompt
    ///
    /// `command` is the command extracted once by the caller
    /// ([`Self::evaluate_non_rate_decision`]); re-deriving it here would
    /// re-parse the command payload on every approval prompt.
    fn build_approval_justification(
        &self,
        tool_name: &str,
        risk_level: &RiskLevel,
        args: &Value,
        intent: &ToolIntent,
        command: Option<&str>,
    ) -> String {
        let mut parts = Vec::new();

        parts.push(format!("Tool: {tool_name}"));
        parts.push(format!("Risk level: {risk_level}"));

        if intent.destructive {
            parts.push("This tool may modify or delete files.".to_string());
        }

        if let Some(command) = command {
            parts.push(format!("Command: {command}"));
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
    pub fn get_stats(&self) -> SafetyStats {
        let state = self.rate_state.lock();
        let preapproved = self.preapproved.lock();
        let config = self.config.read();

        SafetyStats {
            turn_count: state.current_turn_count,
            session_count: state.session_count,
            max_per_turn: config.max_per_turn,
            max_per_session: config.max_per_session,
            planning_active: config.planning_active,
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
    pub planning_active: bool,
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
            .check_safety(&ctx, "delete_file", &serde_json::json!({"path": "/tmp/test"}))
            .await;

        assert!(decision.needs_approval());
    }

    #[tokio::test]
    async fn test_planning_workflow_does_not_double_block_mutating() {
        let gateway = SafetyGateway::new();
        gateway.set_planning_workflow(true);
        let ctx = make_ctx();

        let decision = gateway
            .check_safety(&ctx, "apply_patch", &serde_json::json!({"patch": "..."}))
            .await;

        assert!(decision.needs_approval());
    }

    /// The command text is extracted once per check and threaded to the
    /// policy, risk, and justification consumers. This pins the observable
    /// behavior (identical policy verdict, risk sentence, and justification
    /// command line as re-derivation would produce) after the extraction
    /// stopped being repeated at each site.
    #[tokio::test]
    async fn command_extraction_feeds_policy_risk_and_justification_identically() {
        let policy = CommandPolicyEvaluator::from_config(&CommandsConfig {
            allow_list: vec!["cargo".into()],
            ..Default::default()
        });
        let gateway = SafetyGateway::new().with_command_policy(policy);

        // Allowed by policy: command is still classified for risk (mutating
        // verifier) and surfaces verbatim in any justification.
        let allowed = gateway
            .check_safety(&make_ctx(), tools::EXEC_COMMAND, &serde_json::json!({"command": "cargo test"}))
            .await;
        assert!(allowed.is_allowed());

        // Denied by policy: the denial reports the same single command.
        let denied = gateway
            .check_safety(&make_ctx(), tools::EXEC_COMMAND, &serde_json::json!({"command": "rm -rf /"}))
            .await;
        let SafetyDecision::Deny(reason) = denied else {
            panic!("policy denial expected");
        };
        assert!(reason.contains("rm -rf /"), "denial must carry the extracted command: {reason}");
        assert!(reason.starts_with("Command 'rm -rf /' blocked by policy"), "unexpected denial shape: {reason}");

        // Justification path: the risk classifier and the justification see
        // the same extracted command (marked unsafe by the gateway config).
        gateway.config.write().approval_risk_threshold.clone_from(&RiskLevel::Low);
        let justified = gateway
            .check_safety(&make_ctx(), tools::EXEC_COMMAND, &serde_json::json!({"command": "cargo publish"}))
            .await;
        if let SafetyDecision::NeedsApproval(justification) = justified {
            assert!(
                justification.contains("Command: cargo publish"),
                "justification must carry the extracted command: {justification}"
            );
        }
        gateway.config.write().approval_risk_threshold = RiskLevel::High;
    }

    #[test]
    fn build_risk_context_uses_caller_supplied_command() {
        let gateway = SafetyGateway::new();
        let args = serde_json::json!({"command": "rg --files"});
        let intent = classify_tool_intent(tools::EXEC_COMMAND, &args);

        let with_command = gateway.build_risk_context(tools::EXEC_COMMAND, &args, &intent, Some("rg --files"));
        assert_eq!(with_command.command_args, vec!["rg".to_string(), "--files".to_string()]);

        let without_command = gateway.build_risk_context(tools::EXEC_COMMAND, &args, &intent, None);
        assert!(without_command.command_args.is_empty());
    }

    #[tokio::test]
    async fn test_preapproved_tools_allowed() {
        let gateway = SafetyGateway::new();
        gateway.preapprove("delete_file");
        let ctx = make_ctx();

        let decision = gateway
            .check_safety(&ctx, "delete_file", &serde_json::json!({"path": "/tmp/test"}))
            .await;

        assert!(decision.is_allowed());
    }

    #[tokio::test]
    async fn test_trust_level_bypass() {
        let gateway = SafetyGateway::new();
        let mut ctx = make_ctx();
        ctx.trust_level = SafetyTrustLevel::Full;

        let decision = gateway
            .check_safety(&ctx, "delete_file", &serde_json::json!({"path": "/tmp/test"}))
            .await;

        assert!(decision.is_allowed());
    }

    #[tokio::test]
    async fn test_rate_limiting() {
        let config = SafetyGatewayConfig { rate_limit_per_second: 2, ..Default::default() };
        let gateway = SafetyGateway::with_config(config);
        let ctx = make_ctx();

        // First two calls should succeed
        gateway.record_execution();
        gateway.record_execution();

        // Third call should be denied
        let decision = gateway.check_safety(&ctx, "read_file", &serde_json::json!({})).await;

        assert!(decision.is_denied());
        assert!(decision.reason().unwrap().contains("Rate limit"));
    }

    #[test]
    fn safety_gateway_defaults_use_shared_tool_call_budget() {
        let config = SafetyGatewayConfig::default();

        assert_eq!(config.max_per_turn, DEFAULT_MAX_TOOL_CALLS_PER_TURN);
        assert_eq!(config.max_per_session, DEFAULT_SAFETY_MAX_TOOL_CALLS_PER_SESSION);
    }

    #[tokio::test]
    async fn test_atomic_check_and_record_rate_limited() {
        let config = SafetyGatewayConfig { rate_limit_per_second: 2, ..Default::default() };
        let gateway = SafetyGateway::with_config(config);
        let ctx = make_ctx();

        let first = gateway.check_and_record(&ctx, "read_file", &serde_json::json!({})).await;
        assert!(first.decision.is_allowed());

        let second = gateway.check_and_record(&ctx, "read_file", &serde_json::json!({})).await;
        assert!(second.decision.is_allowed());

        let third = gateway.check_and_record(&ctx, "read_file", &serde_json::json!({})).await;
        assert!(third.decision.is_denied());
        assert!(third.retry_after.is_some());
        assert!(matches!(third.violation, Some(SafetyError::RateLimitExceeded { .. })));
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
    async fn test_command_session_command_policy_enforcement_with_indexed_args() {
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
    async fn test_command_session_continue_command_policy_enforcement_with_input() {
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
    async fn test_apply_patch_dotfile_input_dotfile_protection_requires_approval() {
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
        assert!(decision.reason().is_some_and(|reason| reason.contains(".gitignore")));
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

    #[test]
    fn unified_file_targets_mirror_legacy_file_helpers() {
        let write = file_access_targets(tools::UNIFIED_FILE, &serde_json::json!({"action": "write", "path": "a.txt"}));
        assert_eq!(write.len(), 1);
        assert_eq!(write[0].path, PathBuf::from("a.txt"));
        assert_eq!(write[0].access_type, AccessType::Write);

        let moved = file_access_targets(
            tools::UNIFIED_FILE,
            &serde_json::json!({"action": "move", "path": "a.txt", "destination": "b.txt"}),
        );
        assert_eq!(moved.len(), 2);
        assert_eq!(moved[0].access_type, AccessType::Modify);
        assert_eq!(moved[1].access_type, AccessType::Write);

        let unknown = file_access_targets(tools::UNIFIED_FILE, &serde_json::json!({"action": "read", "path": "a.txt"}));
        assert!(unknown.is_empty());
    }

    #[test]
    fn proposed_changes_preview_bounds_old_and_new_str() {
        // Short replacements render exactly as before (no truncation
        // marker); long ones stay bounded with a marker; a multi-byte
        // char on the 500-byte boundary must not panic.
        let short = proposed_changes_preview(&serde_json::json!({"old_str": "a", "new_str": "b"}));
        assert_eq!(short, "Replace:\n  'a'\nWith:\n  'b'");

        let long_old = "a".repeat(600);
        let bounded = proposed_changes_preview(&serde_json::json!({"old_str": long_old, "new_str": ""}));
        assert!(bounded.contains("..."));
        assert!(bounded.len() < 600, "preview must stay bounded: {}", bounded.len());

        let boundary_old = format!("{}{}", "a".repeat(499), "日".repeat(10));
        let boundary = proposed_changes_preview(&serde_json::json!({"old_str": boundary_old, "new_str": "b"}));
        assert!(boundary.contains("..."));
        assert!(boundary.ends_with("With:\n  'b'"));
    }

    #[test]
    fn proposed_changes_preview_never_splits_utf8_boundary() {
        // 500-byte cut lands mid-`日` (3 bytes): the old `&text[..500]`
        // panicked; the canonical prefix rounds down to the boundary.
        let content = format!("{}{}", "a".repeat(499), "日".repeat(10));
        let preview = proposed_changes_preview(&serde_json::json!({"content": content}));
        assert!(preview.contains("Content ("));
        assert!(preview.ends_with("..."));
        assert!(preview.len() < content.len() + 64);
    }

    #[tokio::test]
    async fn test_apply_patch_patch_field_dotfile_protection_requires_approval() {
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
                    "patch": "*** Begin Patch\n*** Update File: .gitignore\n@@\n-old\n+new\n*** End Patch\n"
                }),
            )
            .await;

        assert!(decision.needs_approval());
        assert!(decision.reason().is_some_and(|reason| reason.contains(".gitignore")));
    }

    #[tokio::test]
    async fn test_stats_tracking() {
        let gateway = SafetyGateway::new();
        gateway.preapprove("test_tool");
        gateway.record_execution();
        gateway.record_execution();

        let stats = gateway.get_stats();
        assert_eq!(stats.turn_count, 2);
        assert_eq!(stats.session_count, 2);
        assert_eq!(stats.preapproved_count, 1);
    }

    #[tokio::test]
    async fn test_start_turn_resets_counters() {
        let gateway = SafetyGateway::new();

        gateway.record_execution();
        gateway.record_execution();

        let stats_before = gateway.get_stats();
        assert_eq!(stats_before.turn_count, 2);

        gateway.start_turn();

        let stats_after = gateway.get_stats();
        assert_eq!(stats_after.turn_count, 0);
        assert_eq!(stats_after.session_count, 2); // Session count preserved
    }

    #[tokio::test]
    async fn test_increase_session_limit_updates_limit() {
        let gateway = SafetyGateway::new();
        gateway.set_limits(10, 1);
        let ctx = make_ctx();

        let first = gateway.check_and_record(&ctx, "read_file", &serde_json::json!({})).await;
        assert!(first.decision.is_allowed());

        let second = gateway.check_and_record(&ctx, "read_file", &serde_json::json!({})).await;
        assert!(second.decision.is_denied());

        gateway.increase_session_limit(1);

        let third = gateway.check_and_record(&ctx, "read_file", &serde_json::json!({})).await;
        assert!(third.decision.is_allowed());
    }

    #[tokio::test]
    async fn control_plane_wait_does_not_consume_per_turn_budget() {
        let config = SafetyGatewayConfig { max_per_turn: 1, ..Default::default() };
        let gateway = SafetyGateway::with_config(config);
        let ctx = make_ctx();

        // The one ordinary slot is consumed by a real work call.
        let work = gateway
            .check_and_record(&ctx, "read_file", &serde_json::json!({"path": "x.txt"}))
            .await;
        assert!(work.decision.is_allowed());

        // A blocking wait is control-plane: it must not be denied by the
        // per-turn counter even though the budget is exhausted.
        let wait = gateway
            .check_and_record(&ctx, tools::UNIFIED_EXEC, &serde_json::json!({"action": "wait", "session_id": "run-1"}))
            .await;
        assert!(wait.decision.is_allowed(), "wait must bypass the per-turn work budget");

        // Ordinary calls remain denied while the turn budget stays exhausted.
        let denied = gateway
            .check_and_record(&ctx, "read_file", &serde_json::json!({"path": "y.txt"}))
            .await;
        assert!(denied.decision.is_denied());
    }

    #[tokio::test]
    async fn control_plane_calls_are_bounded_by_their_own_cap() {
        let config = SafetyGatewayConfig {
            max_per_turn: 1_000,
            enforce_rate_limits: false,
            ..Default::default()
        };
        let gateway = SafetyGateway::with_config(config);
        let ctx = make_ctx();

        for _ in 0..MAX_CONTROL_PLANE_TOOL_CALLS_PER_TURN {
            let call = gateway
                .check_and_record(
                    &ctx,
                    tools::UNIFIED_EXEC,
                    &serde_json::json!({"action": "wait", "session_id": "run-1"}),
                )
                .await;
            assert!(call.decision.is_allowed());
        }

        // The exempt bucket has its own ceiling; the next wait is denied.
        let overflow = gateway
            .check_and_record(&ctx, tools::UNIFIED_EXEC, &serde_json::json!({"action": "wait", "session_id": "run-1"}))
            .await;
        assert!(overflow.decision.is_denied());
        assert!(overflow.decision.reason().is_some_and(|reason| reason.contains("limit")));
    }

    #[tokio::test]
    async fn control_plane_calls_still_consume_session_and_rate_budget() {
        // A tiny session fuse must still trip on control-plane calls.
        let config = SafetyGatewayConfig {
            max_per_turn: 1_000,
            max_per_session: 2,
            enforce_rate_limits: false,
            ..Default::default()
        };
        let gateway = SafetyGateway::with_config(config);
        let ctx = make_ctx();

        for _ in 0..2 {
            let call = gateway
                .check_and_record(
                    &ctx,
                    tools::UNIFIED_EXEC,
                    &serde_json::json!({"action": "wait", "session_id": "run-1"}),
                )
                .await;
            assert!(call.decision.is_allowed());
        }
        let third = gateway
            .check_and_record(&ctx, tools::UNIFIED_EXEC, &serde_json::json!({"action": "wait", "session_id": "run-1"}))
            .await;
        assert!(third.decision.is_denied(), "session fuse must bound control-plane calls");
    }

    #[tokio::test]
    async fn start_turn_resets_control_plane_counter() {
        let config = SafetyGatewayConfig {
            max_per_turn: 1_000,
            enforce_rate_limits: false,
            ..Default::default()
        };
        let gateway = SafetyGateway::with_config(config);
        let ctx = make_ctx();
        let wait_args = serde_json::json!({"action": "wait", "session_id": "run-1"});

        for _ in 0..MAX_CONTROL_PLANE_TOOL_CALLS_PER_TURN {
            assert!(
                gateway
                    .check_and_record(&ctx, tools::UNIFIED_EXEC, &wait_args)
                    .await
                    .decision
                    .is_allowed()
            );
        }
        assert!(
            gateway
                .check_and_record(&ctx, tools::UNIFIED_EXEC, &wait_args)
                .await
                .decision
                .is_denied()
        );

        gateway.start_turn();

        assert!(
            gateway
                .check_and_record(&ctx, tools::UNIFIED_EXEC, &wait_args)
                .await
                .decision
                .is_allowed(),
            "a fresh turn must refill the control-plane bucket"
        );
    }
}
