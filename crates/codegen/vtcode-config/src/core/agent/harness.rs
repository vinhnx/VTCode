//! Harness execution, verification, and tool-result-clearing configuration.

use serde::{Deserialize, Serialize};

use crate::constants::defaults;
use crate::constants::tool_limits;

use super::approval::{AsyncApprovalConfig, ConfidenceEscalationConfig, SkepticPanelConfig};
use super::{ContextResetMode, ContinuationPolicy, HarnessOrchestrationMode};

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AgentHarnessConfig {
    /// Source exposure policy; advisory by default, independently of verification.
    #[serde(default)]
    pub inspection: InspectionConfig,
    /// Maximum number of tool calls allowed per turn. Defaults to `120`.
    /// Set to `0` to disable the cap.
    #[serde(default = "default_harness_max_tool_calls_per_turn")]
    pub max_tool_calls_per_turn: usize,
    /// Maximum wall clock time (seconds) for tool execution in a turn
    #[serde(default = "default_harness_max_tool_wall_clock_secs")]
    pub max_tool_wall_clock_secs: u64,
    /// Maximum retries for retryable tool errors
    #[serde(default = "default_harness_max_tool_retries")]
    pub max_tool_retries: u32,
    /// Maximum number of tool calls that may execute concurrently within a single parallel batch.
    /// Set to `0` to disable the cap (unlimited concurrency).  Default: 4.
    #[serde(default = "default_harness_max_parallel_tool_calls")]
    pub max_parallel_tool_calls: usize,
    /// Enable automatic context compaction when token pressure crosses threshold.
    ///
    /// Enabled by default. When disabled, normal threshold-triggered automatic
    /// compaction is skipped; the bounded post-tool recovery path may still
    /// compact the older prefix as a safety fallback after a provider failure.
    #[serde(default = "default_harness_auto_compaction_enabled")]
    pub auto_compaction_enabled: bool,
    /// Optional absolute compaction threshold (tokens) for native and local compaction.
    ///
    /// When set, this may lower the trigger but cannot bypass the resolved
    /// model capacity or `context.max_context_tokens` safety ceiling.
    /// The next response budget is reserved before deriving the trigger.
    #[serde(default)]
    pub auto_compaction_threshold_tokens: Option<u64>,
    /// Optional custom instructions for the compaction summarization prompt.
    /// When set, replaces the default Anthropic compaction prompt entirely.
    /// Useful for tool-use scenarios to prevent the model from calling tools
    /// during summarization. Only applies to Anthropic provider.
    #[serde(default)]
    pub auto_compaction_instructions: Option<String>,
    /// Whether to pause after compaction (Anthropic only).
    /// When true and compaction triggers, the API returns early with
    /// `stop_reason: "compaction"` and only the compaction block.
    /// The caller can then insert additional messages before the model
    /// generates its text response.
    #[serde(default)]
    pub auto_compaction_pause_after: bool,
    /// Automatically compact conversation context when the main session model
    /// or provider is switched mid-conversation, so the newly selected model
    /// starts from a summary instead of the outgoing model's raw trace.
    /// Default: true. Disable to keep the full history across a model switch.
    #[serde(default = "default_harness_compact_on_model_switch")]
    pub compact_on_model_switch: bool,
    /// Provider-native tool-result clearing policy. When enabled, old tool
    /// results are stripped from the context once it grows past
    /// `trigger_tokens`, protecting recent results and assistant tool-call batches
    /// and always retaining at least `clear_at_least_tokens`. This bounds
    /// per-turn context growth and is enabled by default.
    #[serde(default)]
    pub tool_result_clearing: ToolResultClearingConfig,
    /// Optional maximum estimated API cost in USD before VT Code stops the session.
    #[serde(default)]
    pub max_budget_usd: Option<f64>,
    /// Fraction of `max_budget_usd` at which VT Code emits a one-time
    /// near-budget warning. Ignored when `max_budget_usd` is unset.
    #[serde(default = "default_harness_budget_warning_threshold")]
    pub budget_warning_threshold: f64,
    /// Controls whether harness-managed continuation loops are enabled.
    #[serde(default)]
    pub continuation_policy: ContinuationPolicy,
    /// When to trigger a context reset — starting a clean session from
    /// external artifacts only, discarding conversation history. Distinct
    /// from compaction (which preserves conversational continuity).
    /// Default: `off` (carry forward history as before).
    #[serde(default)]
    pub context_reset_mode: ContextResetMode,
    /// Number of consecutive stall turns before `on_stall` context reset
    /// triggers. Ignored unless `context_reset_mode = "on_stall"`.
    /// Default: 2.
    #[serde(default = "default_harness_context_reset_stall_threshold")]
    pub context_reset_stall_threshold: u32,
    /// Optional compatibility/export JSONL path for harness events.
    /// Canonical events are always stored under the workspace session store;
    /// unset configuration creates no global harness file.
    #[serde(default)]
    pub event_log_path: Option<String>,
    /// Select the exec/full-auto harness orchestration path.
    #[serde(default)]
    pub orchestration_mode: HarnessOrchestrationMode,
    /// Maximum generator revision rounds after evaluator rejection.
    #[serde(default = "default_harness_max_revision_rounds")]
    pub max_revision_rounds: usize,
    /// Confidence-based escalation for autonomous decisions.
    ///
    /// Implements the escalation decision rule:
    ///   Escalate iff p_success < tau_conf OR action in A_irreversible OR cost > B_auto
    ///
    /// When a tool call is classified as irreversible or below the confidence
    /// threshold, the harness escalates to blocked-handoff instead of proceeding
    /// autonomously.  Opt-in (default: disabled).
    #[serde(default)]
    pub confidence_escalation: ConfidenceEscalationConfig,
    /// Async (out-of-band) approval for deferred tool execution requests.
    ///
    /// When enabled, approval requests exceeding the auto-approve cost threshold
    /// write a blocker file and notify the user out-of-band rather than blocking
    /// on terminal input.  Opt-in (default: disabled).
    #[serde(default)]
    async_approval: AsyncApprovalConfig,
    /// Adversarial multi-model evaluator panel.  When enabled, the harness
    /// runs the evaluator prompt against every listed model in parallel and
    /// aggregates the strictest verdict/scorecard across the panel.
    /// Opt-in (default: disabled).
    #[serde(default)]
    pub skeptic_panel: SkepticPanelConfig,
    /// Autonomous recovery for the anti-blind-editing verification gate.
    /// When the model emits text instead of running a verifier, the harness
    /// grants bounded directive retries and (optionally) runs the detected
    /// project verifier itself instead of forcing manual `continue`.
    #[serde(default)]
    pub verification: VerificationAutoRecoveryConfig,
    /// Tracker-aware auto-continuation: when `task_tracker` still has
    /// incomplete steps, keep looping / auto-queue the next turn instead of
    /// ending and nudging the user to resume.
    #[serde(default)]
    pub continuation: TrackerContinuationConfig,
}

impl Default for AgentHarnessConfig {
    fn default() -> Self {
        Self {
            inspection: InspectionConfig::default(),
            max_tool_calls_per_turn: default_harness_max_tool_calls_per_turn(),
            max_tool_wall_clock_secs: default_harness_max_tool_wall_clock_secs(),
            max_tool_retries: default_harness_max_tool_retries(),
            max_parallel_tool_calls: default_harness_max_parallel_tool_calls(),
            auto_compaction_enabled: default_harness_auto_compaction_enabled(),
            auto_compaction_threshold_tokens: None,
            auto_compaction_instructions: None,
            auto_compaction_pause_after: false,
            compact_on_model_switch: default_harness_compact_on_model_switch(),
            tool_result_clearing: ToolResultClearingConfig::default(),
            max_budget_usd: None,
            budget_warning_threshold: default_harness_budget_warning_threshold(),
            continuation_policy: ContinuationPolicy::default(),
            context_reset_mode: ContextResetMode::default(),
            context_reset_stall_threshold: default_harness_context_reset_stall_threshold(),
            event_log_path: None,
            orchestration_mode: HarnessOrchestrationMode::default(),
            max_revision_rounds: default_harness_max_revision_rounds(),
            confidence_escalation: ConfidenceEscalationConfig::default(),
            async_approval: AsyncApprovalConfig::default(),
            skeptic_panel: SkepticPanelConfig::default(),
            verification: VerificationAutoRecoveryConfig::default(),
            continuation: TrackerContinuationConfig::default(),
        }
    }
}
/// Source inspection policy, independent of post-edit verification.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct InspectionConfig {
    #[serde(default)]
    pub mode: InspectionMode,
}

/// Whether missing source exposure blocks mutations.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InspectionMode {
    Enforce,
    #[default]
    Warn,
    Off,
}
/// Tracker-aware auto-continuation policy.
///
/// When `task_tracker` still has incomplete items, the binary runloop
/// continues in-turn for status-only responses and, after recoverable
/// budget/recovery turn ends (or on session resume), auto-queues a bounded
/// follow-up turn instead of requiring the user to type `continue`.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TrackerContinuationConfig {
    /// Auto-continue while the task tracker has incomplete steps.
    /// Default: true.
    #[serde(default = "default_tracker_auto_continue")]
    pub auto_continue_tracker: bool,
    /// Bounded cross-turn auto-continue turns after a recoverable end
    /// (budget/preview/tool-free recovery) or on resume while tracker work
    /// remains. The episode budget **progress-resets** when any tracker step
    /// completes. `0` disables cross-turn tracker auto-queue (in-turn
    /// continuation still applies when `auto_continue_tracker` is true).
    /// Default: 32.
    #[serde(default = "default_tracker_cross_turn_turns")]
    pub cross_turn_turns: u8,
}

impl Default for TrackerContinuationConfig {
    fn default() -> Self {
        Self {
            auto_continue_tracker: default_tracker_auto_continue(),
            cross_turn_turns: default_tracker_cross_turn_turns(),
        }
    }
}

#[inline]
const fn default_tracker_auto_continue() -> bool {
    true
}

#[inline]
const fn default_tracker_cross_turn_turns() -> u8 {
    32
}
/// Autonomous recovery policy for the anti-blind-editing verification gate.
///
/// After 6 consecutive successful mutations without verification, text-only
/// responses no longer block the turn immediately: the harness grants bounded
/// directive retries naming the exact project verifier, then (when
/// `auto_execute` is set) runs that verifier itself through the normal tool
/// pipeline instead of forcing the user to type `continue`. A verifier that
/// keeps failing escalates to a manual blocked handoff carrying the failure
/// log after `max_consecutive_failures` consecutive failures.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct VerificationAutoRecoveryConfig {
    /// Run the detected project verifier through the normal tool pipeline
    /// when the model exhausts its directive retries without verifying.
    /// Default: true. Set to false to restore directive-only recovery.
    #[serde(default = "default_verification_auto_execute")]
    pub auto_execute: bool,
    /// Bounded in-turn directive retries granted when the model emits text
    /// instead of a verifier while the gate is pending. Each grant resets the
    /// text-response streak once and injects a project-aware directive.
    /// Default: 2.
    #[serde(default = "default_verification_in_turn_attempts")]
    pub in_turn_attempts: u8,
    /// Autonomous cross-turn recovery turns scheduled after a
    /// verification-blocked turn before a manual blocked handoff is written.
    /// Default: 2.
    #[serde(default = "default_verification_cross_turn_turns")]
    pub cross_turn_turns: u8,
    /// Explicit verifier command overriding project-marker detection
    /// (e.g. `"cargo nextest run -p mycrate"`). Must be a standalone verifier
    /// or pure `&&` chain; pipes and `;`/`||` joins are rejected at use.
    #[serde(default)]
    pub default_verifier_override: Option<String>,
    /// Consecutive failed harness auto-verifications before escalation to a
    /// manual blocked handoff carrying the failure log. Reset by any success,
    /// completed turn, or fresh user input. Default: 3.
    #[serde(default = "default_verification_max_consecutive_failures")]
    pub max_consecutive_failures: u8,
}

impl Default for VerificationAutoRecoveryConfig {
    fn default() -> Self {
        Self {
            auto_execute: default_verification_auto_execute(),
            in_turn_attempts: default_verification_in_turn_attempts(),
            cross_turn_turns: default_verification_cross_turn_turns(),
            default_verifier_override: None,
            max_consecutive_failures: default_verification_max_consecutive_failures(),
        }
    }
}
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ToolResultClearingConfig {
    /// Enables native or request-only local clearing; set `false` to opt out.
    /// Durable history and events retain full outputs.
    #[serde(default = "default_tool_result_clearing_enabled")]
    pub enabled: bool,
    /// Local clearing caps this at 60% of the provider/session history budget
    /// after output reserves and system/tool-schema overhead (minimum positive
    /// cap: 1). Lower configured triggers win; unknown capacity keeps this
    /// value. Configured values must be positive. Native edit thresholds keep
    /// the configured value.
    #[serde(default = "default_tool_result_clearing_trigger_tokens")]
    pub trigger_tokens: u64,
    /// Protect newest results. Local clearing also retains all results from
    /// their originating assistant tool-call batches, matching call IDs across
    /// reordered completions and intervening directives.
    #[serde(default = "default_tool_result_clearing_keep_tool_uses")]
    pub keep_tool_uses: u32,
    /// Local clearing additionally protects the newest assistant messages with
    /// tool calls, selected by originating message order (default 2). Protection
    /// is the union with keep_tool_uses; 0 restores count-based batch retention.
    /// Native Anthropic context edits continue to use keep_tool_uses only.
    #[serde(default = "default_tool_result_clearing_keep_recent_tool_batches")]
    pub keep_recent_tool_batches: u32,
    #[serde(default = "default_tool_result_clearing_clear_at_least_tokens")]
    pub clear_at_least_tokens: u64,
    /// Replace paired `tool_calls[].function.arguments` for stubbed results.
    /// Leaving inputs in place keeps full `apply_patch`/`write_file` bodies on
    /// every request after the result was already reclaimed. Defaults to
    /// `true`; set `false` to opt out. Bare `#[serde(default)]` on a `bool`
    /// is `false`, so this field names its default fn explicitly.
    #[serde(default = "default_tool_result_clearing_clear_tool_inputs")]
    pub clear_tool_inputs: bool,
}

impl Default for ToolResultClearingConfig {
    fn default() -> Self {
        Self {
            enabled: default_tool_result_clearing_enabled(),
            trigger_tokens: default_tool_result_clearing_trigger_tokens(),
            keep_tool_uses: default_tool_result_clearing_keep_tool_uses(),
            keep_recent_tool_batches: default_tool_result_clearing_keep_recent_tool_batches(),
            clear_at_least_tokens: default_tool_result_clearing_clear_at_least_tokens(),
            clear_tool_inputs: default_tool_result_clearing_clear_tool_inputs(),
        }
    }
}

impl ToolResultClearingConfig {
    pub(crate) fn validate(&self) -> Result<(), String> {
        if self.trigger_tokens == 0 {
            return Err("tool_result_clearing.trigger_tokens must be greater than 0".to_string());
        }
        if self.keep_tool_uses == 0 {
            return Err("tool_result_clearing.keep_tool_uses must be greater than 0".to_string());
        }
        if self.clear_at_least_tokens == 0 {
            return Err("tool_result_clearing.clear_at_least_tokens must be greater than 0".to_string());
        }
        Ok(())
    }
}
#[inline]
const fn default_harness_max_tool_calls_per_turn() -> usize {
    tool_limits::DEFAULT_MAX_TOOL_CALLS_PER_TURN
}

#[inline]
const fn default_harness_max_tool_wall_clock_secs() -> u64 {
    defaults::DEFAULT_MAX_TOOL_WALL_CLOCK_SECS
}

#[inline]
const fn default_harness_max_tool_retries() -> u32 {
    defaults::DEFAULT_MAX_TOOL_RETRIES
}

#[inline]
const fn default_harness_max_parallel_tool_calls() -> usize {
    4 // Cap parallel fan-out at 4; set to 0 in vtcode.toml to remove the limit.
}

#[inline]
const fn default_harness_auto_compaction_enabled() -> bool {
    true
}

const fn default_harness_compact_on_model_switch() -> bool {
    true
}

#[inline]
const fn default_harness_context_reset_stall_threshold() -> u32 {
    2
}

#[inline]
const fn default_verification_auto_execute() -> bool {
    true
}

#[inline]
const fn default_verification_in_turn_attempts() -> u8 {
    2
}

#[inline]
const fn default_verification_cross_turn_turns() -> u8 {
    2
}

#[inline]
const fn default_verification_max_consecutive_failures() -> u8 {
    3
}

#[inline]
const fn default_tool_result_clearing_enabled() -> bool {
    true
}

#[inline]
const fn default_tool_result_clearing_trigger_tokens() -> u64 {
    // 40k: research/audit turns were observed at ~1M input tokens/turn with
    // the old 100k trigger — tool results piled up long before any clearing.
    40_000
}

#[inline]
const fn default_tool_result_clearing_keep_recent_tool_batches() -> u32 {
    2
}

const fn default_tool_result_clearing_keep_tool_uses() -> u32 {
    2
}

#[inline]
const fn default_tool_result_clearing_clear_at_least_tokens() -> u64 {
    30_000
}

#[inline]
const fn default_tool_result_clearing_clear_tool_inputs() -> bool {
    true
}

#[inline]
const fn default_harness_max_revision_rounds() -> usize {
    2
}

#[inline]
const fn default_harness_budget_warning_threshold() -> f64 {
    0.75
}

#[cfg(test)]
mod inspection_tests {
    use super::*;

    #[test]
    fn source_inspection_defaults_warns_and_parses_explicit_modes() {
        assert_eq!(AgentHarnessConfig::default().inspection.mode, InspectionMode::Warn);
        for text in ["", "[inspection]"] {
            let config: AgentHarnessConfig = toml::from_str(text).unwrap();
            assert_eq!(config.inspection.mode, InspectionMode::Warn);
        }
        for (text, expected) in [
            ("enforce", InspectionMode::Enforce),
            ("warn", InspectionMode::Warn),
            ("off", InspectionMode::Off),
        ] {
            let config: AgentHarnessConfig = toml::from_str(&format!("[inspection]\nmode = '{text}'")).unwrap();
            assert_eq!(config.inspection.mode, expected);
        }
        assert!(toml::from_str::<AgentHarnessConfig>("[inspection]\nmode = 'permissive'").is_err());
    }
}
