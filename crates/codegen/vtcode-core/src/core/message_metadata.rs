//! Re-export of [`vtcode_commons::message_metadata`].
//!
//! The canonical definition lives in `vtcode-commons` so that `vtcode-llm` can
//! embed `MessageMetadata` directly in its `Message` type without depending on
//! `vtcode-core`. This module preserves the historical
//! `crate::core::message_metadata` path used throughout the codebase.

pub use vtcode_commons::message_metadata::{CompressionStatus, MessageMetadata};
