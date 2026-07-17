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
        let keywords = ["def", "class"];

        for keyword in &keywords {
            if context.prefix.is_empty() || keyword.starts_with(&context.prefix) {
                suggestions.push(CompletionSuggestion::new(
                    keyword.to_string(),
                    CompletionKind::Keyword,
                    context.clone(),
                ));
            }
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
