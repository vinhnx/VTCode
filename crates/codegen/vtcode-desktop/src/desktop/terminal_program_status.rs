//! Terminal-side Program Status Protocol (OSC 7501) records for the embedded shell.
//!
//! Rio parses and validates reports behind the tab's opt-in; this store keeps
//! the terminal's record set per the spec: complete replacement, hierarchical
//! clear, prompt/exit/reset lifetimes, and a 64-record cap with
//! least-recently-updated eviction. Child text is untrusted: the summary
//! sanitizes before display, and the view localizes its own fallback labels.

use std::collections::BTreeMap;

use rio_vt::event::{ProgramState, ProgramStatus, ProgramStatusEvent};

/// Records kept per terminal. The spec requires at least 64; VT Code's own
/// emitter caps at the same number, so one peer can never overflow the set.
const RECORD_CAP: usize = 64;
/// Header detail budget in characters. Long child messages truncate with an
/// ellipsis instead of stretching the tab.
const SUMMARY_DETAIL_CHARS: usize = 80;

/// One stored record: the last validated report plus its update order for
/// least-recently-updated eviction.
#[derive(Clone, Debug)]
struct StoredRecord {
    status: ProgramStatus,
    seq: u64,
}

/// The terminal's OSC 7501 record set. `None` keys address the root record.
#[derive(Clone, Debug, Default)]
pub(super) struct TerminalProgramStatus {
    records: BTreeMap<Option<String>, StoredRecord>,
    next_seq: u64,
    exited: bool,
}

/// Aggregate status for the terminal tab: the record the header shows, if any.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct TerminalProgramStatusSummary {
    pub(super) state: ProgramState,
    /// Sanitized child text (`msg`, else `title`, else inherited `app`), empty
    /// when the record carries none — the view falls back to a localized label.
    pub(super) detail: String,
    pub(super) progress: Option<u8>,
}

impl TerminalProgramStatus {
    /// Fold one validated parser event into the record set.
    pub(super) fn apply(&mut self, event: &ProgramStatusEvent) {
        match event {
            ProgramStatusEvent::Report(status) => self.apply_report(status),
            ProgramStatusEvent::PromptStart => self.drop_transient(),
            ProgramStatusEvent::Reset => self.records.clear(),
        }
        debug_assert!(self.records.len() <= RECORD_CAP, "record set must stay capped");
    }

    /// Shell exit: drop `working`/`blocked` now and ignore late ones, since
    /// ordered PTY bytes can still deliver stale pre-exit reports afterwards.
    /// `done`/`error` survive, so a finished run stays visible.
    pub(super) fn on_process_exit(&mut self) {
        self.exited = true;
        self.drop_transient();
    }

    /// The record the header shows: most urgent state wins, then the freshest
    /// update, then the smallest id for determinism. Idle-only sets stay quiet.
    pub(super) fn summary(&self) -> Option<TerminalProgramStatusSummary> {
        let (id, record) = self
            .records
            .iter()
            .filter(|(_, record)| record.status.state != ProgramState::Idle)
            .max_by_key(|(id, record)| (state_priority(record.status.state), record.seq, std::cmp::Reverse(*id)))?;
        let status = &record.status;
        let detail = status
            .msg
            .as_deref()
            .or(status.title.as_deref())
            .or_else(|| self.inherited_app(id))
            .map(|text| vtcode_commons::sanitizer::sanitize_terminal_text(text, SUMMARY_DETAIL_CHARS))
            .unwrap_or_default();
        Some(TerminalProgramStatusSummary {
            state: status.state,
            detail,
            progress: status.progress,
        })
    }

    fn apply_report(&mut self, status: &ProgramStatus) {
        if status.state == ProgramState::Clear {
            self.clear_subtree(status.id.as_ref());
            return;
        }
        if self.exited && matches!(status.state, ProgramState::Working | ProgramState::Blocked) {
            return;
        }
        if !self.records.contains_key(&status.id) && self.records.len() >= RECORD_CAP {
            self.evict_least_recently_updated();
        }
        let seq = self.next_seq();
        let _ = self
            .records
            .insert(status.id.clone(), StoredRecord { status: status.clone(), seq });
    }

    fn clear_subtree(&mut self, id: Option<&String>) {
        match id {
            None => self.records.clear(),
            Some(id) => {
                let prefix = format!("{id}/");
                self.records.retain(|key, _| match key {
                    None => true,
                    Some(key) => key != id && !key.starts_with(&prefix),
                });
            }
        }
    }

    /// Prompt start and process exit retire `working`/`blocked`. `idle` stays
    /// (the spec lets terminals keep it) and `done`/`error` survive.
    fn drop_transient(&mut self) {
        self.records
            .retain(|_, record| !matches!(record.status.state, ProgramState::Working | ProgramState::Blocked));
    }

    fn evict_least_recently_updated(&mut self) {
        let oldest = self
            .records
            .iter()
            .min_by_key(|(_, record)| record.seq)
            .map(|(id, _)| id.clone());
        if let Some(id) = oldest {
            let _ = self.records.remove(&id);
        }
    }

    /// Nearest `app` on the record or its ancestors, for display fallback.
    /// Each step strictly shortens the path, so the walk always reaches root.
    fn inherited_app(&self, id: &Option<String>) -> Option<&str> {
        let mut current = id.clone();
        loop {
            if let Some(record) = self.records.get(&current)
                && let Some(app) = record.status.app.as_deref()
            {
                return Some(app);
            }
            let path = current?;
            current = path.rsplit_once('/').map(|(parent, _)| parent.to_string());
        }
    }

    fn next_seq(&mut self) -> u64 {
        let seq = self.next_seq;
        self.next_seq = self.next_seq.wrapping_add(1);
        seq
    }
}

/// Display precedence: the record most in need of the user's eye wins.
fn state_priority(state: ProgramState) -> u8 {
    match state {
        ProgramState::Blocked => 4,
        ProgramState::Working => 3,
        ProgramState::Error => 2,
        ProgramState::Done => 1,
        ProgramState::Idle | ProgramState::Clear => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rio_vt::event::BlockedKind;

    fn status(
        state: ProgramState,
        id: Option<&str>,
        msg: Option<&str>,
        title: Option<&str>,
        app: Option<&str>,
        progress: Option<u8>,
    ) -> ProgramStatus {
        ProgramStatus {
            state,
            kind: (state == ProgramState::Blocked).then_some(BlockedKind::Permission),
            app: app.map(str::to_string),
            msg: msg.map(str::to_string),
            title: title.map(str::to_string),
            progress,
            id: id.map(str::to_string),
        }
    }

    fn report(status: ProgramStatus) -> ProgramStatusEvent {
        ProgramStatusEvent::Report(status)
    }

    fn working(id: Option<&str>) -> ProgramStatusEvent {
        report(status(ProgramState::Working, id, None, None, None, None))
    }

    #[test]
    fn report_replaces_record_wholesale() {
        let mut store = TerminalProgramStatus::default();
        store.apply(&report(status(
            ProgramState::Working,
            Some("job"),
            Some("old message"),
            Some("old title"),
            Some("old-app"),
            Some(40),
        )));
        // A report with fewer keys clears the dropped ones; nothing lingers.
        store.apply(&report(status(ProgramState::Working, Some("job"), None, None, None, None)));
        let summary = store.summary().expect("working record must surface");
        assert_eq!(summary.state, ProgramState::Working);
        assert_eq!(summary.detail, "");
        assert_eq!(summary.progress, None);
    }

    #[test]
    fn clear_removes_record_and_subtree_only() {
        let mut store = TerminalProgramStatus::default();
        for id in [None, Some("a"), Some("a/b"), Some("a/b/c"), Some("other")] {
            store.apply(&working(id));
        }
        store.apply(&report(status(ProgramState::Clear, Some("a"), None, None, None, None)));
        let ids: Vec<_> = store.records.keys().cloned().collect();
        assert_eq!(ids, [None, Some("other".to_string())]);
    }

    #[test]
    fn clear_without_id_removes_every_record() {
        let mut store = TerminalProgramStatus::default();
        for id in [None, Some("a"), Some("a/b")] {
            store.apply(&working(id));
        }
        store.apply(&report(status(ProgramState::Clear, None, None, None, None, None)));
        assert!(store.records.is_empty());
        assert_eq!(store.summary(), None);
    }

    #[test]
    fn prompt_start_drops_working_and_blocked_but_keeps_settled_states() {
        let mut store = TerminalProgramStatus::default();
        let states = [
            (ProgramState::Working, "w"),
            (ProgramState::Blocked, "b"),
            (ProgramState::Idle, "i"),
            (ProgramState::Done, "d"),
            (ProgramState::Error, "e"),
        ];
        for (state, id) in states {
            store.apply(&report(status(state, Some(id), None, None, None, None)));
        }
        store.apply(&ProgramStatusEvent::PromptStart);
        for (id, expected) in [
            ("i", ProgramState::Idle),
            ("d", ProgramState::Done),
            ("e", ProgramState::Error),
        ] {
            let state = store.records.get(&Some(id.to_string())).map(|record| record.status.state);
            assert_eq!(state, Some(expected), "{id} must survive the prompt");
        }
        assert_eq!(store.records.len(), 3, "working and blocked must go");
    }

    #[test]
    fn reset_drops_every_record() {
        let mut store = TerminalProgramStatus::default();
        store.apply(&working(None));
        store.apply(&report(status(ProgramState::Done, Some("d"), None, None, None, None)));
        store.apply(&ProgramStatusEvent::Reset);
        assert!(store.records.is_empty());
    }

    #[test]
    fn exit_drops_transient_records_and_ignores_late_ones() {
        let mut store = TerminalProgramStatus::default();
        store.apply(&working(Some("w")));
        store.apply(&report(status(ProgramState::Blocked, Some("b"), None, None, None, None)));
        store.apply(&report(status(ProgramState::Done, Some("d"), None, None, None, None)));
        store.on_process_exit();
        let remaining: Vec<_> = store.records.keys().cloned().collect();
        assert_eq!(remaining, [Some("d".to_string())]);

        // Stale pre-exit bytes arriving late must not resurrect transient states.
        store.apply(&working(Some("late-w")));
        store.apply(&report(status(ProgramState::Blocked, Some("late-b"), None, None, None, None)));
        // Settled outcomes and clears still apply after exit.
        store.apply(&report(status(ProgramState::Done, Some("late-d"), None, None, None, None)));
        store.apply(&report(status(ProgramState::Error, Some("late-e"), None, None, None, None)));
        store.apply(&report(status(ProgramState::Idle, Some("late-i"), None, None, None, None)));
        store.apply(&report(status(ProgramState::Clear, Some("d"), None, None, None, None)));
        let remaining: Vec<_> = store.records.keys().cloned().collect();
        assert_eq!(
            remaining,
            [
                Some("late-d".to_string()),
                Some("late-e".to_string()),
                Some("late-i".to_string())
            ]
        );
    }

    #[test]
    fn full_set_evicts_least_recently_updated_on_new_id_only() {
        let mut store = TerminalProgramStatus::default();
        for index in 0..RECORD_CAP {
            store.apply(&working(Some(&format!("job-{index:02}"))));
        }
        // Touch job-00 so it is no longer the eviction candidate.
        store.apply(&working(Some("job-00")));
        // Replacing an existing record at capacity evicts nothing.
        assert_eq!(store.records.len(), RECORD_CAP);
        store.apply(&working(Some("job-63")));
        assert_eq!(store.records.len(), RECORD_CAP);
        assert!(store.records.contains_key(&Some("job-00".to_string())));

        store.apply(&working(Some("job-new")));
        assert_eq!(store.records.len(), RECORD_CAP);
        assert!(store.records.contains_key(&Some("job-new".to_string())));
        assert!(store.records.contains_key(&Some("job-00".to_string())), "fresh update must survive");
        assert!(!store.records.contains_key(&Some("job-01".to_string())), "oldest untouched record goes");
    }

    #[test]
    fn summary_prefers_blocked_over_working_over_error_over_done() {
        let mut store = TerminalProgramStatus::default();
        // Insert worst-first so priority, not recency, decides each round.
        for (state, id) in [
            (ProgramState::Done, "d"),
            (ProgramState::Error, "e"),
            (ProgramState::Working, "w"),
            (ProgramState::Blocked, "b"),
        ] {
            store.apply(&report(status(state, Some(id), Some(id), None, None, None)));
        }
        assert_eq!(store.summary().expect("records exist").detail, "b");
        store.apply(&report(status(ProgramState::Clear, Some("b"), None, None, None, None)));
        assert_eq!(store.summary().expect("records exist").detail, "w");
        store.apply(&report(status(ProgramState::Clear, Some("w"), None, None, None, None)));
        assert_eq!(store.summary().expect("records exist").detail, "e");
        store.apply(&report(status(ProgramState::Clear, Some("e"), None, None, None, None)));
        assert_eq!(store.summary().expect("records exist").detail, "d");
    }

    #[test]
    fn summary_prefers_freshest_update_within_a_state() {
        let mut store = TerminalProgramStatus::default();
        store.apply(&report(status(ProgramState::Working, Some("zzz-old"), Some("old"), None, None, None)));
        store.apply(&report(status(ProgramState::Working, Some("aaa-new"), Some("new"), None, None, None)));
        assert_eq!(store.summary().expect("records exist").detail, "new");
    }

    #[test]
    fn summary_breaks_seq_ties_by_smallest_id() {
        // Equal sequences are unreachable through `apply` (every report takes
        // the next one), so build the tie directly to pin the determinism clause.
        let mut store = TerminalProgramStatus::default();
        for id in ["b-second", "a-first"] {
            let _ = store.records.insert(
                Some(id.to_string()),
                StoredRecord {
                    status: status(ProgramState::Working, Some(id), Some(id), None, None, None),
                    seq: 7,
                },
            );
        }
        assert_eq!(store.summary().expect("records exist").detail, "a-first");
    }

    #[test]
    fn summary_stays_quiet_without_non_idle_records() {
        let mut store = TerminalProgramStatus::default();
        assert_eq!(store.summary(), None);
        store.apply(&report(status(ProgramState::Idle, None, Some("resting"), None, None, None)));
        assert_eq!(store.summary(), None, "idle alone shows no indicator");
    }

    #[test]
    fn detail_prefers_msg_over_title_over_app() {
        let mut store = TerminalProgramStatus::default();
        store.apply(&report(status(
            ProgramState::Working,
            Some("full"),
            Some("the message"),
            Some("the title"),
            Some("the-app"),
            None,
        )));
        store.apply(&report(status(
            ProgramState::Working,
            Some("no-msg"),
            None,
            Some("the title"),
            Some("the-app"),
            None,
        )));
        // Freshest record wins, so clear down to inspect each fallback alone.
        assert_eq!(store.summary().expect("records exist").detail, "the title");
        store.apply(&report(status(ProgramState::Clear, Some("no-msg"), None, None, None, None)));
        assert_eq!(store.summary().expect("records exist").detail, "the message");
    }

    #[test]
    fn detail_falls_back_to_nearest_ancestor_app() {
        let mut store = TerminalProgramStatus::default();
        store.apply(&report(status(ProgramState::Working, None, None, None, Some("root-app"), None)));
        store.apply(&report(status(ProgramState::Working, Some("mid"), None, None, None, None)));
        store.apply(&report(status(ProgramState::Working, Some("mid/leaf"), None, None, None, None)));
        assert_eq!(store.summary().expect("records exist").detail, "root-app");
        // The nearer ancestor wins once it names itself.
        store.apply(&report(status(ProgramState::Working, Some("mid"), None, None, Some("mid-app"), None)));
        // `mid` is now fresher than the leaf, so it surfaces instead; re-touch
        // the leaf to ask what the leaf itself resolves.
        store.apply(&report(status(ProgramState::Working, Some("mid/leaf"), None, None, None, None)));
        assert_eq!(store.summary().expect("records exist").detail, "mid-app");
    }

    #[test]
    fn detail_without_any_text_is_empty_for_the_localized_fallback() {
        let mut store = TerminalProgramStatus::default();
        store.apply(&report(status(ProgramState::Done, Some("bare"), None, None, None, None)));
        let summary = store.summary().expect("records exist");
        assert_eq!(summary.state, ProgramState::Done);
        assert_eq!(summary.detail, "");
    }

    #[test]
    fn detail_neutralizes_controls_and_bidi_and_truncates() {
        let mut store = TerminalProgramStatus::default();
        store.apply(&report(status(
            ProgramState::Blocked,
            Some("hostile"),
            Some("run\u{202e}evil\u{7}line\nnext"),
            None,
            None,
            None,
        )));
        assert_eq!(
            store.summary().expect("records exist").detail,
            "run�evil�line�next",
            "bidi, BEL, and newline all become U+FFFD"
        );

        // Blocked outranks working, so retire it before probing truncation.
        store.apply(&report(status(ProgramState::Clear, Some("hostile"), None, None, None, None)));
        let long = "x".repeat(200);
        store.apply(&report(status(ProgramState::Working, Some("long"), Some(&long), None, None, None)));
        assert_eq!(store.summary().expect("records exist").detail, format!("{}…", "x".repeat(80)));
    }

    #[test]
    fn summary_carries_progress_through() {
        let mut store = TerminalProgramStatus::default();
        store.apply(&report(status(ProgramState::Working, Some("w"), Some("pushing"), None, None, Some(40))));
        let summary = store.summary().expect("records exist");
        assert_eq!(summary.progress, Some(40));
    }
}
