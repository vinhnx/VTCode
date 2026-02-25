use std::collections::HashMap;

/// Supported language for linting tools
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LanguageSupport {
    Rust,
    Python,
    JavaScript,
    TypeScript,
    Go,
    Java,
    Bash,
    Swift,
}


/// Linting tool configuration
#[derive(Debug, Clone)]
pub struct LintConfig {
    pub language: LanguageSupport,
    pub tool_name: String,
    pub command: Vec<String>,
    pub args: Vec<String>,
    pub severity_levels: HashMap<String, LintSeverity>,
    pub enabled: bool,
}

/// Lint result severity levels
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LintSeverity {
    Info,
    Warning,
    Error,
    Critical,
}

impl LintConfig {
    /// Helper to convert string slices to owned Strings
    fn vec_from(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    /// Create clippy configuration
    pub fn clippy() -> Self {
        Self {
            language: LanguageSupport::Rust,
            tool_name: "clippy".to_string(),
            command: Self::vec_from(&["cargo", "clippy"]),
            args: Self::vec_from(&["--", "-D", "warnings"]),
            severity_levels: HashMap::new(),
            enabled: true,
        }
    }

    /// Create ESLint configuration
    pub fn eslint() -> Self {
        Self {
            language: LanguageSupport::TypeScript,
            tool_name: "eslint".to_string(),
            command: Self::vec_from(&["eslint"]),
            args: Self::vec_from(&["--format", "json"]),
            severity_levels: HashMap::new(),
            enabled: true,
        }
    }

    /// Create pylint configuration
    pub fn pylint() -> Self {
        Self {
            language: LanguageSupport::Python,
            tool_name: "pylint".to_string(),
            command: Self::vec_from(&["pylint"]),
            args: Self::vec_from(&["--output-format", "json"]),
            severity_levels: HashMap::new(),
            enabled: true,
        }
    }
}
