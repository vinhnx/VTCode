#![allow(
    missing_docs,
    reason = "Intentional compatibility, platform, or test-only suppression."
)]
pub(super) use std::sync::Arc;

pub(super) use serde_json::json;
pub(super) use tokio::sync::Notify;
pub(super) use vtcode_config::core::permissions::{AgentPermissionsConfig, PermissionDefault};
pub(super) use vtcode_core::acp::PermissionGrant;
pub(super) use vtcode_core::acp::permission_cache::ToolPermissionCache;
pub(super) use vtcode_core::config::constants::tools;
pub(super) use vtcode_core::config::loader::VTCodeConfig;
pub(super) use vtcode_core::core::decision_tracker::DecisionTracker;
pub(super) use vtcode_core::core::trajectory::TrajectoryLogger;
pub(super) use vtcode_core::tools::result_cache::ToolResultCache;

pub(super) use super::*;
pub(super) use vtcode_commons::canonicalize;

mod budget_hooks;
mod cache_preflight;
mod planning_permissions;
mod shell_stream;
