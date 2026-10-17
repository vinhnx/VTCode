//! Canonical durable contracts for local matrix execution.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
pub struct MatrixSpec {
    pub id: String,
    pub tasks: Vec<MatrixTaskSpec>,
    #[serde(default)]
    pub resources: BTreeMap<String, u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
pub struct MatrixTaskSpec {
    pub id: String,
    pub instructions: String,
    #[serde(default)]
    pub dependencies: Vec<String>,
    pub workspace: String,
    pub access: WorkspaceAccess,
    pub checks: Vec<String>,
    #[serde(default)]
    pub resources: BTreeMap<String, u32>,
    pub timeout_secs: u64,
    #[serde(default)]
    pub replay_safe: bool,
    #[serde(default)]
    pub inputs: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceAccess {
    Read,
    Write,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum MatrixLifecycle {
    Created,
    Running,
    Paused,
    Verifying,
    Succeeded,
    Cancelled,
    Blocked,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum MatrixTaskStatus {
    Queued,
    Assigned,
    Executed,
    Verified,
    Failed,
    Interrupted,
    TimedOut,
    CleanupUncertain,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum MatrixPhase {
    Execute,
    Verify,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum MatrixOutcome {
    Success,
    Failed,
    Interrupted,
    TimedOut,
    Cancelled,
    PermissionDenied,
    BudgetExhausted,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
pub struct MatrixCommandEvidence {
    pub command: String,
    pub attempt_id: String,
    pub worker_id: String,
    pub generation: String,
    pub event_id: String,
    pub exit_code: Option<i32>,
    pub cancelled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
pub struct MatrixAttempt {
    pub id: String,
    pub worker_id: String,
    pub phase: MatrixPhase,
    pub generation: Option<String>,
    pub launch_requested: bool,
    pub outcome: Option<MatrixOutcome>,
    pub cleanup_confirmed: bool,
    pub evidence: Vec<MatrixCommandEvidence>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
pub struct MatrixTaskState {
    pub id: String,
    pub status: MatrixTaskStatus,
    pub attempts: Vec<MatrixAttempt>,
    pub automatic_retries: u8,
}

/// A complete checkpoint carried in the canonical event log; derived views replay it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
pub struct MatrixSnapshot {
    pub spec: MatrixSpec,
    pub lifecycle: MatrixLifecycle,
    pub tasks: Vec<MatrixTaskState>,
    pub generation: Option<String>,
    pub revision: u64,
}

/// Runtime-created assignment identity. Workers never choose these identifiers.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
pub struct MatrixAssignment {
    pub task_id: String,
    pub attempt_id: String,
    pub worker_id: String,
    pub phase: MatrixPhase,
    pub generation: Option<String>,
}
