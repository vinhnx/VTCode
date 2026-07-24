//! System prompt provider for LLM providers.
//!
//! Default implementations are provided here. vtcode-core can override these
//! at runtime by calling the setter functions.

use std::sync::OnceLock;

type PromptFn = Box<dyn Fn() -> String + Send + Sync>;

static DEFAULT_SYSTEM_PROMPT: OnceLock<PromptFn> = OnceLock::new();

const FALLBACK_SYSTEM_PROMPT: &str = "You are VT Code, a coding assistant.";

/// Get the default system prompt string.
///
/// Returns the prompt set via [`set_default_system_prompt`] if available,
/// otherwise falls back to a built-in default.
pub(crate) fn default_system_prompt() -> String {
    DEFAULT_SYSTEM_PROMPT
        .get()
        .map_or_else(|| FALLBACK_SYSTEM_PROMPT.to_string(), |f| f())
}

/// Override the default system prompt (called by vtcode-core at init).
pub fn set_default_system_prompt<F: Fn() -> String + Send + Sync + 'static>(f: F) {
    let _ = DEFAULT_SYSTEM_PROMPT.set(Box::new(f));
}
