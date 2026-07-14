use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Notify;

use crate::agent::runloop::unified::state::CtrlCState;

mod cache;

/// Bundles the Ctrl+C state and notification handle together to reduce
/// parameter counts in internal tool-pipeline functions.
#[derive(Clone)]
pub(super) struct CancellationTokens {
    pub state: Arc<CtrlCState>,
    pub notify: Arc<Notify>,
}

mod execution;
pub(crate) mod execution_attempts;
mod execution_events;
mod execution_helpers;
mod execution_run;
mod execution_runtime;
mod file_conflict_prompt;
mod file_conflict_runtime;
mod hitl;
mod pty_stream;
pub(crate) mod status;
#[cfg(test)]
mod tests;
mod timeout;
pub(crate) mod validation;

pub(crate) use execution::{
    execute_tool_with_timeout_ref_prevalidated, run_tool_call, run_tool_call_with_args,
};
pub(crate) use execution_run::exec_settlement_mode_for_tool_call;
pub(crate) use hitl::execute_hitl_tool;
pub(crate) use pty_stream::PtyStreamRuntime;
pub(crate) use status::{ToolBatchOutcome, ToolExecutionStatus, ToolPipelineOutcome};

/// Default timeout for tool execution if no policy is configured.
/// Sourced from `vtcode_config::constants::execution::DEFAULT_TOOL_TIMEOUT_SECS`
/// so the value is tunable without touching this file.
const DEFAULT_TOOL_TIMEOUT: Duration =
    Duration::from_secs(vtcode_config::constants::execution::DEFAULT_TOOL_TIMEOUT_SECS);
/// Minimum buffer before cancelling a tool once a warning fires
const MIN_TIMEOUT_WARNING_HEADROOM: Duration = Duration::from_secs(5);
const RETRY_BACKOFF_BASE: Duration = Duration::from_millis(200);
const MAX_RETRY_BACKOFF: Duration = Duration::from_secs(3);
