//! Coalescing for repeated exec-session polls and waits.
//!
//! A long command is polled once per wait round-trip, so one `cargo check`
//! can project seven `Wait for command session` rows while it runs.
//! Consecutive follow-ups for the same session collapse into the first row
//! instead: the survivor keeps the latest status and wait deadline, and its
//! title gains a `×N` repeat count. Anything else — a different session, a
//! different action, or any intervening row — starts a fresh run.

use serde_json::Value;

use super::{SessionPollRun, Transcript, TranscriptItem};

/// Identity of one coalescible poll run: the session plus the base row title.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SessionPollKey {
    pub session_id: String,
    pub base_title: String,
}

/// Extract the coalescing key for a session poll/wait invocation.
///
/// Delegates to the shared predicate so desktop and CLI runs agree by
/// construction; `None` rows (launches, stdin sends, closes, inspects, and
/// calls without a session id) always project their own row.
pub(crate) fn session_poll_key(tool_name: &str, args: Option<&Value>) -> Option<SessionPollKey> {
    let empty = Value::Null;
    let key = vtcode_commons::tool_headlines::session_poll_key(tool_name, args.unwrap_or(&empty))?;
    Some(SessionPollKey {
        session_id: key.session_id,
        base_title: key.label.to_string(),
    })
}

/// Row title for a poll run: the base title alone for a single poll, with a
/// `×N` repeat count once follow-ups merge. Numerals only, so no locale key
/// is needed (same rule as the edit-stats suffix).
pub(crate) fn title_with_poll_count(base: &str, count: usize) -> String {
    if count <= 1 {
        base.to_string()
    } else {
        format!("{base} ×{count}")
    }
}

impl Transcript {
    /// Project a session poll/wait invocation, merging it into the trailing
    /// survivor when the previous row polls the same session.
    ///
    /// Returns the surviving row id: the trailing row's id on merge, else the
    /// item's own id. Merging rewrites the survivor's title (`×N`) and text
    /// (latest status and wait deadline) through [`Transcript::upsert`], so
    /// group summaries and layout bookkeeping stay identical to any other row
    /// update; the absorbed call id redirects to the survivor for its later
    /// output ([`Transcript::resolve_coalesced_call`]). A repeated projection
    /// of an already-known id (started → completed), or of an absorbed id,
    /// refreshes the survivor in place and never grows the run.
    pub(in crate::desktop) fn upsert_session_poll(&mut self, mut item: TranscriptItem, key: SessionPollKey) -> String {
        if let Some(index) = self.position_by_id(&item.id) {
            // Started → completed re-projection of a row we already own: keep
            // its run count (the fresh gist never carries the suffix) and let
            // the status/text update flow through the normal upsert.
            if let Some(run) = self.get(index).and_then(|row| row.session_poll.clone()) {
                if run.latest_call_id != item.id {
                    return item.id;
                }
                item.title = title_with_poll_count(&run.base_title, run.count).into();
                item.session_poll = Some(run);
            } else {
                item.session_poll = Some(SessionPollRun {
                    session_id: key.session_id,
                    base_title: key.base_title,
                    count: 1,
                    latest_call_id: item.id.clone(),
                });
            }
            let id = item.id.clone();
            self.upsert(item);
            return id;
        }
        if let Some(survivor_id) = self.coalesced_call_rows.get(&item.id).cloned() {
            // Re-projection (started → completed) of an absorbed call: refresh
            // the survivor without growing the run.
            if let Some(index) = self.position_by_id(&survivor_id)
                && let Some(run) = self.get(index).and_then(|row| row.session_poll.clone())
            {
                if run.latest_call_id != item.id {
                    return survivor_id;
                }
                item.id = survivor_id.clone();
                item.title = title_with_poll_count(&run.base_title, run.count).into();
                item.session_poll = Some(run);
                self.upsert(item);
                return survivor_id;
            }
            // The survivor is gone (evicted or removed): drop the stale
            // redirect and project fresh below.
            let _stale = self.coalesced_call_rows.remove(&item.id);
        }
        let survivor = self
            .items
            .back()
            .filter(|row| {
                row.is_tool() && row.turn_id == item.turn_id && row.canonical_turn_id == item.canonical_turn_id
            })
            .and_then(|row| row.session_poll.as_ref().map(|run| (row.id.clone(), run.clone())))
            .filter(|(_, run)| run.session_id == key.session_id && run.base_title == key.base_title);
        if let Some((survivor_id, run)) = survivor
            && self.coalesced_call_rows.len() < super::MAX_TRANSCRIPT_ITEMS
        {
            let count = run.count.saturating_add(1);
            let absorbed_id = item.id.clone();
            item.id = survivor_id.clone();
            item.title = title_with_poll_count(&key.base_title, count).into();
            item.session_poll = Some(SessionPollRun {
                session_id: key.session_id,
                base_title: key.base_title,
                count,
                latest_call_id: absorbed_id.clone(),
            });
            self.upsert(item);
            if absorbed_id != survivor_id {
                let _previous = self.coalesced_call_rows.insert(absorbed_id, survivor_id.clone());
            }
            return survivor_id;
        }
        let id = item.id.clone();
        item.title = title_with_poll_count(&key.base_title, 1).into();
        item.session_poll = Some(SessionPollRun {
            session_id: key.session_id,
            base_title: key.base_title,
            count: 1,
            latest_call_id: id.clone(),
        });
        self.upsert(item);
        id
    }

    /// Resolve an absorbed poll call id to its surviving row id.
    ///
    /// Unknown ids (normal rows, or survivors lost to eviction) resolve to
    /// themselves; callers still check row existence before merging output.
    pub(in crate::desktop) fn resolve_coalesced_call<'a>(&'a self, call_id: &'a str) -> &'a str {
        self.coalesced_call_rows.get(call_id).map(String::as_str).unwrap_or(call_id)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn key(tool: &str, args: Value) -> Option<SessionPollKey> {
        session_poll_key(tool, Some(&args))
    }

    #[test]
    fn poll_key_covers_session_waits_and_reads_only() {
        let wait = json!({"session_id": "run-1", "action": "wait", "wait_timeout_seconds": 8});
        assert_eq!(
            key("write_stdin", wait),
            Some(SessionPollKey {
                session_id: "run-1".into(),
                base_title: "Wait for command session".into(),
            })
        );
        // Case-insensitive actions and the other poll spellings join the run.
        for (tool, args) in [
            ("write_stdin", json!({"session_id": "run-1", "action": "WAIT"})),
            ("write_stdin", json!({"session_id": "run-1", "action": "poll"})),
            ("write_stdin", json!({"session_id": "run-1"})),
            ("write_stdin", json!({"session_id": "run-1", "action": "write", "input": ""})),
            ("command_session_internal", json!({"session_id": "s", "action": "wait"})),
            ("command_session_internal", json!({"session_id": "s", "action": "poll"})),
            ("read_pty_session", json!({"session_id": "s"})),
        ] {
            assert!(key(tool, args.clone()).is_some(), "{tool} {args} coalesces");
        }
        // Sends, launches, lifecycle actions, and session-less calls keep rows.
        for (tool, args) in [
            ("write_stdin", json!({"session_id": "run-1", "action": "write", "input": "ls"})),
            ("write_stdin", json!({"action": "wait"})),
            ("write_stdin", json!({"session_id": "run-1", "action": "close"})),
            ("write_stdin", json!({"session_id": "run-1", "action": "inspect"})),
            ("write_stdin", json!({"session_id": "run-1", "action": "terminate"})),
            ("command_session_internal", json!({"session_id": "s", "action": "run", "cmd": "ls"})),
            ("command_session_internal", json!({"session_id": "s", "action": "close"})),
            ("send_pty_input", json!({"session_id": "s"})),
            ("exec_command", json!({"command": "ls"})),
            ("read_file", json!({"path": "a.rs"})),
        ] {
            assert!(key(tool, args.clone()).is_none(), "{tool} {args} keeps its row");
        }
        assert_eq!(session_poll_key("write_stdin", None), None);
    }

    #[test]
    fn poll_count_suffix_starts_at_two() {
        assert_eq!(title_with_poll_count("Wait for command session", 0), "Wait for command session");
        assert_eq!(title_with_poll_count("Wait for command session", 1), "Wait for command session");
        assert_eq!(title_with_poll_count("Wait for command session", 2), "Wait for command session ×2");
        assert_eq!(title_with_poll_count("Read command session", 7), "Read command session ×7");
    }

    fn poll_item(id: &str, text: &str) -> TranscriptItem {
        TranscriptItem::new(id.to_string(), "tool".to_string(), "Wait for command session".into(), text.to_string())
    }

    fn poll_key_for(session: &str) -> SessionPollKey {
        SessionPollKey {
            session_id: session.into(),
            base_title: "Wait for command session".into(),
        }
    }

    #[test]
    fn consecutive_polls_merge_and_track_the_latest_text() {
        let mut transcript = Transcript::default();
        let first = transcript.upsert_session_poll(poll_item("wait-1", "in progress"), poll_key_for("run-1"));
        assert_eq!(first, "wait-1");
        assert_eq!(transcript.len(), 1);
        let survivor = transcript.upsert_session_poll(poll_item("wait-2", "completed"), poll_key_for("run-1"));
        assert_eq!(survivor, "wait-1");
        assert_eq!(transcript.len(), 1);
        let row = transcript.get(0).expect("survivor");
        assert_eq!(row.title.as_str(), "Wait for command session ×2");
        assert_eq!(row.text.as_str(), "completed");
        assert_eq!(row.session_poll.as_ref().map(|run| run.count), Some(2));
        assert_eq!(transcript.resolve_coalesced_call("wait-2"), "wait-1");
        assert_eq!(transcript.resolve_coalesced_call("wait-1"), "wait-1");
        assert_eq!(transcript.resolve_coalesced_call("unknown"), "unknown");
    }

    #[test]
    fn same_id_reprojection_keeps_the_run_count() {
        let mut transcript = Transcript::default();
        let _ = transcript.upsert_session_poll(poll_item("wait-1", "in progress"), poll_key_for("run-1"));
        let _ = transcript.upsert_session_poll(poll_item("wait-2", "completed"), poll_key_for("run-1"));
        // A late completion from the first call cannot overwrite the latest.
        let _ = transcript.upsert_session_poll(poll_item("wait-1", "failed"), poll_key_for("run-1"));
        assert_eq!(transcript.len(), 1);
        let row = transcript.get(0).expect("survivor");
        assert_eq!(row.title.as_str(), "Wait for command session ×2");
        assert_eq!(row.text.as_str(), "completed");
    }

    #[test]
    fn absorbed_id_reprojection_refreshes_without_growing() {
        let mut transcript = Transcript::default();
        let _ = transcript.upsert_session_poll(poll_item("wait-1", "in progress"), poll_key_for("run-1"));
        let _ = transcript.upsert_session_poll(poll_item("wait-2", "in progress"), poll_key_for("run-1"));
        // wait-2's completion arrives under its absorbed id: the survivor
        // refreshes, the run does not grow.
        let _ = transcript.upsert_session_poll(poll_item("wait-2", "completed"), poll_key_for("run-1"));
        assert_eq!(transcript.len(), 1);
        let row = transcript.get(0).expect("survivor");
        assert_eq!(row.title.as_str(), "Wait for command session ×2");
        assert_eq!(row.text.as_str(), "completed");
    }

    #[test]
    fn other_session_or_row_breaks_the_run() {
        let mut transcript = Transcript::default();
        let _ = transcript.upsert_session_poll(poll_item("wait-1", "completed"), poll_key_for("run-1"));
        let other = transcript.upsert_session_poll(poll_item("wait-2", "completed"), poll_key_for("run-2"));
        assert_eq!(other, "wait-2");
        assert_eq!(transcript.len(), 2);
        // An intervening non-poll row ends the run even for the same session.
        transcript.push(TranscriptItem::new(
            "read-1".to_string(),
            "tool".to_string(),
            "Read a".into(),
            "completed".to_string(),
        ));
        let fresh = transcript.upsert_session_poll(poll_item("wait-3", "completed"), poll_key_for("run-1"));
        assert_eq!(fresh, "wait-3");
        assert_eq!(transcript.len(), 4);
        assert_eq!(transcript.get(3).expect("row").title.as_str(), "Wait for command session");
    }
}
