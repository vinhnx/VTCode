use anyhow::Result;
use std::path::PathBuf;
use vtcode_core::config::VTCodeConfig;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::review::ReviewSpec;

use super::exec::{ExecCommandKind, ExecCommandOptions};

#[derive(Debug, Clone)]
pub struct ReviewCommandOptions {
    pub json: bool,
    pub events_path: Option<PathBuf>,
    pub last_message_file: Option<PathBuf>,
    pub spec: ReviewSpec,
}

pub async fn handle_review_command(
    config: &CoreAgentConfig,
    vt_cfg: &VTCodeConfig,
    options: ReviewCommandOptions,
) -> Result<()> {
    let exec_options = ExecCommandOptions {
        json: options.json,
        dry_run: false,
        events_path: options.events_path,
        last_message_file: options.last_message_file,
        command: ExecCommandKind::Review { spec: options.spec },
    };

    super::exec::handle_exec_command(config, vt_cfg, exec_options).await
}
