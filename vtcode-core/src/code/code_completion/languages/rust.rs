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
        let keywords = [("fn", true), ("struct", true), ("impl", true)];

        for (keyword, _) in &keywords {
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
        "rust"
    }

    fn supports_language(&self, language: &str) -> bool {
        language == "rust" || language == "rs"
    }
}
