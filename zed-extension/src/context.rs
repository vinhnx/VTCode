/// Editor Context Management
///
/// Provides rich context from the Zed editor for VT Code commands.
/// Captures selection, file info, workspace structure, and environment.
use std::path::PathBuf;

/// Rich context from the editor for a VT Code command
#[derive(Debug, Clone)]
pub struct EditorContext {
    /// Currently active file path
    pub active_file: Option<PathBuf>,
    /// Active file language/extension
    pub language: Option<String>,
    /// Selected code text
    pub selection: Option<String>,
    /// Selection range (line, column) to (line, column)
    pub selection_range: Option<(usize, usize, usize, usize)>,
    /// Workspace root directory
    pub workspace_root: Option<PathBuf>,
    /// Open file paths
    pub open_files: Vec<PathBuf>,
    /// Current cursor position (line, column)
    pub cursor_position: Option<(usize, usize)>,
}

impl EditorContext {
    /// Create a new empty context
    pub fn new() -> Self {
        Self {
            active_file: None,
            language: None,
            selection: None,
            selection_range: None,
            workspace_root: None,
            open_files: Vec::new(),
            cursor_position: None,
        }
    }

    /// Check if there's an active selection
    pub fn has_selection(&self) -> bool {
        self.selection.is_some() && !self.selection.as_ref().unwrap().is_empty()
    }

    /// Get file extension from active file
    pub fn file_extension(&self) -> Option<String> {
        self.active_file.as_ref().and_then(|p| {
            p.extension()
                .and_then(|e| e.to_str())
                .map(|s| s.to_string())
        })
    }

    /// Get the language, preferring explicit language over extension
    pub fn get_language(&self) -> Option<String> {
        self.language.clone().or_else(|| self.file_extension())
    }

    /// Get the relative path from workspace root
    pub fn relative_file_path(&self) -> Option<PathBuf> {
        match (&self.active_file, &self.workspace_root) {
            (Some(file), Some(root)) => file.strip_prefix(root).ok().map(|p| p.to_path_buf()),
            _ => None,
        }
    }

    /// Build a context summary for logging
    pub fn summary(&self) -> String {
        let file = self
            .relative_file_path()
            .and_then(|p| p.to_str().map(|s| s.to_string()))
            .unwrap_or_else(|| "unknown".to_string());
        let lang = self.get_language().unwrap_or_default();
        let selection_size = self.selection.as_ref().map(|s| s.len()).unwrap_or(0);

        format!(
            "file: {}, language: {}, selection_size: {} bytes",
            file, lang, selection_size
        )
    }
}

impl Default for EditorContext {
    fn default() -> Self {
        Self::new()
    }
}

/// Diagnostic result from VT Code analysis
#[derive(Debug, Clone)]
pub struct Diagnostic {
    /// Severity: error, warning, info
    pub severity: DiagnosticSeverity,
    /// Message text
    pub message: String,
    /// File path
    pub file: PathBuf,
    /// Line number (0-based)
    pub line: usize,
    /// Column number (0-based)
    pub column: usize,
    /// Suggested fix (if available)
    pub suggested_fix: Option<String>,
}

/// Severity level for diagnostics
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticSeverity {
    Error,
    Warning,
    Information,
}

impl DiagnosticSeverity {
    pub fn as_str(&self) -> &'static str {
        match self {
            DiagnosticSeverity::Error => "error",
            DiagnosticSeverity::Warning => "warning",
            DiagnosticSeverity::Information => "info",
        }
    }
}

impl Diagnostic {
    /// Create a new diagnostic
    pub fn new(
        severity: DiagnosticSeverity,
        message: String,
        file: PathBuf,
        line: usize,
        column: usize,
    ) -> Self {
        Self {
            severity,
            message,
            file,
            line,
            column,
            suggested_fix: None,
        }
    }

    /// Add a suggested fix
    pub fn with_fix(mut self, fix: String) -> Self {
        self.suggested_fix = Some(fix);
        self
    }

    /// Format as a readable string
    pub fn format(&self) -> String {
        format!(
            "{}:{}:{} [{}] {}",
            self.file.display(),
            self.line + 1,
            self.column + 1,
            self.severity.as_str(),
            self.message
        )
    }
}

/// Quick fix action
#[derive(Debug, Clone)]
pub struct QuickFix {
    /// Title of the fix
    pub title: String,
    /// Description of what the fix does
    pub description: Option<String>,
    /// Code replacement
    pub replacement: String,
    /// File to apply fix to
    pub file: PathBuf,
    /// Range to replace (start_line, start_col, end_line, end_col)
    pub range: (usize, usize, usize, usize),
}

impl QuickFix {
    /// Create a new quick fix
    pub fn new(
        title: String,
        replacement: String,
        file: PathBuf,
        range: (usize, usize, usize, usize),
    ) -> Self {
        Self {
            title,
            description: None,
            replacement,
            file,
            range,
        }
    }

    /// Add a description
    pub fn with_description(mut self, desc: String) -> Self {
        self.description = Some(desc);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_editor_context_creation() {
        let ctx = EditorContext::new();
        assert!(!ctx.has_selection());
        assert!(ctx.active_file.is_none());
    }

    #[test]
    fn test_has_selection() {
        let mut ctx = EditorContext::new();
        assert!(!ctx.has_selection());

        ctx.selection = Some("code".to_string());
        assert!(ctx.has_selection());

        ctx.selection = Some(String::new());
        assert!(!ctx.has_selection());
    }

    #[test]
    fn test_file_extension() {
        let mut ctx = EditorContext::new();
        ctx.active_file = Some(PathBuf::from("test.rs"));
        assert_eq!(ctx.file_extension(), Some("rs".to_string()));
    }

    #[test]
    fn test_get_language_priority() {
        let mut ctx = EditorContext::new();
        ctx.active_file = Some(PathBuf::from("test.rs"));

        // File extension as fallback
        assert_eq!(ctx.get_language(), Some("rs".to_string()));

        // Explicit language takes priority
        ctx.language = Some("rust".to_string());
        assert_eq!(ctx.get_language(), Some("rust".to_string()));
    }

    #[test]
    fn test_relative_file_path() {
        let mut ctx = EditorContext::new();
        ctx.workspace_root = Some(PathBuf::from("/workspace"));
        ctx.active_file = Some(PathBuf::from("/workspace/src/main.rs"));

        let rel = ctx.relative_file_path();
        assert_eq!(rel, Some(PathBuf::from("src/main.rs")));
    }

    #[test]
    fn test_context_summary() {
        let mut ctx = EditorContext::new();
        ctx.active_file = Some(PathBuf::from("/workspace/test.rs"));
        ctx.workspace_root = Some(PathBuf::from("/workspace"));
        ctx.language = Some("rust".to_string());
        ctx.selection = Some("let x = 5;".to_string());

        let summary = ctx.summary();
        assert!(
            summary.contains("test.rs"),
            "Summary should contain 'test.rs': {}",
            summary
        );
        assert!(
            summary.contains("rust"),
            "Summary should contain 'rust': {}",
            summary
        );
        assert!(
            summary.contains("10 bytes"),
            "Summary should contain '10 bytes': {}",
            summary
        );
    }

    #[test]
    fn test_diagnostic_creation() {
        let diag = Diagnostic::new(
            DiagnosticSeverity::Error,
            "Undefined variable".to_string(),
            PathBuf::from("main.rs"),
            5,
            10,
        );

        assert_eq!(diag.severity, DiagnosticSeverity::Error);
        assert!(diag.suggested_fix.is_none());
    }

    #[test]
    fn test_diagnostic_with_fix() {
        let diag = Diagnostic::new(
            DiagnosticSeverity::Warning,
            "Unused import".to_string(),
            PathBuf::from("main.rs"),
            1,
            0,
        )
        .with_fix("Remove import".to_string());

        assert_eq!(diag.severity, DiagnosticSeverity::Warning);
        assert!(diag.suggested_fix.is_some());
    }

    #[test]
    fn test_diagnostic_format() {
        let diag = Diagnostic::new(
            DiagnosticSeverity::Error,
            "Test error".to_string(),
            PathBuf::from("test.rs"),
            0,
            0,
        );

        let formatted = diag.format();
        assert!(formatted.contains("test.rs"));
        assert!(formatted.contains("[error]"));
        assert!(formatted.contains("Test error"));
    }

    #[test]
    fn test_quick_fix_creation() {
        let fix = QuickFix::new(
            "Fix typo".to_string(),
            "correct".to_string(),
            PathBuf::from("main.rs"),
            (10, 5, 10, 12),
        );

        assert_eq!(fix.title, "Fix typo");
        assert!(fix.description.is_none());
    }

    #[test]
    fn test_quick_fix_with_description() {
        let fix = QuickFix::new(
            "Fix typo".to_string(),
            "correct".to_string(),
            PathBuf::from("main.rs"),
            (10, 5, 10, 12),
        )
        .with_description("Changes 'incorect' to 'correct'".to_string());

        assert_eq!(
            fix.description,
            Some("Changes 'incorect' to 'correct'".to_string())
        );
    }

    #[test]
    fn test_diagnostic_severity_str() {
        assert_eq!(DiagnosticSeverity::Error.as_str(), "error");
        assert_eq!(DiagnosticSeverity::Warning.as_str(), "warning");
        assert_eq!(DiagnosticSeverity::Information.as_str(), "info");
    }
}
