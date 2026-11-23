//! Desktop diagnostics: protocol handlers over the shared repair core.
//!
//! The CLI `/doctor` repairs view and these methods share detection and
//! apply (`crate::agent::runloop::unified::setup_repairs`); only the
//! rendering differs. Picker and secret repairs are routed client-side to
//! native UI, so `apply` only executes directly-appliable repairs.
use super::*;
use crate::agent::runloop::unified::setup_repairs as repairs;

pub(super) fn checkup(workspace: &Path) -> protocol::DiagnosticsCheckupResult {
    let list = repairs::compute_setup_repairs(workspace)
        .into_iter()
        .map(|repair| {
            let kind = match repair.kind {
                repairs::SetupRepairKind::SetConfig { .. }
                | repairs::SetupRepairKind::ScaffoldWorkspaceConfig
                | repairs::SetupRepairKind::ResetWorkspaceConfigWithBackup => protocol::DiagnosticsRepairKind::Apply,
                repairs::SetupRepairKind::OpenModelPicker => protocol::DiagnosticsRepairKind::ModelPicker,
                repairs::SetupRepairKind::GuidedSecret { .. } => protocol::DiagnosticsRepairKind::SecretStore,
            };
            protocol::DiagnosticsRepair {
                id: repair.id.to_string(),
                title: repair.title,
                subtitle: repair.subtitle,
                kind,
                destructive: repair.destructive,
                preview: protocol::DiagnosticsPreview {
                    target_display: repair.preview.target_display,
                    changes: repair
                        .preview
                        .changes
                        .into_iter()
                        .map(|change| protocol::DiagnosticsPreviewChange {
                            path: change.path,
                            old: change.old,
                            new: change.new,
                        })
                        .collect(),
                    creates: repair.preview.creates,
                    note: repair.preview.note,
                },
            }
        })
        .collect();
    protocol::DiagnosticsCheckupResult { repairs: list }
}

pub(super) fn apply(workspace: &Path, repair_id: &str) -> Result<protocol::DiagnosticsApplyResult> {
    let repair = repairs::compute_setup_repairs(workspace)
        .into_iter()
        .find(|repair| repair.id == repair_id);
    let Some(repair) = repair else {
        bail!("Unknown or no-longer-applicable repair: {repair_id}");
    };
    match repair.kind {
        repairs::SetupRepairKind::OpenModelPicker => Ok(protocol::DiagnosticsApplyResult {
            applied: false,
            message: "Open the model picker to complete this repair.".to_string(),
        }),
        repairs::SetupRepairKind::GuidedSecret { .. } => Ok(protocol::DiagnosticsApplyResult {
            applied: false,
            message: "Use the Secrets settings to complete this repair.".to_string(),
        }),
        repairs::SetupRepairKind::SetConfig { .. }
        | repairs::SetupRepairKind::ScaffoldWorkspaceConfig
        | repairs::SetupRepairKind::ResetWorkspaceConfigWithBackup => {
            let message = repairs::apply_setup_repair(workspace, &repair)?;
            Ok(protocol::DiagnosticsApplyResult { applied: true, message })
        }
    }
}
