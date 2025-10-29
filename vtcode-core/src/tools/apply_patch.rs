//! Patch tool facade that exposes Codex-compatible patch parsing and application.
//!
//! Actual patch parsing logic lives in `tools::editing::patch` so future edit
//! features can reuse the same primitives without depending on this facade.

use serde::{Deserialize, Serialize};

pub use crate::tools::editing::{Patch, PatchError, PatchHunk, PatchLine, PatchOperation};

/// Input structure for the apply_patch tool
#[derive(Debug, Deserialize, Serialize)]
pub struct ApplyPatchInput {
    pub input: String,
}
