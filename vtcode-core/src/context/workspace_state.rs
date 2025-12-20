//! Workspace state tracking for vibe coding support
//!
//! Tracks file activity, edits, and value changes to provide context for
//! lazy/vague user requests.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::time::Instant;

/// Maximum number of recent files to track
const MAX_RECENT_FILES: usize = 20;

/// Maximum number of recent changes to track
const MAX_RECENT_CHANGES: usize = 50;

/// Maximum number of hot files to track
const MAX_HOT_FILES: usize = 10;

/// Type of file activity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ActivityType {
    Read,
    Edit,
    Create,
    Delete,
}

/// A file activity event
#[derive(Debug, Clone)]
pub struct FileActivity {
    pub path: PathBuf,
    pub action: ActivityType,
    pub timestamp: Instant,
    pub related_terms: Vec<String>,
}

/// A file change event
#[derive(Debug, Clone)]
pub struct FileChange {
    pub path: PathBuf,
    pub content_before: Option<String>,
    pub content_after: String,
    pub timestamp: Instant,
}

/// History of a value over time
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValueHistory {
    pub key: String,
    pub current: String,
    pub previous: Vec<String>,
    pub file: PathBuf,
    pub line: usize,
}

/// An unresolved reference that needs context
#[derive(Debug, Clone)]
pub struct UnresolvedReference {
    pub reference: String,
    pub context: String,
    pub timestamp: Instant,
}

/// Relative operation types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelativeOp {
    Half,
    Double,
    Increase(u32),  // Increase by percentage
    Decrease(u32),  // Decrease by percentage
}

/// Tracks workspace state for contextual inference
pub struct WorkspaceState {
    /// Recent file activities (bounded queue)
    recent_files: VecDeque<FileActivity>,

    /// Files currently open/being edited
    open_files: HashSet<PathBuf>,

    /// Recent changes
    recent_changes: Vec<FileChange>,

    /// Hot files (most frequently edited)
    hot_files: Vec<(PathBuf, usize)>,

    /// Value snapshots for inference
    value_snapshots: HashMap<String, ValueHistory>,

    /// Last user intent/request
    last_user_intent: Option<String>,

    /// Pending unresolved references
    pending_references: Vec<UnresolvedReference>,
}

impl Default for WorkspaceState {
    fn default() -> Self {
        Self::new()
    }
}

impl WorkspaceState {
    /// Create a new workspace state tracker
    pub fn new() -> Self {
        Self {
            recent_files: VecDeque::with_capacity(MAX_RECENT_FILES),
            open_files: HashSet::new(),
            recent_changes: Vec::with_capacity(MAX_RECENT_CHANGES),
            hot_files: Vec::with_capacity(MAX_HOT_FILES),
            value_snapshots: HashMap::new(),
            last_user_intent: None,
            pending_references: Vec::new(),
        }
    }

    /// Record a file access
    pub fn record_file_access(&mut self, path: &Path, access_type: ActivityType) {
        let activity = FileActivity {
            path: path.to_path_buf(),
            action: access_type,
            timestamp: Instant::now(),
            related_terms: self.extract_terms_from_path(path),
        };

        self.recent_files.push_back(activity);

        // Keep bounded
        while self.recent_files.len() > MAX_RECENT_FILES {
            self.recent_files.pop_front();
        }

        // Update hot files on edit
        if access_type == ActivityType::Edit {
            self.update_hot_files(path);
        }
    }

    /// Update hot files list with edit count
    fn update_hot_files(&mut self, path: &Path) {
        // Find existing entry
        if let Some(entry) = self.hot_files.iter_mut().find(|(p, _)| p == path) {
            entry.1 += 1;
        } else {
            self.hot_files.push((path.to_path_buf(), 1));
        }

        // Sort by edit count (descending)
        self.hot_files.sort_by(|a, b| b.1.cmp(&a.1));

        // Keep bounded
        self.hot_files.truncate(MAX_HOT_FILES);
    }

    /// Extract terms from file path (for entity matching)
    fn extract_terms_from_path(&self, path: &Path) -> Vec<String> {
        let mut terms = Vec::new();

        // Extract filename without extension
        if let Some(file_stem) = path.file_stem() {
            if let Some(name) = file_stem.to_str() {
                // Split on common separators
                for term in name.split(|c: char| !c.is_alphanumeric()) {
                    if !term.is_empty() {
                        terms.push(term.to_lowercase());
                    }
                }
            }
        }

        terms
    }

    /// Infer reference target from vague term
    pub fn infer_reference_target(&self, vague_term: &str) -> Option<PathBuf> {
        let term_lower = vague_term.to_lowercase();

        // Priority: most recent file containing term
        self.recent_files
            .iter()
            .rev()
            .find(|activity| activity.related_terms.contains(&term_lower))
            .map(|activity| activity.path.clone())
    }

    /// Resolve relative value expression
    pub fn resolve_relative_value(&self, expression: &str) -> Option<String> {
        let op = self.parse_relative_expression(expression)?;

        match op {
            RelativeOp::Half => {
                let current = self.get_recent_numeric_value()?;
                Some(format!("{}", current / 2.0))
            }
            RelativeOp::Double => {
                let current = self.get_recent_numeric_value()?;
                Some(format!("{}", current * 2.0))
            }
            RelativeOp::Increase(pct) => {
                let current = self.get_recent_numeric_value()?;
                let multiplier = 1.0 + (pct as f64 / 100.0);
                Some(format!("{}", current * multiplier))
            }
            RelativeOp::Decrease(pct) => {
                let current = self.get_recent_numeric_value()?;
                let multiplier = 1.0 - (pct as f64 / 100.0);
                Some(format!("{}", current * multiplier))
            }
        }
    }

    /// Parse relative expression to operation
    fn parse_relative_expression(&self, expression: &str) -> Option<RelativeOp> {
        let expr_lower = expression.to_lowercase();

        if expr_lower.contains("half") || expr_lower.contains("by 2") {
            return Some(RelativeOp::Half);
        }

        if expr_lower.contains("double") || expr_lower.contains("twice") {
            return Some(RelativeOp::Double);
        }

        // Try to extract percentage
        if let Some(pct) = self.extract_percentage(&expr_lower) {
            if expr_lower.contains("increase") {
                return Some(RelativeOp::Increase(pct));
            }
            if expr_lower.contains("decrease") || expr_lower.contains("reduce") {
                return Some(RelativeOp::Decrease(pct));
            }
        }

        None
    }

    /// Extract percentage from string
    fn extract_percentage(&self, text: &str) -> Option<u32> {
        // Look for patterns like "20%", "20 percent", etc.
        for word in text.split_whitespace() {
            if let Some(num_str) = word.strip_suffix('%') {
                if let Ok(num) = num_str.parse::<u32>() {
                    return Some(num);
                }
            }
            if let Ok(num) = word.parse::<u32>() {
                return Some(num);
            }
        }
        None
    }

    /// Get most recent numeric value from edits
    fn get_recent_numeric_value(&self) -> Option<f64> {
        // Look at recent changes for numeric values
        for change in self.recent_changes.iter().rev() {
            if let Some(value) = self.extract_numeric_value(&change.content_after) {
                return Some(value);
            }
        }

        // Fallback to value snapshots
        if let Some((_, history)) = self.value_snapshots.iter().next() {
            return self.parse_value_string(&history.current);
        }

        None
    }

    /// Extract numeric value from content
    fn extract_numeric_value(&self, content: &str) -> Option<f64> {
        // Try multiple patterns in order of specificity
        for line in content.lines().rev().take(10) {
            // CSS patterns: padding: 16px, width: 50%, etc.
            if let Some(value) = self.extract_css_value(line) {
                return Some(value);
            }

            // JSON/TOML patterns: "timeout": 5000, timeout = 30
            if let Some(value) = self.extract_config_value(line) {
                return Some(value);
            }

            // Programming language patterns: padding = 16, const size = 20
            if let Some(value) = self.extract_code_value(line) {
                return Some(value);
            }
        }

        None
    }

    /// Extract numeric value from config files (JSON, TOML, YAML)
    fn extract_config_value(&self, line: &str) -> Option<f64> {
        // JSON: "key": 123 or "key": "123px"
        // TOML: key = 123
        // YAML: key: 123

        if let Some(colon_pos) = line.find(':').or_else(|| line.find('=')) {
            let value_part = line[colon_pos + 1..].trim();

            // Remove quotes and commas
            let mut cleaned = value_part
                .trim_matches(',')
                .trim_matches('"')
                .trim_matches('\'');

            // Try to strip common unit suffixes
            for suffix in &["px", "rem", "em", "ms", "s", "pt"] {
                if let Some(stripped) = cleaned.strip_suffix(suffix) {
                    cleaned = stripped;
                    break;
                }
            }

            if let Ok(num) = cleaned.parse::<f64>() {
                return Some(num);
            }
        }

        None
    }

    /// Extract numeric value from code (Python, JavaScript, Rust, etc.)
    fn extract_code_value(&self, line: &str) -> Option<f64> {
        // Patterns: const x = 10, let y = 20, var z = 30, x = 40

        if let Some(eq_pos) = line.find('=') {
            let value_part = line[eq_pos + 1..].trim();

            // Extract first numeric token
            for word in value_part.split_whitespace() {
                let cleaned = word
                    .trim_matches(';')
                    .trim_matches(',')
                    .trim_end_matches("px")
                    .trim_end_matches("rem");

                if let Ok(num) = cleaned.parse::<f64>() {
                    return Some(num);
                }
            }
        }

        None
    }

    /// Extract numeric value from CSS line
    fn extract_css_value(&self, line: &str) -> Option<f64> {
        // Look for patterns like "padding: 16px"
        if let Some(colon_pos) = line.find(':') {
            let value_part = line[colon_pos + 1..].trim();

            // Extract number (handling px, rem, %, etc.)
            for word in value_part.split_whitespace() {
                // Strip semicolon first
                let mut num_str = word.trim_end_matches(';');

                // Try to strip common CSS units (use strip_suffix for literal matching)
                for suffix in &["px", "rem", "em", "%", "pt", "vh", "vw"] {
                    if let Some(stripped) = num_str.strip_suffix(suffix) {
                        num_str = stripped;
                        break;
                    }
                }

                if let Ok(num) = num_str.parse::<f64>() {
                    return Some(num);
                }
            }
        }

        None
    }

    /// Parse value string to number
    fn parse_value_string(&self, value: &str) -> Option<f64> {
        let mut num_str = value;

        // Try to strip common units
        for suffix in &["px", "rem", "em", "%", "pt", "ms", "s"] {
            if let Some(stripped) = num_str.strip_suffix(suffix) {
                num_str = stripped;
                break;
            }
        }

        num_str.parse::<f64>().ok()
    }

    /// Record a file change
    pub fn record_change(&mut self, path: PathBuf, content_before: Option<String>, content_after: String) {
        let change = FileChange {
            path,
            content_before,
            content_after,
            timestamp: Instant::now(),
        };

        self.recent_changes.push(change);

        // Keep bounded
        while self.recent_changes.len() > MAX_RECENT_CHANGES {
            self.recent_changes.remove(0);
        }
    }

    /// Record value snapshot
    pub fn record_value(&mut self, key: String, value: String, file: PathBuf, line: usize) {
        if let Some(history) = self.value_snapshots.get_mut(&key) {
            // Move current to previous
            history.previous.push(history.current.clone());
            history.current = value;
            history.file = file;
            history.line = line;

            // Keep bounded
            if history.previous.len() > 10 {
                history.previous.remove(0);
            }
        } else {
            // Create new history
            self.value_snapshots.insert(
                key.clone(),
                ValueHistory {
                    key,
                    current: value,
                    previous: Vec::new(),
                    file,
                    line,
                },
            );
        }
    }

    /// Set last user intent
    pub fn set_user_intent(&mut self, intent: String) {
        self.last_user_intent = Some(intent);
    }

    /// Get last user intent
    pub fn last_user_intent(&self) -> Option<&str> {
        self.last_user_intent.as_deref()
    }

    /// Get recent files (up to N)
    pub fn recent_files(&self, count: usize) -> Vec<&FileActivity> {
        self.recent_files
            .iter()
            .rev()
            .take(count)
            .collect()
    }

    /// Check if file was recently accessed
    pub fn was_recently_accessed(&self, path: &Path) -> bool {
        self.recent_files
            .iter()
            .any(|activity| activity.path == path)
    }

    /// Get hot files (most edited)
    pub fn hot_files(&self) -> &[(PathBuf, usize)] {
        &self.hot_files
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_relative_expression_half() {
        let state = WorkspaceState::new();
        assert_eq!(
            state.parse_relative_expression("by half"),
            Some(RelativeOp::Half)
        );
        assert_eq!(
            state.parse_relative_expression("divide by 2"),
            Some(RelativeOp::Half)
        );
    }

    #[test]
    fn test_parse_relative_expression_double() {
        let state = WorkspaceState::new();
        assert_eq!(
            state.parse_relative_expression("double it"),
            Some(RelativeOp::Double)
        );
        assert_eq!(
            state.parse_relative_expression("twice as much"),
            Some(RelativeOp::Double)
        );
    }

    #[test]
    fn test_parse_relative_expression_percentage() {
        let state = WorkspaceState::new();
        assert_eq!(
            state.parse_relative_expression("increase by 20%"),
            Some(RelativeOp::Increase(20))
        );
        assert_eq!(
            state.parse_relative_expression("decrease by 50%"),
            Some(RelativeOp::Decrease(50))
        );
    }

    #[test]
    fn test_extract_css_value() {
        let state = WorkspaceState::new();
        assert_eq!(
            state.extract_css_value("  padding: 16px;"),
            Some(16.0)
        );
        assert_eq!(
            state.extract_css_value("  width: 50%;"),
            Some(50.0)
        );
        assert_eq!(
            state.extract_css_value("  margin: 1.5rem;"),
            Some(1.5)
        );
    }

    #[test]
    fn test_record_file_access() {
        let mut state = WorkspaceState::new();
        let path = PathBuf::from("src/components/Sidebar.tsx");

        state.record_file_access(&path, ActivityType::Edit);

        assert_eq!(state.recent_files.len(), 1);
        assert!(state.was_recently_accessed(&path));
    }

    #[test]
    fn test_hot_files_tracking() {
        let mut state = WorkspaceState::new();
        let path1 = PathBuf::from("src/App.tsx");
        let path2 = PathBuf::from("src/Sidebar.tsx");

        // Edit path1 three times
        state.record_file_access(&path1, ActivityType::Edit);
        state.record_file_access(&path1, ActivityType::Edit);
        state.record_file_access(&path1, ActivityType::Edit);

        // Edit path2 once
        state.record_file_access(&path2, ActivityType::Edit);

        let hot = state.hot_files();
        assert_eq!(hot.len(), 2);
        assert_eq!(hot[0].0, path1); // Most edited
        assert_eq!(hot[0].1, 3);
        assert_eq!(hot[1].0, path2);
        assert_eq!(hot[1].1, 1);
    }

    // Phase 4: Enhanced value extraction tests
    #[test]
    fn test_extract_config_value_json() {
        let state = WorkspaceState::new();
        assert_eq!(
            state.extract_config_value(r#"  "timeout": 5000,"#),
            Some(5000.0)
        );
        assert_eq!(
            state.extract_config_value(r#"  "padding": "16px","#),
            Some(16.0)
        );
    }

    #[test]
    fn test_extract_config_value_toml() {
        let state = WorkspaceState::new();
        assert_eq!(
            state.extract_config_value("timeout = 30"),
            Some(30.0)
        );
        assert_eq!(
            state.extract_config_value("max_retries = 5"),
            Some(5.0)
        );
    }

    #[test]
    fn test_extract_code_value_javascript() {
        let state = WorkspaceState::new();
        assert_eq!(
            state.extract_code_value("const padding = 16;"),
            Some(16.0)
        );
        assert_eq!(
            state.extract_code_value("let width = 320;"),
            Some(320.0)
        );
    }

    #[test]
    fn test_extract_code_value_python() {
        let state = WorkspaceState::new();
        assert_eq!(
            state.extract_code_value("padding = 24"),
            Some(24.0)
        );
        assert_eq!(
            state.extract_code_value("TIMEOUT = 1000"),
            Some(1000.0)
        );
    }

    #[test]
    fn test_extract_code_value_rust() {
        let state = WorkspaceState::new();
        assert_eq!(
            state.extract_code_value("let size = 42;"),
            Some(42.0)
        );
        assert_eq!(
            state.extract_code_value("const MAX_SIZE: usize = 100;"),
            Some(100.0)
        );
    }
}
