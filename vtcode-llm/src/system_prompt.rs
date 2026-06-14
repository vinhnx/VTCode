//! System prompt provider for LLM providers.
//!
//! Default implementations are provided here. vtcode-core can override these
//! at runtime by calling the setter functions.

use std::sync::OnceLock;

type PromptFn = Box<dyn Fn() -> String + Send + Sync>;

static DEFAULT_SYSTEM_PROMPT: OnceLock<PromptFn> = OnceLock::new();
static OPENAI_GPT55_ADDENDUM: OnceLock<PromptFn> = OnceLock::new();

const FALLBACK_SYSTEM_PROMPT: &str = "You are VT Code, a coding assistant.";
const FALLBACK_GPT55_ADDENDUM: &str = "";

/// Get the default system prompt string.
///
/// Returns the prompt set via [`set_default_system_prompt`] if available,
/// otherwise falls back to a built-in default.
pub fn default_system_prompt() -> String {
    DEFAULT_SYSTEM_PROMPT
        .get()
        .map_or_else(|| FALLBACK_SYSTEM_PROMPT.to_string(), |f| f())
}

/// Get the OpenAI GPT-5.5 contract addendum.
///
/// Returns the addendum set via [`set_openai_gpt55_addendum`] if available,
/// otherwise falls back to an empty string.
pub fn openai_gpt55_contract_addendum() -> String {
    OPENAI_GPT55_ADDENDUM
        .get()
        .map_or_else(|| FALLBACK_GPT55_ADDENDUM.to_string(), |f| f())
}

/// Override the default system prompt (called by vtcode-core at init).
pub fn set_default_system_prompt<F: Fn() -> String + Send + Sync + 'static>(f: F) {
    let _ = DEFAULT_SYSTEM_PROMPT.set(Box::new(f));
}

/// Override the OpenAI GPT-5.5 addendum (called by vtcode-core at init).
pub fn set_openai_gpt55_addendum<F: Fn() -> String + Send + Sync + 'static>(f: F) {
    let _ = OPENAI_GPT55_ADDENDUM.set(Box::new(f));
}
