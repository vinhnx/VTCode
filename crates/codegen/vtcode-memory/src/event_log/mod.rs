//! Append-only per-session `ThreadEvent` log plus index and manifest.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::fs::File;
use std::io::{BufRead, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, Weak};

use chrono::Utc;
use serde::{Deserialize, Serialize};
use vtcode_commons::VtCodePaths;
use vtcode_exec_events::{EVENT_SCHEMA_VERSION, ThreadEvent, ThreadItemDetails, VersionedThreadEvent};

use crate::error::SessionStoreError;

/// Sidecar lock file flock-held while a session's event-log handles are open.
pub(crate) const SESSION_LOCK_FILE: &str = "session.lock";
use crate::manifest::{ManifestStore, PendingCapRewrite};
use crate::session_dir;

mod eviction;
mod manifest;
mod scan;

use eviction::default_eviction_summary_hook;
pub use manifest::{SessionManifest, TurnIndex, TurnIndexEntry};
use scan::{
    count_persisted_event_records, decode_events, infer_scan_turn_base, requires_full_lifecycle_validation,
    valid_lifecycle_payload,
};

/// Default maximum number of events retained per session before the oldest
/// completed turns are evicted.
pub const DEFAULT_MAX_EVENTS: usize = 10_000;

/// Maximum serialized event bytes retained before an append forces a write.
/// Turn boundaries and reads still flush immediately.
const MAX_WRITE_BUFFER_BYTES: usize = 64 * 1024;

/// Callback used to persist a summary of events before they are evicted.
///
/// The callback runs after the event bytes have been flushed and decoded, but
/// before the canonical log is rewritten. A failure leaves the original log
/// and in-memory index untouched, so retention never silently discards
/// history.
pub type EvictionSummaryHook = Arc<dyn Fn(&[ThreadEvent]) -> Result<(), SessionStoreError> + Send + Sync>;

/// Minimal envelope used while rebuilding the turn index.
///
/// The index only needs the event discriminator. Deserializing a complete
/// [`VersionedThreadEvent`] here would allocate every nested tool argument,
/// output, and thread item even though none of that payload is retained.
#[derive(Debug, Deserialize)]
struct VersionedEventKind<'a> {
    #[serde(rename = "schema_version", borrow)]
    _schema_version: &'a str,
    #[serde(borrow)]
    event: EventKind<'a>,
}

#[derive(Debug, Deserialize)]
struct EventKind<'a> {
    #[serde(rename = "type", borrow)]
    kind: &'a str,
}

/// Zero-clone serialization envelope for `ThreadEvent`.
///
/// Produces JSON byte-identical to `VersionedThreadEvent` but borrows the
/// event by reference instead of cloning it. `append` is called for every
/// runtime event, and `ThreadEvent` can carry large tool outputs / thread
/// items — cloning just to feed `serde_json::to_string` was pure waste.
#[derive(Serialize)]
struct BorrowedVersionedEvent<'a> {
    schema_version: &'a str,
    event: &'a ThreadEvent,
}

/// Turn-lifecycle discriminator extracted from either a `ThreadEvent` (at
/// append time) or a raw `&str` kind (during scan).  This is the single
/// representation that both code paths feed into
/// [`LogState::apply_lifecycle_event`], eliminating a duplicated state machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LifecycleKind {
    ThreadStarted,
    ThreadCompleted,
    TurnStarted,
    TurnCompleted,
    TurnFailed,
    Other,
}

impl LifecycleKind {
    /// Discriminate from a runtime `ThreadEvent` at append time.
    #[inline]
    fn from_event(event: &ThreadEvent) -> Self {
        match event {
            ThreadEvent::ThreadStarted(_) => Self::ThreadStarted,
            ThreadEvent::ThreadCompleted(_) => Self::ThreadCompleted,
            ThreadEvent::TurnStarted(_) => Self::TurnStarted,
            ThreadEvent::TurnCompleted(_) => Self::TurnCompleted,
            ThreadEvent::TurnFailed(_) => Self::TurnFailed,
            _ => Self::Other,
        }
    }

    /// Discriminate from a raw event-type string at scan time.
    #[inline]
    fn from_kind(kind: &str) -> Self {
        match kind {
            "thread.started" => Self::ThreadStarted,
            "thread.completed" => Self::ThreadCompleted,
            "turn.started" => Self::TurnStarted,
            "turn.completed" => Self::TurnCompleted,
            "turn.failed" => Self::TurnFailed,
            _ => Self::Other,
        }
    }
}

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
    /// Buffered pending writes to batch syscalls. Events are appended here
    /// and flushed to disk at turn boundaries or before read operations.
    write_buf: Vec<u8>,
}

#[derive(Debug, Clone, Copy)]
struct CapEvictionPlan {
    truncate_offset: u64,
    evicted_event_count: u64,
    evicted_turn_count: usize,
}

impl LogState {
    fn new(session_id: &str) -> Self {
        Self {
            manifest: SessionManifest::new(session_id),
            index: TurnIndex::default(),
            in_turn: false,
            next_offset: 0,
            write_buf: Vec::with_capacity(65536),
        }
    }

    /// Serialize `event` directly into the reusable write buffer with rollback
    /// on failure.
    ///
    /// This encapsulates the invariant that `write_buf` never contains a
    /// partial JSON document: if `serde_json::to_writer` fails mid-write the
    /// buffer is truncated back to its pre-serialization boundary.  Returns
    /// the `(start, end)` byte offsets of the serialized event so the caller
    /// can feed them to [`Self::apply_lifecycle_event`].
    fn serialize_event(&mut self, event: &ThreadEvent) -> Result<(u64, u64), SessionStoreError> {
        let start = self.next_offset;
        let buf_len_before = self.write_buf.len();
        if let Err(err) = serde_json::to_writer(
            &mut self.write_buf,
            &BorrowedVersionedEvent { schema_version: EVENT_SCHEMA_VERSION, event },
        ) {
            self.write_buf.truncate(buf_len_before);
            return Err(err.into());
        }
        self.write_buf.push(b'\n');
        let written = self.write_buf.len() - buf_len_before;
        let end = start + written as u64;
        self.next_offset = end;
        Ok((start, end))
    }

    /// Update the in-memory turn index and manifest counters for a single
    /// event.
    ///
    /// This is the single implementation of the turn-lifecycle state machine;
    /// both the append path (via [`LifecycleKind::from_event`]) and the scan
    /// path (via [`LifecycleKind::from_kind`]) route through here, eliminating
    /// a previously duplicated match block.
    ///
    /// Returns `true` when the event closes a turn boundary
    /// (`TurnCompleted` / `TurnFailed`) so the caller can persist metadata
    /// at the appropriate time (append persists immediately; scan persists
    /// once after the full scan).
    fn apply_lifecycle_event(&mut self, kind: LifecycleKind, start: u64, end: u64) -> bool {
        let is_boundary = match kind {
            LifecycleKind::ThreadStarted => {
                self.manifest.status = "active".to_string();
                false
            }
            LifecycleKind::ThreadCompleted => {
                self.manifest.status = "completed".to_string();
                true
            }
            LifecycleKind::TurnStarted => {
                self.manifest.status = "active".to_string();
                self.in_turn = true;
                let n = self.manifest.turn_count + 1;
                self.index.entries.push_back(TurnIndexEntry {
                    turn_number: n,
                    start_offset: start,
                    end_offset: end,
                    event_count: 1,
                    ts: now_rfc3339(),
                });
                false
            }
            LifecycleKind::TurnCompleted | LifecycleKind::TurnFailed => {
                if self.in_turn {
                    if let Some(entry) = self.index.entries.back_mut() {
                        entry.end_offset = end;
                        entry.event_count += 1;
                        // `turn_number` remains monotonic when older completed
                        // turns have been evicted. The manifest is the source
                        // for the next ordinal, so never replace it with the
                        // retained index length.
                        self.manifest.turn_count = self.manifest.turn_count.max(entry.turn_number);
                    }
                    self.in_turn = false;
                }
                true
            }
            LifecycleKind::Other => {
                if self.in_turn
                    && let Some(entry) = self.index.entries.back_mut()
                {
                    entry.end_offset = end;
                    entry.event_count += 1;
                }
                false
            }
        };
        // Persist the open-turn marker alongside the manifest. The marker is
        // deliberately optional for backwards compatibility: an older
        // manifest without it forces a scan on reopen so the state can be
        // reconstructed from the canonical event log.
        self.manifest.in_turn = Some(self.in_turn);
        is_boundary
    }

    /// Plan a cap-enforcement eviction: pop the oldest completed turns from
    /// the index until `event_count` is within `max_events`.
    ///
    /// Returns the byte offset at which the file should be rewritten and the
    /// counts needed to apply the eviction after successful I/O. Returns
    /// `None` when no eviction is needed.
    fn plan_cap_eviction(&self, max_events: usize) -> Option<CapEvictionPlan> {
        if max_events == 0 || self.manifest.event_count <= max_events as u64 {
            return None;
        }
        let mut evicted_event_count = 0u64;
        let mut truncate_offset = 0u64;
        let mut evicted_turn_count = 0;
        for (entry_index, oldest) in self.index.entries.iter().enumerate() {
            // Never evict the active turn. Its entry has a provisional end
            // offset and will be closed by a later completion/failure event;
            // removing it here would make that event unindexed and lose the
            // in-flight turn from reconstruction. If the completed history
            // alone cannot bring the log under the cap, retain the active
            // turn until it reaches a terminal boundary.
            if self.in_turn && entry_index + 1 == self.index.entries.len() {
                break;
            }
            if self.manifest.event_count.saturating_sub(evicted_event_count) <= max_events as u64 {
                break;
            }
            truncate_offset = oldest.end_offset;
            evicted_event_count += oldest.event_count;
            evicted_turn_count += 1;
        }
        if truncate_offset == 0 || evicted_turn_count == 0 {
            None
        } else {
            Some(CapEvictionPlan {
                truncate_offset,
                evicted_event_count,
                evicted_turn_count,
            })
        }
    }

    fn apply_cap_eviction(&mut self, plan: CapEvictionPlan, next_offset: u64) {
        for _ in 0..plan.evicted_turn_count {
            let _ = self.index.entries.pop_front();
        }
        for entry in &mut self.index.entries {
            entry.start_offset = entry.start_offset.saturating_sub(plan.truncate_offset);
            entry.end_offset = entry.end_offset.saturating_sub(plan.truncate_offset);
        }
        self.next_offset = next_offset;
        self.manifest.event_count = self.manifest.event_count.saturating_sub(plan.evicted_event_count);
        self.manifest.retained_turn_base = Some(self.retained_turn_base_after_eviction(0));
    }

    fn retained_turn_base_after_eviction(&self, evicted_turn_count: usize) -> u64 {
        self.index
            .entries
            .get(evicted_turn_count)
            .map(|entry| entry.turn_number)
            .or_else(|| self.manifest.turn_count.checked_add(1))
            .unwrap_or(1)
            .max(1)
    }
}

/// State and file handle shared by every owner of one session in this process.
///
/// A keyed operation lock alone is insufficient: two independently opened
/// handles could still carry stale turn counters and overwrite each other's
/// metadata after taking the lock. Sharing the mutable state and append file
/// makes the lock a true session boundary while preserving the value-type
/// `SessionEventLog` API.
struct SessionShared {
    file: Mutex<Option<File>>,
    state: Mutex<LogState>,
    eviction_lock: Mutex<()>,
    initialized: AtomicBool,
    /// Exclusive flock on the session's `session.lock`, held for as long as
    /// any handle to this session exists. Retention reads it as a liveness
    /// signal so an open-but-idle session is never marked or evicted.
    liveness_lock: Option<File>,
}

/// Return the process-wide shared state for one session's canonical event file.
///
/// The weak registry avoids retaining closed sessions forever while still
/// making repeated `open` calls converge on one file handle and turn state.
fn shared_session(events_path: &Path, session_id: &str) -> Result<Arc<SessionShared>, SessionStoreError> {
    static SESSION_SHARED: OnceLock<Mutex<HashMap<PathBuf, Weak<SessionShared>>>> = OnceLock::new();

    let key = events_path
        .parent()
        .and_then(|parent| vtcode_commons::paths::canonicalize(parent).ok())
        .and_then(|parent| events_path.file_name().map(|name| parent.join(name)))
        .unwrap_or_else(|| events_path.to_path_buf());
    let registry = SESSION_SHARED.get_or_init(|| Mutex::new(HashMap::new()));
    let mut shared_by_path = match registry.lock() {
        Ok(locks) => locks,
        Err(poisoned) => poisoned.into_inner(),
    };
    // Do not retain dead weak entries for every session ever opened by a
    // long-running process. The registry is only an in-process coordination
    // aid, so removing entries with no live owners is safe.
    shared_by_path.retain(|_, shared| shared.strong_count() > 0);
    if let Some(shared) = shared_by_path.get(&key).and_then(Weak::upgrade) {
        return Ok(shared);
    }

    let file = VtCodePaths::open_private_append_file(events_path)
        .map_err(|error| SessionStoreError::io(events_path.to_path_buf(), std::io::Error::other(error)))?;
    let shared = Arc::new(SessionShared {
        file: Mutex::new(Some(file)),
        state: Mutex::new(LogState::new(session_id)),
        eviction_lock: Mutex::new(()),
        initialized: AtomicBool::new(false),
        liveness_lock: acquire_liveness_lock(events_path),
    });
    shared_by_path.insert(key, Arc::downgrade(&shared));
    Ok(shared)
}

/// Best-effort exclusive flock on the session's `session.lock`.
///
/// The lock is the liveness signal retention reads (`session_dir_is_live`): a
/// held lock means a live process still has the session open. The crate has
/// no logging surface and this is an advisory signal, not persistence, so a
/// failure to create/open/lock degrades to unlocked (previous retention
/// behavior) instead of failing the session open or swallowing a persistence
/// error. The fd is released automatically when the shared handle drops.
fn acquire_liveness_lock(events_path: &Path) -> Option<File> {
    let dir = events_path.parent()?;
    let lock_path = dir.join(SESSION_LOCK_FILE);
    VtCodePaths::write_private_file_atomic_if_absent(&lock_path, b"").ok()?;
    let file = std::fs::OpenOptions::new().read(true).write(true).open(&lock_path).ok()?;
    match file.try_lock() {
        Ok(()) => Some(file),
        Err(_) => None,
    }
}

/// Canonical append-only event log for a single session.
///
/// All session history is reconstructable from this log. Live conversation
/// state is never read back into context from here; the log is only consumed
/// for revert, compaction, analytics, and long-term-learning queries.
pub struct SessionEventLog {
    events_path: PathBuf,
    manifest_store: ManifestStore,
    shared: Arc<SessionShared>,
    max_events: usize,
    eviction_summary_hook: EvictionSummaryHook,
}

impl SessionEventLog {
    /// Open the log for `session_id`, creating the session directory tree and
    /// rebuilding the index from `events.jsonl` if it already exists.
    pub(crate) fn open(workspace: &Path, session_id: &str, max_events: usize) -> Result<Self, SessionStoreError> {
        let dir = session_dir(workspace, session_id);
        let hook = default_eviction_summary_hook(dir.join(crate::DERIVED_DIR), session_id.to_string());
        Self::open_with_eviction_summary(workspace, session_id, max_events, hook)
    }

    /// Open a log with an explicit eviction-summary callback.
    ///
    /// This is useful for hosts that keep derived memory in another store and
    /// for deterministic failure-path tests. The callback must persist its
    /// summary before returning `Ok(())`.
    pub fn open_with_eviction_summary(
        workspace: &Path,
        session_id: &str,
        max_events: usize,
        eviction_summary_hook: EvictionSummaryHook,
    ) -> Result<Self, SessionStoreError> {
        let dir = session_dir(workspace, session_id);
        crate::ensure_private_directory(&crate::sessions_root(workspace))?;
        crate::ensure_private_directory(&dir)?;
        crate::ensure_private_directory(&dir.join(crate::DERIVED_DIR))?;
        crate::ensure_private_directory(&dir.join("index"))?;
        let events_path = dir.join("events.jsonl");
        let manifest_store = ManifestStore::new(dir.clone());
        let pending_rewrite = manifest_store.load_pending_cap_rewrite()?;
        let shared = shared_session(&events_path, session_id)?;
        let log = Self {
            events_path: events_path.clone(),
            manifest_store,
            shared,
            max_events,
            eviction_summary_hook,
        };
        let _eviction_guard = log.shared.eviction_lock.lock().map_err(poison)?;
        // Try the fast path: read the persisted manifest + index and skip
        // the O(n) scan when they are present and consistent.
        if !log.shared.initialized.load(Ordering::Acquire) {
            let manifest_opt = log.manifest_store.load_manifest()?;
            let index_opt = log.manifest_store.load_turn_index()?;
            let file_len = log.event_file_metadata_len()?;
            let pending_rewrite_matches_file = pending_rewrite.as_ref().is_some_and(|pending| {
                pending.new_file_len == file_len && pending.new_file_len < pending.previous_file_len
            });
            match (&manifest_opt, &index_opt) {
                (Some(manifest), Some(index))
                    if !pending_rewrite_matches_file
                        && manifest.in_turn.is_some()
                        && manifest.persisted_file_len == Some(file_len)
                        && index.is_valid_for_file(file_len)
                        && index.is_consistent_with_manifest(manifest) =>
                {
                    let mut st = log.shared.state.lock().map_err(poison)?;
                    st.in_turn = manifest.in_turn.unwrap_or(false);
                    st.manifest = manifest.clone();
                    st.index = index.clone();
                    st.next_offset = file_len;
                }
                _ => {
                    let scan_turn_base = infer_scan_turn_base(
                        manifest_opt.as_ref(),
                        index_opt.as_ref(),
                        pending_rewrite.as_ref().filter(|_| pending_rewrite_matches_file),
                        file_len,
                    );
                    {
                        let mut st = log.shared.state.lock().map_err(poison)?;
                        if let Some(previous) = manifest_opt.as_ref() {
                            st.manifest = previous.clone();
                        }
                        // The canonical event file is authoritative after any
                        // stale/corrupt metadata. Preserve the ordinal of the
                        // first retained turn while rebuilding all counters.
                        st.manifest.turn_count = scan_turn_base.saturating_sub(1);
                        st.manifest.event_count = 0;
                        st.manifest.status = "active".to_string();
                        st.manifest.in_turn = Some(false);
                        st.manifest.retained_turn_base = Some(scan_turn_base);
                        st.index = TurnIndex::default();
                        st.in_turn = false;
                    }
                    log.scan()?;
                    let mut st = log.shared.state.lock().map_err(poison)?;
                    st.next_offset = file_len;
                    log.persist_meta_locked(&mut st)?;
                }
            }
            if pending_rewrite.is_some() {
                // A marker whose file length did not match either side of the
                // rewrite is stale, while a matching marker has now been
                // incorporated into the repaired metadata. In both cases it
                // is safe to remove it after the open path has persisted the
                // authoritative state.
                log.manifest_store.clear_pending_cap_rewrite()?;
            }
            log.shared.initialized.store(true, Ordering::Release);
        }
        drop(_eviction_guard);
        Ok(log)
    }

    /// Append an event to the log and update the in-memory index/manifest.
    pub fn append(&self, event: &ThreadEvent) -> Result<(), SessionStoreError> {
        let _eviction_guard = self.shared.eviction_lock.lock().map_err(poison)?;
        let mut st = self.shared.state.lock().map_err(poison)?;

        // Serialize into the write buffer with rollback on failure — the
        // invariant that `write_buf` never contains partial JSON is
        // encapsulated in `serialize_event`.
        let (start, end) = st.serialize_event(event)?;

        st.manifest.event_count += 1;
        st.manifest.updated_at = now_rfc3339();

        // Route through the single turn-lifecycle state machine.  When the
        // event closes a turn, persist metadata immediately so a reopen
        // after a mid-turn crash sees a consistent index.
        let is_turn_boundary = st.apply_lifecycle_event(LifecycleKind::from_event(event), start, end);
        if is_turn_boundary {
            self.persist_meta_locked(&mut st)?;
        }

        if st.write_buf.len() >= MAX_WRITE_BUFFER_BYTES {
            // Persist metadata with the bounded byte flush so a reopen after
            // a mid-turn crash does not trust an index that predates these
            // already-written events.
            self.persist_meta_locked(&mut st)?;
        }
        drop(st);
        self.enforce_event_cap()
    }

    /// Enforce the per-session event cap by evicting the oldest completed
    /// turns when the log exceeds [`Self::max_events`]. Returns `Ok(())` even
    /// when no truncation is needed or the cap is disabled (`max_events == 0`).
    fn enforce_event_cap(&self) -> Result<(), SessionStoreError> {
        let mut st = self.shared.state.lock().map_err(poison)?;

        // `plan_cap_eviction` encapsulates the index arithmetic and returns
        // `None` when the cap is disabled or not yet exceeded.
        let Some(plan) = st.plan_cap_eviction(self.max_events) else {
            return Ok(());
        };

        // Keep ordinary appends in memory until a turn boundary or an
        // explicit read. Cap enforcement is the one append-time path that
        // needs the complete on-disk file before rewriting it.
        self.flush_write_buf_locked(&mut st)?;

        let (evicted, mut remaining, previous_file_len) = {
            let mut file_slot = self.shared.file.lock().map_err(poison)?;
            let file = file_slot.as_mut().ok_or_else(|| self.event_file_unavailable())?;
            let file_len = file
                .metadata()
                .map_err(|error| SessionStoreError::io(&self.events_path, error))?
                .len();
            if plan.truncate_offset > file_len {
                return Err(SessionStoreError::io(
                    &self.events_path,
                    std::io::Error::new(std::io::ErrorKind::InvalidData, "cap offset exceeds event log length"),
                ));
            }
            file.seek(SeekFrom::Start(0))
                .map_err(|e| SessionStoreError::io(&self.events_path, e))?;
            let mut evicted = vec![
                0u8;
                usize::try_from(plan.truncate_offset).map_err(|error| {
                    SessionStoreError::io(
                        &self.events_path,
                        std::io::Error::new(std::io::ErrorKind::InvalidData, error),
                    )
                })?
            ];
            file.read_exact(&mut evicted)
                .map_err(|e| SessionStoreError::io(&self.events_path, e))?;
            file.seek(SeekFrom::Start(plan.truncate_offset))
                .map_err(|e| SessionStoreError::io(&self.events_path, e))?;
            let mut remaining = Vec::new();
            file.read_to_end(&mut remaining)
                .map_err(|e| SessionStoreError::io(&self.events_path, e))?;
            (evicted, remaining, file_len)
        };
        let retained_turn_base = st.retained_turn_base_after_eviction(plan.evicted_turn_count);
        drop(st);

        // The turn index counts only records inside indexed turns. The bytes
        // removed by a cap rewrite may also contain valid session-level
        // records (for example `thread.started`) before the first turn, so
        // reconcile the manifest against the actual persisted prefix rather
        // than the turn-only estimate from `plan_cap_eviction`.
        let mut plan = plan;
        plan.evicted_event_count = count_persisted_event_records(&evicted);
        let evicted_events = decode_events(&evicted);
        (self.eviction_summary_hook)(&evicted_events)?;

        // Matrix snapshots are complete session-level checkpoints. Preserve the
        // latest evicted checkpoint only when no newer snapshot survives.
        let mut matrix_checkpoints = BTreeMap::new();
        let mut planning_checkpoint = None;
        for event in &evicted_events {
            if let ThreadEvent::PlanDelta(delta) = event
                && delta.recovery.is_some()
            {
                planning_checkpoint = Some(event);
            }
            if let ThreadEvent::MatrixUpdated(snapshot) = event {
                matrix_checkpoints.insert(snapshot.spec.id.clone(), event);
            }
        }
        for event in decode_events(&remaining) {
            if let ThreadEvent::PlanDelta(delta) = &event
                && delta.recovery.is_some()
            {
                planning_checkpoint = None;
            }
            if let ThreadEvent::MatrixUpdated(snapshot) = event {
                matrix_checkpoints.remove(&snapshot.spec.id);
            }
        }
        let mut checkpoint_prefix = Vec::new();
        for event in matrix_checkpoints.values().copied().chain(planning_checkpoint) {
            serde_json::to_writer(&mut checkpoint_prefix, &VersionedThreadEvent::new((*event).clone()))
                .map_err(|error| SessionStoreError::io(&self.events_path, std::io::Error::other(error)))?;
            checkpoint_prefix.push(b'\n');
        }
        let checkpoint_bytes = checkpoint_prefix.len() as u64;
        let checkpoint_count = matrix_checkpoints.len() as u64 + u64::from(planning_checkpoint.is_some());
        if !checkpoint_prefix.is_empty() {
            checkpoint_prefix.extend_from_slice(&remaining);
            remaining = checkpoint_prefix;
        }

        let new_file_len = u64::try_from(remaining.len()).map_err(|error| {
            SessionStoreError::io(&self.events_path, std::io::Error::new(std::io::ErrorKind::InvalidData, error))
        })?;
        self.manifest_store.write_pending_cap_rewrite(&PendingCapRewrite {
            previous_file_len,
            new_file_len,
            retained_turn_base,
        })?;
        let next_offset = self.replace_event_file_contents(&remaining)?;
        let mut st = self.shared.state.lock().map_err(poison)?;
        st.apply_cap_eviction(plan, next_offset);
        for entry in &mut st.index.entries {
            entry.start_offset += checkpoint_bytes;
            entry.end_offset += checkpoint_bytes;
        }
        st.manifest.event_count += checkpoint_count;
        // The rewrite changed byte offsets and retained counts; persist the
        // derived metadata before exposing the append as successful.
        self.persist_meta_locked(&mut st)?;
        self.manifest_store.clear_pending_cap_rewrite()?;
        Ok(())
    }

    /// Reconstruct every event belonging to `turn`.
    pub(crate) fn reconstruct_turn(&self, turn: u64) -> Result<Vec<ThreadEvent>, SessionStoreError> {
        // Keep the index snapshot and byte-range read together with cap
        // rewriting. Otherwise an eviction can replace the file between these
        // steps and leave the snapshot offsets pointing into unrelated events.
        let _eviction_guard = self.shared.eviction_lock.lock().map_err(poison)?;
        let entry = {
            let st = self.shared.state.lock().map_err(poison)?;
            st.index
                .entries
                .iter()
                .find(|e| e.turn_number == turn)
                .cloned()
                .ok_or(SessionStoreError::TurnNotFound { session: st.manifest.session_id.clone(), turn })?
        };
        {
            let mut st = self.shared.state.lock().map_err(poison)?;
            self.flush_write_buf_locked(&mut st)?;
        }
        let buf = {
            let mut file_slot = self.shared.file.lock().map_err(poison)?;
            let file = file_slot.as_mut().ok_or_else(|| self.event_file_unavailable())?;
            file.seek(SeekFrom::Start(entry.start_offset))
                .map_err(|e| SessionStoreError::io(&self.events_path, e))?;
            let len = usize::try_from(entry.end_offset.checked_sub(entry.start_offset).ok_or_else(|| {
                SessionStoreError::io(
                    &self.events_path,
                    std::io::Error::new(std::io::ErrorKind::InvalidData, "turn index offsets are out of order"),
                )
            })?)
            .map_err(|error| {
                SessionStoreError::io(&self.events_path, std::io::Error::new(std::io::ErrorKind::InvalidData, error))
            })?;
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
            // The index scan only validates the event envelope (plus the
            // lifecycle shape) so it can rebuild cheaply. A line accepted by
            // the scan can therefore still fail full decoding here; skip it
            // instead of failing the whole reconstruction (revert, compaction,
            // and analytics must not break on a single malformed record).
            let v: VersionedThreadEvent = match serde_json::from_str(line) {
                Ok(v) => v,
                Err(_) => continue,
            };
            events.push(v.into_event());
        }
        Ok(events)
    }

    /// Number of turns recorded.
    #[must_use]
    pub(crate) fn turn_count(&self) -> u64 {
        self.shared.state.lock().map_err(poison).map_or(0, |s| s.manifest.turn_count)
    }

    /// Number of events recorded.
    #[must_use]
    pub fn event_count(&self) -> u64 {
        self.shared.state.lock().map_err(poison).map_or(0, |s| s.manifest.event_count)
    }

    /// Flush pending event bytes and metadata to the session store.
    /// Visit one consistent retained range, including records outside turns.
    /// Reads are streamed while cap rewrites and appends are excluded.
    pub fn visit_snapshot<F>(&self, mut visitor: F) -> Result<SessionManifest, SessionStoreError>
    where
        F: FnMut(u64, &[u8]),
    {
        use std::io::{BufRead, BufReader};
        let _eviction_guard = self.shared.eviction_lock.lock().map_err(poison)?;
        let mut st = self.shared.state.lock().map_err(poison)?;
        self.persist_meta_locked(&mut st)?;
        let mut file_slot = self.shared.file.lock().map_err(poison)?;
        let file = file_slot.as_mut().ok_or_else(|| self.event_file_unavailable())?;
        file.seek(SeekFrom::Start(0))
            .map_err(|e| SessionStoreError::io(&self.events_path, e))?;
        let mut reader = BufReader::new(file.take(st.next_offset));
        let mut offset = 0;
        let mut line = Vec::new();
        loop {
            line.clear();
            let length = reader
                .read_until(b'\n', &mut line)
                .map_err(|e| SessionStoreError::io(&self.events_path, e))?;
            if length == 0 {
                break;
            }
            visitor(offset, &line);
            offset += length as u64;
        }
        Ok(st.manifest.clone())
    }

    /// Flush pending event bytes and metadata to the session store.
    pub fn flush(&self) -> Result<(), SessionStoreError> {
        let _eviction_guard = self.shared.eviction_lock.lock().map_err(poison)?;
        let mut st = self.shared.state.lock().map_err(poison)?;
        self.persist_meta_locked(&mut st)
    }

    /// Snapshot of the session manifest.
    #[must_use]
    pub fn manifest(&self) -> SessionManifest {
        self.shared
            .state
            .lock()
            .map_err(poison)
            .map(|s| s.manifest.clone())
            .unwrap_or_else(|_| SessionManifest::new(""))
    }

    /// Snapshot of the turn index.
    #[must_use]
    pub fn turn_index(&self) -> TurnIndex {
        self.shared
            .state
            .lock()
            .map_err(poison)
            .map(|s| s.index.clone())
            .unwrap_or_default()
    }

    /// Flush metadata for callers that explicitly close a log handle.
    ///
    /// Terminal status is intentionally controlled only by a persisted
    /// `thread.completed` event. This method does not synthesize lifecycle
    /// state for callers that merely release a store handle.
    pub(crate) fn complete(&self) -> Result<(), SessionStoreError> {
        let _eviction_guard = self.shared.eviction_lock.lock().map_err(poison)?;
        let mut st = self.shared.state.lock().map_err(poison)?;
        st.manifest.updated_at = now_rfc3339();
        self.persist_meta_locked(&mut st)
    }

    /// Rebuild index + manifest by scanning `events.jsonl` (authoritative).
    ///
    /// Reads the file line-by-line via `BufReader` to avoid loading the entire
    /// log into memory. Long-lived sessions can otherwise produce multi-megabyte
    /// logs that spike memory on every reopen.
    fn scan(&self) -> Result<(), SessionStoreError> {
        let mut st = self.shared.state.lock().map_err(poison)?;
        let file = self
            .shared
            .file
            .lock()
            .map_err(poison)?
            .as_ref()
            .ok_or_else(|| self.event_file_unavailable())?
            .try_clone()
            .map_err(|e| SessionStoreError::io(&self.events_path, e))?;
        let mut reader = std::io::BufReader::new(file);
        reader
            .seek(SeekFrom::Start(0))
            .map_err(|e| SessionStoreError::io(&self.events_path, e))?;
        let mut buf = Vec::new();
        let mut pos = 0u64;
        let mut first_ts: Option<String> = None;
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
                && let Ok(v) = serde_json::from_str::<VersionedEventKind<'_>>(trimmed)
            {
                let kind = v.event.kind;
                if requires_full_lifecycle_validation(kind) && !valid_lifecycle_payload(kind, trimmed) {
                    pos = line_end;
                    continue;
                }
                st.manifest.event_count += 1;
                // `thread.started` is not part of the turn lifecycle — it
                // only seeds `created_at` on the first occurrence.
                if kind == "thread.started" && first_ts.is_none() {
                    first_ts = Some(now_rfc3339());
                }
                // Route turn-lifecycle events through the same state machine
                // as `append`, eliminating a previously duplicated match block.
                st.apply_lifecycle_event(LifecycleKind::from_kind(kind), pos, line_end);
            }
            pos = line_end;
        }
        // Keep the open-turn state reconstructed from the canonical event log.
        // This lets a reopened session continue a turn that was flushed before
        // its completion event was written.
        st.manifest.in_turn = Some(st.in_turn);
        if let Some(ts) = first_ts
            && st.manifest.created_at.is_empty()
        {
            st.manifest.created_at = ts;
        }
        Ok(())
    }

    fn persist_meta_locked(&self, st: &mut LogState) -> Result<(), SessionStoreError> {
        self.flush_write_buf_locked(st)?;
        // The manifest is only eligible for the fast reopen path when it
        // describes the complete on-disk event file. Drop intentionally
        // flushes bytes without metadata, so a length mismatch safely forces
        // the authoritative scan on the next open.
        st.manifest.persisted_file_len = Some(st.next_offset);
        // Publish the derived index first. If a process stops between these
        // two atomic renames, the older manifest still carries a stale file
        // length and forces a scan instead of allowing the new manifest to
        // pair with an older, apparently valid index.
        self.manifest_store.write_turn_index(&st.index)?;
        self.manifest_store.write_manifest(&st.manifest)?;
        Ok(())
    }

    /// Flush the in-memory write buffer to the underlying file.
    fn flush_write_buf_locked(&self, st: &mut LogState) -> Result<(), SessionStoreError> {
        if st.write_buf.is_empty() {
            return Ok(());
        }
        let mut file_slot = self.shared.file.lock().map_err(poison)?;
        let file = file_slot.as_mut().ok_or_else(|| self.event_file_unavailable())?;
        let previous_len = file.metadata().map_err(|e| SessionStoreError::io(&self.events_path, e))?.len();
        if let Err(error) = file.write_all(&st.write_buf) {
            if file.set_len(previous_len).is_err() {
                st.write_buf.clear();
            }
            return Err(SessionStoreError::io(&self.events_path, error));
        }
        if let Err(error) = file.sync_data() {
            st.write_buf.clear();
            return Err(SessionStoreError::io(&self.events_path, error));
        }
        st.write_buf.clear();
        Ok(())
    }

    fn event_file_metadata_len(&self) -> Result<u64, SessionStoreError> {
        let file_slot = self.shared.file.lock().map_err(poison)?;
        file_slot
            .as_ref()
            .ok_or_else(|| self.event_file_unavailable())?
            .metadata()
            .map(|metadata| metadata.len())
            .map_err(|error| SessionStoreError::io(&self.events_path, error))
    }

    fn open_event_file(&self) -> Result<File, SessionStoreError> {
        VtCodePaths::open_private_append_file(&self.events_path)
            .map_err(|error| SessionStoreError::io(&self.events_path, std::io::Error::other(error)))
    }

    fn replace_event_file_contents(&self, contents: &[u8]) -> Result<u64, SessionStoreError> {
        let old_file = {
            let mut file_slot = self.shared.file.lock().map_err(poison)?;
            file_slot.take().ok_or_else(|| self.event_file_unavailable())?
        };
        drop(old_file);

        if let Err(error) = VtCodePaths::write_private_file_atomic(&self.events_path, contents)
            .map_err(|error| SessionStoreError::io(&self.events_path, std::io::Error::other(error)))
        {
            let restored = self.open_event_file();
            if let Ok(file) = restored {
                let mut file_slot = self.shared.file.lock().map_err(poison)?;
                *file_slot = Some(file);
                return Err(error);
            }
            return Err(SessionStoreError::io(
                &self.events_path,
                std::io::Error::other(format!("{error}; failed to restore event log handle")),
            ));
        }

        let replacement = self.open_event_file()?;
        let next_offset = replacement
            .metadata()
            .map_err(|error| SessionStoreError::io(&self.events_path, error))?
            .len();
        let mut file_slot = self.shared.file.lock().map_err(poison)?;
        *file_slot = Some(replacement);
        Ok(next_offset)
    }

    fn event_file_unavailable(&self) -> SessionStoreError {
        SessionStoreError::io(&self.events_path, std::io::Error::other("event log file is unavailable"))
    }
}

impl Drop for SessionEventLog {
    fn drop(&mut self) {
        if let Ok(_eviction_guard) = self.shared.eviction_lock.lock()
            && let Ok(mut st) = self.shared.state.lock()
        {
            // The fallible `flush` method is the authoritative shutdown path;
            // Drop only provides a best-effort byte flush for callers that do
            // not explicitly close the log. Rewriting metadata here could
            // overwrite a manifest update made by another owner after the
            // last append.
            let _ = self.flush_write_buf_locked(&mut st);
        }
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

#[cfg(test)]
mod borrowed_envelope_tests;
#[cfg(test)]
mod cap_eviction_tests;
#[cfg(test)]
mod lifecycle_state_machine_tests;
