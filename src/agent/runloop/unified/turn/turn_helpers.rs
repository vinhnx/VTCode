//! Common helpers for turn processing extracted to reduce duplication

use anyhow::Result;
use std::time::Duration;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};
use crate::agent::runloop::unified::state::CtrlCState;

/// Centralized error display with consistent formatting
pub fn display_error(renderer: &mut AnsiRenderer, category: &str, error: &anyhow::Error) -> Result<()> {
    renderer.line_if_not_empty(MessageStyle::Output)?;
    renderer.line(MessageStyle::Error, &format!("{}: {}", category, error))
}

/// Centralized status message display
pub fn display_status(renderer: &mut AnsiRenderer, message: &str) -> Result<()> {
    renderer.line(MessageStyle::Info, message)
}

/// Check if operation should continue based on ctrl-c state
pub fn should_continue_operation(ctrl_c_state: &CtrlCState) -> bool {
    !ctrl_c_state.is_cancel_requested() && !ctrl_c_state.is_exit_requested()
}

/// Exponential backoff calculation
pub fn calculate_backoff(attempt: usize, base_ms: u64, max_ms: u64) -> Duration {
    let exp = 2_u64.saturating_pow(attempt.min(4) as u32);
    let backoff_ms = base_ms.saturating_mul(exp);
    Duration::from_millis(backoff_ms.min(max_ms))
}
