//! Evaluator response types for the plan-build-evaluate harness.
//!
//! These types represent the structured JSON response from the LLM evaluator
//! that judges whether a sprint implementation meets the execution contract.

use serde::Deserialize;

/// Minimum score required for each evaluator scorecard dimension.
const EVALUATOR_SCORE_THRESHOLD: u8 = 4;
const MAX_GENERALIZATION_NOTES: usize = 8;
const MAX_GENERALIZATION_NOTE_FIELD_CHARS: usize = 1_000;

/// A bounded, task-scoped observation that may guide a later replan.
///
/// These notes deliberately do not model durable beliefs. Every note must
/// carry the evidence that supports it and a concrete way to falsify it before
/// it can enter an evaluation artifact or a tracker.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct GeneralizationNote {
    pub(super) claim: String,
    pub(super) scope: String,
    pub(super) evidence: String,
    pub(super) falsifier: String,
}

impl GeneralizationNote {
    fn validate(self) -> Result<Self, String> {
        let fields = [
            ("claim", &self.claim),
            ("scope", &self.scope),
            ("evidence", &self.evidence),
            ("falsifier", &self.falsifier),
        ];
        for (name, value) in fields {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                return Err(format!("generalization note {name} must not be empty"));
            }
            if trimmed.chars().count() > MAX_GENERALIZATION_NOTE_FIELD_CHARS {
                return Err(format!(
                    "generalization note {name} exceeds {MAX_GENERALIZATION_NOTE_FIELD_CHARS} characters"
                ));
            }
        }

        Ok(Self {
            claim: self.claim.trim().to_string(),
            scope: self.scope.trim().to_string(),
            evidence: self.evidence.trim().to_string(),
            falsifier: self.falsifier.trim().to_string(),
        })
    }
}

/// Structured response from the evaluator LLM.
#[derive(Debug, Clone, Deserialize)]
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
    /// Task-scoped observations. These are validated before rendering and are
    /// never copied into global beliefs or persistent memory automatically.
    #[serde(default, deserialize_with = "deserialize_generalization_notes")]
    pub(super) generalization_notes: Vec<GeneralizationNote>,
}

fn deserialize_generalization_notes<'de, D>(deserializer: D) -> Result<Vec<GeneralizationNote>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let notes = Vec::<GeneralizationNote>::deserialize(deserializer)?;
    if notes.len() > MAX_GENERALIZATION_NOTES {
        return Err(serde::de::Error::custom(format!(
            "at most {MAX_GENERALIZATION_NOTES} generalization notes are allowed"
        )));
    }
    Ok(notes)
}

/// A single finding from the evaluator.
#[derive(Debug, Clone, Deserialize)]
pub(super) struct EvaluatorFinding {
    pub(super) severity: String,
    #[serde(alias = "finding")]
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

    fn missing_criteria(&self) -> Vec<String> {
        self.entries()
            .into_iter()
            .filter(|(_, score)| score.is_none())
            .map(|(label, _)| label.to_string())
            .collect()
    }

    fn invalid_criteria(&self) -> Vec<String> {
        self.entries()
            .into_iter()
            .filter_map(|(label, score)| {
                score
                    .filter(|score| !(1..=5).contains(score))
                    .map(|score| format!("{label} {score}/5"))
            })
            .collect()
    }

    fn failing_criteria(&self) -> Vec<String> {
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

    fn with_min_score(mut self, label: &str, new_score: u8) -> Self {
        let current = match label {
            "Contract fidelity" => self.contract_fidelity,
            "Functionality" => self.functionality,
            "Code quality" => self.code_quality,
            "Verification integrity" => self.verification_integrity,
            _ => None,
        };
        let merged = match (current, new_score) {
            (Some(c), n) if n < c => Some(n),
            (None, n) => Some(n),
            (Some(c), _) => Some(c),
        };
        match label {
            "Contract fidelity" => self.contract_fidelity = merged,
            "Functionality" => self.functionality = merged,
            "Code quality" => self.code_quality = merged,
            "Verification integrity" => self.verification_integrity = merged,
            _ => {}
        }
        self
    }
}

impl EvaluatorResponse {
    pub(super) fn validated_generalization_notes(&self) -> Result<Vec<GeneralizationNote>, String> {
        self.generalization_notes
            .iter()
            .cloned()
            .map(GeneralizationNote::validate)
            .collect()
    }

    fn effective_scorecard(&self) -> EvaluatorScorecard {
        self.scorecard.unwrap_or_default()
    }

    fn missing_criteria(&self) -> Vec<String> {
        self.effective_scorecard().missing_criteria()
    }

    fn invalid_criteria(&self) -> Vec<String> {
        self.effective_scorecard().invalid_criteria()
    }

    fn failing_criteria(&self) -> Vec<String> {
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
            let prefix = format!("Scorecard below threshold (>= {EVALUATOR_SCORE_THRESHOLD}/5 required)");
            append_clause(&failing_criteria, &prefix);
        }

        if summary.is_empty() {
            if self.high_severity_findings > 0 {
                return format!("Evaluator reported {} high-severity finding(s).", self.high_severity_findings);
            }
            if missing_criteria.is_empty() && invalid_criteria.is_empty() && failing_criteria.is_empty() {
                return "Evaluator returned no summary.".to_string();
            }
        }

        summary
    }
}

/// Single skeptic panel entry: model id + its evaluator response.
#[derive(Debug, Clone)]
pub(super) struct SkepticPanelEntry {
    pub(super) response: EvaluatorResponse,
}

/// Aggregated result of the skeptic panel.
///
/// The panel passes only when every skeptic verdict is "pass" and every
/// scorecard dimension meets the threshold across all panelists.
#[derive(Debug, Clone)]
pub(super) struct SkepticPanelAggregate {
    pub(super) verdict: String,
    pub(super) summary: String,
    pub(super) scorecard: EvaluatorScorecard,
    pub(super) high_severity_findings: usize,
    pub(super) generalization_notes: Vec<GeneralizationNote>,
}

impl SkepticPanelAggregate {
    /// Aggregate the strictest verdict/scorecard across all skeptics.
    pub(super) fn from_entries(entries: Vec<SkepticPanelEntry>) -> Self {
        let all_passed = entries.iter().all(|e| e.response.passed());
        let verdict = if all_passed {
            "pass".to_string()
        } else {
            "fail".to_string()
        };
        let high_severity_findings = entries.iter().map(|e| e.response.high_severity_findings).max().unwrap_or(0);
        let generalization_notes = entries
            .iter()
            .flat_map(|entry| entry.response.generalization_notes.iter().cloned())
            .take(MAX_GENERALIZATION_NOTES)
            .collect();
        let mut summaries = entries.iter().map(|e| e.response.summary.trim()).collect::<Vec<_>>();
        if summaries.len() > 3 {
            summaries.truncate(3);
        }
        let summary = if summaries.is_empty() {
            "Skeptic panel: no responses.".to_string()
        } else {
            format!("Skeptic panel ({} models): {}", entries.len(), summaries.join(" | "))
        };
        let mut scorecard = EvaluatorScorecard::default();
        for entry in &entries {
            let sc = entry.response.effective_scorecard();
            for (label, _current) in scorecard.entries() {
                if let Some(new) = sc.entries().iter().find(|(l, _)| **l == *label).and_then(|(_, s)| *s) {
                    scorecard = scorecard.with_min_score(label, new);
                }
            }
        }
        Self {
            verdict,
            summary,
            scorecard,
            high_severity_findings,
            generalization_notes,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn response_with_notes(notes: serde_json::Value) -> EvaluatorResponse {
        serde_json::from_value(json!({
            "verdict": "pass",
            "summary": "checked",
            "scorecard": {
                "contract_fidelity": 5,
                "functionality": 5,
                "code_quality": 5,
                "verification_integrity": 5,
            },
            "generalization_notes": notes,
        }))
        .expect("evaluator response should deserialize")
    }

    #[test]
    fn generalization_notes_are_trimmed_and_require_all_fields() {
        let response = response_with_notes(json!([{
            "claim": "  claim  ",
            "scope": "  one task  ",
            "evidence": "  a test  ",
            "falsifier": "  another test  ",
        }]));

        let notes = response.validated_generalization_notes().expect("note should validate");
        assert_eq!(notes[0].claim, "claim");
        assert_eq!(notes[0].scope, "one task");
        assert_eq!(notes[0].evidence, "a test");
        assert_eq!(notes[0].falsifier, "another test");

        let invalid = response_with_notes(json!([{
            "claim": " ",
            "scope": "scope",
            "evidence": "evidence",
            "falsifier": "falsifier",
        }]));
        assert!(invalid.validated_generalization_notes().is_err());

        let oversized_claim = "x".repeat(MAX_GENERALIZATION_NOTE_FIELD_CHARS + 1);
        let oversized = response_with_notes(json!([{
            "claim": oversized_claim,
            "scope": "scope",
            "evidence": "evidence",
            "falsifier": "falsifier",
        }]));
        assert!(oversized.validated_generalization_notes().is_err());
    }

    #[test]
    fn generalization_notes_are_bounded() {
        let note = json!({
            "claim": "claim",
            "scope": "scope",
            "evidence": "evidence",
            "falsifier": "falsifier",
        });
        let too_many = serde_json::from_value::<EvaluatorResponse>(json!({
            "verdict": "pass",
            "summary": "checked",
            "generalization_notes": vec![note; MAX_GENERALIZATION_NOTES + 1],
        }));
        assert!(too_many.is_err());
    }

    #[test]
    fn skeptic_panel_retains_bounded_task_scoped_notes() {
        let note = json!({
            "claim": "claim",
            "scope": "scope",
            "evidence": "evidence",
            "falsifier": "falsifier",
        });
        let first = response_with_notes(json!(vec![note.clone(); MAX_GENERALIZATION_NOTES]));
        let second = response_with_notes(json!([note]));
        let aggregate = SkepticPanelAggregate::from_entries(vec![
            SkepticPanelEntry { response: first },
            SkepticPanelEntry { response: second },
        ]);
        assert_eq!(aggregate.generalization_notes.len(), MAX_GENERALIZATION_NOTES);
    }
}
