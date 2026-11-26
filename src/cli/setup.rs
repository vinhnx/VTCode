use std::path::Path;

use anyhow::Result;
use vtcode_core::config::loader::VTCodeConfig;

use crate::startup::run_setup_wizard;

/// Re-run first-time setup for a workspace on demand (`vtcode setup`).
///
/// Unlike the automatic first-run path this has no freshness gate: answers
/// overwrite the previous setup choices (workspace file plus the home-scoped
/// provider/model/permission records).
pub(crate) async fn handle_setup_command(workspace: &Path, config: &VTCodeConfig) -> Result<()> {
    let mut config = config.clone();
    run_setup_wizard(workspace, &mut config).await
}
