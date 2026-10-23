use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use tokio::task::spawn_blocking;
use vtcode_config::loader::VTCodeConfig;
use vtcode_core::core::agent::features::FeatureSet;
use vtcode_core::exec::events::{ThreadEvent, ThreadStartedEvent};
use vtcode_memory::RetentionPolicy;

use crate::agent::runloop::unified::inline_events::harness::{
    HARNESS_LOG_MAX_AGE_DAYS, HarnessEventEmitter, prune_old_harness_logs, resolve_event_log_path,
};
use crate::agent::runloop::unified::run_loop_context::TurnRunId;

/// Open canonical harness persistence only. Retention sweeps are maintenance
/// and run in [`run_harness_retention`], spawned after first paint is available.
pub(super) async fn initialize_harness(
    workspace: &Path,
    vt_cfg: Option<&VTCodeConfig>,
    model: &str,
    turn_run_id: &TurnRunId,
    live_event_sender: Option<vtcode_desktop_protocol::LiveEventSender>,
) -> Result<Option<HarnessEventEmitter>> {
    let harness_config = vt_cfg.map(|cfg| cfg.agent.harness.clone()).unwrap_or_default();
    let legacy_path = harness_config
        .event_log_path
        .as_deref()
        .filter(|path| !path.trim().is_empty())
        .map(|path| resolve_event_log_path(path, turn_run_id));

    // Canonical persistence is mandatory. Setup errors propagate instead of
    // silently downgrading the run to an unpersisted session.
    let emitter = HarnessEventEmitter::new_async(workspace, &turn_run_id.0, legacy_path).await?;
    if let Some(sender) = live_event_sender {
        emitter.attach_live_event_stream(sender)?;
    }

    let session_derived = vtcode_memory::session_directory(workspace, &turn_run_id.0).join("derived");
    let features = FeatureSet::from_config(vt_cfg);
    if features.open_responses.emit_events {
        let open_responses_config = vt_cfg.map(|cfg| cfg.agent.open_responses.clone()).unwrap_or_default();
        let output_path = session_derived.join("open-responses.jsonl");
        if let Err(error) = emitter
            .enable_open_responses_async(open_responses_config, model, Some(output_path.clone()))
            .await
        {
            tracing::warn!(target: "vtcode.harness", phase = "open_responses_setup", path = %output_path.display(), error = %error, "Open Responses setup failed");
        }
    }

    if vt_cfg.is_some_and(|cfg| cfg.telemetry.atif_enabled) {
        let atif_path = session_derived.join("atif-trajectory.json");
        if let Err(error) = emitter.enable_atif(model, atif_path.clone()) {
            tracing::warn!(target: "vtcode.harness", phase = "atif_setup", path = %atif_path.display(), error = %error, "ATIF setup failed");
        }
    }

    if let Err(error) = emitter
        .emit(ThreadEvent::ThreadStarted(ThreadStartedEvent { thread_id: turn_run_id.0.clone() }))
        .context("failed to emit canonical thread.started event")
    {
        emitter.finish_after_unexpected_exit().await;
        return Err(error);
    }
    Ok(Some(emitter))
}

/// Session-store retention and legacy harness-log pruning.
///
/// Spawned after `initialize_session_ui` so a large `.vtcode/sessions` tree never delays
/// the first frame. Failures are logged and non-fatal.
pub(super) async fn run_harness_retention(workspace: &Path, vt_cfg: Option<&VTCodeConfig>, turn_run_id: &TurnRunId) {
    let harness_config = vt_cfg.map(|cfg| cfg.agent.harness.clone()).unwrap_or_default();
    if let Some(log_path) = harness_config
        .event_log_path
        .as_deref()
        .filter(|path| !path.trim().is_empty())
        .map(|path| resolve_event_log_path(path, turn_run_id))
    {
        let log_dir = if log_path.extension().is_some() {
            log_path.parent().unwrap_or(Path::new(".")).to_path_buf()
        } else {
            log_path.clone()
        };
        let prune_result = spawn_blocking(move || {
            if log_dir.is_dir() {
                prune_old_harness_logs(&log_dir, HARNESS_LOG_MAX_AGE_DAYS)
            } else {
                Ok(())
            }
        })
        .await;
        match prune_result {
            Ok(Ok(())) => {}
            Ok(Err(error)) => {
                tracing::warn!(target: "vtcode.harness", phase = "legacy_prune", path = %log_path.display(), error = %error, "legacy harness log pruning failed");
            }
            Err(error) => {
                tracing::warn!(target: "vtcode.harness", phase = "legacy_prune", path = %log_path.display(), error = %error, "legacy harness log pruning task failed");
            }
        }
    }

    let retention_workspace = workspace.to_path_buf();
    let retention_session_id = turn_run_id.0.clone();
    match spawn_blocking(move || {
        vtcode_memory::apply_retention_preserving(
            &retention_workspace,
            RetentionPolicy::default(),
            Some(retention_session_id.as_str()),
        )
    })
    .await
    {
        Ok(Ok(removed)) if removed > 0 => {
            tracing::debug!(target: "vtcode.harness", removed, "pruned closed canonical sessions");
        }
        Ok(Ok(_)) => {}
        Ok(Err(error)) => {
            tracing::warn!(target: "vtcode.harness", phase = "canonical_retention", error = %error, "canonical session retention failed")
        }
        Err(error) => {
            tracing::warn!(target: "vtcode.harness", phase = "canonical_retention", error = %error, "canonical session retention task failed")
        }
    }

    let history_workspace = workspace.to_path_buf();
    let history_preserve = turn_run_id.0.clone();
    let history_max_age_days = RetentionPolicy::default().max_age_days;
    match spawn_blocking(move || prune_history_envelopes(&history_workspace, &history_preserve, history_max_age_days))
        .await
    {
        Ok(Ok(removed)) if removed > 0 => {
            tracing::debug!(target: "vtcode.harness", removed, "pruned legacy history memory envelopes");
        }
        Ok(Ok(_)) => {}
        Ok(Err(error)) => {
            tracing::warn!(target: "vtcode.harness", phase = "history_envelope_retention", error = %error, "history envelope pruning failed")
        }
        Err(error) => {
            tracing::warn!(target: "vtcode.harness", phase = "history_envelope_retention", error = %error, "history envelope pruning task failed")
        }
    }
}

/// Cap `.vtcode/history/*.memory.json` so legacy envelopes cannot grow without
/// bound while dual writes still land there.
///
/// Keeps the `HISTORY_ENVELOPE_KEEP` newest files, anything belonging to
/// `preserve_session_id`, and anything belonging to a retention-pinned
/// session; drops envelopes older than `max_age_days`. The pinned exclusion
/// reuses `vtcode_memory::retention_pinned_session_ids`: a blocked session is
/// pinned so ordinary retention cannot erase its evidence, and this prune is
/// ordinary retention.
fn prune_history_envelopes(workspace: &Path, preserve_session_id: &str, max_age_days: u64) -> Result<usize> {
    const HISTORY_ENVELOPE_KEEP: usize = 50;

    let history_dir = workspace.join(".vtcode").join("history");
    let Ok(entries) = std::fs::read_dir(&history_dir) else {
        return Ok(0);
    };
    let pinned_session_ids = vtcode_memory::retention_pinned_session_ids(workspace);
    let mut envelopes: Vec<(PathBuf, SystemTime)> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if !name.ends_with(".memory.json") || !path.is_file() {
            continue;
        }
        // Exact sanitized-id match (same rule as envelope writers). Loose
        // prefix matching would preserve unrelated sessions.
        if vtcode_core::compaction::memory_envelope::memory_envelope_file_matches_session(name, preserve_session_id) {
            continue;
        }
        if pinned_session_ids.iter().any(|session_id| {
            vtcode_core::compaction::memory_envelope::memory_envelope_file_matches_session(name, session_id)
        }) {
            continue;
        }
        let modified = entry
            .metadata()
            .ok()
            .and_then(|meta| meta.modified().ok())
            .unwrap_or(SystemTime::UNIX_EPOCH);
        envelopes.push((path, modified));
    }
    envelopes.sort_by_key(|(_, modified)| std::cmp::Reverse(*modified));

    let cutoff = SystemTime::now() - Duration::from_secs(max_age_days * 24 * 3600);
    let mut removed = 0usize;
    for (index, (path, modified)) in envelopes.iter().enumerate() {
        let over_count = index >= HISTORY_ENVELOPE_KEEP;
        let aged_out = *modified < cutoff;
        if !over_count && !aged_out {
            continue;
        }
        match std::fs::remove_file(path) {
            Ok(()) => removed += 1,
            Err(error) => {
                tracing::warn!(target: "vtcode.harness", path = %path.display(), %error, "failed to prune history envelope");
            }
        }
    }
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use vtcode_core::exec::events::{
        AgentMessageItem, ItemUpdatedEvent, ThreadItem, ThreadItemDetails,
    };

    #[tokio::test]
    async fn initialize_harness_emits_thread_started_without_retention_side_effects() {
        let temp = TempDir::new().expect("temp dir");
        let turn = TurnRunId("test-harness-init".to_string());
        let emitter = initialize_harness(temp.path(), None, "gpt-5", &turn, None)
            .await
            .expect("emitter");
        assert!(emitter.is_some(), "canonical emitter is mandatory");
        // Retention is deferred: no session store is required for emitter setup.
        assert!(
            !temp.path().join("sessions").exists(),
            "initialize_harness must not create or sweep the session store before first paint"
        );
    }

    #[tokio::test]
    async fn saturated_live_stream_never_fails_emit_or_loses_lifecycle_events() {
        let temp = TempDir::new().expect("temp dir");
        let turn = TurnRunId("test-live-saturation".to_string());
        let (sender, receiver) = vtcode_desktop_protocol::live_event_channel(4);
        let emitter = initialize_harness(temp.path(), None, "gpt-5", &turn, Some(sender))
            .await
            .expect("emitter setup")
            .expect("canonical emitter");

        // A stalled consumer must not fail the producing turn: every emit
        // succeeds even though nobody drains the queue.
        for round in 0..64 {
            emitter
                .emit(ThreadEvent::ItemUpdated(ItemUpdatedEvent {
                    item: ThreadItem {
                        context: None,
                        id: "item-stream".to_string(),
                        details: ThreadItemDetails::AgentMessage(AgentMessageItem {
                            text: format!("round {round}"),
                        }),
                    },
                }))
                .expect("emit must survive a stalled consumer");
        }
        drop(emitter);

        let mut saw_thread_started = false;
        let mut latest_snapshot = None;
        while let Some(event) = receiver.recv().await {
            match event.event() {
                ThreadEvent::ThreadStarted(_) => saw_thread_started = true,
                ThreadEvent::ItemUpdated(update) => {
                    if let ThreadItemDetails::AgentMessage(message) = &update.item.details {
                        latest_snapshot = Some(message.text.clone());
                    }
                }
                _ => {}
            }
        }
        assert!(saw_thread_started, "thread.started must survive saturation");
        assert_eq!(
            latest_snapshot.as_deref(),
            Some("round 63"),
            "the newest snapshot for an item must survive saturation"
        );
    }

    #[tokio::test]
    async fn run_harness_retention_is_idempotent_on_empty_workspace() {
        let temp = TempDir::new().expect("temp dir");
        let turn = TurnRunId("test-harness-retention".to_string());
        run_harness_retention(temp.path(), None, &turn).await;
        run_harness_retention(temp.path(), None, &turn).await;
    }

    #[test]
    fn prune_history_envelopes_keeps_newest_and_preserved_session() {
        let temp = TempDir::new().expect("temp dir");
        let history = temp.path().join(".vtcode").join("history");
        std::fs::create_dir_all(&history).expect("history dir");
        for i in 0..60 {
            let name = format!("session-{i:03}.memory.json");
            std::fs::write(history.join(name), b"{}").expect("write envelope");
        }
        std::fs::write(history.join("session-keep.memory.json"), b"{}").expect("write preserved");

        let removed = prune_history_envelopes(temp.path(), "session-keep", 30).expect("prune");
        assert!(removed >= 10, "count cap should drop the oldest extras: {removed}");
        assert!(history.join("session-keep.memory.json").exists(), "preserved session stays");
        let remaining: Vec<_> = std::fs::read_dir(&history)
            .expect("read history")
            .flatten()
            .map(|e| e.file_name())
            .collect();
        assert!(remaining.len() <= 51, "bounded history envelopes: {}", remaining.len());
    }

    #[test]
    fn prune_history_envelopes_preserves_real_session_id_prefix() {
        let temp = TempDir::new().expect("temp dir");
        let history = temp.path().join(".vtcode").join("history");
        std::fs::create_dir_all(&history).expect("history dir");
        // Real ids are sanitized to 32 chars for envelope filenames.
        let session_id = "session-vtcode-20260925T234343Z_201620-81429";
        let envelope_name = "session-vtcode-20260925T234343Z_.memory.json";
        std::fs::write(history.join(envelope_name), b"{}").expect("write preserved");
        for i in 0..60 {
            std::fs::write(history.join(format!("other-{i:03}.memory.json")), b"{}").expect("write other");
        }

        prune_history_envelopes(temp.path(), session_id, 30).expect("prune");
        assert!(
            history.join(envelope_name).exists(),
            "finalizing session envelope must match on the 32-char sanitized prefix"
        );
    }

    #[test]
    fn prune_history_envelopes_does_not_over_preserve_shared_prefixes() {
        let temp = TempDir::new().expect("temp dir");
        let history = temp.path().join(".vtcode").join("history");
        std::fs::create_dir_all(&history).expect("history dir");
        // Near-miss name that loose `starts_with("session-keep")` would wrongly preserve.
        std::fs::write(history.join("session-keep.memory.json"), b"{}").expect("write preserved");
        std::fs::write(history.join("session-keeper.memory.json"), b"{}").expect("write near-miss");
        for i in 0..60 {
            std::fs::write(history.join(format!("other-{i:03}.memory.json")), b"{}").expect("write other");
        }

        // max_age_days = 0 ages out every non-preserved envelope so the
        // assertion is about matching, not the count cap.
        prune_history_envelopes(temp.path(), "session-keep", 0).expect("prune");
        assert!(history.join("session-keep.memory.json").exists(), "exact session stays");
        assert!(
            !history.join("session-keeper.memory.json").exists(),
            "near-miss name must not be preserved by loose prefix matching"
        );
    }

    #[test]
    fn prune_history_envelopes_never_drops_pinned_session_envelopes() {
        // A blocked session is retention-pinned so ordinary retention cannot
        // erase its evidence; the legacy envelope prune is ordinary retention.
        let temp = TempDir::new().expect("temp dir");
        let history = temp.path().join(".vtcode").join("history");
        let sessions_root = temp.path().join(".vtcode").join("sessions");
        std::fs::create_dir_all(&history).expect("history dir");
        let pinned_dir = sessions_root.join("session-pinned");
        std::fs::create_dir_all(&pinned_dir).expect("pinned session dir");
        std::fs::write(pinned_dir.join("retention-pin.json"), b"{}").expect("write pin");

        std::fs::write(history.join("session-pinned.memory.json"), b"{}").expect("write pinned envelope");
        std::fs::write(history.join("session-unpinned.memory.json"), b"{}").expect("write unpinned envelope");

        // max_age_days = 0 ages out everything that is not protected.
        prune_history_envelopes(temp.path(), "session-finalizing", 0).expect("prune");
        assert!(
            history.join("session-pinned.memory.json").exists(),
            "a retention-pinned session's envelope must survive the prune"
        );
        assert!(!history.join("session-unpinned.memory.json").exists(), "unpinned envelopes still age out");
    }
}
