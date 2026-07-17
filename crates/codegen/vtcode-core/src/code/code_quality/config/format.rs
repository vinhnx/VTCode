/// Supported language for formatting tools
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

/// Code formatting tool configuration
#[derive(Debug, Clone)]
pub struct FormatConfig {
    pub language: LanguageSupport,
    pub tool_name: String,
    pub command: Vec<String>,
    pub args: Vec<String>,
    pub file_extensions: Vec<String>,
    pub enabled: bool,
}

impl FormatConfig {
    /// Helper to convert string slices to owned Strings
    fn vec_from(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    /// Create rustfmt configuration
    pub fn rustfmt() -> Self {
        Self {
            language: LanguageSupport::Rust,
            tool_name: "rustfmt".to_string(),
            command: Self::vec_from(&["rustfmt"]),
            args: Self::vec_from(&["--edition", "2021"]),
            file_extensions: Self::vec_from(&[".rs"]),
            enabled: true,
        }
    }

    /// Create prettier configuration
    pub fn prettier() -> Self {
        Self {
            language: LanguageSupport::TypeScript,
            tool_name: "prettier".to_string(),
            command: Self::vec_from(&["prettier"]),
            args: Self::vec_from(&["--write"]),
            file_extensions: Self::vec_from(&[".ts", ".js", ".json"]),
            enabled: true,
        }
    }

    /// Create black configuration
    pub fn black() -> Self {
        Self {
            language: LanguageSupport::Python,
            tool_name: "black".to_string(),
            command: Self::vec_from(&["black"]),
            args: vec![],
            file_extensions: Self::vec_from(&[".py"]),
            enabled: true,
        }
    }
}
