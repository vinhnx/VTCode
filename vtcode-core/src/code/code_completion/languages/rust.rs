use super::LanguageProvider;
use crate::code::code_completion::context::CompletionContext;
use crate::code::code_completion::engine::{CompletionKind, CompletionSuggestion};

/// Rust-specific completion provider
pub struct RustProvider;

impl RustProvider {
    pub fn new() -> Self {
        Self
    }
}

impl Default for RustProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl LanguageProvider for RustProvider {
    fn get_completions(&self, context: &CompletionContext) -> Vec<CompletionSuggestion> {
        let mut suggestions = Vec::new();

        // Add Rust-specific keywords
        if context.prefix.is_empty() || "fn".starts_with(&context.prefix) {
            suggestions.push(CompletionSuggestion::new(
                "fn".to_owned(),
                CompletionKind::Keyword,
                context.clone(),
            ));
        }

        if context.prefix.is_empty() || "struct".starts_with(&context.prefix) {
            suggestions.push(CompletionSuggestion::new(
                "struct".to_owned(),
                CompletionKind::Keyword,
                context.clone(),
            ));
        }

        if context.prefix.is_empty() || "impl".starts_with(&context.prefix) {
            suggestions.push(CompletionSuggestion::new(
                "impl".to_owned(),
                CompletionKind::Keyword,
                context.clone(),
            ));
        }

        suggestions
    }

    fn language_name(&self) -> &str {
        "rust"
    }

    fn supports_language(&self, language: &str) -> bool {
        language == "rust" || language == "rs"
    }
}
