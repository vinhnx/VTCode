//! Setup diagnostics: shared repair model for the Desktop self-solve entry.
//!
//! Mirrors the CLI `/doctor` repairs computed by the service from the same
//!core: [`DiagnosticsCheckupResult`] lists applicable repairs, and
//!`diagnostics/apply` executes the directly-appliable ones. Picker and
//!secret kinds are routed client-side to native UI instead.
use serde::{Deserialize, Serialize};

/// One applicable setup repair.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticsRepair {
    /// Stable repair id, echoed back to `diagnostics/apply`.
    pub id: String,
    /// Short user-facing title.
    pub title: String,
    /// One-line explanation of what is wrong and what happens.
    pub subtitle: String,
    /// How the client carries the repair out.
    pub kind: DiagnosticsRepairKind,
    /// Destructive repairs render danger styling and preselect cancel.
    pub destructive: bool,
    /// What applying would change. Empty for routed repairs; absent on older services.
    #[serde(default)]
    pub preview: DiagnosticsPreview,
}

/// Preview of a repair's config changes, shown for confirmation before apply.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticsPreview {
    /// Where the change lands, human-readable.
    #[serde(default)]
    pub target_display: String,
    /// Per-key old → new values.
    #[serde(default)]
    pub changes: Vec<DiagnosticsPreviewChange>,
    /// Files a scaffold would create (missing files only).
    #[serde(default)]
    pub creates: Vec<String>,
    /// Extra context (backups, reversibility).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// One key change inside a preview.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticsPreviewChange {
    /// Dotted setting path.
    pub path: String,
    /// Current effective value, or `(unset)`.
    pub old: String,
    /// Value that would be written.
    pub new: String,
}

/// How the client carries a repair out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticsRepairKind {
    /// Applied server-side via `diagnostics/apply`.
    Apply,
    /// Client opens its native provider/model picker.
    ModelPicker,
    /// Client routes to its secret store UI.
    SecretStore,
}

/// Result of a `diagnostics/checkup` call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticsCheckupResult {
    /// Currently applicable repairs, empty when the setup is healthy.
    pub repairs: Vec<DiagnosticsRepair>,
}

/// Params for a `diagnostics/apply` call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticsApplyParams {
    /// Repair id from a previous checkup result.
    pub repair_id: String,
}

/// Result of a `diagnostics/apply` call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticsApplyResult {
    /// Whether anything changed. False for client-routed repairs.
    pub applied: bool,
    /// Human-readable outcome or next step.
    pub message: String,
}
