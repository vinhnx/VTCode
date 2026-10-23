#![allow(
    missing_docs,
    reason = "Intentional compatibility, platform, or test-only suppression."
)]
use std::time::{Duration, Instant};

use super::*;
use crate::agent::runloop::unified::tool_pipeline::{ToolExecutionStatus, ToolPipelineOutcome};
use crate::agent::runloop::unified::turn::tool_outcomes::execution_result::handle_tool_execution_result;
use vtcode_core::config::ToolDisplayMode;
use vtcode_core::tools::registry::ToolExecutionError;

mod admission;
mod budgets;
mod corrections;
mod history;
mod lifecycle;
mod patching;
mod planning;
mod preflight;
mod read_reuse;
