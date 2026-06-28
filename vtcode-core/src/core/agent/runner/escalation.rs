//! Escalation gate for irreversible-action classification and confidence-based escalation.
//!
//! Implements the escalation decision rule from Eq. 18.12 of
//! "The Hitchhiker's Guide to Agentic AI":
//!
//! ```text
//! Escalate iff p_success < tau_conf OR action in A_irreversible OR cost > B_auto
//! ```
//!
//! The gate runs per-tool-call during the ReAct loop, *after* the LLM returns tool
//! calls but *before* any of them are dispatched.  Each tool call produces an
//! `EscalationDecision` that the caller (execute.rs) acts on.

use crate::core::agent::error_recovery::ErrorRecoveryState;
use crate::llm::provider::ToolCall;
use vtcode_config::core::agent::ConfidenceEscalationConfig;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Classification of a tool call's action by reversibility.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IrreversibilityClass {
    /// Safe to undo or low impact (reads, queries, compilations).
    Reversible,
    /// Significant cost or time even if technically reversible
    /// (large batch writes, long-running builds).
    HighCost,
    /// Cannot be undone by normal means (file deletion, destructive commands,
    /// force-push, privilege changes).
    Irreversible,
}

/// What to do with a tool call after escalation analysis.
#[derive(Debug, Clone)]
pub enum EscalationDecision {
    /// Proceed with the tool call as normal.
    Proceed,
    /// Escalate — halt the loop and write a blocked handoff.
    Escalate {
        /// Human-readable reason for the escalation.
        reason: String,
        /// The tool name that triggered escalation.
        tool_name: String,
        /// Serialized arguments for the blocked-handoff report.
        args: serde_json::Value,
    },
}

// ---------------------------------------------------------------------------
// Irreversibility classifier
// ---------------------------------------------------------------------------

/// Static analysis of tool name and arguments to determine action reversibility.
pub struct IrreversibilityClassifier;

impl IrreversibilityClassifier {
    /// Classify a tool call by its function name and argument content.
    pub fn classify(tool_name: &str, args: &serde_json::Value) -> IrreversibilityClass {
        // --- Irreversible patterns ---
        match tool_name {
            "delete_file" | "remove" | "rm" => return IrreversibilityClass::Irreversible,

            "move_file" | "mv" | "rename" => return IrreversibilityClass::Irreversible,

            "apply_patch" => {
                // Large destructive patches are irreversible
                if let Some(patch) = args.get("patch").and_then(|v| v.as_str()) {
                    if patch.len() > 5000 || patch.contains("--- /dev/null") {
                        return IrreversibilityClass::HighCost;
                    }
                }
                return IrreversibilityClass::HighCost;
            }

            "git_push" | "git_force_push" => return IrreversibilityClass::Irreversible,

            _ => {}
        }

        // --- High-cost patterns ---
        if tool_name == "unified_exec" || tool_name == "bash" || tool_name == "sh" {
            if let Some(command) = args.get("command").and_then(|v| v.as_str()) {
                let lower = command.to_lowercase();

                // Destructive shell commands
                if lower.contains("rm -rf /")
                    || lower.contains("rm -r /")
                    || lower.contains("rm --recursive /")
                {
                    return IrreversibilityClass::Irreversible;
                }
                if lower.contains("git push --force") || lower.contains("git push -f") {
                    return IrreversibilityClass::Irreversible;
                }
                if lower.contains("terraform apply") || lower.contains("kubectl delete") {
                    return IrreversibilityClass::Irreversible;
                }
                if lower.contains("chmod 777") || lower.contains("chown") {
                    return IrreversibilityClass::Irreversible;
                }
                if lower.contains("drop table") || lower.contains("drop database") {
                    return IrreversibilityClass::Irreversible;
                }

                // Bulk destructive operations
                if lower.contains("rm -rf") || lower.starts_with("rm ") {
                    return IrreversibilityClass::HighCost;
                }
            }
        }

        // --- Reversible by default ---
        IrreversibilityClass::Reversible
    }
}

// ---------------------------------------------------------------------------
// Confidence estimator
// ---------------------------------------------------------------------------

/// Heuristic confidence estimator for tool call success probability.
///
/// Uses error history, circuit-breaker state, and tool-class heuristics to
/// estimate p_success in `[0.0, 1.0]`.  When `use_llm_confidence` is true
/// the harness may also solicit an LLM judgment (not yet implemented).
pub struct ConfidenceEstimator;

impl ConfidenceEstimator {
    /// Estimate p_success for a tool call given the current error-recovery state.
    ///
    /// Returns a value in `[0.0, 1.0]` where 0.0 = certain failure,
    /// 1.0 = certain success.
    pub fn estimate(tool_name: &str, error_state: &ErrorRecoveryState, _use_llm: bool) -> f64 {
        let mut confidence = 1.0_f64;

        // --- Penalty from recent errors for this tool ---
        let recent_count = error_state
            .recent_errors
            .iter()
            .filter(|e| e.tool_name == tool_name)
            .count();
        if recent_count > 0 {
            // Each recent error reduces confidence by 15%, min 0.1
            let penalty = 0.15_f64.mul_add(recent_count as f64, 0.0);
            confidence = (confidence - penalty).max(0.1);
        }

        // --- Penalty from open circuits for this tool ---
        let circuit_count = error_state
            .circuit_events
            .iter()
            .filter(|e| e.tool_name == tool_name)
            .count();
        if circuit_count > 0 {
            let penalty = 0.25_f64.mul_add(circuit_count as f64, 0.0);
            confidence = (confidence - penalty).max(0.05);
        }

        // --- Tool-class baseline adjustments ---
        match tool_name {
            "read_file" | "glob" | "grep" | "unified_search" => {
                // Reads are generally reliable even with errors
                confidence = confidence.max(0.7);
            }
            "write_file" | "edit_file" | "apply_patch" => {
                // Writes have higher inherent risk
                confidence *= 0.9;
            }
            "unified_exec" | "bash" | "sh" => {
                // Commands are high-variance
                confidence *= 0.85;
            }
            _ => {}
        }

        confidence.clamp(0.0, 1.0)
    }
}

// ---------------------------------------------------------------------------
// Escalation gate
// ---------------------------------------------------------------------------

/// Result of running the escalation gate over a batch of tool calls.
#[derive(Debug, Clone)]
pub struct EscalationGateResult {
    /// Per-tool-call decisions, in the same order as the input.
    pub decisions: Vec<EscalationDecision>,
    /// True if at least one tool call triggered escalation.
    pub any_escalated: bool,
}

/// Runs the escalation decision rule over a set of tool calls.
pub struct EscalationGate;

impl EscalationGate {
    /// Evaluate tool calls against the escalation formula.
    ///
    /// Each tool call is independently assessed:
    ///   1. If `action in A_irreversible` → Escalate
    ///   2. If `p_success < tau_conf` → Escalate
    ///   3. If `cost > B_auto` → Escalate
    ///   4. Otherwise → Proceed
    #[allow(clippy::too_many_arguments)]
    pub fn decide(
        tool_calls: &[ToolCall],
        config: &ConfidenceEscalationConfig,
        error_state: &ErrorRecoveryState,
        estimated_cost_usd: Option<f64>,
        orchestration_mode: &str,
    ) -> EscalationGateResult {
        // Plan-mode-only gate: skip if not in PBE mode and plan_mode_only is set
        if config.plan_mode_only && orchestration_mode != "plan_build_evaluate" {
            return EscalationGateResult {
                decisions: tool_calls
                    .iter()
                    .map(|_| EscalationDecision::Proceed)
                    .collect(),
                any_escalated: false,
            };
        }

        let mut any_escalated = false;
        let decisions: Vec<EscalationDecision> = tool_calls
            .iter()
            .map(|tc| {
                let result = Self::decide_one(tc, config, error_state, estimated_cost_usd);
                if matches!(result, EscalationDecision::Escalate { .. }) {
                    any_escalated = true;
                }
                result
            })
            .collect();

        EscalationGateResult {
            decisions,
            any_escalated,
        }
    }

    /// Evaluate a single tool call.
    fn decide_one(
        tc: &ToolCall,
        config: &ConfidenceEscalationConfig,
        error_state: &ErrorRecoveryState,
        estimated_cost_usd: Option<f64>,
    ) -> EscalationDecision {
        // Extract function details; proceed if no function data is available
        let func = match tc.function.as_ref() {
            Some(f) => f,
            None => return EscalationDecision::Proceed,
        };
        let tool_name = &func.name;
        let args_value: serde_json::Value =
            serde_json::from_str(&func.arguments).unwrap_or(serde_json::Value::Null);

        // --- Rule 1: Action in A_irreversible ---
        if config.always_escalate_tools.iter().any(|t| t == tool_name) {
            return EscalationDecision::Escalate {
                reason: format!("Tool `{tool_name}` is in the always-escalate list"),
                tool_name: tool_name.clone(),
                args: args_value,
            };
        }

        let irrev_class = IrreversibilityClassifier::classify(tool_name, &args_value);
        if irrev_class == IrreversibilityClass::Irreversible {
            return EscalationDecision::Escalate {
                reason: format!("Tool `{tool_name}` performs an irreversible action"),
                tool_name: tool_name.clone(),
                args: args_value,
            };
        }

        // --- Rule 2: p_success < tau_conf ---
        let p_success =
            ConfidenceEstimator::estimate(tool_name, error_state, config.use_llm_confidence);
        if p_success < config.confidence_threshold {
            return EscalationDecision::Escalate {
                reason: format!(
                    "Estimated success probability {p_success:.2} is below threshold {:.2} for `{tool_name}`",
                    config.confidence_threshold,
                ),
                tool_name: tool_name.clone(),
                args: args_value,
            };
        }

        // --- Rule 3: cost > B_auto ---
        if let Some(cost) = estimated_cost_usd {
            if cost > config.cost_threshold_usd {
                return EscalationDecision::Escalate {
                    reason: format!(
                        "Estimated cost ${cost:.4} exceeds threshold ${:.4} for `{tool_name}`",
                        config.cost_threshold_usd,
                    ),
                    tool_name: tool_name.clone(),
                    args: args_value,
                };
            }
        }

        EscalationDecision::Proceed
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::agent::error_recovery::{ErrorType, RecentError};
    use std::time::Instant;

    // -- IrreversibilityClassifier tests --

    #[test]
    fn classify_delete_file_is_irreversible() {
        let args = serde_json::json!({"path": "/tmp/foo.txt"});
        assert_eq!(
            IrreversibilityClassifier::classify("delete_file", &args),
            IrreversibilityClass::Irreversible,
        );
    }

    #[test]
    fn classify_read_file_is_reversible() {
        let args = serde_json::json!({"path": "/tmp/foo.txt"});
        assert_eq!(
            IrreversibilityClassifier::classify("read_file", &args),
            IrreversibilityClass::Reversible,
        );
    }

    #[test]
    fn classify_rm_rf_root_is_irreversible() {
        let args = serde_json::json!({"command": "rm -rf /var/log"});
        assert_eq!(
            IrreversibilityClassifier::classify("unified_exec", &args),
            IrreversibilityClass::Irreversible,
        );
    }

    #[test]
    fn classify_git_force_push_is_irreversible() {
        let args = serde_json::json!({"command": "git push --force origin main"});
        // The tool name for this is "unified_exec" or "bash", not "git_push"
        assert_eq!(
            IrreversibilityClassifier::classify("unified_exec", &args),
            IrreversibilityClass::Irreversible,
        );
    }

    #[test]
    fn classify_destructive_shell_is_irreversible() {
        let args = serde_json::json!({"command": "rm -rf /"});
        assert_eq!(
            IrreversibilityClassifier::classify("unified_exec", &args),
            IrreversibilityClass::Irreversible,
        );
    }

    #[test]
    fn classify_drop_table_is_irreversible() {
        let args = serde_json::json!({"command": "DROP TABLE users"});
        assert_eq!(
            IrreversibilityClassifier::classify("unified_exec", &args),
            IrreversibilityClass::Irreversible,
        );
    }

    // -- ConfidenceEstimator tests --

    #[test]
    fn estimate_full_confidence_with_no_errors() {
        let state = ErrorRecoveryState::new();
        let confidence = ConfidenceEstimator::estimate("read_file", &state, false);
        assert!((confidence - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn estimate_penalized_with_recent_errors() {
        let mut state = ErrorRecoveryState::new();
        state.recent_errors.push_back(RecentError {
            tool_name: "write_file".into(),
            timestamp: Instant::now(),
            error_message: "permission denied".into(),
            error_type: ErrorType::ToolExecution,
            category: None,
        });
        let confidence = ConfidenceEstimator::estimate("write_file", &state, false);
        // 1.0 - 0.15 = 0.85, then * 0.9 (write penality) = 0.765
        assert!(confidence < 0.9);
        assert!(confidence > 0.0);
    }

    #[test]
    fn estimate_floor_at_zero() {
        let mut state = ErrorRecoveryState::new();
        for _ in 0..100 {
            state.recent_errors.push_back(RecentError {
                tool_name: "bash".into(),
                timestamp: Instant::now(),
                error_message: "error".into(),
                error_type: ErrorType::ToolExecution,
                category: None,
            });
        }
        let confidence = ConfidenceEstimator::estimate("bash", &state, false);
        assert!(confidence >= 0.0);
        // After many errors: 1.0 - 100 * 0.15 = negative, clamped to 0.1,
        // then * 0.85 (bash penalty) = 0.085, clamped to 0.0 = 0.085
        assert!(confidence <= 0.1);
    }

    // -- EscalationGate tests --

    #[test]
    fn gate_proceeds_with_safe_tool() {
        let config = ConfidenceEscalationConfig {
            enabled: true,
            confidence_threshold: 0.7,
            cost_threshold_usd: 0.05,
            ..Default::default()
        };
        let state = ErrorRecoveryState::new();
        let tc = make_tool("read_file", r#"{"path":"/tmp/x"}"#);

        let result = EscalationGate::decide(&[tc], &config, &state, None, "single");
        assert!(!result.any_escalated);
        assert!(matches!(result.decisions[0], EscalationDecision::Proceed));
    }

    #[test]
    fn gate_escalates_always_escalate_tool() {
        let config = ConfidenceEscalationConfig {
            enabled: true,
            always_escalate_tools: vec!["delete_file".into()],
            ..Default::default()
        };
        let state = ErrorRecoveryState::new();
        let tc = make_tool("delete_file", r#"{"path":"/tmp/x"}"#);

        let result = EscalationGate::decide(&[tc], &config, &state, None, "single");
        assert!(result.any_escalated);
        assert!(matches!(
            result.decisions[0],
            EscalationDecision::Escalate { .. }
        ));
    }

    #[test]
    fn gate_escalates_irreversible_classification() {
        let config = ConfidenceEscalationConfig {
            enabled: true,
            always_escalate_tools: vec![],
            confidence_threshold: 0.0, // disable p_success gate
            ..Default::default()
        };
        let state = ErrorRecoveryState::new();
        let tc = make_tool("move_file", r#"{"from":"/a","to":"/b"}"#);

        let result = EscalationGate::decide(&[tc], &config, &state, None, "single");
        assert!(result.any_escalated);
    }

    #[test]
    fn gate_escalates_low_confidence() {
        let config = ConfidenceEscalationConfig {
            enabled: true,
            always_escalate_tools: vec![],
            confidence_threshold: 0.99, // very high threshold
            ..Default::default()
        };
        let mut state = ErrorRecoveryState::new();
        state.recent_errors.push_back(RecentError {
            tool_name: "unified_exec".into(),
            timestamp: Instant::now(),
            error_message: "error".into(),
            error_type: ErrorType::ToolExecution,
            category: None,
        });
        let tc = make_tool("unified_exec", r#"{"command":"cargo build"}"#);

        let result = EscalationGate::decide(&[tc], &config, &state, None, "single");
        assert!(result.any_escalated);
    }

    #[test]
    fn gate_escalates_cost_exceeded() {
        let config = ConfidenceEscalationConfig {
            enabled: true,
            cost_threshold_usd: 0.01,
            ..Default::default()
        };
        let state = ErrorRecoveryState::new();
        let tc = make_tool("write_file", r#"{"path":"/tmp/x"}"#);

        let result = EscalationGate::decide(&[tc], &config, &state, Some(0.50), "single");
        assert!(result.any_escalated);
    }

    #[test]
    fn gate_plan_mode_only_skips_non_pbe() {
        let config = ConfidenceEscalationConfig {
            enabled: true,
            plan_mode_only: true,
            always_escalate_tools: vec!["delete_file".into()],
            ..Default::default()
        };
        let state = ErrorRecoveryState::new();
        let tc = make_tool("delete_file", r#"{"path":"/tmp/x"}"#);

        // In "single" mode, plan_mode_only=true should skip escalation
        let result = EscalationGate::decide(&[tc], &config, &state, None, "single");
        assert!(!result.any_escalated);
    }

    #[test]
    fn gate_mixed_batch() {
        let config = ConfidenceEscalationConfig {
            enabled: true,
            always_escalate_tools: vec!["delete_file".into()],
            ..Default::default()
        };
        let state = ErrorRecoveryState::new();
        let tc_safe = make_tool("read_file", r#"{"path":"/tmp/x"}"#);
        let tc_irrev = make_tool("delete_file", r#"{"path":"/tmp/y"}"#);

        let result = EscalationGate::decide(&[tc_safe, tc_irrev], &config, &state, None, "single");
        assert!(result.any_escalated);
        assert!(matches!(result.decisions[0], EscalationDecision::Proceed));
        assert!(matches!(
            result.decisions[1],
            EscalationDecision::Escalate { .. }
        ));
    }

    // -- Helper constructor for ToolCall --

    fn make_tool(name: &str, args: &str) -> ToolCall {
        ToolCall::function("test_call".to_string(), name.to_string(), args.to_string())
    }
}
