pub mod python;
pub mod rust;
pub mod typescript;

use crate::code::code_completion::context::CompletionContext;
use crate::code::code_completion::engine::CompletionSuggestion;

/// Language-specific completion provider trait
pub trait LanguageProvider {
    /// Get language-specific completions
    fn get_completions(&self, context: &CompletionContext) -> Vec<CompletionSuggestion>;

    /// Get language name
    fn language_name(&self) -> &str;

    /// Check if this provider supports the given language
    fn supports_language(&self, language: &str) -> bool;
}

/// Language provider registry
pub struct LanguageRegistry {
    providers: Vec<Box<dyn LanguageProvider + Send + Sync>>,
}

impl LanguageRegistry {
    pub fn new() -> Self {
        let mut registry = Self {
            providers: Vec::new(),
        };

        // Register default providers
        registry.register(Box::new(rust::RustProvider::new()));
        registry.register(Box::new(typescript::TypeScriptProvider::new()));
        registry.register(Box::new(python::PythonProvider::new()));

        registry
    }

    /// Register a language provider
    pub fn register(&mut self, provider: Box<dyn LanguageProvider + Send + Sync>) {
        self.providers.push(provider);
    }

    /// Get completions for a specific language
    pub fn get_completions(&self, context: &CompletionContext) -> Vec<CompletionSuggestion> {
        for provider in &self.providers {
            if provider.supports_language(&context.language) {
                return provider.get_completions(context);
            }
        }
        Vec::new()
    }
}

impl Default for LanguageRegistry {
    fn default() -> Self {
        Self::new()
    }
}
