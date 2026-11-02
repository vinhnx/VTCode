pub(super) use serde_json::json;
pub(super) use vtcode_core::config::constants::tools;

mod verification;

mod verification_recovery;

mod verifier_sessions;

use super::*;

fn successful_exec_output() -> ToolPipelineOutcome {
    ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output: serde_json::json!({}),
        stdout: None,
        modified_files: vec![],
        command_success: true,
    })
}

mod listing_telemetry;
mod low_signal;
mod mutations;
mod navigation;
mod pressure_recovery;
mod push_response;
mod replay_reuse;
mod signature_keys;
mod verifier_progress;
