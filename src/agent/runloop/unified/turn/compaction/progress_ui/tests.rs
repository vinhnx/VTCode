use super::*;
use tokio::sync::mpsc::unbounded_channel;
use vtcode_ui::tui::app::{InlineCommand, InlineHandle};

#[test]
fn compacted_summary_includes_strategy_duration_and_estimated_tokens_saved() {
    let label = format_compacted_summary(Some(10_000), Some(3_900), "local", Duration::from_secs(74));
    assert_eq!(label, "Context compacted · 1m 14s · local · ↓ 6.1k tokens");
}

#[test]
fn compacted_summary_reports_growth_and_unavailable_estimates_honestly() {
    let grew = format_compacted_summary(Some(3_000), Some(3_600), "provider", Duration::from_secs(2));
    assert_eq!(grew, "Context compacted · 2s · provider · ↑ 600 tokens");

    let unavailable = format_compacted_summary(None, Some(3_600), "local", Duration::from_secs(2));
    assert_eq!(unavailable, "Context compacted · 2s · local · token savings unavailable");
}

#[test]
fn already_compact_includes_duration() {
    let label = format_already_compact(Duration::from_secs(3));
    assert!(label.contains("already compact"));
    assert!(label.contains("3s"));
}

#[test]
fn failed_label_includes_duration() {
    let error = anyhow::anyhow!("Unknown parameter: 'stream'. Body: {{\"error\":\"duplicate\"}}")
        .context("Failed to compact history via provider-native compaction");
    let label = format_compaction_failed(Duration::from_secs(5), &error);
    assert!(label.contains("compaction failed"));
    assert!(label.contains("5s"));
    assert!(label.contains("History preserved"));
    assert!(label.contains("Unknown parameter: 'stream'"));
    assert!(!label.contains("duplicate"));
}

#[test]
fn failed_label_bounds_untrusted_unicode_diagnostics() {
    let error = anyhow::anyhow!("bad response\n{}", "故障".repeat(1000));
    let label = format_compaction_failed(Duration::from_secs(1), &error);
    assert!(!label.contains('\n'));
    assert!(label.chars().count() < 650);
    assert!(label.contains("..."));
}

#[test]
fn compaction_debug_details_preserve_chain_and_redact_secrets() {
    let secret = "sk-proj-abcdefghijklmnopqrstuvwxyz0123456789";
    let error = anyhow::anyhow!("provider rejected api_key={secret}").context("Compaction request failed");
    let details = format_compaction_error_details(&error);
    assert!(details.contains("Compaction request failed"));
    assert!(details.contains("provider rejected"));
    assert!(!details.contains(secret));
    let label = format_compaction_failed(Duration::from_secs(1), &error);
    assert!(!label.contains(secret));
}

#[tokio::test]
async fn owned_compaction_progress_finishes_on_success_or_drop() {
    let (sender, mut receiver) = unbounded_channel();
    let handle = InlineHandle::new_for_tests(sender);
    let progress = CompactionProgressGuard::start(&handle, Some(12_345));
    let operation = progress.operation;
    assert!(matches!(
        receiver.recv().await,
        Some(InlineCommand::UpdateProgress(ProgressUpdate::Begin {
            operation: started,
            phase: ProgressPhase::Compacting { estimated_input_tokens: Some(12_345), .. },
        })) if started == operation
    ));
    progress.finish();
    assert!(matches!(
        receiver.recv().await,
        Some(InlineCommand::UpdateProgress(ProgressUpdate::Finish { operation: finished })) if finished == operation
    ));
    assert_eq!(handle.current_progress_operation(), None);

    let dropped = CompactionProgressGuard::start(&handle, None);
    let dropped_operation = dropped.operation;
    assert!(matches!(
        receiver.recv().await,
        Some(InlineCommand::UpdateProgress(ProgressUpdate::Begin {
            operation: started,
            phase: ProgressPhase::Compacting { estimated_input_tokens: None, .. },
        })) if started == dropped_operation
    ));
    drop(dropped);
    assert!(matches!(
        receiver.recv().await,
        Some(InlineCommand::UpdateProgress(ProgressUpdate::Finish { operation: finished })) if finished == dropped_operation
    ));
    assert_eq!(handle.current_progress_operation(), None);
}

#[tokio::test]
async fn scoped_compaction_progress_restores_only_its_original_operation() {
    let (sender, mut receiver) = unbounded_channel();
    let handle = InlineHandle::new_for_tests(sender);
    let original = handle.begin_progress(ProgressPhase::WaitingForModel);
    let operation = original.operation();
    assert!(matches!(
        receiver.recv().await,
        Some(InlineCommand::UpdateProgress(ProgressUpdate::Begin { operation: started, .. }))
            if started == operation
    ));

    let scoped = CompactionProgressGuard::start(&handle, Some(9_876));
    assert_eq!(scoped.operation, operation);
    assert!(matches!(
        receiver.recv().await,
        Some(InlineCommand::UpdateProgress(ProgressUpdate::Phase {
            operation: updated,
            phase: ProgressPhase::Compacting { estimated_input_tokens: Some(9_876), .. },
        })) if updated == operation
    ));
    drop(scoped);
    assert!(matches!(
        receiver.recv().await,
        Some(InlineCommand::UpdateProgress(ProgressUpdate::Phase {
            operation: restored,
            phase: ProgressPhase::WaitingForModel,
        })) if restored == operation
    ));
    assert_eq!(handle.current_progress_operation(), Some(operation));

    let replacement = handle.begin_progress(ProgressPhase::Processing);
    let replacement_operation = replacement.operation();
    assert!(matches!(
        receiver.recv().await,
        Some(InlineCommand::UpdateProgress(ProgressUpdate::Begin { operation: started, .. }))
            if started == replacement_operation
    ));
    let stale_scope = CompactionProgressGuard::start(&handle, Some(100));
    assert_eq!(stale_scope.operation, replacement_operation);
    assert!(matches!(receiver.recv().await, Some(InlineCommand::UpdateProgress(ProgressUpdate::Phase { .. }))));
    let newer = handle.begin_progress(ProgressPhase::SavingCheckpoint);
    let newer_operation = newer.operation();
    assert!(matches!(
        receiver.recv().await,
        Some(InlineCommand::UpdateProgress(ProgressUpdate::Begin { operation: started, .. }))
            if started == newer_operation
    ));
    drop(stale_scope);
    assert!(receiver.try_recv().is_err(), "stale scope must not restore a replaced operation");
    drop(original);
    assert!(matches!(
        receiver.recv().await,
        Some(InlineCommand::UpdateProgress(ProgressUpdate::Finish { operation: finished })) if finished == operation
    ));
    assert_eq!(handle.current_progress_operation(), Some(newer_operation));
    drop(newer);
    assert!(matches!(
        receiver.recv().await,
        Some(InlineCommand::UpdateProgress(ProgressUpdate::Finish { operation: finished })) if finished == newer_operation
    ));
    drop(replacement);
    assert!(matches!(
        receiver.recv().await,
        Some(InlineCommand::UpdateProgress(ProgressUpdate::Finish { operation: finished })) if finished == replacement_operation
    ));
    assert_eq!(handle.current_progress_operation(), None);
}

#[tokio::test]
async fn scoped_compaction_progress_preserves_newer_phases_and_finished_operations() {
    let (sender, mut receiver) = unbounded_channel();
    let handle = InlineHandle::new_for_tests(sender);
    let owner = handle.begin_progress(ProgressPhase::PreparingContext);
    let progress = CompactionProgressGuard::start(&handle, Some(1_234));
    handle.set_progress_phase(ProgressPhase::WaitingForModel);
    while receiver.try_recv().is_ok() {}

    drop(progress);
    assert!(receiver.try_recv().is_err(), "a later phase on the same operation must win");
    let previous = handle.replace_progress_phase(ProgressPhase::Processing);
    assert_eq!(
        previous,
        Some(ProgressUpdate::Phase {
            operation: owner.operation(),
            phase: ProgressPhase::WaitingForModel
        })
    );

    let progress = CompactionProgressGuard::start(&handle, None);
    drop(owner);
    while receiver.try_recv().is_ok() {}
    drop(progress);
    assert!(receiver.try_recv().is_err(), "a finished operation must not restart");
    assert_eq!(handle.current_progress_operation(), None);
}

#[tokio::test]
async fn cancelled_compaction_future_releases_owned_or_scoped_progress() {
    for scoped in [false, true] {
        let (sender, mut receiver) = unbounded_channel();
        let handle = InlineHandle::new_for_tests(sender);
        let parent = scoped.then(|| handle.begin_progress(ProgressPhase::PreparingContext));
        while receiver.try_recv().is_ok() {}
        let worker_handle = handle.clone();
        let worker = tokio::spawn(async move {
            let _progress = CompactionProgressGuard::start(&worker_handle, Some(9_876));
            std::future::pending::<()>().await;
        });
        let operation = match receiver.recv().await {
            Some(InlineCommand::UpdateProgress(
                ProgressUpdate::Begin { operation, phase: ProgressPhase::Compacting { .. } }
                | ProgressUpdate::Phase { operation, phase: ProgressPhase::Compacting { .. } },
            )) => operation,
            _ => panic!("expected active compaction progress"),
        };
        worker.abort();
        assert!(worker.await.expect_err("worker should be cancelled").is_cancelled());
        let expected = if scoped {
            ProgressUpdate::Phase { operation, phase: ProgressPhase::PreparingContext }
        } else {
            ProgressUpdate::Finish { operation }
        };
        assert!(matches!(receiver.recv().await, Some(InlineCommand::UpdateProgress(update)) if update == expected));
        assert_eq!(handle.current_progress_operation(), parent.as_ref().map(ProgressGuard::operation));
    }
}
