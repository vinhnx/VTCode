use std::path::Path;

use anyhow::{Context, Result};
use async_trait::async_trait;
use vtcode_core::config::WorkspaceTrustLevel;
use vtcode_core::utils::dot_config::{load_workspace_trust_level, update_workspace_trust};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceTrustSyncOutcome {
    AlreadyMatches(WorkspaceTrustLevel),
    Upgraded {
        previous: Option<WorkspaceTrustLevel>,
        new: WorkspaceTrustLevel,
    },
    SkippedDowngrade(WorkspaceTrustLevel),
}

#[async_trait]
pub trait WorkspaceTrustSynchronizer {
    async fn synchronize(
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

#[async_trait]
impl WorkspaceTrustSynchronizer for DefaultWorkspaceTrustSynchronizer {
    async fn synchronize(
        &self,
        workspace: &Path,
        desired_level: WorkspaceTrustLevel,
    ) -> Result<WorkspaceTrustSyncOutcome> {
        ensure_workspace_trust_level_silent(workspace, desired_level).await
    }
}

pub async fn ensure_workspace_trust_level_silent(
    workspace: &Path,
    desired_level: WorkspaceTrustLevel,
) -> Result<WorkspaceTrustSyncOutcome> {
    let current_level = load_workspace_trust_level(workspace)
        .await
        .context("Failed to load user configuration for trust sync")?;

    if let Some(level) = current_level {
        if level == desired_level {
            return Ok(WorkspaceTrustSyncOutcome::AlreadyMatches(level));
        }

        if level == WorkspaceTrustLevel::FullAuto
            && desired_level == WorkspaceTrustLevel::ToolsPolicy
        {
            return Ok(WorkspaceTrustSyncOutcome::SkippedDowngrade(level));
        }
    }

    update_workspace_trust(workspace, desired_level)
        .await
        .context("Failed to persist workspace trust sync")?;

    Ok(WorkspaceTrustSyncOutcome::Upgraded {
        previous: current_level,
        new: desired_level,
    })
}
