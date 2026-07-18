//! Structured evaluator with scoring dimensions and hard thresholds.
//!
//! Following the long-running harness pattern from Anthropic's research:
//! "When using a judge, we cannot ask abstractly whether this is 'good.' We need
//! to break 'good' into multiple checkable dimensions."
//!
//! The evaluator scores a sprint across multiple dimensions, each with a hard
//! threshold. If any dimension falls below its threshold, the sprint fails and
//! the generator must revise based on concrete feedback. This prevents the
//! common failure mode where an agent sees a button render and declares the
//! feature complete.
//!
//! Key principles:
//! - Evaluate outcomes, not claims (check actual test results, not agent assertions)
//! - Every dimension has a hard threshold (below = sprint fails)
//! - Feedback is specific and actionable (not "looks good")

use serde::{Deserialize, Serialize};

/// A scoring dimension with a hard threshold.
///
/// Each dimension represents one aspect of quality that must be evaluated
/// independently. The hard_threshold is the minimum acceptable score --
/// falling below it means the sprint fails regardless of other scores.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoringDimension {
    /// Short identifier (e.g. "correctness", "functionality").
    pub name: String,
    /// Weight for overall score computation (0.0..=1.0).
    pub weight: f32,
    /// Minimum acceptable score. Below this = sprint fails.
    pub hard_threshold: f32,
    /// Human-readable description of what this dimension measures.
    pub description: String,
}

/// Score for a single dimension in an evaluation result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DimensionScore {
    /// Which dimension was scored.
    pub dimension: String,
    /// Score achieved (0.0..=1.0).
    pub score: f32,
    /// Hard threshold for this dimension (0.0..=1.0).
    pub hard_threshold: f32,
    /// Whether this score is below the hard threshold.
    pub below_threshold: bool,
    /// Specific, actionable notes about this score.
    pub notes: String,
}

/// The complete evaluation result for a sprint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvaluationResult {
    /// Per-dimension scores.
    pub scores: Vec<DimensionScore>,
    /// Weighted overall score (0.0..=1.0).
    pub overall_score: f32,
    /// Whether all dimensions meet their hard thresholds.
    pub overall_pass: bool,
    /// High-level feedback summary.
    pub feedback: String,
    /// Specific issues that must be fixed.
    pub issues: Vec<String>,
}

/// The rubric that defines how a sprint is evaluated.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvaluationRubric {
    /// Scoring dimensions with hard thresholds.
    pub dimensions: Vec<ScoringDimension>,
}

impl EvaluationRubric {
    /// Create a new rubric with the given dimensions.
    ///
    /// Clamps all weights and hard thresholds to [0.0, 1.0] to prevent
    /// invalid rubrics from producing confusing results.
    pub fn new(dimensions: Vec<ScoringDimension>) -> Self {
        let dimensions = dimensions
            .into_iter()
            .map(|mut d| {
                d.weight = d.weight.clamp(0.0, 1.0);
                d.hard_threshold = d.hard_threshold.clamp(0.0, 1.0);
                d
            })
            .collect();
        Self { dimensions }
    }

    /// Evaluate scores against this rubric.
    ///
    /// Takes a map of dimension name -> (score, notes) and produces a full
    /// EvaluationResult with weighted overall score and pass/fail determination.
    pub fn evaluate(&self, raw_scores: &[(&str, f32, &str)]) -> EvaluationResult {
        let mut scores = Vec::with_capacity(self.dimensions.len());
        let mut weighted_sum = 0.0f32;
        let mut weight_total = 0.0f32;
        let mut overall_pass = true;
        let mut issues = Vec::new();

        for dim in &self.dimensions {
            let (score, notes) = raw_scores
                .iter()
                .find(|(name, _, _)| *name == dim.name)
                .map(|(_, s, n)| (*s, *n))
                .unwrap_or((0.0, "not evaluated"));

            let clamped = score.clamp(0.0, 1.0);
            let below_threshold = clamped < dim.hard_threshold;

            if below_threshold {
                overall_pass = false;
                issues.push(format!(
                    "{}: {:.0}% below hard threshold {:.0}% -- {}",
                    dim.name,
                    clamped * 100.0,
                    dim.hard_threshold * 100.0,
                    notes,
                ));
            }

            weighted_sum += clamped * dim.weight;
            weight_total += dim.weight;

            scores.push(DimensionScore {
                dimension: dim.name.clone(),
                score: clamped,
                below_threshold,
                notes: notes.to_string(),
                hard_threshold: dim.hard_threshold,
            });
        }

        let overall_score = if weight_total > 0.0 {
            weighted_sum / weight_total
        } else {
            0.0
        };

        let feedback = if overall_pass {
            format!("Sprint PASSED. Overall score: {:.0}%. All dimensions meet hard thresholds.", overall_score * 100.0,)
        } else {
            format!(
                "Sprint FAILED. Overall score: {:.0}%. {} dimension(s) below hard threshold: {}",
                overall_score * 100.0,
                issues.len(),
                issues
                    .iter()
                    .map(|i| i.split(':').next().unwrap_or("?"))
                    .collect::<Vec<_>>()
                    .join(", "),
            )
        };

        EvaluationResult {
            scores,
            overall_score,
            overall_pass,
            feedback,
            issues,
        }
    }
}

/// Default rubric for code tasks.
///
/// Dimensions:
/// - correctness (weight 0.4, hard 0.9): Does the code do what was asked?
/// - functionality (weight 0.3, hard 0.8): Do tests pass? Does it run?
/// - code_quality (weight 0.2, hard 0.7): Is the code clean and maintainable?
/// - test_coverage (weight 0.1, hard 0.6): Are there adequate tests?
pub fn default_code_rubric() -> EvaluationRubric {
    EvaluationRubric::new(vec![
        ScoringDimension {
            name: "correctness".to_string(),
            weight: 0.4,
            hard_threshold: 0.9,
            description: "The code implements what was asked, not just something \
                that looks right. Edge cases are handled. Behavior matches the spec."
                .to_string(),
        },
        ScoringDimension {
            name: "functionality".to_string(),
            weight: 0.3,
            hard_threshold: 0.8,
            description: "Tests pass. The application runs. Commands produce expected \
                output. No regressions in existing functionality."
                .to_string(),
        },
        ScoringDimension {
            name: "code_quality".to_string(),
            weight: 0.2,
            hard_threshold: 0.7,
            description: "Code follows project conventions. No unwrap() in production. \
                Proper error handling. Clear naming. Files under 500 lines."
                .to_string(),
        },
        ScoringDimension {
            name: "test_coverage".to_string(),
            weight: 0.1,
            hard_threshold: 0.6,
            description: "New code has tests. Edge cases are covered. Tests are \
                deterministic and fast."
                .to_string(),
        },
    ])
}

/// Render an EvaluationResult as a markdown report suitable for harness artifacts.
pub fn evaluation_to_markdown(result: &EvaluationResult) -> String {
    let mut out = String::new();
    out.push_str("# Evaluation Report\n\n");
    out.push_str(&format!("**Overall:** {}\n\n", result.feedback));

    out.push_str("## Scores\n\n");
    out.push_str("| Dimension | Score | Threshold | Status | Notes |\n");
    out.push_str("|-----------|-------|-----------|--------|-------|\n");
    for score in &result.scores {
        let status = if score.below_threshold { "BELOW" } else { "OK" };
        out.push_str(&format!(
            "| {} | {:.0}% | {:.0}% | {} | {} |\n",
            score.dimension,
            score.score * 100.0,
            score.hard_threshold * 100.0,
            status,
            score.notes,
        ));
    }

    if !result.issues.is_empty() {
        out.push_str("\n## Issues (must fix)\n\n");
        for issue in &result.issues {
            out.push_str(&format!("- {issue}\n"));
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_dimensions_pass() {
        let rubric = default_code_rubric();
        let result = rubric.evaluate(&[
            ("correctness", 0.95, "handles all edge cases"),
            ("functionality", 1.0, "all tests pass"),
            ("code_quality", 0.85, "clean and follows conventions"),
            ("test_coverage", 0.7, "good coverage"),
        ]);
        assert!(result.overall_pass);
        assert!(result.overall_score > 0.8);
        assert!(result.issues.is_empty());
    }

    #[test]
    fn one_dimension_below_threshold_fails_sprint() {
        let rubric = default_code_rubric();
        let result = rubric.evaluate(&[
            ("correctness", 0.5, "misses edge cases"),
            ("functionality", 1.0, "all tests pass"),
            ("code_quality", 0.85, "clean"),
            ("test_coverage", 0.7, "good"),
        ]);
        assert!(!result.overall_pass);
        assert_eq!(result.issues.len(), 1);
        assert!(result.issues[0].contains("correctness"));
    }

    #[test]
    fn multiple_failures_reported() {
        let rubric = default_code_rubric();
        let result = rubric.evaluate(&[
            ("correctness", 0.5, "wrong behavior"),
            ("functionality", 0.3, "tests fail"),
            ("code_quality", 0.85, "clean"),
            ("test_coverage", 0.7, "good"),
        ]);
        assert!(!result.overall_pass);
        assert_eq!(result.issues.len(), 2);
    }

    #[test]
    fn missing_dimension_scores_zero() {
        let rubric = default_code_rubric();
        let result = rubric.evaluate(&[
            ("correctness", 0.95, "good"),
            // functionality, code_quality, test_coverage missing
        ]);
        assert!(!result.overall_pass);
        // Missing dimensions score 0.0, which is below all hard thresholds
        assert!(result.issues.len() >= 3);
    }

    #[test]
    fn scores_clamped_to_unit_range() {
        let rubric = EvaluationRubric::new(vec![ScoringDimension {
            name: "test".to_string(),
            weight: 1.0,
            hard_threshold: 0.5,
            description: "test".to_string(),
        }]);
        let result = rubric.evaluate(&[("test", 1.5, "over max")]);
        assert!((result.scores[0].score - 1.0).abs() < f32::EPSILON);
        assert!(result.overall_pass);

        let result2 = rubric.evaluate(&[("test", -0.5, "under min")]);
        assert!((result2.scores[0].score - 0.0).abs() < f32::EPSILON);
        assert!(!result2.overall_pass);
    }

    #[test]
    fn evaluation_markdown_contains_issues() {
        let rubric = default_code_rubric();
        let result = rubric.evaluate(&[
            ("correctness", 0.5, "wrong"),
            ("functionality", 1.0, "ok"),
            ("code_quality", 0.85, "ok"),
            ("test_coverage", 0.7, "ok"),
        ]);
        let md = evaluation_to_markdown(&result);
        assert!(md.contains("# Evaluation Report"));
        assert!(md.contains("BELOW"));
        assert!(md.contains("Issues (must fix)"));
    }
}
