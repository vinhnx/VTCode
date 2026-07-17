pub mod complexity;
pub mod coverage;

pub use complexity::ComplexityAnalyzer;
pub use coverage::CoverageAnalyzer;

/// Code quality metrics
#[derive(Debug, Clone, Default)]
pub struct QualityMetrics {
    pub total_files: usize,
    pub formatted_files: usize,
    pub lint_errors: usize,
    pub lint_warnings: usize,
    pub lint_info: usize,
    pub cyclomatic_complexity: f64,
    pub test_coverage: f64,
    pub maintainability_index: f64,
}

impl QualityMetrics {
    /// Calculate overall quality score (0-100)
    pub fn quality_score(&self) -> f64 {
        let format_score = if self.total_files > 0 {
            (self.formatted_files as f64 / self.total_files as f64) * 25.0
        } else {
            25.0
        };

        let lint_score = if self.lint_errors == 0 {
            25.0
        } else {
            (25.0 * (1.0 - (self.lint_errors as f64 / 10.0).min(1.0))).max(0.0)
        };

        let complexity_score =
            (25.0 * (1.0 - (self.cyclomatic_complexity / 20.0).min(1.0))).max(0.0);
        let coverage_score = self.test_coverage * 0.25;

        format_score + lint_score + complexity_score + coverage_score
    }
}
