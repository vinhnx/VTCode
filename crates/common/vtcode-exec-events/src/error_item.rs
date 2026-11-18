//! Error item payload carried by [`crate::ThreadItemDetails::Error`].
//!
//! Split out of `lib.rs` so the authoritative `ThreadEvent` enum and its
//! wrapper stay focused; re-exported from the crate root so the public path
//! `vtcode_exec_events::ErrorItem` is unchanged.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
pub struct ErrorItem {
    /// Error message displayed to the user or logs.
    pub message: String,
}
