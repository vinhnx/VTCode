//! Agent Trace storage layer for VT Code.
//!
//! This module provides file-based storage for Agent Trace records,
//! supporting reading/writing traces to `.vtcode/traces/` directory.

mod store;
mod generator;

pub use store::*;
pub use generator::*;
