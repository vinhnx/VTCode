//! Re-export the shared ask-first consent model from the `vtcode-config`
//! crate so downstream consumers can continue importing it through
//! `vtcode_core::config::consent` while the logic lives in the dedicated
//! configuration crate.
pub use vtcode_config::consent::*;
