//! Binary-specific memory envelope refresh. The pure envelope build/persist
//! logic lives in `vtcode_core::compaction::memory_envelope`; this module only
//! adds the binary runloop's `SessionStats`-backed refresh path.

use super::*;
use crate::agent::runloop::unified::state::SessionStats;

pub(crate) fn refresh_session_memory_envelope(
    workspace_root: &Path,
    session_id: &str,
    vt_cfg: Option<&VTCodeConfig>,
    history: &[Message],
    session_stats: &SessionStats,
    envelope_update: Option<&SessionMemoryEnvelopeUpdate>,
) -> Result<Option<SessionMemoryEnvelope>> {
    if history.is_empty() || !should_persist_memory_envelope(vt_cfg) {
        return Ok(None);
    }

    let prior = load_latest_memory_envelope(workspace_root, session_id);
    let task_snapshot = read_task_tracker_snapshot(workspace_root);
    let touched_files = session_stats.recent_touched_files();
    let envelope = build_session_memory_envelope(
        session_id,
        workspace_root,
        history,
        &touched_files,
        derive_continuity_summary(history, prior.as_ref(), &task_snapshot),
        None,
        prior.as_ref(),
        &task_snapshot,
        envelope_update,
    );

    // Throttle: if nothing meaningful changed, keep the existing envelope and
    // avoid inserting another large system message into the history.
    if prior
        .as_ref()
        .is_some_and(|prior_envelope| prior_envelope.is_content_equivalent_to(&envelope))
    {
        return Ok(None);
    }

    let path = latest_memory_envelope_path_for_session(workspace_root, session_id)
        .unwrap_or_else(|| default_memory_envelope_path_for_session(workspace_root, session_id));
    write_memory_envelope_to_path(&path, &envelope)?;
    Ok(Some(envelope))
}
