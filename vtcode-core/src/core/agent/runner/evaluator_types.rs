//! Evaluator response types for the plan-build-evaluate harness.
//!
//! These types represent the structured JSON response from the LLM evaluator
//! that judges whether a sprint implementation meets the execution contract.

use serde::Deserialize;

/// Minimum score required for each evaluator scorecard dimension.
pub(super) const EVALUATOR_SCORE_THRESHOLD: u8 = 4;

/// Structured response from the evaluator LLM.
#[derive(Debug, Deserialize)]
pub(super) struct EvaluatorResponse {
    pub(super) verdict: String,
    pub(super) summary: String,
    #[serde(default)]
    pub(super) high_severity_findings: usize,
    #[serde(default)]
    pub(super) scorecard: Option<EvaluatorScorecard>,
    #[serde(default)]
    pub(super) findings: Vec<EvaluatorFinding>,
    #[serde(default)]
    pub(super) unmet_contract_items: Vec<String>,
    #[serde(default)]
    pub(super) residual_risks: Vec<String>,
    #[serde(default)]
    pub(super) required_tracker_updates: Vec<String>,
}

/// A single finding from the evaluator.
#[derive(Debug, Deserialize)]
pub(super) struct EvaluatorFinding {
    pub(super) severity: String,
    pub(super) title: String,
    #[serde(default)]
    pub(super) detail: Option<String>,
}

/// Scorecard with 1-5 scores across four evaluation dimensions.
#[derive(Debug, Clone, Copy, Deserialize, Default)]
pub(super) struct EvaluatorScorecard {
    #[serde(default)]
    contract_fidelity: Option<u8>,
    #[serde(default)]
    functionality: Option<u8>,
    #[serde(default)]
    code_quality: Option<u8>,
    #[serde(default)]
    verification_integrity: Option<u8>,
}

impl EvaluatorScorecard {
    pub(super) fn entries(&self) -> [(&'static str, Option<u8>); 4] {
        [
            ("Contract fidelity", self.contract_fidelity),
            ("Functionality", self.functionality),
            ("Code quality", self.code_quality),
            ("Verification integrity", self.verification_integrity),
        ]
    }

    pub(super) fn missing_criteria(&self) -> Vec<String> {
        self.entries()
            .into_iter()
            .filter(|(_, score)| score.is_none())
            .map(|(label, _)| label.to_string())
            .collect()
    }

    pub(super) fn invalid_criteria(&self) -> Vec<String> {
        self.entries()
            .into_iter()
            .filter_map(|(label, score)| {
                score
                    .filter(|score| !(1..=5).contains(score))
                    .map(|score| format!("{label} {score}/5"))
            })
            .collect()
    }

    pub(super) fn failing_criteria(&self) -> Vec<String> {
        self.entries()
            .into_iter()
            .filter_map(|(label, score)| {
                score
                    .filter(|score| (1..=5).contains(score) && *score < EVALUATOR_SCORE_THRESHOLD)
                    .map(|score| format!("{label} {score}/5"))
            })
            .collect()
    }

    pub(super) fn has_scores(&self) -> bool {
        self.entries().into_iter().any(|(_, score)| score.is_some())
    }
}

impl EvaluatorResponse {
    pub(super) fn effective_scorecard(&self) -> EvaluatorScorecard {
        self.scorecard.unwrap_or_default()
    }

    pub(super) fn missing_criteria(&self) -> Vec<String> {
        self.effective_scorecard().missing_criteria()
    }

    pub(super) fn invalid_criteria(&self) -> Vec<String> {
        self.effective_scorecard().invalid_criteria()
    }

    pub(super) fn failing_criteria(&self) -> Vec<String> {
        self.effective_scorecard().failing_criteria()
    }

    /// Whether the evaluator passed the implementation.
    pub(super) fn passed(&self) -> bool {
        self.verdict.eq_ignore_ascii_case("pass")
            && self.high_severity_findings == 0
            && self.missing_criteria().is_empty()
            && self.invalid_criteria().is_empty()
            && self.failing_criteria().is_empty()
    }

    /// Render the effective summary including scorecard diagnostics.
    pub(super) fn effective_summary(&self) -> String {
        use std::fmt::Write as _;

        let mut summary = self.summary.trim().to_string();
        let missing_criteria = self.missing_criteria();
        let invalid_criteria = self.invalid_criteria();
        let failing_criteria = self.failing_criteria();

        let mut append_clause = |labels: &[String], prefix: &str| {
            if labels.is_empty() {
                return;
            }
            if !summary.is_empty() {
                summary.push(' ');
            }
            let _ = write!(summary, "{prefix}: {}.", labels.join(", "));
        };

        append_clause(&missing_criteria, "Scorecard incomplete: missing");
        append_clause(&invalid_criteria, "Scorecard invalid (scores must be 1-5)");
        if !failing_criteria.is_empty() {
            let prefix =
                format!("Scorecard below threshold (>= {EVALUATOR_SCORE_THRESHOLD}/5 required)");
            append_clause(&failing_criteria, &prefix);
        }

        if summary.is_empty() {
            if self.high_severity_findings > 0 {
                return format!(
                    "Evaluator reported {} high-severity finding(s).",
                    self.high_severity_findings
                );
            }
            if missing_criteria.is_empty()
                && invalid_criteria.is_empty()
                && failing_criteria.is_empty()
            {
                return "Evaluator returned no summary.".to_string();
            }
        }

        summary
    }
}
