//! Tracker probe outcomes: parsing live tracker state into cache updates.

const TRACKER_CONTINUE_ITEM_CAP: usize = 4;

/// Outcome of a live `task_tracker` probe, distinguishing completion from
/// probe failure so caches do not retain stale incomplete steps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TrackerProbeOutcome {
    /// Checklist exists and at least one step is not `completed`.
    Incomplete(Vec<String>),
    /// Tracker is empty or every step is `completed` — authoritative clear.
    Complete,
    /// Tool missing / execute failed / malformed payload — keep last cache.
    Unavailable,
}

/// Classify a `task_tracker` `action=list` payload for cache/gate decisions.
pub(crate) fn tracker_probe_outcome(payload: &serde_json::Value) -> TrackerProbeOutcome {
    let Some(status) = payload.get("status").and_then(serde_json::Value::as_str) else {
        return TrackerProbeOutcome::Unavailable;
    };
    if status == "empty" {
        return TrackerProbeOutcome::Complete;
    }
    let Some(checklist) = payload.get("checklist") else {
        return TrackerProbeOutcome::Unavailable;
    };
    let Some(raw_items) = checklist.get("items").and_then(serde_json::Value::as_array) else {
        // Malformed checklist without items: do not treat as Complete (that
        // would clear caches and stop auto-continue on a broken probe shape).
        return TrackerProbeOutcome::Unavailable;
    };
    let items: Vec<String> = raw_items
        .iter()
        .filter(|item| item.get("status").and_then(serde_json::Value::as_str) != Some("completed"))
        .filter_map(|item| {
            let description = item.get("description").and_then(serde_json::Value::as_str)?;
            let status = item.get("status").and_then(serde_json::Value::as_str).unwrap_or("pending");
            let index = item.get("index").and_then(serde_json::Value::as_u64);
            Some(match index {
                Some(index) if index > 0 => format!("#{} {} ({})", index, description, status),
                _ => format!("{} ({})", description, status),
            })
        })
        .take(TRACKER_CONTINUE_ITEM_CAP)
        .collect();
    if items.is_empty() {
        TrackerProbeOutcome::Complete
    } else {
        TrackerProbeOutcome::Incomplete(items)
    }
}

/// Parse incomplete step labels from a `task_tracker` list payload.
///
/// Thin wrapper over [`tracker_probe_outcome`] for tests and call sites that
/// only need the incomplete `Option` shape.
#[cfg(test)]
pub(crate) fn parse_incomplete_tracker_items(payload: &serde_json::Value) -> Option<Vec<String>> {
    match tracker_probe_outcome(payload) {
        TrackerProbeOutcome::Incomplete(items) => Some(items),
        TrackerProbeOutcome::Complete | TrackerProbeOutcome::Unavailable => None,
    }
}

/// Count completed checklist items in a `task_tracker` `action=list` payload.
///
/// Used for progress-reset of the cross-turn auto-continue episode budget.
/// Returns `0` when the tracker is empty/absent.
pub(crate) fn parse_tracker_completed_count(payload: &serde_json::Value) -> u32 {
    let Some(status) = payload.get("status").and_then(serde_json::Value::as_str) else {
        return 0;
    };
    if status == "empty" {
        return 0;
    }
    payload
        .get("checklist")
        .and_then(|c| c.get("items"))
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter(|item| item.get("status").and_then(serde_json::Value::as_str) == Some("completed"))
        .count() as u32
}

/// Apply a live probe to an incomplete-items cache.
///
/// `Incomplete` replaces the cache; `Complete` **clears** it; `Unavailable`
/// leaves the previous value so transient probe failures do not drop
/// auto-continue. Returns the effective incomplete slice after the update.
pub(crate) fn apply_tracker_probe_to_cache(
    cache: &mut Option<Vec<String>>,
    probe: TrackerProbeOutcome,
) -> Option<&[String]> {
    match probe {
        TrackerProbeOutcome::Incomplete(items) => {
            *cache = Some(items);
        }
        TrackerProbeOutcome::Complete => {
            *cache = None;
        }
        TrackerProbeOutcome::Unavailable => {}
    }
    cache.as_deref()
}

/// Load incomplete `task_tracker` step descriptions via the live tool registry.
///
/// Returns `None` when the tracker is absent, empty, fully completed, or the
/// probe fails. Prefer [`probe_tracker_incomplete`] when a cache must
/// distinguish complete from unavailable.
pub(crate) async fn incomplete_tracker_items(
    tool_registry: &vtcode_core::tools::registry::ToolRegistry,
) -> Option<Vec<String>> {
    match probe_tracker_incomplete(tool_registry).await {
        TrackerProbeOutcome::Incomplete(items) => Some(items),
        TrackerProbeOutcome::Complete | TrackerProbeOutcome::Unavailable => None,
    }
}

/// Live tracker probe that distinguishes complete from unavailable.
pub(crate) async fn probe_tracker_incomplete(
    tool_registry: &vtcode_core::tools::registry::ToolRegistry,
) -> TrackerProbeOutcome {
    let Some(tool) = tool_registry.get_tool(vtcode_core::config::constants::tools::TASK_TRACKER) else {
        return TrackerProbeOutcome::Unavailable;
    };
    let Ok(payload) = tool.execute(serde_json::json!({ "action": "list" })).await else {
        return TrackerProbeOutcome::Unavailable;
    };
    tracker_probe_outcome(&payload)
}

/// Live completed-item count for progress-reset. Returns `None` on probe failure.
pub(crate) async fn tracker_completed_count(tool_registry: &vtcode_core::tools::registry::ToolRegistry) -> Option<u32> {
    let tool = tool_registry.get_tool(vtcode_core::config::constants::tools::TASK_TRACKER)?;
    let payload = tool.execute(serde_json::json!({ "action": "list" })).await.ok()?;
    Some(parse_tracker_completed_count(&payload))
}
