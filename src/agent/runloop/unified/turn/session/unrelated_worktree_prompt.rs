use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Result;
use tokio::sync::Notify;
use vtcode_core::core::interfaces::ui::UiSession;
use vtcode_core::notifications::{NotificationEvent, send_global_notification};
use vtcode_tui::app::{
    DiffOverlayRequest, DiffPreviewMode, InlineHandle, InlineListItem, InlineListSelection,
    OverlayRequest, OverlaySubmission, WizardModalMode, WizardStep,
};

use crate::agent::runloop::git::{
    DirtyWorktreeEntry, git_diff_preview_for_path, workspace_relative_display,
};
use crate::agent::runloop::unified::overlay_prompt::{OverlayWaitOutcome, show_overlay_and_wait};
use crate::agent::runloop::unified::state::CtrlCState;

const QUESTION_ID: &str = "unrelated_worktree";
const CHOICE_LEAVE_AS_IS: &str = "leave_as_is";
const CHOICE_INSPECT_CHANGES: &str = "inspect_changes";
const CHOICE_STOP_HERE: &str = "stop_here";

#[derive(Default)]
pub(crate) struct UnrelatedWorktreePromptState {
    acknowledged_fingerprints: BTreeMap<PathBuf, String>,
}

impl UnrelatedWorktreePromptState {
    pub(crate) fn is_acknowledged(&self, entry: &DirtyWorktreeEntry) -> bool {
        self.acknowledged_fingerprints
            .get(&entry.path)
            .is_some_and(|fingerprint| fingerprint == &entry.fingerprint)
    }

    pub(crate) fn acknowledge(&mut self, entry: &DirtyWorktreeEntry) {
        self.acknowledged_fingerprints
            .insert(entry.path.clone(), entry.fingerprint.clone());
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum UnrelatedWorktreePromptOutcome {
    LeaveAsIs,
    StopHere,
    Other(String),
    Exit,
}

pub(crate) async fn prompt_for_unrelated_worktree_change<S>(
    handle: &InlineHandle,
    session: &mut S,
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
    workspace: &Path,
    entry: &DirtyWorktreeEntry,
    hitl_notification_bell: bool,
) -> Result<UnrelatedWorktreePromptOutcome>
where
    S: UiSession + ?Sized,
{
    let display_path = workspace_relative_display(workspace, &entry.path);
    if hitl_notification_bell {
        let _ = send_global_notification(NotificationEvent::HumanInTheLoop {
            prompt: "Unrelated worktree change requires review".to_string(),
            context: format!("File: {display_path}"),
        })
        .await;
    }

    loop {
        let outcome = show_overlay_and_wait(
            handle,
            session,
            OverlayRequest::Wizard(vtcode_tui::app::WizardOverlayRequest {
                title: "VT Code is requesting information".to_string(),
                steps: vec![build_prompt_step(&display_path)],
                current_step: 0,
                search: None,
                mode: WizardModalMode::TabbedList,
            }),
            ctrl_c_state,
            ctrl_c_notify,
            |submission| match submission {
                OverlaySubmission::Wizard(mut selections) => selections.pop(),
                _ => None,
            },
        )
        .await?;

        match outcome {
            OverlayWaitOutcome::Submitted(InlineListSelection::RequestUserInputAnswer {
                selected,
                other,
                ..
            }) => {
                let choice = selected.first().map(String::as_str);
                let note = other
                    .map(|text| text.trim().to_string())
                    .filter(|text| !text.is_empty());
                match choice {
                    Some(CHOICE_LEAVE_AS_IS) => {
                        return Ok(UnrelatedWorktreePromptOutcome::LeaveAsIs);
                    }
                    Some(CHOICE_STOP_HERE) => {
                        return Ok(UnrelatedWorktreePromptOutcome::StopHere);
                    }
                    Some(CHOICE_INSPECT_CHANGES) => {
                        if let Some(preview) = git_diff_preview_for_path(workspace, &entry.path)? {
                            let _ = show_overlay_and_wait(
                                handle,
                                session,
                                OverlayRequest::Diff(DiffOverlayRequest {
                                    file_path: preview.display_path,
                                    before: preview.before,
                                    after: preview.after,
                                    hunks: Vec::new(),
                                    current_hunk: 0,
                                    mode: DiffPreviewMode::ReadonlyReview,
                                }),
                                ctrl_c_state,
                                ctrl_c_notify,
                                |submission| match submission {
                                    OverlaySubmission::DiffAbort
                                    | OverlaySubmission::DiffProceed
                                    | OverlaySubmission::DiffReject => Some(()),
                                    _ => None,
                                },
                            )
                            .await?;
                        }
                        continue;
                    }
                    _ => {
                        if let Some(note) = note {
                            return Ok(UnrelatedWorktreePromptOutcome::Other(note));
                        }
                    }
                }
            }
            OverlayWaitOutcome::Cancelled | OverlayWaitOutcome::Interrupted => {
                return Ok(UnrelatedWorktreePromptOutcome::StopHere);
            }
            OverlayWaitOutcome::Exit => return Ok(UnrelatedWorktreePromptOutcome::Exit),
            OverlayWaitOutcome::Submitted(_) => {}
        }
    }
}

fn build_prompt_step(display_path: &str) -> WizardStep {
    WizardStep {
        title: "Action".to_string(),
        question: format!(
            "I found an unrelated modified file in the worktree: `{display_path}`. I didn't change it as part of this task.\nHow would you like me to proceed?\n\nChoose an action:"
        ),
        items: vec![
            choice_item(CHOICE_LEAVE_AS_IS),
            choice_item(CHOICE_INSPECT_CHANGES),
            choice_item(CHOICE_STOP_HERE),
            InlineListItem {
                title: "Other (type your answer)".to_string(),
                subtitle: None,
                badge: None,
                indent: 0,
                selection: Some(InlineListSelection::RequestUserInputAnswer {
                    question_id: QUESTION_ID.to_string(),
                    selected: Vec::new(),
                    other: Some(String::new()),
                }),
                search_value: Some("other custom guidance".to_string()),
            },
        ],
        completed: false,
        answer: None,
        allow_freeform: true,
        freeform_label: Some("Other".to_string()),
        freeform_placeholder: Some("type your answer".to_string()),
    }
}

fn choice_item(choice: &str) -> InlineListItem {
    InlineListItem {
        title: choice.to_string(),
        subtitle: None,
        badge: None,
        indent: 0,
        selection: Some(InlineListSelection::RequestUserInputAnswer {
            question_id: QUESTION_ID.to_string(),
            selected: vec![choice.to_string()],
            other: None,
        }),
        search_value: Some(choice.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::fs;
    use std::process::Command;
    use tempfile::TempDir;
    use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
    use vtcode_core::core::interfaces::ui::UiSession;
    use vtcode_tui::app::{InlineCommand, InlineEvent, OverlayEvent};

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

    fn init_repo() -> TempDir {
        let temp = TempDir::new().expect("temp dir");
        let run = |args: &[&str]| {
            let status = Command::new("git")
                .args(args)
                .current_dir(temp.path())
                .status()
                .expect("git command");
            assert!(status.success(), "git command failed: {args:?}");
        };

        run(&["init"]);
        run(&["config", "user.name", "VT Code"]);
        run(&["config", "user.email", "vtcode@example.com"]);
        temp
    }

    #[tokio::test]
    async fn leave_as_is_returns_ack_choice() -> Result<()> {
        let workspace = TempDir::new()?;
        let path = workspace.path().join("docs/project/TODO.md");
        fs::create_dir_all(path.parent().expect("parent"))?;
        fs::write(&path, "changed\n")?;
        let entry = DirtyWorktreeEntry {
            path,
            status: crate::agent::runloop::git::DirtyWorktreeStatus::Modified,
            fingerprint: "fp".to_string(),
        };

        let (mut session, event_tx, _commands) = test_session();
        event_tx.send(InlineEvent::Overlay(OverlayEvent::Submitted(
            OverlaySubmission::Wizard(vec![InlineListSelection::RequestUserInputAnswer {
                question_id: QUESTION_ID.to_string(),
                selected: vec![CHOICE_LEAVE_AS_IS.to_string()],
                other: None,
            }]),
        )))?;

        let handle = session.inline_handle().clone();
        let outcome = prompt_for_unrelated_worktree_change(
            &handle,
            &mut session,
            &Arc::new(CtrlCState::new()),
            &Arc::new(Notify::new()),
            workspace.path(),
            &entry,
            false,
        )
        .await?;

        assert!(matches!(outcome, UnrelatedWorktreePromptOutcome::LeaveAsIs));
        Ok(())
    }

    #[tokio::test]
    async fn custom_other_submission_returns_text() -> Result<()> {
        let workspace = TempDir::new()?;
        let path = workspace.path().join("docs/project/TODO.md");
        fs::create_dir_all(path.parent().expect("parent"))?;
        fs::write(&path, "changed\n")?;
        let entry = DirtyWorktreeEntry {
            path,
            status: crate::agent::runloop::git::DirtyWorktreeStatus::Modified,
            fingerprint: "fp".to_string(),
        };

        let (mut session, event_tx, _commands) = test_session();
        event_tx.send(InlineEvent::Overlay(OverlayEvent::Submitted(
            OverlaySubmission::Wizard(vec![InlineListSelection::RequestUserInputAnswer {
                question_id: QUESTION_ID.to_string(),
                selected: vec![],
                other: Some("inspect this manually".to_string()),
            }]),
        )))?;

        let handle = session.inline_handle().clone();
        let outcome = prompt_for_unrelated_worktree_change(
            &handle,
            &mut session,
            &Arc::new(CtrlCState::new()),
            &Arc::new(Notify::new()),
            workspace.path(),
            &entry,
            false,
        )
        .await?;

        match outcome {
            UnrelatedWorktreePromptOutcome::Other(text) => {
                assert_eq!(text, "inspect this manually");
            }
            other => panic!("unexpected outcome: {other:?}"),
        }
        Ok(())
    }

    #[tokio::test]
    async fn inspect_changes_returns_to_prompt_after_diff_review() -> Result<()> {
        let workspace = init_repo();
        let path = workspace.path().join("docs/project/TODO.md");
        fs::create_dir_all(path.parent().expect("parent"))?;
        fs::write(&path, "before\n")?;

        let run = |args: &[&str]| {
            let status = Command::new("git")
                .args(args)
                .current_dir(workspace.path())
                .status()
                .expect("git command");
            assert!(status.success(), "git command failed: {args:?}");
        };

        run(&["add", "."]);
        run(&["commit", "-m", "test: seed repo"]);
        fs::write(&path, "after\n")?;

        let entry = DirtyWorktreeEntry {
            path,
            status: crate::agent::runloop::git::DirtyWorktreeStatus::Modified,
            fingerprint: "fp".to_string(),
        };

        let (mut session, event_tx, _commands) = test_session();
        event_tx.send(InlineEvent::Overlay(OverlayEvent::Submitted(
            OverlaySubmission::Wizard(vec![InlineListSelection::RequestUserInputAnswer {
                question_id: QUESTION_ID.to_string(),
                selected: vec![CHOICE_INSPECT_CHANGES.to_string()],
                other: None,
            }]),
        )))?;
        event_tx.send(InlineEvent::Overlay(OverlayEvent::Submitted(
            OverlaySubmission::DiffAbort,
        )))?;
        event_tx.send(InlineEvent::Overlay(OverlayEvent::Submitted(
            OverlaySubmission::Wizard(vec![InlineListSelection::RequestUserInputAnswer {
                question_id: QUESTION_ID.to_string(),
                selected: vec![CHOICE_STOP_HERE.to_string()],
                other: None,
            }]),
        )))?;

        let handle = session.inline_handle().clone();
        let outcome = prompt_for_unrelated_worktree_change(
            &handle,
            &mut session,
            &Arc::new(CtrlCState::new()),
            &Arc::new(Notify::new()),
            workspace.path(),
            &entry,
            false,
        )
        .await?;

        assert!(matches!(outcome, UnrelatedWorktreePromptOutcome::StopHere));
        Ok(())
    }
}
