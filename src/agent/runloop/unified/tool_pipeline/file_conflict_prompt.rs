use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Result, anyhow};
use serde_json::{Value, json};
use tokio::sync::Notify;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::core::interfaces::ui::UiSession;
use vtcode_core::notifications::{NotificationEvent, send_global_notification};
use vtcode_core::tools::edited_file_monitor::{
    FILE_CONFLICT_DETECTED_FIELD, FILE_CONFLICT_OVERRIDE_ARG, FILE_CONFLICT_PATH_FIELD,
};
use vtcode_core::tools::registry::ToolRegistry;
use vtcode_core::tools::result_cache::ToolResultCache;
use vtcode_tui::{
    DiffOverlayRequest, DiffPreviewMode, InlineHandle, InlineListItem, InlineListSelection,
    ListOverlayRequest, OverlayRequest, OverlaySubmission,
};

use crate::agent::runloop::unified::inline_events::harness::HarnessEventEmitter;
use crate::agent::runloop::unified::overlay_prompt::{OverlayWaitOutcome, show_overlay_and_wait};
use crate::agent::runloop::unified::state::CtrlCState;

use super::execution_runtime::execute_with_cache_and_streaming;
use super::status::ToolExecutionStatus;

#[derive(Clone)]
struct PendingFileConflict {
    output: Value,
    display_path: String,
    absolute_path: PathBuf,
    message: String,
    approved_snapshot: Option<Value>,
    disk_content: Option<String>,
    intended_content: Option<String>,
    emit_hitl_notification: bool,
}

enum ConflictResolution {
    Reload,
    Abort,
    Proceed,
}

pub(super) async fn resolve_file_conflict_status<S>(
    registry: &mut ToolRegistry,
    tool_result_cache: &Arc<tokio::sync::RwLock<ToolResultCache>>,
    session: &mut S,
    handle: &InlineHandle,
    name: &str,
    tool_item_id: &str,
    args_val: &Value,
    mut status: ToolExecutionStatus,
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
    harness_emitter: Option<HarnessEventEmitter>,
    vt_cfg: Option<&VTCodeConfig>,
    max_tool_retries: usize,
) -> Result<ToolExecutionStatus>
where
    S: UiSession + ?Sized,
{
    loop {
        let Some(conflict) = extract_pending_conflict(registry, &status).await? else {
            return Ok(status);
        };

        match prompt_for_conflict_resolution(
            session,
            handle,
            ctrl_c_state,
            ctrl_c_notify,
            &conflict,
            vt_cfg
                .map(|cfg| cfg.security.hitl_notification_bell)
                .unwrap_or(true),
        )
        .await?
        {
            OverlayWaitOutcome::Submitted(ConflictResolution::Reload) => {
                registry
                    .edited_file_monitor()
                    .accept_disk_version(&conflict.absolute_path)
                    .await?;
                return Ok(conflict_resolution_status(
                    conflict.finalized_output(
                        "reloaded",
                        "Reloaded the file from disk. Pending agent changes were discarded.",
                        read_disk_text(&conflict.absolute_path).await,
                    ),
                    true,
                ));
            }
            OverlayWaitOutcome::Submitted(ConflictResolution::Abort) => {
                return Ok(aborted_conflict_status(&conflict));
            }
            OverlayWaitOutcome::Submitted(ConflictResolution::Proceed) => {
                let override_args = build_override_args(args_val, &conflict)?;
                status = execute_with_cache_and_streaming(
                    registry,
                    tool_result_cache,
                    name,
                    tool_item_id,
                    &override_args,
                    ctrl_c_state,
                    ctrl_c_notify,
                    handle,
                    harness_emitter.clone(),
                    vt_cfg,
                    max_tool_retries,
                )
                .await;
            }
            OverlayWaitOutcome::Cancelled => {
                return Ok(aborted_conflict_status(&conflict));
            }
            OverlayWaitOutcome::Interrupted | OverlayWaitOutcome::Exit => {
                return Ok(ToolExecutionStatus::Cancelled);
            }
        }
    }
}

impl PendingFileConflict {
    fn finalized_output(
        &self,
        resolution: &str,
        message: &str,
        disk_content: Option<String>,
    ) -> Value {
        let mut output = self.output.clone();
        if let Some(obj) = output.as_object_mut() {
            obj.insert("resolution".to_string(), json!(resolution));
            obj.insert("message".to_string(), json!(message));
            obj.insert("emit_hitl_notification".to_string(), Value::Bool(false));
            obj.insert(
                "disk_content".to_string(),
                disk_content.map(Value::String).unwrap_or(Value::Null),
            );
        }
        output
    }
}

async fn extract_pending_conflict(
    registry: &ToolRegistry,
    status: &ToolExecutionStatus,
) -> Result<Option<PendingFileConflict>> {
    let ToolExecutionStatus::Success { output, .. } = status else {
        return Ok(None);
    };
    if output
        .get(FILE_CONFLICT_DETECTED_FIELD)
        .and_then(Value::as_bool)
        != Some(true)
    {
        return Ok(None);
    }

    let Some(display_path) = output
        .get(FILE_CONFLICT_PATH_FIELD)
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
    else {
        return Ok(None);
    };
    let absolute_path = registry.file_ops_tool().normalize_user_path(&display_path).await?;
    let message = output
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("File changed on disk since the agent last read it.")
        .to_string();

    Ok(Some(PendingFileConflict {
        output: output.clone(),
        display_path,
        absolute_path,
        message,
        approved_snapshot: output.get("disk_snapshot").cloned().filter(Value::is_object),
        disk_content: output
            .get("disk_content")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        intended_content: output
            .get("intended_content")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        emit_hitl_notification: output
            .get("emit_hitl_notification")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    }))
}

async fn prompt_for_conflict_resolution<S>(
    session: &mut S,
    handle: &InlineHandle,
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
    conflict: &PendingFileConflict,
    hitl_notification_bell: bool,
) -> Result<OverlayWaitOutcome<ConflictResolution>>
where
    S: UiSession + ?Sized,
{
    let can_show_diff = conflict.approved_snapshot.is_some()
        && conflict.disk_content.is_some()
        && conflict.intended_content.is_some();
    if hitl_notification_bell && conflict.emit_hitl_notification {
        let _ = send_global_notification(NotificationEvent::HumanInTheLoop {
            prompt: "File conflict requires review".to_string(),
            context: format!("File: {}", conflict.display_path),
        })
        .await;
    }

    let lines = vec![
        conflict.message.clone(),
        format!("File: {}", conflict.display_path),
        if can_show_diff {
            "Reload uses the external disk version. View diff shows disk content versus the agent's intended write.".to_string()
        } else {
            "Reload uses the external disk version. Diff preview is unavailable for this file content.".to_string()
        },
    ];
    let outcome = show_overlay_and_wait(
        handle,
        session,
        OverlayRequest::List(ListOverlayRequest {
            title: "File Changed On Disk".to_string(),
            lines,
            footer_hint: None,
            items: conflict_resolution_items(can_show_diff),
            selected: Some(InlineListSelection::FileConflictReload),
            search: None,
            hotkeys: Vec::new(),
        }),
        ctrl_c_state,
        ctrl_c_notify,
        |submission| match submission {
            OverlaySubmission::Selection(InlineListSelection::FileConflictReload) => {
                Some(InlineListSelection::FileConflictReload)
            }
            OverlaySubmission::Selection(InlineListSelection::FileConflictAbort) => {
                Some(InlineListSelection::FileConflictAbort)
            }
            OverlaySubmission::Selection(InlineListSelection::FileConflictViewDiff)
                if can_show_diff =>
            {
                Some(InlineListSelection::FileConflictViewDiff)
            }
            _ => None,
        },
    )
    .await?;

    match outcome {
        OverlayWaitOutcome::Submitted(InlineListSelection::FileConflictReload) => {
            Ok(OverlayWaitOutcome::Submitted(ConflictResolution::Reload))
        }
        OverlayWaitOutcome::Submitted(InlineListSelection::FileConflictAbort) => {
            Ok(OverlayWaitOutcome::Submitted(ConflictResolution::Abort))
        }
        OverlayWaitOutcome::Submitted(InlineListSelection::FileConflictViewDiff)
            if can_show_diff =>
        {
            show_overlay_and_wait(
                handle,
                session,
                OverlayRequest::Diff(DiffOverlayRequest {
                    file_path: conflict.display_path.clone(),
                    before: conflict.disk_content.clone().unwrap_or_default(),
                    after: conflict.intended_content.clone().unwrap_or_default(),
                    hunks: Vec::new(),
                    current_hunk: 0,
                    mode: DiffPreviewMode::FileConflict,
                }),
                ctrl_c_state,
                ctrl_c_notify,
                |submission| match submission {
                    OverlaySubmission::DiffProceed => Some(ConflictResolution::Proceed),
                    OverlaySubmission::DiffReload => Some(ConflictResolution::Reload),
                    OverlaySubmission::DiffAbort => Some(ConflictResolution::Abort),
                    _ => None,
                },
            )
            .await
        }
        OverlayWaitOutcome::Cancelled => Ok(OverlayWaitOutcome::Cancelled),
        OverlayWaitOutcome::Interrupted => Ok(OverlayWaitOutcome::Interrupted),
        OverlayWaitOutcome::Exit => Ok(OverlayWaitOutcome::Exit),
        OverlayWaitOutcome::Submitted(_) => unreachable!("handled list selections above"),
    }
}

fn conflict_resolution_items(can_show_diff: bool) -> Vec<InlineListItem> {
    let mut items = Vec::with_capacity(if can_show_diff { 3 } else { 2 });
    items.push(InlineListItem {
        title: "Reload from disk".to_string(),
        subtitle: Some(
            "Discard pending agent changes and continue from the external version.".to_string(),
        ),
        badge: None,
        indent: 0,
        selection: Some(InlineListSelection::FileConflictReload),
        search_value: Some("reload disk external version".to_string()),
    });
    if can_show_diff {
        items.push(InlineListItem {
            title: "View unified diff".to_string(),
            subtitle: Some(
                "Review external changes against the agent's intended write.".to_string(),
            ),
            badge: None,
            indent: 0,
            selection: Some(InlineListSelection::FileConflictViewDiff),
            search_value: Some("diff compare review changes".to_string()),
        });
    }
    items.push(InlineListItem {
        title: "Abort".to_string(),
        subtitle: Some("Cancel this write and leave disk unchanged.".to_string()),
        badge: None,
        indent: 0,
        selection: Some(InlineListSelection::FileConflictAbort),
        search_value: Some("abort cancel stop".to_string()),
    });
    items
}

fn build_override_args(args_val: &Value, conflict: &PendingFileConflict) -> Result<Value> {
    let mut args = args_val.clone();
    let Some(map) = args.as_object_mut() else {
        return Err(anyhow!(
            "Cannot override a file-conflict tool call without structured arguments"
        ));
    };
    let approved_snapshot = conflict.approved_snapshot.clone().ok_or_else(|| {
        anyhow!("Cannot proceed with a file-conflict write without an approved disk snapshot")
    })?;
    map.insert(FILE_CONFLICT_OVERRIDE_ARG.to_string(), approved_snapshot);
    Ok(args)
}

fn conflict_resolution_status(output: Value, command_success: bool) -> ToolExecutionStatus {
    ToolExecutionStatus::Success {
        output,
        stdout: None,
        modified_files: vec![],
        command_success,
    }
}

fn aborted_conflict_status(conflict: &PendingFileConflict) -> ToolExecutionStatus {
    conflict_resolution_status(
        conflict.finalized_output(
            "aborted",
            "Aborted the agent write because the file changed on disk.",
            None,
        ),
        false,
    )
}

async fn read_disk_text(path: &Path) -> Option<String> {
    let bytes = tokio::fs::read(path).await.ok()?;
    String::from_utf8(bytes).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use tempfile::TempDir;
    use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
    use vtcode_core::config::constants::tools;
    use vtcode_core::core::interfaces::ui::UiSession;
    use vtcode_core::tools::result_cache::ToolResultCache;
    use vtcode_tui::{InlineCommand, InlineEvent, OverlayEvent};

    use crate::agent::runloop::unified::state::CtrlCState;

    struct TestUiSession {
        handle: InlineHandle,
        events: UnboundedReceiver<InlineEvent>,
    }

    #[async_trait]
    impl UiSession for TestUiSession {
        fn inline_handle(&self) -> &InlineHandle {
            &self.handle
        }

        async fn next_event(&mut self) -> Option<InlineEvent> {
            self.events.recv().await
        }
    }

    fn test_session() -> (
        TestUiSession,
        UnboundedSender<InlineEvent>,
        UnboundedReceiver<InlineCommand>,
    ) {
        let (command_tx, command_rx) = unbounded_channel();
        let (event_tx, event_rx) = unbounded_channel();
        (
            TestUiSession {
                handle: InlineHandle::new_for_tests(command_tx),
                events: event_rx,
            },
            event_tx,
            command_rx,
        )
    }

    async fn create_registry(workspace: &TempDir) -> ToolRegistry {
        ToolRegistry::new(workspace.path().to_path_buf()).await
    }

    fn disable_hitl_notification(output: &mut Value) {
        output["emit_hitl_notification"] = Value::Bool(false);
    }

    #[tokio::test]
    async fn reload_resolution_discards_agent_write() -> Result<()> {
        let workspace = TempDir::new()?;
        let mut registry = create_registry(&workspace).await;
        let path = workspace.path().join("sample.txt");
        std::fs::write(&path, "before\n")?;
        registry.read_file(json!({ "path": "sample.txt" })).await?;
        std::fs::write(&path, "external\n")?;
        let mut conflict_output = registry
            .write_file(json!({"path": "sample.txt", "content": "agent\n", "mode": "overwrite"}))
            .await?;
        disable_hitl_notification(&mut conflict_output);

        let (mut session, event_tx, _commands) = test_session();
        let handle = session.inline_handle().clone();
        event_tx.send(InlineEvent::Overlay(OverlayEvent::Submitted(
            OverlaySubmission::Selection(InlineListSelection::FileConflictReload),
        )))?;

        let status = resolve_file_conflict_status(
            &mut registry,
            &Arc::new(tokio::sync::RwLock::new(ToolResultCache::new(8))),
            &mut session,
            &handle,
            tools::WRITE_FILE,
            "tool_1",
            &json!({"path": "sample.txt", "content": "agent\n", "mode": "overwrite"}),
            ToolExecutionStatus::Success {
                output: conflict_output,
                stdout: None,
                modified_files: vec![],
                command_success: true,
            },
            &Arc::new(CtrlCState::new()),
            &Arc::new(Notify::new()),
            None,
            None,
            0,
        )
        .await?;

        match status {
            ToolExecutionStatus::Success { output, .. } => {
                assert_eq!(output["resolution"], json!("reloaded"));
                assert_eq!(output["disk_content"], json!("external\n"));
            }
            other => panic!("unexpected status: {other:?}"),
        }
        assert_eq!(std::fs::read_to_string(path)?, "external\n");
        Ok(())
    }

    #[tokio::test]
    async fn abort_resolution_leaves_disk_unchanged() -> Result<()> {
        let workspace = TempDir::new()?;
        let mut registry = create_registry(&workspace).await;
        let path = workspace.path().join("sample.txt");
        std::fs::write(&path, "before\n")?;
        registry.read_file(json!({ "path": "sample.txt" })).await?;
        std::fs::write(&path, "external\n")?;
        let mut conflict_output = registry
            .write_file(json!({"path": "sample.txt", "content": "agent\n", "mode": "overwrite"}))
            .await?;
        disable_hitl_notification(&mut conflict_output);

        let (mut session, event_tx, _commands) = test_session();
        let handle = session.inline_handle().clone();
        event_tx.send(InlineEvent::Overlay(OverlayEvent::Submitted(
            OverlaySubmission::Selection(InlineListSelection::FileConflictAbort),
        )))?;

        let status = resolve_file_conflict_status(
            &mut registry,
            &Arc::new(tokio::sync::RwLock::new(ToolResultCache::new(8))),
            &mut session,
            &handle,
            tools::WRITE_FILE,
            "tool_1",
            &json!({"path": "sample.txt", "content": "agent\n", "mode": "overwrite"}),
            ToolExecutionStatus::Success {
                output: conflict_output,
                stdout: None,
                modified_files: vec![],
                command_success: true,
            },
            &Arc::new(CtrlCState::new()),
            &Arc::new(Notify::new()),
            None,
            None,
            0,
        )
        .await?;

        match status {
            ToolExecutionStatus::Success {
                output,
                command_success,
                ..
            } => {
                assert!(!command_success);
                assert_eq!(output["resolution"], json!("aborted"));
            }
            other => panic!("unexpected status: {other:?}"),
        }
        assert_eq!(std::fs::read_to_string(path)?, "external\n");
        Ok(())
    }

    #[tokio::test]
    async fn proceed_resolution_reexecutes_with_override() -> Result<()> {
        let workspace = TempDir::new()?;
        let mut registry = create_registry(&workspace).await;
        let path = workspace.path().join("sample.txt");
        std::fs::write(&path, "before\n")?;
        registry.read_file(json!({ "path": "sample.txt" })).await?;
        std::fs::write(&path, "external\n")?;
        let mut conflict_output = registry
            .write_file(json!({"path": "sample.txt", "content": "agent\n", "mode": "overwrite"}))
            .await?;
        disable_hitl_notification(&mut conflict_output);

        let (mut session, event_tx, _commands) = test_session();
        let handle = session.inline_handle().clone();
        event_tx.send(InlineEvent::Overlay(OverlayEvent::Submitted(
            OverlaySubmission::Selection(InlineListSelection::FileConflictViewDiff),
        )))?;
        event_tx.send(InlineEvent::Overlay(OverlayEvent::Submitted(
            OverlaySubmission::DiffProceed,
        )))?;

        let status = resolve_file_conflict_status(
            &mut registry,
            &Arc::new(tokio::sync::RwLock::new(ToolResultCache::new(8))),
            &mut session,
            &handle,
            tools::WRITE_FILE,
            "tool_1",
            &json!({"path": "sample.txt", "content": "agent\n", "mode": "overwrite"}),
            ToolExecutionStatus::Success {
                output: conflict_output,
                stdout: None,
                modified_files: vec![],
                command_success: true,
            },
            &Arc::new(CtrlCState::new()),
            &Arc::new(Notify::new()),
            None,
            None,
            0,
        )
        .await?;

        match status {
            ToolExecutionStatus::Success { output, .. } => {
                assert_ne!(output["resolution"], json!("pending"));
            }
            other => panic!("unexpected status: {other:?}"),
        }
        assert_eq!(std::fs::read_to_string(path)?, "agent\n");
        Ok(())
    }
}
