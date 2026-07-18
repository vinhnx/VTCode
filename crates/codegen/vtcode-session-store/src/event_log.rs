//! Append-only per-session `ThreadEvent` log plus index and manifest.

use std::fs::{File, OpenOptions};
use std::io::{BufRead, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use chrono::Utc;
use serde::{Deserialize, Serialize};
use vtcode_exec_events::{ThreadEvent, VersionedThreadEvent};

use crate::error::SessionStoreError;
use crate::session_dir;

/// Default maximum number of events retained per session before the oldest
/// completed turns are evicted.
pub const DEFAULT_MAX_EVENTS: usize = 10_000;

/// In-memory state protected by a mutex (cheap; appends are infrequent relative
/// to model inference).
struct LogState {
    manifest: SessionManifest,
    index: TurnIndex,
    /// Whether we are currently inside a turn (between TurnStarted and
    /// TurnCompleted/TurnFailed). Used to update the last index entry's
    /// offsets as intermediate events arrive.
    in_turn: bool,
    /// Running byte offset of the next append. Avoids a `stat` syscall per
    /// event (the previous implementation re-statted the file twice on every
    /// `append`); initialized from the file length on `open`.
    next_offset: u64,
}

impl LogState {
    fn new(session_id: &str) -> Self {
        Self {
            manifest: SessionManifest::new(session_id),
            index: TurnIndex::default(),
            in_turn: false,
            next_offset: 0,
        }
    }
}

/// Canonical append-only event log for a single session.
///
/// All session history is reconstructable from this log. Live conversation
/// state is never read back into context from here; the log is only consumed
/// for revert, compaction, analytics, and long-term-learning queries.
pub struct SessionEventLog {
    session_dir: PathBuf,
    events_path: PathBuf,
    file: Mutex<File>,
    state: Mutex<LogState>,
    max_events: usize,
}

impl SessionEventLog {
    /// Open the log for `session_id`, creating the session directory tree and
    /// rebuilding the index from `events.jsonl` if it already exists.
    pub fn open(workspace: &Path, session_id: &str, max_events: usize) -> Result<Self, SessionStoreError> {
        let dir = session_dir(workspace, session_id);
        std::fs::create_dir_all(dir.join(crate::DERIVED_DIR))
            .map_err(|e| SessionStoreError::CreateDir { path: dir.clone(), source: e })?;
        std::fs::create_dir_all(dir.join("index"))
            .map_err(|e| SessionStoreError::CreateDir { path: dir.clone(), source: e })?;
        let events_path = dir.join("events.jsonl");
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .append(true)
            .open(&events_path)
            .map_err(|e| SessionStoreError::io(events_path.clone(), e))?;
        let log = Self {
            session_dir: dir,
            events_path,
            file: Mutex::new(file),
            state: Mutex::new(LogState::new(session_id)),
            max_events,
        };
        log.scan()?;
        // Seed the running offset from the current file length so appends
        // continue seamlessly after a reopen (one stat at open, not per event).
        {
            let mut st = log.state.lock().map_err(poison)?;
            let len = log
                .file
                .lock()
                .map_err(poison)?
                .metadata()
                .map_err(|e| SessionStoreError::io(&log.events_path, e))?
                .len();
            st.next_offset = len;
        }
        Ok(log)
    }

    /// Append an event to the log and update the in-memory index/manifest.
    pub fn append(&self, event: &ThreadEvent) -> Result<(), SessionStoreError> {
        let line = serde_json::to_string(&VersionedThreadEvent::new(event.clone()))?;
        let written = line.len() + 1;
        let mut st = self.state.lock().map_err(poison)?;
        let start = st.next_offset;
        {
            let mut file = self.file.lock().map_err(poison)?;
            writeln!(file, "{line}").map_err(|e| SessionStoreError::io(&self.events_path, e))?;
        }
        let end = start + written as u64;
        st.next_offset = end;

        st.manifest.event_count += 1;
        st.manifest.updated_at = now_rfc3339();
        match event {
            ThreadEvent::TurnStarted(_) => {
                st.in_turn = true;
                let n = st.manifest.turn_count + 1;
                st.index.entries.push(TurnIndexEntry {
                    turn_number: n,
                    start_offset: start,
                    end_offset: end,
                    event_count: 1,
                    ts: now_rfc3339(),
                });
            }
            ThreadEvent::TurnCompleted(_) => {
                if st.in_turn {
                    if let Some(entry) = st.index.entries.last_mut() {
                        entry.end_offset = end;
                        entry.event_count += 1;
                    }
                    st.in_turn = false;
                    st.manifest.turn_count = st.index.entries.len() as u64;
                }
                st.manifest.status = "completed".to_string();
                self.persist_meta_locked(&st)?;
            }
            ThreadEvent::TurnFailed(_) => {
                if st.in_turn {
                    if let Some(entry) = st.index.entries.last_mut() {
                        entry.end_offset = end;
                        entry.event_count += 1;
                    }
                    st.in_turn = false;
                    st.manifest.turn_count = st.index.entries.len() as u64;
                }
                st.manifest.status = "failed".to_string();
                self.persist_meta_locked(&st)?;
            }
            _ => {
                if st.in_turn
                    && let Some(entry) = st.index.entries.last_mut()
                {
                    entry.end_offset = end;
                    entry.event_count += 1;
                }
            }
        }
        drop(st);
        let _ = self.enforce_event_cap();
        Ok(())
    }

    /// Enforce the per-session event cap by evicting the oldest completed
    /// turns when the log exceeds [`Self::max_events`]. Returns `Ok(())` even
    /// when no truncation is needed or the cap is disabled (`max_events == 0`).
    fn enforce_event_cap(&self) -> Result<(), SessionStoreError> {
        if self.max_events == 0 {
            return Ok(());
        }
        let mut st = self.state.lock().map_err(poison)?;
        if st.manifest.event_count <= self.max_events as u64 {
            return Ok(());
        }
        while st.manifest.event_count > self.max_events as u64
            && let Some(oldest) = st.index.entries.first()
        {
            let truncate_offset = oldest.end_offset;
            let _evicted = oldest.event_count;
            {
                let mut file = self.file.lock().map_err(poison)?;
                file.seek(SeekFrom::Start(truncate_offset))
                    .map_err(|e| SessionStoreError::io(&self.events_path, e))?;
                let mut remaining = Vec::new();
                file.read_to_end(&mut remaining)
                    .map_err(|e| SessionStoreError::io(&self.events_path, e))?;
                file.set_len(0).map_err(|e| SessionStoreError::io(&self.events_path, e))?;
                file.seek(SeekFrom::Start(0))
                    .map_err(|e| SessionStoreError::io(&self.events_path, e))?;
                file.write_all(&remaining)
                    .map_err(|e| SessionStoreError::io(&self.events_path, e))?;
                file.flush().map_err(|e| SessionStoreError::io(&self.events_path, e))?;
            }
            st.index.entries.remove(0);
            for entry in &mut st.index.entries {
                entry.start_offset -= truncate_offset;
                entry.end_offset -= truncate_offset;
            }
            st.next_offset -= truncate_offset;
            st.manifest.event_count = st.index.entries.iter().map(|e| e.event_count).sum();
            let _ = st.manifest.event_count; // approximate; turns dominate
        }
        Ok(())
    }

    /// Reconstruct every event belonging to `turn`.
    pub fn reconstruct_turn(&self, turn: u64) -> Result<Vec<ThreadEvent>, SessionStoreError> {
        let entry = {
            let st = self.state.lock().map_err(poison)?;
            st.index
                .entries
                .iter()
                .find(|e| e.turn_number == turn)
                .cloned()
                .ok_or(SessionStoreError::TurnNotFound { session: st.manifest.session_id.clone(), turn })?
        };
        let buf = {
            let mut file = self.file.lock().map_err(poison)?;
            file.seek(SeekFrom::Start(entry.start_offset))
                .map_err(|e| SessionStoreError::io(&self.events_path, e))?;
            let len = (entry.end_offset - entry.start_offset) as usize;
            let mut buf = vec![0u8; len];
            file.read_exact(&mut buf)
                .map_err(|e| SessionStoreError::io(&self.events_path, e))?;
            buf
        };
        let text = String::from_utf8_lossy(&buf);
        let mut events = Vec::new();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let v: VersionedThreadEvent = serde_json::from_str(line).map_err(SessionStoreError::Json)?;
            events.push(v.into_event());
        }
        Ok(events)
    }

    /// Number of turns recorded.
    #[must_use]
    pub fn turn_count(&self) -> u64 {
        self.state.lock().map_err(poison).map_or(0, |s| s.manifest.turn_count)
    }

    /// Number of events recorded.
    #[must_use]
    pub fn event_count(&self) -> u64 {
        self.state.lock().map_err(poison).map_or(0, |s| s.manifest.event_count)
    }

    /// Snapshot of the session manifest.
    #[must_use]
    pub fn manifest(&self) -> SessionManifest {
        self.state
            .lock()
            .map_err(poison)
            .map(|s| s.manifest.clone())
            .unwrap_or_else(|_| SessionManifest::new(""))
    }

    /// Snapshot of the turn index.
    #[must_use]
    pub fn turn_index(&self) -> TurnIndex {
        self.state.lock().map_err(poison).map(|s| s.index.clone()).unwrap_or_default()
    }

    /// Mark the session completed and flush metadata.
    pub fn complete(&self) -> Result<(), SessionStoreError> {
        let mut st = self.state.lock().map_err(poison)?;
        st.manifest.status = "completed".to_string();
        st.manifest.updated_at = now_rfc3339();
        self.persist_meta_locked(&st)
    }

    /// Rebuild index + manifest by scanning `events.jsonl` (authoritative).
    ///
    /// Reads the file line-by-line via `BufReader` to avoid loading the entire
    /// log into memory. Long-lived sessions can otherwise produce multi-megabyte
    /// logs that spike memory on every reopen.
    fn scan(&self) -> Result<(), SessionStoreError> {
        let mut st = self.state.lock().map_err(poison)?;
        if !self.events_path.exists() {
            return Ok(());
        }
        let file = File::open(&self.events_path).map_err(|e| SessionStoreError::io(&self.events_path, e))?;
        let mut reader = std::io::BufReader::new(file);
        let mut buf = Vec::new();
        let mut pos = 0u64;
        let mut first_ts: Option<String> = None;
        let mut in_turn = false;
        loop {
            buf.clear();
            let n = reader
                .read_until(b'\n', &mut buf)
                .map_err(|e| SessionStoreError::io(&self.events_path, e))?;
            if n == 0 {
                break;
            }
            let line_end = pos + n as u64;
            let trimmed = std::str::from_utf8(&buf).unwrap_or("").trim();
            if !trimmed.is_empty()
                && let Ok(v) = serde_json::from_str::<VersionedThreadEvent>(trimmed)
            {
                let event = v.into_event();
                st.manifest.event_count += 1;
                match &event {
                    ThreadEvent::ThreadStarted(_) => {
                        if first_ts.is_none() {
                            first_ts = Some(now_rfc3339());
                        }
                    }
                    ThreadEvent::TurnStarted(_) => {
                        in_turn = true;
                        let n = st.manifest.turn_count + 1;
                        st.index.entries.push(TurnIndexEntry {
                            turn_number: n,
                            start_offset: pos,
                            end_offset: line_end,
                            event_count: 1,
                            ts: now_rfc3339(),
                        });
                    }
                    ThreadEvent::TurnCompleted(_) | ThreadEvent::TurnFailed(_) => {
                        if in_turn {
                            if let Some(entry) = st.index.entries.last_mut() {
                                entry.end_offset = line_end;
                                entry.event_count += 1;
                            }
                            in_turn = false;
                            st.manifest.turn_count = st.index.entries.len() as u64;
                        }
                        match &event {
                            ThreadEvent::TurnCompleted(_) => {
                                st.manifest.status = "completed".to_string();
                            }
                            ThreadEvent::TurnFailed(_) => {
                                st.manifest.status = "failed".to_string();
                            }
                            _ => {}
                        }
                    }
                    _ => {
                        if in_turn && let Some(entry) = st.index.entries.last_mut() {
                            entry.end_offset = line_end;
                            entry.event_count += 1;
                        }
                    }
                }
            }
            pos = line_end;
        }
        if let Some(ts) = first_ts
            && st.manifest.created_at.is_empty()
        {
            st.manifest.created_at = ts;
        }
        Ok(())
    }

    fn persist_meta_locked(&self, st: &LogState) -> Result<(), SessionStoreError> {
        let mpath = self.session_dir.join("manifest.json");
        let bytes = serde_json::to_string_pretty(&st.manifest)?;
        std::fs::write(&mpath, bytes).map_err(|e| SessionStoreError::io(mpath, e))?;
        let ipath = self.session_dir.join("index").join("turns.json");
        let bytes = serde_json::to_string_pretty(&st.index)?;
        std::fs::write(&ipath, bytes).map_err(|e| SessionStoreError::io(ipath, e))?;
        Ok(())
    }
}

/// Locate the next newline at or after `from`, returning a past-the-end index.
fn poison<T>(_e: std::sync::PoisonError<T>) -> SessionStoreError {
    SessionStoreError::Io {
        path: PathBuf::new(),
        source: std::io::Error::other("session store lock poisoned"),
    }
}

fn now_rfc3339() -> String {
    Utc::now().to_rfc3339()
}

/// Session-level metadata persisted to `manifest.json`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionManifest {
    /// Stable session identifier (directory name).
    pub session_id: String,
    /// Layout schema version (`SESSION_STORE_SCHEMA_VERSION`).
    pub schema_version: u32,
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
}

impl SessionManifest {
    /// Create a fresh manifest for a session.
    #[must_use]
    pub fn new(session_id: &str) -> Self {
        let ts = now_rfc3339();
        Self {
            session_id: session_id.to_string(),
            schema_version: crate::SESSION_STORE_SCHEMA_VERSION,
            created_at: ts.clone(),
            updated_at: ts,
            turn_count: 0,
            event_count: 0,
            status: "active".to_string(),
        }
    }
}

/// Byte-offset index of a single turn within `events.jsonl`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TurnIndexEntry {
    /// Turn ordinal (1-based).
    pub turn_number: u64,
    /// Byte offset of the turn's first event.
    pub start_offset: u64,
    /// Byte offset just past the turn's last event.
    pub end_offset: u64,
    /// Number of events in the turn.
    pub event_count: u64,
    /// RFC3339 timestamp of turn start.
    pub ts: String,
}

/// Ordered index of all turns in a session.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct TurnIndex {
    /// Turn entries in ordinal order.
    pub entries: Vec<TurnIndexEntry>,
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
}
