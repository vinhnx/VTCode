//! Session-owned terminal status projection. Never controls input or execution.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, IsTerminal, Write};
use std::sync::{
    Mutex,
    atomic::{AtomicU64, Ordering},
};

use vtcode_commons::program_status::{
    InteractionKind, ProgramState, ProgramStatusUpdate, encode_report, record_segment,
};
use vtcode_commons::ui_protocol::{ActivityState, ProgressPhase, ProgressUpdate};

use super::types::{LocalAgentEntry, LocalAgentKind};

const RECORD_CAP: usize = 64;
static NEXT_SESSION: AtomicU64 = AtomicU64::new(1);
// Owned delivery attempts, used by the canonical panic/signal restoration path.
// The state comes from the current runtime projection, not delivery success.
// Lock order: terminal operations, then this registry. No writes after restore.
static CLEANUP_RECORDS: Mutex<BTreeMap<String, ProgramState>> = Mutex::new(BTreeMap::new());

#[derive(Clone, Debug, PartialEq, Eq)]
struct Report {
    state: ProgramState,
    kind: Option<InteractionKind>,
    title: &'static str,
    message: &'static str,
}

pub(crate) struct ProgramStatus {
    parent: String,
    enabled: bool,
    terminal: bool,
    closed: bool,
    underlying: ProgramState,
    operation: Option<u64>,
    newest_operation: u64,
    phase: Option<ProgressPhase>,
    waits: BTreeMap<u64, InteractionKind>,
    children: BTreeMap<String, Report>,
    // A failed flush may still have reached the terminal. Retain owned IDs
    // for removal without treating their report contents as delivered.
    attempted: BTreeSet<String>,
    delivered: BTreeMap<String, Report>,
}

impl Default for ProgramStatus {
    fn default() -> Self {
        let nonce = NEXT_SESSION.fetch_add(1, Ordering::Relaxed);
        let origin = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        Self {
            parent: record_segment("vtcode", &format!("{}-{origin}-{nonce}", std::process::id())),
            enabled: false,
            terminal: false,
            closed: false,
            underlying: ProgramState::Idle,
            operation: None,
            newest_operation: 0,
            phase: None,
            waits: BTreeMap::new(),
            children: BTreeMap::new(),
            attempted: BTreeSet::new(),
            delivered: BTreeMap::new(),
        }
    }
}

impl ProgramStatus {
    /// Set only by the real interactive runner, never a test/headless session.
    pub(crate) fn attach_terminal(&mut self) {
        self.terminal = io::stdin().is_terminal() && io::stdout().is_terminal() && io::stderr().is_terminal();
    }

    pub(crate) fn apply(&mut self, update: ProgramStatusUpdate) {
        match update {
            ProgramStatusUpdate::Configure { enabled } => {
                self.enabled = enabled;
                if !enabled {
                    self.children.clear();
                }
            }
            ProgramStatusUpdate::Wait { token, kind } => {
                self.waits.insert(token, kind);
            }
            ProgramStatusUpdate::Resume { token } => {
                self.waits.remove(&token);
            }
            ProgramStatusUpdate::Outcome(state) => {
                if matches!(
                    state,
                    ProgramState::Done | ProgramState::Error | ProgramState::Idle | ProgramState::Blocked
                ) {
                    self.underlying = state;
                    self.phase = None;
                }
            }
        }
    }

    pub(crate) fn activity(&mut self, state: ActivityState) {
        if state == ActivityState::Blocked {
            self.underlying = ProgramState::Blocked;
            self.phase = None;
        } else if state.is_busy() {
            self.underlying = ProgramState::Working;
        } else if state == ActivityState::Idle && self.operation.is_none() && self.underlying == ProgramState::Working {
            self.underlying = ProgramState::Idle;
        }
    }

    pub(crate) fn progress(&mut self, update: ProgressUpdate) {
        match update {
            ProgressUpdate::Begin { operation, phase } if operation.id() > self.newest_operation => {
                self.newest_operation = operation.id();
                self.operation = Some(operation.id());
                self.underlying = ProgramState::Working;
                self.phase = Some(phase);
            }
            ProgressUpdate::Phase { operation, phase }
                if self.operation == Some(operation.id()) && self.underlying == ProgramState::Working =>
            {
                self.phase = Some(phase);
            }
            ProgressUpdate::Finish { operation } if self.operation == Some(operation.id()) => {
                self.operation = None;
                self.phase = None;
                if self.underlying == ProgramState::Working {
                    self.underlying = ProgramState::Idle;
                }
            }
            _ => {}
        }
    }

    pub(crate) fn children(&mut self, entries: &[LocalAgentEntry]) {
        // The session already retains the latest Local Agents snapshot for
        // rendering. Reproject it on enable rather than maintaining an off copy.
        if !self.enabled {
            return;
        }
        let mut entries: Vec<_> = entries.iter().collect();
        entries.sort_by_key(|entry| {
            let priority = match entry.program_status {
                ProgramState::Working | ProgramState::Blocked => 0,
                ProgramState::Done | ProgramState::Error => 1,
                _ => 2,
            };
            (priority, std::cmp::Reverse(entry.updated_at), entry.kind, &entry.id)
        });
        self.children = entries
            .into_iter()
            .take(RECORD_CAP - 1)
            .map(|entry| {
                let id = format!("{}/{}", self.parent, record_segment(entry.kind.as_str(), &entry.id));
                let title = match entry.kind {
                    LocalAgentKind::Delegated => "Delegated agent",
                    LocalAgentKind::Background => "Background task",
                    LocalAgentKind::ExecSession => "Command session",
                };
                (
                    id,
                    Report {
                        state: entry.program_status,
                        kind: None,
                        title,
                        message: entry.program_status.as_str(),
                    },
                )
            })
            .collect();
    }

    fn desired(&self) -> BTreeMap<String, Report> {
        if !self.enabled {
            return BTreeMap::new();
        }
        let mut records = self.children.clone();
        let kind = self.waits.last_key_value().map(|(_, kind)| *kind);
        let state = if kind.is_some() {
            ProgramState::Blocked
        } else {
            self.underlying
        };
        let message = if let Some(kind) = kind {
            match kind {
                InteractionKind::Permission => "Waiting for permission",
                InteractionKind::Question => "Waiting for an answer",
                InteractionKind::Auth => "Waiting for authentication",
            }
        } else if state == ProgramState::Working {
            self.phase.map_or("Working", ProgressPhase::label)
        } else {
            match state {
                ProgramState::Done => "Completed",
                ProgramState::Error => "Failed",
                ProgramState::Blocked => "Waiting for guidance",
                _ => "Idle",
            }
        };
        records.insert(self.parent.clone(), Report { state, kind, title: "VT Code", message });
        if self.closed {
            records.retain(|_, report| report.state.is_finished());
        }
        records
    }

    /// Write changes only. Failed reports remain pending, including failed clears.
    fn deliver(&mut self, writer: &mut impl Write) -> anyhow::Result<()> {
        use anyhow::Context;
        let desired = self.desired();
        let removed: Vec<_> = self.attempted.iter().filter(|id| !desired.contains_key(*id)).cloned().collect();
        for id in removed {
            // Clearing a parent would also remove its retained finished children.
            // Retire an unfinished parent with idle when preserving descendants.
            let state = if id == self.parent && desired.keys().any(|key| key.starts_with(&format!("{id}/"))) {
                ProgramState::Idle
            } else {
                ProgramState::Clear
            };
            let wire = encode_report(&id, state, None, "", "")?;
            // Even a failed flush may change the terminal. A subtree clear
            // invalidates all successful-report cache entries beneath it.
            if id == self.parent && state == ProgramState::Clear {
                self.delivered.clear();
            } else {
                self.delivered.remove(&id);
            }
            writer
                .write_all(wire.as_bytes())
                .context("writing removed program status record")?;
            writer.flush().context("flushing removed program status record")?;
            if id == self.parent && state == ProgramState::Clear {
                self.attempted.clear();
                break;
            }
            self.attempted.remove(&id);
        }
        for (id, report) in desired {
            if self.delivered.get(&id) == Some(&report) {
                continue;
            }
            let wire = encode_report(&id, report.state, report.kind, report.title, report.message)?;
            self.attempted.insert(id.clone());
            self.delivered.remove(&id);
            writer.write_all(wire.as_bytes()).context("writing program status record")?;
            writer.flush().context("flushing program status record")?;
            self.delivered.insert(id, report);
        }
        Ok(())
    }

    pub(crate) fn flush(&mut self) {
        // A disabled session still needs to retry uncertain subtree clears.
        if !self.terminal || (!self.enabled && self.attempted.is_empty()) {
            return;
        }
        let _guard = super::panic_hook::lock_terminal_operations();
        if super::panic_hook::is_restore_claimed() {
            return;
        }
        if let Err(error) = self.deliver(&mut io::stderr().lock()) {
            tracing::warn!(error = ?error, "terminal program status delivery failed");
        }
        let mut records = CLEANUP_RECORDS.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        records.retain(|id, _| id != &self.parent && !id.starts_with(&format!("{}/", self.parent)));
        let desired = self.desired();
        records.extend(self.attempted.iter().map(|id| {
            let state = desired.get(id).map_or(ProgramState::Idle, |report| report.state);
            (id.clone(), state)
        }));
    }

    pub(crate) fn shutdown(&mut self) {
        self.closed = true;
        self.flush();
    }
}

/// Canonical restoration calls this under the terminal-operation lock.
/// Preserve finished outcomes and retire unfinished records best effort once.
pub(crate) fn cleanup_before_restore(writer: &mut impl Write) {
    let records = std::mem::take(&mut *CLEANUP_RECORDS.lock().unwrap_or_else(|poisoned| poisoned.into_inner()));
    retire_records(&records, writer);
}

fn retire_records(records: &BTreeMap<String, ProgramState>, writer: &mut impl Write) {
    for (id, state) in records {
        if state.is_finished() {
            continue;
        }
        let has_finished_children = records
            .iter()
            .any(|(child, state)| child.starts_with(&format!("{id}/")) && state.is_finished());
        let state = if has_finished_children {
            ProgramState::Idle
        } else {
            ProgramState::Clear
        };
        if let Err(error) = encode_report(id, state, None, "", "").and_then(|wire| {
            writer.write_all(wire.as_bytes())?;
            writer.flush()?;
            Ok(())
        }) {
            tracing::warn!(%error, "program status restoration cleanup failed");
        }
    }
}

#[cfg(test)]
mod tests;
