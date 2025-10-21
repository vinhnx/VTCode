use std::path::Path;

use anyhow::Result;

use vtcode_core::config::WorkspaceTrustLevel;

use crate::workspace_trust::{WorkspaceTrustSyncOutcome, ensure_workspace_trust_level_silent};

pub trait WorkspaceTrustSynchronizer {
    fn synchronize(
        &self,
        workspace: &Path,
        desired_level: WorkspaceTrustLevel,
    ) -> Result<WorkspaceTrustSyncOutcome>;
}

#[derive(Default, Clone, Copy)]
pub struct DefaultWorkspaceTrustSynchronizer;

impl DefaultWorkspaceTrustSynchronizer {
    pub fn new() -> Self {
        Self
    }
}

impl WorkspaceTrustSynchronizer for DefaultWorkspaceTrustSynchronizer {
    fn synchronize(
        &self,
        workspace: &Path,
        desired_level: WorkspaceTrustLevel,
    ) -> Result<WorkspaceTrustSyncOutcome> {
        ensure_workspace_trust_level_silent(workspace, desired_level)
    }
}
