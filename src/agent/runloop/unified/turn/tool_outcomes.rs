//! Tool outcome handlers for the agent turn loop.
//!
//! This module contains the functions for handling tool execution outcomes:
//! - Permission checking (prepare)
//! - Execution with caching
//! - Success/failure/timeout/cancelled handling

mod helpers;
mod handlers;
mod execution;
mod execution_result;
mod messages;
mod dispatch;
mod apply;

pub(crate) use messages::{handle_assistant_response, handle_text_response, HandleTextResponseParams};
pub(crate) use dispatch::handle_tool_calls;
pub(crate) use apply::apply_turn_outcome;

#[allow(dead_code)]
pub enum PrepareToolCallResult {
    Approved,
    Denied,
    Exit,
    Interrupted,
}
