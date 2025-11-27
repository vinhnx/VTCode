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

        // Add TypeScript-specific keywords
        if context.prefix.is_empty() || "function".starts_with(&context.prefix) {
            suggestions.push(CompletionSuggestion::new(
                "function".to_owned(),
                CompletionKind::Keyword,
                context.clone(),
            ));
        }

        if context.prefix.is_empty() || "interface".starts_with(&context.prefix) {
            suggestions.push(CompletionSuggestion::new(
                "interface".to_owned(),
                CompletionKind::Keyword,
                context.clone(),
            ));
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
