/// Editor Integration
///
/// Provides deep integration with Zed editor UI including status bar,
/// inline diagnostics, and quick fixes.
use crate::context::{Diagnostic, DiagnosticSeverity, EditorContext, QuickFix};
use std::sync::{Arc, Mutex};

/// Status indicator for the status bar
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusIndicator {
    /// VT Code CLI is ready
    Ready,
    /// VT Code CLI is executing a command
    Executing,
    /// VT Code CLI is unavailable
    Unavailable,
    /// An error occurred
    Error,
}

impl StatusIndicator {
    pub fn symbol(&self) -> &'static str {
        match self {
            StatusIndicator::Ready => "●",
            StatusIndicator::Executing => "◐",
            StatusIndicator::Unavailable => "○",
            StatusIndicator::Error => "[E]",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            StatusIndicator::Ready => "VT Code Ready",
            StatusIndicator::Executing => "VT Code Running...",
            StatusIndicator::Unavailable => "VT Code Unavailable",
            StatusIndicator::Error => "VT Code Error",
        }
    }
}

/// Editor state management
pub struct EditorState {
    /// Current status
    status: Arc<Mutex<StatusIndicator>>,
    /// Active editor context
    context: Arc<Mutex<EditorContext>>,
    /// Current diagnostics
    diagnostics: Arc<Mutex<Vec<Diagnostic>>>,
    /// Available quick fixes
    quick_fixes: Arc<Mutex<Vec<QuickFix>>>,
}

impl EditorState {
    /// Create a new editor state
    pub fn new() -> Self {
        Self {
            status: Arc::new(Mutex::new(StatusIndicator::Unavailable)),
            context: Arc::new(Mutex::new(EditorContext::new())),
            diagnostics: Arc::new(Mutex::new(Vec::new())),
            quick_fixes: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Update the status indicator
    pub fn set_status(&self, status: StatusIndicator) -> Result<(), String> {
        self.status
            .lock()
            .map(|mut s| *s = status)
            .map_err(|e| format!("Failed to lock status: {}", e))
    }

    /// Get the current status
    pub fn get_status(&self) -> Result<StatusIndicator, String> {
        self.status
            .lock()
            .map(|s| *s)
            .map_err(|e| format!("Failed to lock status: {}", e))
    }

    /// Update the editor context
    pub fn set_context(&self, context: EditorContext) -> Result<(), String> {
        self.context
            .lock()
            .map(|mut c| *c = context)
            .map_err(|e| format!("Failed to lock context: {}", e))
    }

    /// Get the current editor context
    pub fn get_context(&self) -> Result<EditorContext, String> {
        self.context
            .lock()
            .map(|c| c.clone())
            .map_err(|e| format!("Failed to lock context: {}", e))
    }

    /// Add a diagnostic
    pub fn add_diagnostic(&self, diagnostic: Diagnostic) -> Result<(), String> {
        self.diagnostics
            .lock()
            .map(|mut diags| diags.push(diagnostic))
            .map_err(|e| format!("Failed to lock diagnostics: {}", e))
    }

    /// Clear all diagnostics
    pub fn clear_diagnostics(&self) -> Result<(), String> {
        self.diagnostics
            .lock()
            .map(|mut diags| diags.clear())
            .map_err(|e| format!("Failed to lock diagnostics: {}", e))
    }

    /// Get all diagnostics
    pub fn get_diagnostics(&self) -> Result<Vec<Diagnostic>, String> {
        self.diagnostics
            .lock()
            .map(|diags| diags.clone())
            .map_err(|e| format!("Failed to lock diagnostics: {}", e))
    }

    /// Get diagnostics count
    pub fn diagnostic_count(&self) -> usize {
        self.diagnostics
            .lock()
            .map(|diags| diags.len())
            .unwrap_or(0)
    }

    /// Add a quick fix
    pub fn add_quick_fix(&self, fix: QuickFix) -> Result<(), String> {
        self.quick_fixes
            .lock()
            .map(|mut fixes| fixes.push(fix))
            .map_err(|e| format!("Failed to lock quick fixes: {}", e))
    }

    /// Get all quick fixes
    pub fn get_quick_fixes(&self) -> Result<Vec<QuickFix>, String> {
        self.quick_fixes
            .lock()
            .map(|fixes| fixes.clone())
            .map_err(|e| format!("Failed to lock quick fixes: {}", e))
    }

    /// Clear quick fixes
    pub fn clear_quick_fixes(&self) -> Result<(), String> {
        self.quick_fixes
            .lock()
            .map(|mut fixes| fixes.clear())
            .map_err(|e| format!("Failed to lock quick fixes: {}", e))
    }

    /// Get quick fixes count
    pub fn quick_fix_count(&self) -> usize {
        self.quick_fixes
            .lock()
            .map(|fixes| fixes.len())
            .unwrap_or(0)
    }

    /// Get a diagnostic summary
    pub fn diagnostic_summary(&self) -> Result<String, String> {
        let diags = self.get_diagnostics()?;
        if diags.is_empty() {
            return Ok("No issues found".to_string());
        }

        let errors = diags
            .iter()
            .filter(|d| d.severity == DiagnosticSeverity::Error)
            .count();
        let warnings = diags
            .iter()
            .filter(|d| d.severity == DiagnosticSeverity::Warning)
            .count();
        let infos = diags
            .iter()
            .filter(|d| d.severity == DiagnosticSeverity::Information)
            .count();

        Ok(format!(
            "{} errors, {} warnings, {} info",
            errors, warnings, infos
        ))
    }
}

impl Default for EditorState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_status_indicator_symbols() {
        assert_eq!(StatusIndicator::Ready.symbol(), "●");
        assert_eq!(StatusIndicator::Executing.symbol(), "◐");
        assert_eq!(StatusIndicator::Unavailable.symbol(), "○");
        assert_eq!(StatusIndicator::Error.symbol(), "[E]");
    }

    #[test]
    fn test_status_indicator_labels() {
        assert_eq!(StatusIndicator::Ready.label(), "VT Code Ready");
        assert_eq!(StatusIndicator::Executing.label(), "VT Code Running...");
        assert_eq!(StatusIndicator::Unavailable.label(), "VT Code Unavailable");
        assert_eq!(StatusIndicator::Error.label(), "VT Code Error");
    }

    #[test]
    fn test_editor_state_creation() {
        let state = EditorState::new();
        assert_eq!(state.get_status().unwrap(), StatusIndicator::Unavailable);
        assert_eq!(state.diagnostic_count(), 0);
        assert_eq!(state.quick_fix_count(), 0);
    }

    #[test]
    fn test_set_and_get_status() {
        let state = EditorState::new();
        state.set_status(StatusIndicator::Ready).unwrap();
        assert_eq!(state.get_status().unwrap(), StatusIndicator::Ready);

        state.set_status(StatusIndicator::Executing).unwrap();
        assert_eq!(state.get_status().unwrap(), StatusIndicator::Executing);
    }

    #[test]
    fn test_set_and_get_context() {
        let state = EditorState::new();
        let mut ctx = EditorContext::new();
        ctx.language = Some("rust".to_string());

        state.set_context(ctx.clone()).unwrap();
        let retrieved = state.get_context().unwrap();
        assert_eq!(retrieved.language, Some("rust".to_string()));
    }

    #[test]
    fn test_diagnostics_management() {
        let state = EditorState::new();
        assert_eq!(state.diagnostic_count(), 0);

        let diag = Diagnostic::new(
            DiagnosticSeverity::Error,
            "Test error".to_string(),
            PathBuf::from("test.rs"),
            0,
            0,
        );

        state.add_diagnostic(diag).unwrap();
        assert_eq!(state.diagnostic_count(), 1);

        state.clear_diagnostics().unwrap();
        assert_eq!(state.diagnostic_count(), 0);
    }

    #[test]
    fn test_quick_fixes_management() {
        let state = EditorState::new();
        assert_eq!(state.quick_fix_count(), 0);

        let fix = QuickFix::new(
            "Fix typo".to_string(),
            "correct".to_string(),
            PathBuf::from("main.rs"),
            (0, 0, 0, 5),
        );

        state.add_quick_fix(fix).unwrap();
        assert_eq!(state.quick_fix_count(), 1);

        state.clear_quick_fixes().unwrap();
        assert_eq!(state.quick_fix_count(), 0);
    }

    #[test]
    fn test_diagnostic_summary() {
        let state = EditorState::new();

        // Empty
        assert_eq!(state.diagnostic_summary().unwrap(), "No issues found");

        // Add diagnostics
        let error = Diagnostic::new(
            DiagnosticSeverity::Error,
            "Error".to_string(),
            PathBuf::from("test.rs"),
            0,
            0,
        );
        let warning = Diagnostic::new(
            DiagnosticSeverity::Warning,
            "Warning".to_string(),
            PathBuf::from("test.rs"),
            1,
            0,
        );

        state.add_diagnostic(error).unwrap();
        state.add_diagnostic(warning).unwrap();

        let summary = state.diagnostic_summary().unwrap();
        assert!(summary.contains("1 errors"));
        assert!(summary.contains("1 warnings"));
    }
}
