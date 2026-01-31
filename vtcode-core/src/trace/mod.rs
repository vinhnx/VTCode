//! Agent Trace storage layer for VT Code.
//!
//! This module provides file-based storage for Agent Trace records,
//! supporting reading/writing traces to `.vtcode/traces/` directory.
//!
//! # Overview
//!
//! Agent Trace is an open specification for tracking AI-generated code.
//! See <https://agent-trace.dev/> for the full specification.
//!
//! # Usage
//!
//! ```rust,ignore
//! use vtcode_core::trace::{TraceStore, TraceGenerator, TraceContext};
//!
//! // Create context for generating traces
//! let ctx = TraceContext::new("claude-opus-4", "anthropic")
//!     .with_workspace_path("/my/workspace")
//!     .with_session_id("session-123")
//!     .with_turn_number(1);
//!
//! // Generate trace from diff tracker (after apply_patch)
//! if let Some(trace) = TraceGenerator::from_diff_tracker(&tracker, &ctx) {
//!     // Store the trace
//!     let store = TraceStore::for_workspace("/my/workspace");
//!     store.write_trace(&trace)?;
//! }
//! ```

mod generator;
mod store;

pub use generator::*;
pub use store::*;

// Re-export core types from vtcode-exec-events for convenience
pub use vtcode_exec_events::trace::{
    Contributor, ContributorType, HashAlgorithm, RelatedResource, ToolInfo, TraceConversation,
    TraceFile, TraceMetadata, TraceRange, TraceRecord, TraceRecordBuilder, VcsInfo, VcsType,
    VtCodeMetadata, AGENT_TRACE_MIME_TYPE, AGENT_TRACE_VERSION, compute_content_hash,
    compute_content_hash_with, normalize_model_id,
};
