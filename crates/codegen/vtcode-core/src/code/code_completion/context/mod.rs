pub mod analyzer;

pub use analyzer::ContextAnalyzer;

use serde::{Deserialize, Serialize};

/// Context information for completion
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletionContext {
    pub line: usize,
    pub column: usize,
    pub prefix: String,
    pub language: String,
    pub scope: Vec<String>,
    pub imports: Vec<String>,
    pub recent_symbols: Vec<String>,
}

impl CompletionContext {
    pub fn new(line: usize, column: usize, prefix: String, language: String) -> Self {
        Self {
            line,
            column,
            prefix,
            language,
            scope: Vec::new(),
            imports: Vec::new(),
            recent_symbols: Vec::new(),
        }
    }

    /// Check if context is suitable for completion
    pub fn is_completion_suitable(&self) -> bool {
        !self.prefix.trim().is_empty()
    }
}
