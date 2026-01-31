//! Tool outcome handlers for the agent turn loop.
//!
//! This module contains the functions for handling tool execution outcomes:
//! - Permission checking (prepare)
//! - Execution with caching
//! - Success/failure/timeout/cancelled handling

mod apply;
mod dispatch;
mod execution_result;
pub(crate) mod handlers;
pub(crate) mod helpers;

pub(crate) use apply::apply_turn_outcome;
pub(crate) use dispatch::handle_tool_calls;
pub(crate) use handlers::ToolOutcomeContext;

#[allow(dead_code)]
pub enum PrepareToolCallResult {
    Approved,
    Denied,
    Exit,
    Interrupted,
}
