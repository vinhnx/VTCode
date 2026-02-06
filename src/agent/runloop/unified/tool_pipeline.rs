#![allow(clippy::too_many_arguments)]
use std::time::Duration;

mod cache;
mod execution;
mod hitl;
mod status;
#[cfg(test)]
mod tests;
mod timeout;

pub(crate) use execution::{execute_tool_with_timeout_ref, run_tool_call};
pub(crate) use hitl::execute_hitl_tool;
pub(crate) use status::{ToolExecutionStatus, ToolPipelineOutcome};

/// Default timeout for tool execution if no policy is configured
const DEFAULT_TOOL_TIMEOUT: Duration = Duration::from_secs(180);
/// Minimum buffer before cancelling a tool once a warning fires
const MIN_TIMEOUT_WARNING_HEADROOM: Duration = Duration::from_secs(5);
const RETRY_BACKOFF_BASE: Duration = Duration::from_millis(200);
const MAX_RETRY_BACKOFF: Duration = Duration::from_secs(3);
