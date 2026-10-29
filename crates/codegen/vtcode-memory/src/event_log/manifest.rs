use std::collections::VecDeque;

use serde::{Deserialize, Serialize};

use super::now_rfc3339;

/// Session-level metadata persisted to `manifest.json`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionManifest {
    /// Stable session identifier (directory name).
    pub session_id: String,
    /// Layout schema version (`SESSION_STORE_SCHEMA_VERSION`).
    schema_version: u32,
    /// RFC3339 creation timestamp.
    pub created_at: String,
    /// RFC3339 last-update timestamp.
    pub updated_at: String,
    /// Number of completed turns.
    pub turn_count: u64,
    /// Total number of events recorded.
    pub event_count: u64,
    /// Lifecycle status (`active` | `completed`).
    pub status: String,
    /// Whether the canonical log currently ends inside an open turn.
    ///
    /// This is optional on read so manifests written before open-turn
    /// persistence was introduced trigger a safe event-log scan instead of
    /// silently losing lifecycle state.
    #[serde(default)]
    pub(super) in_turn: Option<bool>,
    /// Byte length covered by the persisted manifest and turn index.
    ///
    /// This is optional for compatibility with manifests written before the
    /// fast-path freshness guard existed; those manifests are rebuilt from the
    /// canonical event log on reopen.
    #[serde(default)]
    pub(super) persisted_file_len: Option<u64>,
    /// Ordinal of the first turn retained in the canonical log.
    ///
    /// Cap eviction removes completed turns but must keep later turn numbers
    /// monotonic. The field lets an authoritative scan restore those ordinals
    /// even when the derived index is stale or missing.
    #[serde(default)]
    pub(super) retained_turn_base: Option<u64>,
}

impl SessionManifest {
    /// Number of turns preceding the retained canonical range.
    #[must_use]
    pub fn evicted_turn_count(&self) -> u64 {
        self.retained_turn_base.unwrap_or(1).saturating_sub(1)
    }
    /// Create a fresh manifest for a session.
    #[must_use]
    pub(crate) fn new(session_id: &str) -> Self {
        let ts = now_rfc3339();
        Self {
            session_id: session_id.to_string(),
            schema_version: crate::SESSION_STORE_SCHEMA_VERSION,
            created_at: ts.clone(),
            updated_at: ts,
            turn_count: 0,
            event_count: 0,
            status: "active".to_string(),
            in_turn: Some(false),
            persisted_file_len: Some(0),
            retained_turn_base: Some(1),
        }
    }
}

/// Byte-offset index of a single turn within `events.jsonl`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TurnIndexEntry {
    /// Turn ordinal (1-based).
    pub(super) turn_number: u64,
    /// Byte offset of the turn's first event.
    pub(super) start_offset: u64,
    /// Byte offset just past the turn's last event.
    pub(super) end_offset: u64,
    /// Number of events in the turn.
    pub(super) event_count: u64,
    /// RFC3339 timestamp of turn start.
    pub(super) ts: String,
}

/// Ordered index of all turns in a session.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct TurnIndex {
    /// Turn entries in ordinal order.
    pub(super) entries: VecDeque<TurnIndexEntry>,
}

impl TurnIndex {
    /// Number of indexed turns.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the index is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub(super) fn is_valid_for_file(&self, file_len: u64) -> bool {
        let mut previous_end = 0u64;
        self.entries.iter().all(|entry| {
            let valid = entry.event_count > 0
                && entry.start_offset >= previous_end
                && entry.start_offset <= entry.end_offset
                && entry.end_offset <= file_len;
            if valid {
                previous_end = entry.end_offset;
            }
            valid
        })
    }

    pub(super) fn is_consistent_with_manifest(&self, manifest: &SessionManifest) -> bool {
        let expected_last_turn = if manifest.in_turn == Some(true) {
            manifest.turn_count.saturating_add(1)
        } else {
            manifest.turn_count
        };
        let entries_are_contiguous = self
            .entries
            .iter()
            .map(|entry| entry.turn_number)
            .try_fold(None::<u64>, |previous, turn_number| {
                if previous.is_some_and(|previous| turn_number != previous.saturating_add(1)) {
                    return Err(());
                }
                Ok(Some(turn_number))
            })
            .is_ok();
        if !entries_are_contiguous {
            return false;
        }

        let expected_first_turn = manifest.retained_turn_base.unwrap_or(1).max(1);
        match (self.entries.front(), self.entries.back()) {
            (Some(first), Some(last)) => {
                first.turn_number == expected_first_turn && last.turn_number == expected_last_turn
            }
            (None, None) => {
                expected_last_turn == 0
                    || manifest
                        .retained_turn_base
                        .is_some_and(|retained_turn_base| retained_turn_base > manifest.turn_count)
            }
            _ => false,
        }
    }
}
