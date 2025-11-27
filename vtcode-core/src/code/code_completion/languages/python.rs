use super::LanguageProvider;
use crate::code::code_completion::context::CompletionContext;
use crate::code::code_completion::engine::{CompletionKind, CompletionSuggestion};

/// Python-specific completion provider
pub struct PythonProvider;

impl PythonProvider {
    pub fn new() -> Self {
        Self
    }
}

impl Default for PythonProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl LanguageProvider for PythonProvider {
    fn get_completions(&self, context: &CompletionContext) -> Vec<CompletionSuggestion> {
        let mut suggestions = Vec::new();

        // Add Python-specific keywords
        if context.prefix.is_empty() || "def".starts_with(&context.prefix) {
            suggestions.push(CompletionSuggestion::new(
                "def".to_owned(),
                CompletionKind::Keyword,
                context.clone(),
            ));
        }

        if context.prefix.is_empty() || "class".starts_with(&context.prefix) {
            suggestions.push(CompletionSuggestion::new(
                "class".to_owned(),
                CompletionKind::Keyword,
                context.clone(),
            ));
        }

        suggestions
    }

    fn language_name(&self) -> &str {
        "python"
    }

    fn supports_language(&self, language: &str) -> bool {
        language == "python" || language == "py"
    }
}
