use crate::tools::tree_sitter::LanguageSupport;

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
    /// Create rustfmt configuration
    pub fn rustfmt() -> Self {
        Self {
            language: LanguageSupport::Rust,
            tool_name: "rustfmt".to_owned(),
            command: vec!["rustfmt".to_owned()],
            args: vec!["--edition".to_owned(), "2021".to_owned()],
            file_extensions: vec![".rs".to_owned()],
            enabled: true,
        }
    }

    /// Create prettier configuration
    pub fn prettier() -> Self {
        Self {
            language: LanguageSupport::TypeScript,
            tool_name: "prettier".to_owned(),
            command: vec!["prettier".to_owned()],
            args: vec!["--write".to_owned()],
            file_extensions: vec![".ts".to_owned(), ".js".to_owned(), ".json".to_owned()],
            enabled: true,
        }
    }

    /// Create black configuration
    pub fn black() -> Self {
        Self {
            language: LanguageSupport::Python,
            tool_name: "black".to_owned(),
            command: vec!["black".to_owned()],
            args: vec![],
            file_extensions: vec![".py".to_owned()],
            enabled: true,
        }
    }
}
