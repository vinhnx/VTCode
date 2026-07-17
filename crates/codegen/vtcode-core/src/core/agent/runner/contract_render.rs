//! Contract and evaluation rendering for the plan-build-evaluate harness.
//!
//! Contains functions for rendering evaluation reports and execution contracts
//! as markdown artifacts.

use super::AgentRunner;
use super::evaluator_types::EvaluatorResponse;
use crate::core::agent::task::Task;
use std::fmt::Write;

impl AgentRunner {
    /// Render an evaluator response as a markdown evaluation report.
    pub(super) fn render_evaluation(&self, evaluation: &EvaluatorResponse) -> String {
        let mut markdown = format!(
            "# Evaluation\n\n## Verdict\n{}\n\n## Summary\n{}\n",
            evaluation.verdict.trim(),
            evaluation.effective_summary()
        );

        if let Some(scorecard) = evaluation.scorecard.as_ref()
            && scorecard.has_scores()
        {
            markdown.push_str("\n## Scorecard\n");
            for (label, score) in scorecard.entries() {
                if let Some(score) = score {
                    let _ = writeln!(markdown, "- {label}: {score}/5");
                }
            }
        }

        if !evaluation.findings.is_empty() {
            markdown.push_str("\n## Findings\n");
            for finding in &evaluation.findings {
                let _ = write!(
                    markdown,
                    "- [{}] {}",
                    finding.severity.trim(),
                    finding.title.trim()
                );
                if let Some(detail) = finding
                    .detail
                    .as_deref()
                    .filter(|text| !text.trim().is_empty())
                {
                    markdown.push_str(": ");
                    markdown.push_str(detail.trim());
                }
                markdown.push('\n');
            }
        }

        super::orchestration::render_markdown_list(
            &mut markdown,
            "Unmet Contract Items",
            &evaluation.unmet_contract_items,
        );
        super::orchestration::render_markdown_list(
            &mut markdown,
            "Residual Risks",
            &evaluation.residual_risks,
        );
        super::orchestration::render_markdown_list(
            &mut markdown,
            "Required Tracker Updates",
            &evaluation.required_tracker_updates,
        );

        markdown
    }

    /// Render an execution contract as markdown.
    pub(super) fn render_contract_markdown(
        &self,
        task: &Task,
        tracker_items: &[serde_json::Value],
    ) -> String {
        let mut markdown = format!(
            "# Execution Contract\n\n## Goal\n{}\n\n## Done Criteria\n",
            task.description.trim()
        );

        if tracker_items.is_empty() {
            markdown.push_str("- Deliver the requested change.\n");
            markdown.push_str("- Keep the result verifiable.\n");
        } else {
            for (index, item) in tracker_items.iter().enumerate() {
                let description = item
                    .get("description")
                    .and_then(serde_json::Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .unwrap_or(task.description.trim());
                let outcome = item
                    .get("outcome")
                    .and_then(serde_json::Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .unwrap_or("Requested work is implemented and tracked.");
                let files = super::orchestration::json_string_list(item, "files");
                let verify = super::orchestration::json_string_list(item, "verify");

                let _ = writeln!(markdown, "- Step {}: {}", index + 1, description);
                let _ = writeln!(markdown, "- Outcome: {outcome}");
                if !files.is_empty() {
                    let _ = writeln!(markdown, "- Files: {}", files.join(", "));
                }
                if !verify.is_empty() {
                    let _ = writeln!(markdown, "- Verify: {}", verify.join(" | "));
                }
            }
        }

        markdown.push_str(
            "\n## Review Standard\n- Prefer observable behavior over claimed completion.\n- Prefer failing borderline output over accepting unverifiable work.\n",
        );
        markdown
    }
}
