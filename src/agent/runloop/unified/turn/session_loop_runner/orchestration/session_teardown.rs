//! Bounded session-exit work. The caller emits the terminal event before entering this phase.
use super::super::metrics::capture_code_change_snapshot;
use super::session_bootstrap::EXIT_BACKGROUND_SHUTDOWN_TIMEOUT;
use crate::agent::runloop::git::FileStat;
use crate::agent::runloop::unified::inline_events::harness::HarnessEventEmitter;
use crate::agent::runloop::unified::state::SessionStats;
use hashbrown::HashMap;
use std::path::{Path, PathBuf};
use tokio::time::{Duration, timeout};
use vtcode_core::core::agent::snapshots::SnapshotManager;
use vtcode_core::exec::events::ThreadCompletionSubtype;
use vtcode_core::hooks::SessionEndReason;
use vtcode_core::tools::ToolRegistry;

/// Tight cap for user-driven teardown (exit/cancel/`/new`): the shell return
/// or fresh prompt is waiting, so background shutdown must not park it.
/// Normal completion keeps the full 2s budget.
const FAST_BACKGROUND_SHUTDOWN_TIMEOUT: Duration = Duration::from_millis(750);

fn is_fast_teardown(reason: SessionEndReason) -> bool {
    matches!(reason, SessionEndReason::Exit | SessionEndReason::Cancelled | SessionEndReason::NewSession)
}

pub(super) struct SessionTeardownContext<'a> {
    pub harness_emitter: Option<&'a HarnessEventEmitter>,
    pub checkpoint_manager: Option<&'a SnapshotManager>,
    pub tool_registry: &'a ToolRegistry,
    pub workspace: &'a Path,
    pub session_stats: &'a SessionStats,
    pub subtype: ThreadCompletionSubtype,
    pub session_end_reason: SessionEndReason,
}

#[derive(Default)]
pub(super) struct SessionTeardownOutput {
    pub harness_finish_error: Option<anyhow::Error>,
    pub end_code_changes: Option<HashMap<PathBuf, FileStat>>,
}

/// Publish a branch's result before another branch can exhaust the shared deadline.
pub(super) async fn retain_completed_result<T>(slot: &mut T, work: impl Future<Output = T>) {
    *slot = work.await;
}

pub(super) async fn drain_session_teardown(context: SessionTeardownContext<'_>, output: &mut SessionTeardownOutput) {
    let SessionTeardownContext {
        harness_emitter,
        checkpoint_manager,
        tool_registry,
        workspace,
        session_stats,
        subtype,
        session_end_reason,
    } = context;
    // Independent, individually-bounded teardown steps run concurrently:
    // their worst-case caps overlap (max) instead of adding (sum) the way
    // the previous sequential chain did. Every await carries a timeout; a
    // timeout only skips waiting (the OS reaps children at process exit),
    // it never leaks the terminal.
    // `/new` skips the end code-change snapshot: the delta only feeds the
    // exit summary, which `/new` bypasses via `continue`, so spawning `git`
    // here is pure latency on the fresh-prompt path.
    let is_new_session = matches!(session_end_reason, SessionEndReason::NewSession);
    let exec_shutdown_timeout = if is_fast_teardown(session_end_reason) {
        FAST_BACKGROUND_SHUTDOWN_TIMEOUT
    } else {
        EXIT_BACKGROUND_SHUTDOWN_TIMEOUT
    };
    tokio::join!(
        retain_completed_result(&mut output.harness_finish_error, async {
            let emitter = harness_emitter?;
            // Fire one best-effort session-completion notification, mirroring
            // the per-turn outcome helper. Reuses the same
            // completion_success/failure gates as turn completion.
            if timeout(
                Duration::from_millis(250),
                super::super::notifications::emit_session_completion_notification(subtype, session_stats.total_turns()),
            )
            .await
            .is_err()
            {
                tracing::debug!(target: "vtcode.harness", "session completion notification timed out during exit");
            }
            // Always attempt exporter finalization after the terminal event.
            // Optional exporters are isolated inside `HarnessEventEmitter`,
            // while a canonical persistence error is retained and returned
            // only after `finish()` has had a chance to drain/close the sink.
            match timeout(Duration::from_millis(500), emitter.finish()).await {
                Ok(Ok(())) => None,
                Ok(Err(error)) => {
                    tracing::error!(
                        target: "vtcode.harness",
                        phase = "canonical_finish",
                        error = %error,
                        "failed to finalize canonical session persistence"
                    );
                    Some(error.context("failed to finalize canonical session persistence"))
                }
                Err(_elapsed) => {
                    tracing::warn!(
                        target: "vtcode.harness",
                        phase = "canonical_finish",
                        "canonical session persistence finish timed out during exit; continuing teardown"
                    );
                    None
                }
            }
        }),
        async {
            if let Some(manager) = checkpoint_manager {
                let session_id = tool_registry.harness_context_snapshot().session_id;
                match timeout(Duration::from_millis(500), manager.complete_session_navigation(&session_id)).await {
                    Ok(Ok(())) => {}
                    Ok(Err(error)) => {
                        tracing::debug!(%error, "checkpoint navigation trim failed after thread completion")
                    }
                    Err(_elapsed) => {
                        tracing::debug!(%session_id, "checkpoint navigation trim timed out during exit")
                    }
                }
            }
        },
        async {
            // Best-effort backstop: a Ctrl+C exit from idle (no turn
            // running) can leave exec/PTY sessions alive because
            // turn-level cancellation never ran. Terminate them before
            // teardown so no child outlives the TUI. `Immediate` PTY mode
            // (group SIGKILL, no SIGTERM grace window) because the user
            // asked to leave. `/new` shares this path so the previous
            // session's children cannot leak into the fresh session or
            // accumulate into a slower final process exit.
            if matches!(
                session_end_reason,
                SessionEndReason::Exit
                    | SessionEndReason::Cancelled
                    | SessionEndReason::Error
                    | SessionEndReason::NewSession
            ) {
                // Empty registries return immediately inside `terminate_all_*`
                // without touching backends, so the idle `/new` hot path pays
                // only a map snapshot + pipe-manager check.
                match timeout(exec_shutdown_timeout, tool_registry.terminate_all_exec_sessions_for_exit_async()).await {
                    Ok(Ok(())) => {}
                    Ok(Err(error)) => {
                        tracing::warn!(%error, "failed to terminate exec sessions during session exit");
                    }
                    Err(_elapsed) => {
                        tracing::warn!("timed out terminating exec sessions during session exit; continuing teardown");
                    }
                }
            }
        },
        retain_completed_result(&mut output.end_code_changes, async {
            if is_new_session {
                None
            } else {
                capture_code_change_snapshot(workspace, "end").await
            }
        }),
    );
}

/// Await filesystem cleanup on the blocking pool before starting the next lifecycle phase.
/// Bounded so a slow disk cannot park `/new` or process exit; retention
/// catches strays on the next startup.
pub(super) async fn cleanup_completed_artifacts(workspace: &Path, turn_run_id: &str, session_id: &str) {
    let workspace = workspace.to_path_buf();
    let turn_run_id = turn_run_id.to_owned();
    let session_id = session_id.to_owned();
    let cleanup_task = tokio::task::spawn_blocking(move || {
        cleanup_completed_artifacts_blocking(&workspace, &turn_run_id, &session_id);
    });
    match timeout(Duration::from_secs(1), cleanup_task).await {
        Ok(Ok(())) => {}
        Ok(Err(error)) => {
            tracing::warn!(%error, "completed session artifact cleanup task failed");
        }
        Err(_elapsed) => {
            tracing::warn!("completed session artifact cleanup timed out after 1s; continuing teardown");
        }
    }
}

/// # Blocking
/// Reads and removes session files and renames completed tracker artifacts.
fn cleanup_completed_artifacts_blocking(workspace: &Path, turn_run_id: &str, session_id: &str) {
    // Empty shells (0 turns, terminal) are not worth keeping: they pollute
    // `.vtcode/sessions/` and hide real sessions. Runs on every close path
    // (including NewSession / resume continues) after `emitter.finish()`
    // has dropped the liveness lock. Best-effort; retention catches strays.
    if let Err(error) = vtcode_memory::evict_zero_turn_completed_store(workspace, turn_run_id) {
        tracing::debug!(target: "vtcode.harness", error = %error, "zero-turn session store cleanup failed");
    }
    // A fully-checked task tracker is archived so it cannot leak into the
    // next session's memory envelope.
    {
        if let Err(error) =
            vtcode_core::core::agent::harness_artifacts::archive_completed_current_task(workspace, session_id)
        {
            tracing::debug!(%error, "completed task tracker archive failed after thread completion");
        }
    }
}

pub(super) async fn finalize_persistent_memory(
    finalize_config: vtcode_core::config::types::AgentConfig,
    finalize_vt_cfg: Option<vtcode_core::config::loader::VTCodeConfig>,
    finalize_messages: std::sync::Arc<Vec<vtcode_core::llm::provider::Message>>,
    finalize_session_id: String,
) {
    let mut finalize_task = tokio::spawn(async move {
        vtcode_core::persistent_memory::finalize_persistent_memory(
            &finalize_config,
            finalize_vt_cfg.as_ref(),
            &finalize_messages,
            &finalize_session_id,
        )
        .await
    });
    match timeout(Duration::from_secs(5), &mut finalize_task).await {
        Ok(Ok(Ok(_))) => {}
        Ok(Ok(Err(err))) => {
            tracing::warn!("Failed to update persistent memory at session finalization: {}", err);
        }
        Ok(Err(join_error)) => {
            tracing::warn!("Persistent memory finalization task failed: {}", join_error);
        }
        Err(_elapsed) => {
            tracing::info!("Persistent memory finalization continues in the background");
            // Detached by design after the 5 s wait, but not silent:
            // an observer task logs the eventual outcome instead of
            // dropping the JoinHandle and losing any error.
            tokio::spawn(async move {
                match finalize_task.await {
                    Ok(Ok(_)) => {}
                    Ok(Err(err)) => {
                        tracing::warn!("Background persistent memory finalization failed: {}", err);
                    }
                    Err(join_error) => {
                        tracing::warn!(
                            "Background persistent memory finalization task panicked or was aborted: {}",
                            join_error
                        );
                    }
                }
            });
        }
    }
}

pub(super) async fn shutdown_subagents(tool_registry: &ToolRegistry) {
    if let Some(controller) = tool_registry.subagent_controller() {
        // Bounded on every teardown path, not just fast exits: nested
        // close_tree walks can stall on contended locks, the OS reaps any
        // remainder at process exit, and `/new` recreates the controller
        // anyway. This intentionally tightens the previous unconditional 2s
        // budget to the 750ms fast cap.
        let budget = FAST_BACKGROUND_SHUTDOWN_TIMEOUT;
        if timeout(budget, controller.signal_shutdown()).await.is_err() {
            tracing::warn!("timed out shutting down subagent controller during session exit");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::cleanup_completed_artifacts;
    use vtcode_core::core::agent::harness_artifacts::current_task_path;

    #[tokio::test]
    async fn deadline_preserves_completed_results_and_canonical_errors() {
        use super::{SessionTeardownOutput, retain_completed_result};
        use crate::agent::runloop::unified::turn::finalization::FinalizationOutput;
        let mut teardown = SessionTeardownOutput::default();
        let mut archive = None;
        let mut unfinished = None::<usize>;
        let work = async {
            tokio::join!(
                retain_completed_result(&mut teardown.harness_finish_error, async {
                    Some(anyhow::anyhow!("canonical write failed"))
                }),
                retain_completed_result(&mut teardown.end_code_changes, async {
                    Some(super::HashMap::from_iter([(
                        "changed.rs".into(),
                        super::FileStat { additions: 17, deletions: 3 },
                    )]))
                }),
                retain_completed_result(&mut archive, async {
                    Some(FinalizationOutput { archive_path: Some("saved-session.json".into()) })
                }),
                retain_completed_result(&mut unfinished, std::future::pending()),
            );
        };
        assert!(tokio::time::timeout(std::time::Duration::from_millis(20), work).await.is_err());
        assert_eq!(teardown.harness_finish_error.unwrap().to_string(), "canonical write failed");
        assert_eq!(
            teardown.end_code_changes.unwrap().get(std::path::Path::new("changed.rs")),
            Some(&super::FileStat { additions: 17, deletions: 3 })
        );
        assert_eq!(archive.unwrap().archive_path.unwrap(), std::path::PathBuf::from("saved-session.json"));
        assert!(unfinished.is_none());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn cleanup_archives_completed_tracker_before_returning() {
        let workspace = tempfile::tempdir().expect("workspace");
        let task_path = current_task_path(workspace.path());
        std::fs::create_dir_all(task_path.parent().expect("tasks directory")).expect("create tasks");
        let content = "# Work\n- [x] first\n- [X] second\n";
        std::fs::write(&task_path, content).expect("write tracker");

        cleanup_completed_artifacts(workspace.path(), "run-id", "session-id").await;

        assert!(!task_path.exists(), "cleanup must finish before the next lifecycle phase");
        let archives = std::fs::read_dir(task_path.parent().expect("tasks directory").join("archive"))
            .expect("archive directory")
            .map(|entry| entry.expect("archive entry").path())
            .collect::<Vec<_>>();
        assert_eq!(archives.len(), 1);
        assert!(
            archives[0]
                .file_name()
                .expect("archive name")
                .to_string_lossy()
                .starts_with("current_task-session-id-")
        );
        assert_eq!(std::fs::read_to_string(&archives[0]).expect("archived tracker"), content);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn cleanup_preserves_unfinished_tracker() {
        let workspace = tempfile::tempdir().expect("workspace");
        let task_path = current_task_path(workspace.path());
        std::fs::create_dir_all(task_path.parent().expect("tasks directory")).expect("create tasks");
        let content = "# Work\n- [x] first\n- [ ] second\n";
        std::fs::write(&task_path, content).expect("write tracker");

        cleanup_completed_artifacts(workspace.path(), "run-id", "session-id").await;

        assert_eq!(std::fs::read_to_string(&task_path).expect("unfinished tracker"), content);
        assert!(!task_path.parent().expect("tasks directory").join("archive").exists());
    }

    #[test]
    fn fast_teardown_covers_exit_cancel_and_new_session() {
        use super::{FAST_BACKGROUND_SHUTDOWN_TIMEOUT, is_fast_teardown};
        use vtcode_core::hooks::SessionEndReason;

        // Asymmetric: fast reasons must map to tight cap, slow reasons keep
        // full budget. Drive via helper so mapping itself is pinned.
        for reason in [
            SessionEndReason::Exit,
            SessionEndReason::Cancelled,
            SessionEndReason::NewSession,
        ] {
            assert!(is_fast_teardown(reason), "{reason:?} should be fast");
        }
        for reason in [SessionEndReason::Completed, SessionEndReason::Error] {
            assert!(!is_fast_teardown(reason), "{reason:?} should keep full budget");
        }
        // Fast cap must be strictly tighter than the normal 2s budget so
        // `/new` and exit never park on nested close_tree walks.
        assert!(
            FAST_BACKGROUND_SHUTDOWN_TIMEOUT < super::super::session_bootstrap::EXIT_BACKGROUND_SHUTDOWN_TIMEOUT,
            "fast shutdown must be tighter than normal"
        );
    }
}
