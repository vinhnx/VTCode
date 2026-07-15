//! Planning workflow static interview question shaping.
//!
//! Plan mode does not spend an LLM round-trip generating interview questions.
//! It injects a single static clarifying question (`build_fallback_question`),
//! which is intentionally fixed — no research context is consulted, so the
//! wording and options are constant.

use serde_json::{Value, json};

/// Build the single static clarifying question injected into plan mode when the
/// model has not already posed one. The question and options are fixed on
/// purpose: plan mode only needs one highest-impact scoping prompt, and any
/// "research context" shaping was removed when interview synthesis collapsed to
/// this fallback.
pub(super) fn build_fallback_question() -> Value {
    json!({
        "id": "scope",
        "header": "Scope",
        "question": "What is the highest-impact planning decision still missing before implementation can start?",
        "options": [
            {
                "label": "Dependency-first slices (Recommended)",
                "description": "Order 3-7 steps around dependency boundaries so each slice is independently verifiable.",
            },
            {
                "label": "User-flow slices",
                "description": "Split work by visible user journey milestones to reduce ambiguity.",
            },
            {
                "label": "Risk-isolated slices",
                "description": "Isolate high-risk steps first so rollback and debugging stay simple.",
            },
        ],
    })
}
