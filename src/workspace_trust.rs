use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use console::style;
use tracing::warn;
use vtcode_core::utils::dot_config::{
    WorkspaceTrustLevel, WorkspaceTrustRecord, get_dot_manager, load_user_config,
};

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceTrustGateResult {
    Trusted(WorkspaceTrustLevel),
    Aborted,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceTrustSyncOutcome {
    AlreadyMatches(WorkspaceTrustLevel),
    Upgraded {
        previous: Option<WorkspaceTrustLevel>,
        new: WorkspaceTrustLevel,
    },
    SkippedDowngrade(WorkspaceTrustLevel),
}

#[allow(dead_code)]
pub fn ensure_workspace_trust(
    workspace: &Path,
    _full_auto_requested: bool,
) -> Result<WorkspaceTrustGateResult> {
    let workspace_key = canonicalize_workspace(workspace)?;
    let config = load_user_config().context("Failed to load user configuration for trust check")?;
    let current_level = config
        .workspace_trust
        .entries
        .get(&workspace_key)
        .map(|record| record.level);

    if let Some(level) = current_level {
        return Ok(WorkspaceTrustGateResult::Trusted(level));
    }

    persist_trust_decision(&workspace_key, WorkspaceTrustLevel::FullAuto)?;
    println!(
        "{}",
        style("Workspace marked as trusted with full auto capabilities.").green()
    );
    Ok(WorkspaceTrustGateResult::Trusted(
        WorkspaceTrustLevel::FullAuto,
    ))
}

#[allow(dead_code)]
pub fn workspace_trust_level(workspace: &Path) -> Result<Option<WorkspaceTrustLevel>> {
    let workspace_key = canonicalize_workspace(workspace)?;
    let config =
        load_user_config().context("Failed to load user configuration for trust lookup")?;
    Ok(config
        .workspace_trust
        .entries
        .get(&workspace_key)
        .map(|record| record.level))
}

#[allow(dead_code)]
pub fn ensure_workspace_trust_level_silent(
    workspace: &Path,
    desired_level: WorkspaceTrustLevel,
) -> Result<WorkspaceTrustSyncOutcome> {
    let workspace_key = canonicalize_workspace(workspace)?;
    let config = load_user_config().context("Failed to load user configuration for trust sync")?;
    let current_level = config
        .workspace_trust
        .entries
        .get(&workspace_key)
        .map(|record| record.level);

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

    persist_trust_decision(&workspace_key, desired_level)?;

    Ok(WorkspaceTrustSyncOutcome::Upgraded {
        previous: current_level,
        new: desired_level,
    })
}

fn persist_trust_decision(workspace_key: &str, level: WorkspaceTrustLevel) -> Result<()> {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let manager = get_dot_manager();
    let guard = manager
        .lock()
        .expect("Workspace trust manager mutex poisoned");
    guard
        .update_config(|cfg| {
            cfg.workspace_trust.entries.insert(
                workspace_key.to_string(),
                WorkspaceTrustRecord {
                    level,
                    trusted_at: timestamp,
                },
            );
        })
        .context("Failed to persist workspace trust decision")
}

fn canonicalize_workspace(workspace: &Path) -> Result<String> {
    match workspace.canonicalize() {
        Ok(canonical) => Ok(canonical.to_string_lossy().into_owned()),
        Err(error) => {
            warn!(
                workspace = %workspace.display(),
                error = ?error,
                "Failed to canonicalize workspace path; using provided path as workspace key"
            );
            Ok(workspace.to_string_lossy().into_owned())
        }
    }
}
