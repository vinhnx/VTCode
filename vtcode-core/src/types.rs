//! Type definitions used throughout the application
//!
//! This module re-exports type definitions from various modules.

pub use crate::core::agent::types::*;

/// Compact inline string — stack-allocated for strings up to 24 bytes.
/// Drop-in replacement for `String` with zero heap allocation for short strings.
pub type CompactStr = compact_str::CompactString;
