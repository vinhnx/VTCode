use std::fs;
use std::path::Path;
use walkdir::WalkDir;

/// Complexity analysis results
#[derive(Debug, Clone)]
pub struct ComplexityResult {
    pub cyclomatic_complexity: f64,
    pub cognitive_complexity: f64,
    pub lines_of_code: usize,
    pub maintainability_index: f64,
}

/// Complexity analyzer for code quality metrics
pub struct ComplexityAnalyzer;

impl ComplexityAnalyzer {
    pub fn new() -> Self {
        Self
    }

    /// Analyze complexity of a source file
    pub fn analyze_file(&self, _file_path: &Path, source: &str) -> ComplexityResult {
        // Calculate actual complexity metrics
        let lines_of_code = source.lines().count();

        // Calculate cyclomatic complexity (simplified)
        let cyclomatic_complexity = self.calculate_cyclomatic_complexity(source);

        // Calculate cognitive complexity (simplified)
        let cognitive_complexity = self.calculate_cognitive_complexity(source);

        // Calculate maintainability index
        let maintainability_index =
            self.calculate_maintainability_index(cyclomatic_complexity, lines_of_code, source);

        ComplexityResult {
            cyclomatic_complexity,
            cognitive_complexity,
            lines_of_code,
            maintainability_index,
        }
    }

    /// Analyze complexity of a directory
    pub fn analyze_directory(&self, dir_path: &Path) -> Vec<ComplexityResult> {
        let mut results = Vec::new();

        // Walk the directory to analyze all source files
        for entry in WalkDir::new(dir_path)
            .follow_links(true)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            if entry.file_type().is_file()
                && let Some(ext) = entry.path().extension()
            {
                // Check if this is a source code file
                let source_extensions = ["rs", "js", "ts", "py", "java", "cpp", "c", "go"];
                if source_extensions.contains(&ext.to_str().unwrap_or(""))
                    && let Ok(content) = fs::read_to_string(entry.path())
                {
                    let result = self.analyze_file(entry.path(), &content);
                    results.push(result);
                }
            }
        }

        results
    }

    /// Calculate cyclomatic complexity (simplified implementation)
    fn calculate_cyclomatic_complexity(&self, source: &str) -> f64 {
        let mut complexity = 1.0; // Base complexity

        // Count decision points
        for line in source.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("if ")
                || trimmed.starts_with("while ")
                || trimmed.starts_with("for ")
                || trimmed.starts_with("match ")
                || trimmed.contains(" && ")
                || trimmed.contains(" || ")
                || trimmed.starts_with("case ")
                || trimmed.starts_with("catch ")
            {
                complexity += 1.0;
            }
        }

        complexity
    }

    /// Calculate cognitive complexity (simplified implementation)
    fn calculate_cognitive_complexity(&self, source: &str) -> f64 {
        let mut complexity = 0.0;

        // Count nesting levels and complex structures
        let mut nesting_level: i32 = 0;
        for line in source.lines() {
            let trimmed = line.trim();

            // Increase nesting level for control structures
            if trimmed.starts_with("if ")
                || trimmed.starts_with("while ")
                || trimmed.starts_with("for ")
                || trimmed.starts_with("match ")
            {
                nesting_level += 1;
                complexity += nesting_level as f64;
            }

            // Decrease nesting level for closing braces/keywords
            if trimmed == "}" || trimmed == "end" {
                nesting_level = nesting_level.saturating_sub(1);
            }
        }

        complexity
    }

    /// Calculate maintainability index
    fn calculate_maintainability_index(
        &self,
        cyclomatic_complexity: f64,
        lines_of_code: usize,
        source: &str,
    ) -> f64 {
        // Count Halstead volume approximation (simplified)
        let halstead_volume = source.chars().count() as f64;

        // Maintainability index formula (simplified)
        let mi = 171.0
            - 5.2 * halstead_volume.ln()
            - 0.23 * cyclomatic_complexity
            - 16.2 * (lines_of_code as f64).ln();

        // Normalize to 0-100 scale
        mi.clamp(0.0, 100.0)
    }
}

impl Default for ComplexityAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}
