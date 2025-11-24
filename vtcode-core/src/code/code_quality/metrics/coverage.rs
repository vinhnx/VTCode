use std::fs;
use std::path::Path;
use walkdir::WalkDir;

/// Coverage analysis results
#[derive(Debug, Clone)]
pub struct CoverageResult {
    pub line_coverage: f64,
    pub branch_coverage: f64,
    pub function_coverage: f64,
    pub total_lines: usize,
    pub covered_lines: usize,
}

/// Coverage analyzer for test coverage metrics
pub struct CoverageAnalyzer;

impl CoverageAnalyzer {
    pub fn new() -> Self {
        Self
    }

    /// Analyze test coverage for a project
    pub fn analyze_project(&self, project_path: &Path) -> CoverageResult {
        let report_path = std::env::var("COVERAGE_REPORT_PATH")
            .map(|p| project_path.join(p))
            .unwrap_or_else(|_| project_path.join("coverage/lcov.info"));

        if let Ok(report) = fs::read_to_string(&report_path) {
            let mut total_lines = 0;
            let mut covered_lines = 0;
            for line in report.lines() {
                if let Some(data) = line.strip_prefix("DA:") {
                    let parts: Vec<&str> = data.split(',').collect();
                    if parts.len() == 2 {
                        total_lines += 1;
                        if parts[1].trim() != "0" {
                            covered_lines += 1;
                        }
                    }
                }
            }

            let line_coverage = if total_lines > 0 {
                (covered_lines as f64 / total_lines as f64) * 100.0
            } else {
                100.0
            };
            let branch_coverage = (line_coverage * 0.8).min(100.0);
            let function_coverage = (line_coverage * 0.9).min(100.0);

            return CoverageResult {
                line_coverage,
                branch_coverage,
                function_coverage,
                total_lines,
                covered_lines,
            };
        }

        let mut total_lines = 0;
        let mut covered_lines = 0;
        let mut _total_files = 0;
        let mut _test_files = 0;

        for entry in WalkDir::new(project_path)
            .follow_links(true)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            if entry.file_type().is_file() {
                if let Some(ext) = entry.path().extension() {
                    let source_extensions = ["rs", "js", "ts", "py", "java", "cpp", "c", "go"];
                    if source_extensions.contains(&ext.to_str().unwrap_or("")) {
                        _total_files += 1;
                        if let Ok(content) = fs::read_to_string(entry.path()) {
                            let lines = content.lines().count();
                            total_lines += lines;
                            if self.has_test_file(entry.path()) {
                                covered_lines += lines;
                            }
                        }
                    }
                }
                if let Some(file_name) = entry.path().file_name().and_then(|n| n.to_str())
                    && (file_name.contains("test") || file_name.contains("spec"))
                {
                    _test_files += 1;
                }
            }
        }

        let line_coverage = if total_lines > 0 {
            (covered_lines as f64 / total_lines as f64) * 100.0
        } else {
            100.0
        };
        let branch_coverage = (line_coverage * 0.8).min(100.0);
        let function_coverage = (line_coverage * 0.9).min(100.0);

        CoverageResult {
            line_coverage,
            branch_coverage,
            function_coverage,
            total_lines,
            covered_lines,
        }
    }

    /// Check if a source file has a corresponding test file
    fn has_test_file(&self, source_path: &Path) -> bool {
        if let Some(file_name) = source_path.file_name().and_then(|n| n.to_str())
            && let Some(parent) = source_path.parent()
        {
            // Look for test files in the same directory
            for entry in WalkDir::new(parent)
                .max_depth(1)
                .into_iter()
                .filter_map(|e| e.ok())
            {
                if entry.file_type().is_file()
                    && let Some(test_file_name) = entry.path().file_name().and_then(|n| n.to_str())
                    && (test_file_name.contains("test") || test_file_name.contains("spec"))
                    && test_file_name.contains(file_name.split('.').next().unwrap_or(""))
                {
                    return true;
                }
            }
        }
        false
    }
}

impl Default for CoverageAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}
