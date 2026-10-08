//! Interactive approval for workspace-controlled lifecycle hooks.
//!
//! Workspace-controlled configuration (a workspace-root `vtcode.toml`, the
//! workspace `.vtcode/vtcode.toml` fallback, a project profile inside the
//! workspace, or a workspace-sourced agent spec with hooks) can declare
//! lifecycle hook commands that VT Code otherwise runs automatically via
//! `sh -c`. Because that configuration is attacker-influenceable in an
//! untrusted repository, no lifecycle hook runs while workspace-controlled
//! hook content is present until the user approves the exact command set the
//! engine will execute. This module renders that approval prompt and maps the
//! outcome; any cancellation or interrupt fails closed to `Denied`.

use std::path::Path;
use std::sync::Arc;

use anyhow::Result;
use tokio::sync::Notify;
use vtcode_commons::modal_hints::{APPROVAL_NAVIGATE_DENY, choose_handling_line};
use vtcode_core::core::interfaces::ui::UiSession;
use vtcode_core::hooks::LifecycleHookCommandPreview;
use vtcode_ui::tui::app::{
    InlineHandle, InlineListItem, InlineListSelection, ListOverlayRequest, TransientRequest, TransientSubmission,
};

use crate::agent::runloop::unified::overlay_prompt::{OverlayWaitOutcome, show_permission_and_wait};
use crate::agent::runloop::unified::state::CtrlCState;

pub(crate) enum HookApprovalDecision {
    Approved,
    Denied,
}

/// Present the workspace lifecycle hook approval overlay and wait for the
/// user's choice.
pub(crate) async fn prompt_workspace_hook_approval<S: UiSession + ?Sized>(
    handle: &InlineHandle,
    session: &mut S,
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
    workspace: &Path,
    commands: &[LifecycleHookCommandPreview],
) -> Result<HookApprovalDecision> {
    let mut lines = vec![
        "The workspace configuration defines lifecycle hook commands that VT Code".to_string(),
        "runs automatically. They execute via `sh -c` from the workspace directory and".to_string(),
        "are controlled by files inside the workspace (vtcode.toml / .vtcode), so an".to_string(),
        "untrusted repository could change them. Approving runs the exact command set".to_string(),
        "below; any later change to these commands requires a new approval.".to_string(),
        String::new(),
        format!("Workspace: {}", workspace.display()),
        String::new(),
        "Commands:".to_string(),
    ];
    for command in commands {
        let matcher = command.matcher.as_deref().unwrap_or("*");
        lines.push(format!("  [{event}/{matcher}] {command}", event = command.event, command = command.command));
    }
    lines.push(String::new());
    lines.push(choose_handling_line("workspace lifecycle hooks"));

    let items = vec![
        InlineListItem {
            title: "Approve".to_string(),
            subtitle: Some(
                "Run these lifecycle hooks and remember this approval for this exact command set".to_string(),
            ),
            badge: None,
            indent: 0,
            selection: Some(InlineListSelection::ToolApproval(true)),
            search_value: Some("approve allow yes run 1".to_string()),
            ..Default::default()
        },
        InlineListItem {
            title: "Deny".to_string(),
            subtitle: Some("Skip lifecycle hooks for this session".to_string()),
            badge: None,
            indent: 0,
            selection: Some(InlineListSelection::ToolApproval(false)),
            search_value: Some("deny no skip block 2".to_string()),
            ..Default::default()
        },
    ];

    handle.set_progress_phase(vtcode_commons::ui_protocol::ProgressPhase::WaitingForApproval);
    let outcome = show_permission_and_wait(
        handle,
        session,
        TransientRequest::List(ListOverlayRequest {
            title: "Workspace Lifecycle Hooks".to_string(),
            lines,
            footer_hint: Some(APPROVAL_NAVIGATE_DENY.to_string()),
            items,
            selected: Some(InlineListSelection::ToolApproval(false)),
            search: None,
            hotkeys: Vec::new(),
            status: None,
        }),
        ctrl_c_state,
        ctrl_c_notify,
        |submission| match submission {
            TransientSubmission::Selection(InlineListSelection::ToolApproval(true)) => {
                Some(HookApprovalDecision::Approved)
            }
            TransientSubmission::Selection(InlineListSelection::ToolApproval(false)) => {
                Some(HookApprovalDecision::Denied)
            }
            _ => None,
        },
    )
    .await?;

    handle.set_progress_phase(vtcode_commons::ui_protocol::ProgressPhase::Initializing);
    Ok(match outcome {
        OverlayWaitOutcome::Submitted(decision) => decision,
        OverlayWaitOutcome::Cancelled
        | OverlayWaitOutcome::Interrupted
        | OverlayWaitOutcome::Deferred
        | OverlayWaitOutcome::Exit => HookApprovalDecision::Denied,
    })
}
