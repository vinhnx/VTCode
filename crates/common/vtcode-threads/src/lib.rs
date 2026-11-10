#![allow(missing_docs, clippy::expect_used, dead_code, reason = "Extracted identifiers")]
//! Thread identifiers extracted from `vtcode-core`.
//!
//! `ThreadId` and `SubmissionId` live here so downstream crates (`vtcode-acp`
//! and `vtcode-core`) can depend on identifiers without pulling the 303k
//! `vtcode-core` monolith. `vtcode-core` re-exports these for compatibility.

use serde::{Deserialize, Serialize};
use uuid::Uuid;
use vtcode_macros::StringNewtype;

/// Unique identifier for a thread in the runtime.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, StringNewtype)]
pub struct ThreadId(String);

/// Unique identifier for a submission within a thread turn.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, StringNewtype)]
pub struct SubmissionId(String);

impl SubmissionId {
    /// Generate a new submission identifier with a `sub-` prefix.
    pub fn generate() -> Self {
        Self(format!("sub-{}", Uuid::new_v4()))
    }
}

impl Default for SubmissionId {
    fn default() -> Self {
        Self::generate()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn submission_id_generate_has_prefix() {
        let id = SubmissionId::generate();
        assert!(id.as_ref().starts_with("sub-"), "generated id must have sub- prefix");
        assert_eq!(id.len(), 36 + 4, "uuid v4 36 plus sub-");
    }

    #[test]
    fn thread_id_string_newtype_roundtrip() {
        let id = ThreadId::new("thread-abc");
        assert_eq!(id.as_ref(), "thread-abc");
        let back: ThreadId = serde_json::from_str(&serde_json::to_string(&id).unwrap()).unwrap();
        assert_eq!(back, id);
    }
}
