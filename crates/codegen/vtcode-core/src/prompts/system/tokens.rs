//! Minimal/lightweight/specialized instruction entry points and token estimation.

use crate::llm::providers::gemini::wire::Content;
use crate::prompts::static_prompts::{
    lightweight_instruction_text, minimal_instruction_text, specialized_instruction_text,
};
use vtcode_commons::estimate_tokens;

/// Generate a minimal system instruction (pi-inspired, <1K tokens)
pub fn generate_minimal_instruction() -> Content {
    Content::system_text(minimal_instruction_text())
}

/// Generate a lightweight system instruction for simple operations
pub fn generate_lightweight_instruction() -> Content {
    Content::system_text(lightweight_instruction_text())
}

/// Generate a specialized system instruction for advanced operations
pub fn generate_specialized_instruction() -> Content {
    Content::system_text(specialized_instruction_text())
}

// ─── Token Estimation ────────────────────────────────────────────────────────

/// Estimate prompt tokens through the shared workspace tokenizer.
///
/// Keeping prompt budgeting on the common estimator makes prompt reports and
/// the other runtime token budgets use the same tokenization semantics.
#[must_use]
pub fn estimate_token_count(text: &str) -> u64 {
    estimate_tokens(text) as u64
}
