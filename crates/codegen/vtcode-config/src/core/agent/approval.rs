//! Approval, confidence-escalation, and skeptic-panel configuration.

use serde::{Deserialize, Serialize};

/// Configuration for async (out-of-band) approval of tool execution requests.
///
/// When enabled, approval requests that exceed the auto-approve threshold are
/// written to a blocker file instead of blocking on terminal input.  A
/// notification command (e.g. email, Slack webhook) can be configured to
/// alert the user.  The user can then approve or reject via the CLI.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AsyncApprovalConfig {
    /// Master switch.  Default: false (opt-in).
    #[serde(default = "default_async_approval_enabled")]
    enabled: bool,

    /// Maximum time in seconds before a deferred approval is auto-denied.
    #[serde(default = "default_async_approval_timeout_secs")]
    timeout_secs: u64,

    /// Optional command invoked with the blocker file path as argument when
    /// an approval request is deferred (e.g. `/usr/local/bin/notify-approval`).
    #[serde(default)]
    notify_command: Option<String>,

    /// Auto-approve tool calls whose estimated cost is below this threshold (USD).
    /// Set to 0.0 to require approval for everything.
    #[serde(default = "default_async_approval_auto_approve_below")]
    auto_approve_below_usd: f64,

    /// When set, auto-approve after this many seconds if no explicit answer.
    /// The blocker file is updated with the auto-approval decision.
    #[serde(default)]
    auto_approve_timeout_secs: Option<u64>,
}

impl Default for AsyncApprovalConfig {
    fn default() -> Self {
        Self {
            enabled: default_async_approval_enabled(),
            timeout_secs: default_async_approval_timeout_secs(),
            notify_command: None,
            auto_approve_below_usd: default_async_approval_auto_approve_below(),
            auto_approve_timeout_secs: None,
        }
    }
}

/// Configuration for the adversarial multi-model evaluator panel.
///
/// When enabled, the evaluator phase runs against every listed model in
/// parallel and aggregates the strictest verdict/scorecard across the panel.
/// This is opt-in because each additional skeptic adds latency and cost.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SkepticPanelConfig {
    /// Master switch. Default: false (opt-in).
    #[serde(default = "default_skeptic_panel_enabled")]
    pub enabled: bool,

    /// Model identifiers to run as skeptic evaluators (in addition to the
    /// primary evaluator). Empty when `enabled = false`.
    #[serde(default)]
    pub models: Vec<String>,
}

impl Default for SkepticPanelConfig {
    fn default() -> Self {
        Self {
            enabled: default_skeptic_panel_enabled(),
            models: Vec::new(),
        }
    }
}

const fn default_skeptic_panel_enabled() -> bool {
    false
}

const fn default_async_approval_enabled() -> bool {
    false
}
const fn default_async_approval_timeout_secs() -> u64 {
    3600
}
const fn default_async_approval_auto_approve_below() -> f64 {
    0.10
}
///
/// Implements the escalation decision rule from "The Hitchhiker's Guide to Agentic AI"
/// (Eq. 18.12):
///   Escalate iff p_success < tau_conf OR action in A_irreversible OR cost > B_auto
///
/// When enabled, the harness evaluates tool calls before execution and escalates
/// high-risk or low-confidence actions to a blocked-handoff (requiring human review).
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ConfidenceEscalationConfig {
    /// Master switch.  Default: false (opt-in).
    #[serde(default = "default_escalation_enabled")]
    pub enabled: bool,

    /// Minimum p_success threshold (0.0–1.0).  Tools whose estimated success
    /// probability falls below this threshold trigger escalation.
    #[serde(default = "default_escalation_confidence_threshold")]
    pub confidence_threshold: f64,

    /// Tool names that always trigger escalation regardless of confidence.
    /// These are matched against the tool call's function name.
    #[serde(default = "default_escalation_always_escalate_tools")]
    pub always_escalate_tools: Vec<String>,

    /// Maximum estimated cost in USD before escalation.  Tool calls whose
    /// estimated cost exceeds this threshold trigger escalation.
    #[serde(default = "default_escalation_cost_threshold")]
    pub cost_threshold_usd: f64,

    /// Whether to solicit LLM-based confidence estimation alongside heuristics.
    /// When false (default), only heuristic signals (error history, tool class)
    /// are used to estimate p_success.
    #[serde(default = "default_escalation_llm_confidence")]
    pub use_llm_confidence: bool,

    /// When true, only escalate during PlanBuildEvaluate orchestration mode.
    /// During single-mode or other orchestration modes, escalation is skipped.
    #[serde(default)]
    pub plan_mode_only: bool,

    /// Maximum number of re-plan attempts via conversation injection before
    /// prompting the user.  The agent sees a structured message explaining
    /// which tools were blocked and is asked to try a different approach.
    #[serde(default = "default_escalation_max_replan")]
    pub max_replan_attempts: u32,

    /// Maximum total escalations (across all chain steps) before aborting
    /// with partial results.  Must be >= max_replan_attempts.
    #[serde(default = "default_escalation_max_total")]
    pub max_total_escalations: u32,

    /// Whether to prompt the user when the escalation chain is exhausted.
    /// When false, the chain falls through directly to abort-with-partial-results.
    #[serde(default = "default_escalation_prompt_user")]
    pub prompt_user_on_exhaust: bool,
}

impl ConfidenceEscalationConfig {
    /// Returns true if any escalation rule would trigger for the given tool name
    /// and estimated cost.  Used as a fast pre-check before running the full gate.
    pub fn would_escalate(&self, tool_name: &str, estimated_cost_usd: Option<f64>) -> bool {
        if self.always_escalate_tools.iter().any(|t| t == tool_name) {
            return true;
        }
        if let Some(cost) = estimated_cost_usd
            && cost > self.cost_threshold_usd
        {
            return true;
        }
        false
    }
}

impl Default for ConfidenceEscalationConfig {
    fn default() -> Self {
        Self {
            enabled: default_escalation_enabled(),
            confidence_threshold: default_escalation_confidence_threshold(),
            always_escalate_tools: default_escalation_always_escalate_tools(),
            cost_threshold_usd: default_escalation_cost_threshold(),
            use_llm_confidence: default_escalation_llm_confidence(),
            plan_mode_only: false,
            max_replan_attempts: default_escalation_max_replan(),
            max_total_escalations: default_escalation_max_total(),
            prompt_user_on_exhaust: default_escalation_prompt_user(),
        }
    }
}

const fn default_escalation_enabled() -> bool {
    false
}
const fn default_escalation_confidence_threshold() -> f64 {
    0.7
}
fn default_escalation_always_escalate_tools() -> Vec<String> {
    vec!["delete_file".to_string(), "remove".to_string(), "rm".to_string()]
}
const fn default_escalation_cost_threshold() -> f64 {
    0.05
}
const fn default_escalation_llm_confidence() -> bool {
    false
}
const fn default_escalation_max_replan() -> u32 {
    3
}
const fn default_escalation_max_total() -> u32 {
    5
}
const fn default_escalation_prompt_user() -> bool {
    true
}
