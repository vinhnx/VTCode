//! Re-export the shared configuration loader implementation from the
//! `vtcode-config` crate so downstream consumers can continue importing it
//! through `vtcode_core::config::loader` while the logic lives in the
//! dedicated configuration crate.
pub use vtcode_config::loader::*;
