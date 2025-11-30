use super::LanguageProvider;
use crate::code::code_completion::context::CompletionContext;
use crate::code::code_completion::engine::{CompletionKind, CompletionSuggestion};

/// TypeScript-specific completion provider
pub struct TypeScriptProvider;

impl TypeScriptProvider {
    pub fn new() -> Self {
        Self
    }
}

impl Default for TypeScriptProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl LanguageProvider for TypeScriptProvider {
    fn get_completions(&self, context: &CompletionContext) -> Vec<CompletionSuggestion> {
        let mut suggestions = Vec::new();
        let keywords = ["function", "interface"];

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
        "typescript"
    }

    fn supports_language(&self, language: &str) -> bool {
        language == "typescript" || language == "ts" || language == "javascript" || language == "js"
    }
}
