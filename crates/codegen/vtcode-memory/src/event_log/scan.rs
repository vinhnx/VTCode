use vtcode_exec_events::{ThreadEvent, VersionedThreadEvent};

use super::{SessionManifest, TurnIndex, VersionedEventKind};
use crate::manifest::PendingCapRewrite;

/// Recover the ordinal of the first retained turn when metadata is stale.
///
/// A cap rewrite atomically replaces the event file before it publishes the
/// shortened index and manifest. If the process crashes in that interval,
/// the old index still records the pre-rewrite offsets. The difference between
/// its persisted length and the current file length is exactly the removed
/// prefix, so the first old index entry at that boundary supplies the retained
/// turn base. Other stale metadata falls back to the durable base field.
pub(super) fn infer_scan_turn_base(
    manifest: Option<&SessionManifest>,
    index: Option<&TurnIndex>,
    pending: Option<&PendingCapRewrite>,
    file_len: u64,
) -> u64 {
    let marker_base = pending
        .filter(|pending| pending.new_file_len == file_len && pending.new_file_len < pending.previous_file_len)
        .map(|pending| pending.retained_turn_base);
    let rewritten_base = manifest.and_then(|manifest| {
        let previous_len = manifest.persisted_file_len?;
        if previous_len <= file_len {
            return None;
        }
        let removed_prefix = previous_len - file_len;
        let index = index.filter(|index| index.is_valid_for_file(previous_len))?;
        let first_retained = index
            .entries
            .iter()
            .find(|entry| entry.start_offset >= removed_prefix && entry.end_offset <= previous_len)
            .map(|entry| entry.turn_number);
        first_retained.or_else(|| {
            // If no indexed turn starts in the shortened file, the rewrite
            // evicted every previously indexed turn. Preserve the next
            // ordinal from the stale manifest so a subsequent append cannot
            // reuse an already-observed turn number.
            (removed_prefix >= previous_len).then(|| manifest.turn_count.saturating_add(1))
        })
    });

    let legacy_index_base = index
        .filter(|index| index.is_valid_for_file(file_len))
        .and_then(|index| index.entries.front().map(|entry| entry.turn_number));

    marker_base
        .or(rewritten_base)
        .or_else(|| manifest.and_then(|manifest| manifest.retained_turn_base))
        .or(legacy_index_base)
        .unwrap_or(1)
        .max(1)
}

pub(super) fn requires_full_lifecycle_validation(kind: &str) -> bool {
    matches!(kind, "thread.started" | "thread.completed" | "turn.started" | "turn.completed" | "turn.failed")
}

pub(super) fn valid_lifecycle_payload(kind: &str, line: &str) -> bool {
    if serde_json::from_str::<VersionedThreadEvent>(line).is_err() {
        return false;
    }
    if kind != "turn.completed" {
        return true;
    }
    let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
        return false;
    };
    value
        .get("event")
        .and_then(|event| event.get("usage"))
        .is_some_and(serde_json::Value::is_object)
}

pub(super) fn decode_events(bytes: &[u8]) -> Vec<ThreadEvent> {
    bytes
        .split(|byte| *byte == b'\n')
        .filter_map(|line| {
            let line = std::str::from_utf8(line).ok()?.trim();
            if line.is_empty() {
                return None;
            }
            serde_json::from_str::<VersionedThreadEvent>(line)
                .ok()
                .map(VersionedThreadEvent::into_event)
        })
        .collect()
}

/// Count records that the authoritative scan would include in the manifest.
/// This deliberately parses the lightweight event envelope instead of using
/// the turn index: a cap rewrite can remove session-level records that never
/// belong to an indexed turn.
pub(super) fn count_persisted_event_records(bytes: &[u8]) -> u64 {
    bytes
        .split(|byte| *byte == b'\n')
        .filter_map(|line| std::str::from_utf8(line).ok())
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter(|line| {
            let Ok(value) = serde_json::from_str::<VersionedEventKind<'_>>(line) else {
                return false;
            };
            let kind = value.event.kind;
            !requires_full_lifecycle_validation(kind) || valid_lifecycle_payload(kind, line)
        })
        .count() as u64
}
