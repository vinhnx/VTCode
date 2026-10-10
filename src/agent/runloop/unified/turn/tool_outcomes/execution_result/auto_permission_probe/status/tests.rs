use super::*;
use tokio::sync::mpsc;
use vtcode_ui::tui::app::InlineCommand;

fn statuses(receiver: &mut mpsc::UnboundedReceiver<InlineCommand>) -> Vec<(Option<String>, Option<String>)> {
    let mut statuses = Vec::new();
    while let Ok(command) = receiver.try_recv() {
        if let InlineCommand::SetInputStatus { left, right } = command {
            statuses.push((left, right));
        }
    }
    statuses
}

async fn settle_spinner() {
    tokio::task::yield_now().await;
    tokio::time::sleep(std::time::Duration::from_millis(1)).await;
}

#[tokio::test]
async fn decisions_probe_status_restores_sequential_and_borrowed_batch_owner() {
    for parallel in [false, true] {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let handle = InlineHandle::new_for_tests(tx);
        let _progress = handle.begin_progress(ProgressPhase::RunningTools);
        let state = InputStatusState {
            left: Some("Ready".into()),
            right: Some("Model".into()),
            ..Default::default()
        };
        let stop = CtrlCState::new();
        let batch = parallel.then(|| start_loading_status(&handle, &state, "Executing 2 tools..."));
        {
            let _status = ProbeStatus::new(&handle, &state, &stop, batch.as_ref());
            settle_spinner().await;
            let during = statuses(&mut rx);
            assert!(
                during
                    .iter()
                    .any(|(left, _)| left.as_deref().is_some_and(|left| left.contains("Checking tool output...")))
            );
            assert!(during.iter().all(|(_, right)| right.as_deref() == Some("Model")));
        }
        settle_spinner().await;
        let after = statuses(&mut rx);
        if let Some(batch) = batch {
            assert!(after.last().unwrap().0.as_deref().unwrap().starts_with("Executing 2 tools..."));
            assert!(!after.iter().any(|(left, _)| left.as_deref() == Some("Ready")));
            batch.update_message("Executing remaining tool...");
            settle_spinner().await;
            assert!(
                statuses(&mut rx)
                    .last()
                    .unwrap()
                    .0
                    .as_deref()
                    .unwrap()
                    .starts_with("Executing remaining tool...")
            );
            drop(batch);
            assert_eq!(statuses(&mut rx).last(), Some(&(Some("Ready".into()), Some("Model".into()))));
        } else {
            assert_eq!(after.last(), Some(&(Some("Ready".into()), Some("Model".into()))));
        }
    }
}

#[tokio::test]
async fn decisions_probe_progress_phase_restores_owner_and_finishes_on_stop() {
    for previous_phase in [ProgressPhase::RunningTools, ProgressPhase::CheckingPermissions] {
        for stop_requested in [false, true] {
            let (tx, mut rx) = mpsc::unbounded_channel();
            let handle = InlineHandle::new_for_tests(tx);
            let progress = handle.begin_progress(previous_phase);
            let operation = progress.operation();
            let stop = CtrlCState::new();
            let status = ProbeStatus::new(&handle, &InputStatusState::default(), &stop, None);
            let mut phases = Vec::new();
            while let Ok(command) = rx.try_recv() {
                if let InlineCommand::UpdateProgress(update) = command {
                    phases.push(update);
                }
            }
            assert_eq!(
                phases.last(),
                Some(&ProgressUpdate::Phase {
                    operation,
                    phase: ProgressPhase::CheckingToolOutput
                })
            );
            if stop_requested {
                stop.request_local_cancel();
            }
            drop(status);
            let mut completion = None;
            while let Ok(command) = rx.try_recv() {
                if let InlineCommand::UpdateProgress(update) = command {
                    completion = Some(update);
                }
            }
            assert_eq!(
                completion,
                Some(if stop_requested {
                    ProgressUpdate::Finish { operation }
                } else {
                    ProgressUpdate::Phase { operation, phase: previous_phase }
                })
            );
        }
    }
}

#[tokio::test]
async fn decisions_probe_progress_does_not_restore_over_a_replacement_owner() {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let handle = InlineHandle::new_for_tests(tx);
    let _original = handle.begin_progress(ProgressPhase::RunningTools);
    let stop = CtrlCState::new();
    let status = ProbeStatus::new(&handle, &InputStatusState::default(), &stop, None);
    let replacement = handle.begin_progress(ProgressPhase::PreparingContext);
    while rx.try_recv().is_ok() {}
    drop(status);
    while let Ok(command) = rx.try_recv() {
        assert!(!matches!(command, InlineCommand::UpdateProgress(_)));
    }
    assert_eq!(handle.current_progress_operation(), Some(replacement.operation()));
}

#[tokio::test]
async fn decisions_probe_status_cancellation_and_exit_never_restore_stale_activity() {
    for parallel in [false, true] {
        for exit in [false, true] {
            let (tx, mut rx) = mpsc::unbounded_channel();
            let handle = InlineHandle::new_for_tests(tx);
            let state = InputStatusState {
                left: Some("Previous activity".into()),
                ..Default::default()
            };
            let stop = CtrlCState::new();
            let batch = parallel.then(|| start_loading_status(&handle, &state, "Executing 2 tools..."));
            let status = ProbeStatus::new(&handle, &state, &stop, batch.as_ref());
            settle_spinner().await;
            statuses(&mut rx);
            if exit {
                stop.request_exit();
            } else {
                stop.request_local_cancel();
            }
            drop(status);
            drop(batch);
            settle_spinner().await;
            let after = statuses(&mut rx);
            assert!(!after.is_empty());
            assert!(after.iter().all(|status| status == &(None, None)));
        }
    }
}
