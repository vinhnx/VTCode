use crate::audit::{FileConflictAuditEvent, FileConflictAuditLog};
use crate::config::PermissionsConfig;
use crate::tools::file_ops::{build_diff_preview, diff_preview_error_skip};
use anyhow::{Context, Result};
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use parking_lot::Mutex;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::Notify;

pub const FILE_CONFLICT_OVERRIDE_ARG: &str = "__vtcode_conflict_override";
pub const FILE_CONFLICT_DETECTED_FIELD: &str = "conflict_detected";
pub const FILE_CONFLICT_PATH_FIELD: &str = "conflict_path";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileSnapshot {
    pub exists: bool,
    pub size_bytes: u64,
    pub modified_millis: Option<u128>,
    pub sha256: String,
    pub text_content: Option<String>,
}

impl FileSnapshot {
    fn to_json(&self) -> Value {
        json!({
            "exists": self.exists,
            "size_bytes": self.size_bytes,
            "modified_millis": self.modified_millis,
            "sha256": self.sha256,
        })
    }

    fn same_contents(&self, other: &Self) -> bool {
        self.exists == other.exists
            && self.size_bytes == other.size_bytes
            && self.sha256 == other.sha256
    }

    fn same_identity(&self, other: &Self) -> bool {
        self.same_contents(other)
            && self.modified_millis == other.modified_millis
    }

    fn from_identity_value(value: &Value) -> Option<Self> {
        let object = value.as_object()?;
        let modified_millis = match object.get("modified_millis") {
            Some(Value::Null) | None => None,
            Some(value) => value
                .as_u64()
                .map(u128::from)
                .or_else(|| value.as_i64().filter(|millis| *millis >= 0).map(|millis| millis as u128)),
        };

        Some(Self {
            exists: object.get("exists")?.as_bool()?,
            size_bytes: object.get("size_bytes")?.as_u64()?,
            modified_millis,
            sha256: object.get("sha256")?.as_str()?.to_string(),
            text_content: None,
        })
    }

    fn from_text_content(content: &str) -> Self {
        let bytes = content.as_bytes();
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        let digest = hasher.finalize();

        Self {
            exists: true,
            size_bytes: bytes.len() as u64,
            modified_millis: None,
            sha256: hex_digest(&digest),
            text_content: Some(content.to_string()),
        }
    }

    fn missing() -> Self {
        Self {
            exists: false,
            size_bytes: 0,
            modified_millis: None,
            sha256: String::new(),
            text_content: None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct FileConflict {
    pub path: PathBuf,
    pub read_snapshot: Option<FileSnapshot>,
    pub disk_snapshot: Option<FileSnapshot>,
    pub intended_content: Option<String>,
    pub emit_hitl_notification: bool,
}

impl FileConflict {
    pub fn to_tool_output(&self, workspace_root: &Path) -> Value {
        let display_path = workspace_relative_display(workspace_root, &self.path);
        let disk_content = self
            .disk_snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.text_content.clone());
        let diff_preview = match (&disk_content, &self.intended_content) {
            (Some(before), Some(after)) => build_diff_preview(&display_path, Some(before), after),
            (None, Some(_)) => diff_preview_error_skip("binary_or_non_utf8_disk_content", None),
            _ => diff_preview_error_skip("missing_intended_content", None),
        };

        json!({
            "success": true,
            FILE_CONFLICT_DETECTED_FIELD: true,
            FILE_CONFLICT_PATH_FIELD: display_path,
            "message": "File changed on disk since the agent last read it.",
            "resolution": "pending",
            "emit_hitl_notification": self.emit_hitl_notification,
            "disk_content": disk_content,
            "intended_content": self.intended_content,
            "read_snapshot": self.read_snapshot.as_ref().map(FileSnapshot::to_json),
            "disk_snapshot": self.disk_snapshot.as_ref().map(FileSnapshot::to_json),
            "diff_preview": diff_preview,
        })
    }
}

#[derive(Clone, Debug)]
struct StaleConflictState {
    notification_emitted: bool,
}

#[derive(Clone, Debug)]
struct TrackedFileState {
    last_read_snapshot: Option<FileSnapshot>,
    last_known_disk_snapshot: Option<FileSnapshot>,
    last_agent_write_snapshot: Option<FileSnapshot>,
    active_mutation: Option<u64>,
    pending_mutations: VecDeque<u64>,
    stale_conflict: Option<StaleConflictState>,
    notify: Arc<Notify>,
}

impl TrackedFileState {
    fn new() -> Self {
        Self {
            last_read_snapshot: None,
            last_known_disk_snapshot: None,
            last_agent_write_snapshot: None,
            active_mutation: None,
            pending_mutations: VecDeque::new(),
            stale_conflict: None,
            notify: Arc::new(Notify::new()),
        }
    }
}

#[derive(Default)]
struct MonitorState {
    tracked_files: HashMap<PathBuf, TrackedFileState>,
    watched_parents: HashSet<PathBuf>,
}

struct EditedFileMonitorInner {
    state: Mutex<MonitorState>,
    watcher: StdMutex<Option<RecommendedWatcher>>,
    audit_log: Mutex<Option<FileConflictAuditLog>>,
    next_mutation_id: AtomicU64,
    debounce_duration: Duration,
}

#[derive(Clone)]
pub struct EditedFileMonitor {
    inner: Arc<EditedFileMonitorInner>,
}

pub struct MutationLease {
    inner: Arc<EditedFileMonitorInner>,
    path: PathBuf,
    mutation_id: u64,
    released: bool,
}

struct PendingMutationGuard {
    inner: Arc<EditedFileMonitorInner>,
    path: PathBuf,
    mutation_id: u64,
    armed: bool,
}

impl PendingMutationGuard {
    fn new(inner: Arc<EditedFileMonitorInner>, path: PathBuf, mutation_id: u64) -> Self {
        Self {
            inner,
            path,
            mutation_id,
            armed: true,
        }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for PendingMutationGuard {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }

        let mut state = self.inner.state.lock();
        let Some(entry) = state.tracked_files.get_mut(&self.path) else {
            return;
        };

        if remove_pending_mutation(entry, self.mutation_id) {
            entry.notify.notify_waiters();
        }
    }
}

impl MutationLease {
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for MutationLease {
    fn drop(&mut self) {
        if self.released {
            return;
        }
        let mut state = self.inner.state.lock();
        if let Some(entry) = state.tracked_files.get_mut(&self.path) {
            let mut released_active = false;
            if entry.active_mutation == Some(self.mutation_id) {
                entry.active_mutation = None;
                released_active = true;
            }
            let removed_pending = remove_pending_mutation(entry, self.mutation_id);
            if released_active || removed_pending {
                entry.notify.notify_waiters();
            }
        }
        self.released = true;
    }
}

impl EditedFileMonitor {
    pub fn new() -> Self {
        let (event_tx, event_rx) = mpsc::channel::<PathBuf>();
        let watcher = RecommendedWatcher::new(
            move |event: notify::Result<notify::Event>| {
                let Ok(event) = event else {
                    return;
                };

                if !matches!(
                    event.kind,
                    EventKind::Modify(_) | EventKind::Create(_) | EventKind::Remove(_)
                ) {
                    return;
                }

                for path in event.paths {
                    let _ = event_tx.send(path);
                }
            },
            notify::Config::default(),
        )
        .ok();

        let inner = Arc::new(EditedFileMonitorInner {
            state: Mutex::new(MonitorState::default()),
            watcher: StdMutex::new(watcher),
            audit_log: Mutex::new(None),
            next_mutation_id: AtomicU64::new(1),
            debounce_duration: Duration::from_millis(250),
        });

        let monitor = Self {
            inner: Arc::clone(&inner),
        };
        spawn_event_loop(inner, event_rx);
        monitor
    }

    pub fn apply_permissions_config(&self, permissions: &PermissionsConfig) {
        let mut audit_log = self.inner.audit_log.lock();
        if !permissions.audit_enabled {
            *audit_log = None;
            return;
        }

        let audit_dir = expand_tilde_path(&permissions.audit_directory);
        match FileConflictAuditLog::new(audit_dir) {
            Ok(log) => *audit_log = Some(log),
            Err(err) => {
                tracing::warn!(error = %err, "Failed to initialize file conflict audit log");
                *audit_log = None;
            }
        }
    }

    pub async fn track_read(&self, path: &Path) -> Result<()> {
        let path = path.to_path_buf();
        let snapshot = snapshot_path_async(path.clone()).await?;
        self.record_read_snapshot(&path, snapshot)
    }

    pub async fn accept_disk_version(&self, path: &Path) -> Result<()> {
        let path = path.to_path_buf();
        let snapshot = snapshot_path_async(path.clone()).await?;
        self.record_read_snapshot(&path, snapshot)
    }

    pub fn record_read_snapshot(&self, path: &Path, snapshot: FileSnapshot) -> Result<()> {
        let path = path.to_path_buf();
        {
            let mut state = self.inner.state.lock();
            let entry = state
                .tracked_files
                .entry(path.clone())
                .or_insert_with(TrackedFileState::new);
            entry.last_read_snapshot = Some(snapshot.clone());
            entry.last_known_disk_snapshot = Some(snapshot);
            entry.stale_conflict = None;
        }
        self.watch_parent(&path);
        Ok(())
    }

    pub fn record_read_text(&self, path: &Path, content: &str) -> Result<()> {
        self.record_read_snapshot(path, FileSnapshot::from_text_content(content))
    }

    pub fn record_agent_write_snapshot(&self, path: &Path, snapshot: FileSnapshot) -> Result<()> {
        let path = path.to_path_buf();
        {
            let mut state = self.inner.state.lock();
            let entry = state
                .tracked_files
                .entry(path.clone())
                .or_insert_with(TrackedFileState::new);
            entry.last_read_snapshot = Some(snapshot.clone());
            entry.last_known_disk_snapshot = Some(snapshot.clone());
            entry.last_agent_write_snapshot = Some(snapshot);
            entry.stale_conflict = None;
        }
        self.watch_parent(&path);
        Ok(())
    }

    pub fn record_agent_write_text(&self, path: &Path, content: &str) -> Result<()> {
        self.record_agent_write_snapshot(path, FileSnapshot::from_text_content(content))
    }

    pub fn record_agent_removal(&self, path: &Path) -> Result<()> {
        self.record_agent_write_snapshot(path, FileSnapshot::missing())
    }

    pub async fn tracked_read_text(&self, path: &Path) -> Option<String> {
        let state = self.inner.state.lock();
        state
            .tracked_files
            .get(path)
            .and_then(|entry| entry.last_read_snapshot.as_ref())
            .and_then(|snapshot| snapshot.text_content.clone())
    }

    pub async fn acquire_mutation(&self, path: &Path) -> MutationLease {
        let path = path.to_path_buf();
        let mutation_id = self.inner.next_mutation_id.fetch_add(1, Ordering::SeqCst);
        let mut pending_guard =
            PendingMutationGuard::new(Arc::clone(&self.inner), path.clone(), mutation_id);

        loop {
            let notify = {
                let mut state = self.inner.state.lock();
                let entry = state
                    .tracked_files
                    .entry(path.clone())
                    .or_insert_with(TrackedFileState::new);

                if entry.active_mutation.is_none() {
                    if let Some(front) = entry.pending_mutations.front() {
                        if *front == mutation_id {
                            let _ = entry.pending_mutations.pop_front();
                            entry.active_mutation = Some(mutation_id);
                            pending_guard.disarm();
                            return MutationLease {
                                inner: Arc::clone(&self.inner),
                                path,
                                mutation_id,
                                released: false,
                            };
                        }
                    } else {
                        entry.active_mutation = Some(mutation_id);
                        pending_guard.disarm();
                        return MutationLease {
                            inner: Arc::clone(&self.inner),
                            path,
                            mutation_id,
                            released: false,
                        };
                    }
                }

                if !entry
                    .pending_mutations
                    .iter()
                    .any(|pending_id| *pending_id == mutation_id)
                {
                    entry.pending_mutations.push_back(mutation_id);
                }

                entry.notify.clone()
            };

            notify.notified().await;
        }
    }

    pub async fn detect_conflict(
        &self,
        path: &Path,
        intended_content: Option<String>,
        approved_snapshot: Option<FileSnapshot>,
    ) -> Result<Option<FileConflict>> {
        let path = path.to_path_buf();
        let current_snapshot = snapshot_path_async(path.clone()).await?;
        let mut should_audit = false;

        let maybe_conflict = {
            let mut state = self.inner.state.lock();
            let entry = state
                .tracked_files
                .entry(path.clone())
                .or_insert_with(TrackedFileState::new);
            entry.last_known_disk_snapshot = Some(current_snapshot.clone());

            if entry
                .last_agent_write_snapshot
                .as_ref()
                .is_some_and(|snapshot| snapshot.same_contents(&current_snapshot))
            {
                entry.last_agent_write_snapshot = None;
                entry.last_read_snapshot = Some(current_snapshot);
                entry.stale_conflict = None;
                return Ok(None);
            }

            let Some(read_snapshot) = entry.last_read_snapshot.clone() else {
                return Ok(None);
            };

            if read_snapshot.same_contents(&current_snapshot) {
                entry.stale_conflict = None;
                return Ok(None);
            }

            if approved_snapshot
                .as_ref()
                .is_some_and(|snapshot| snapshot.same_identity(&current_snapshot))
            {
                entry.stale_conflict = None;
                return Ok(None);
            }

            let emit_hitl_notification = match entry.stale_conflict.as_mut() {
                Some(existing) => {
                    let emit = !existing.notification_emitted;
                    existing.notification_emitted = true;
                    emit
                }
                None => {
                    entry.stale_conflict = Some(StaleConflictState {
                        notification_emitted: true,
                    });
                    should_audit = true;
                    true
                }
            };

            Some(FileConflict {
                path: path.clone(),
                read_snapshot: Some(read_snapshot),
                disk_snapshot: Some(current_snapshot.clone()),
                intended_content,
                emit_hitl_notification,
            })
        };

        if should_audit {
            self.record_conflict_audit(&path, &current_snapshot, "pre_write_conflict");
        }

        Ok(maybe_conflict)
    }

    #[cfg(test)]
    pub async fn debug_process_path_change(&self, path: &Path) -> Result<()> {
        self.process_path_change(path.to_path_buf())
    }

    fn watch_parent(&self, path: &Path) {
        let Some(parent) = path.parent().map(Path::to_path_buf) else {
            return;
        };

        let should_watch = {
            let mut state = self.inner.state.lock();
            state.watched_parents.insert(parent.clone())
        };

        if !should_watch {
            return;
        }

        let Ok(mut watcher) = self.inner.watcher.lock() else {
            return;
        };
        let Some(watcher) = watcher.as_mut() else {
            return;
        };

        if let Err(err) = watcher.watch(&parent, RecursiveMode::NonRecursive) {
            tracing::warn!(path = %parent.display(), error = %err, "Failed to watch edited-file parent directory");
        }
    }

    fn record_conflict_audit(&self, path: &Path, snapshot: &FileSnapshot, reason: &str) {
        let event = FileConflictAuditEvent {
            timestamp: chrono::Local::now(),
            path: path.to_path_buf(),
            reason: reason.to_string(),
            file_exists: snapshot.exists,
            size_bytes: snapshot.exists.then_some(snapshot.size_bytes),
            sha256: snapshot.exists.then_some(snapshot.sha256.clone()),
        };

        if let Some(log) = self.inner.audit_log.lock().as_mut()
            && let Err(err) = log.record(&event)
        {
            tracing::warn!(error = %err, "Failed to record file conflict audit event");
        }
    }

    fn process_path_change(&self, path: PathBuf) -> Result<()> {
        let tracked_path = normalize_event_path(&path);
        let snapshot = snapshot_path_sync(&tracked_path)
            .with_context(|| format!("Failed to snapshot externally modified file {}", tracked_path.display()))?;

        let mut should_audit = false;

        {
            let mut state = self.inner.state.lock();
            let Some(entry) = state.tracked_files.get_mut(&tracked_path) else {
                return Ok(());
            };

            if entry
                .last_known_disk_snapshot
                .as_ref()
                .is_some_and(|known| known.same_contents(&snapshot))
            {
                return Ok(());
            }

            entry.last_known_disk_snapshot = Some(snapshot.clone());

            if entry
                .last_agent_write_snapshot
                .as_ref()
                .is_some_and(|known| known.same_contents(&snapshot))
            {
                entry.last_read_snapshot = Some(snapshot);
                entry.last_agent_write_snapshot = None;
                entry.stale_conflict = None;
                return Ok(());
            }

            if entry
                .last_read_snapshot
                .as_ref()
                .is_some_and(|known| known.same_contents(&snapshot))
            {
                entry.stale_conflict = None;
                return Ok(());
            }

            if entry
                .last_read_snapshot
                .as_ref()
                .is_some_and(|read_snapshot| !read_snapshot.same_contents(&snapshot))
            {
                should_audit = true;
                match entry.stale_conflict.as_mut() {
                    Some(_) => {}
                    None => {
                        entry.stale_conflict = Some(StaleConflictState {
                            notification_emitted: false,
                        });
                    }
                }
            }
        }

        if should_audit {
            self.record_conflict_audit(&tracked_path, &snapshot, "watcher_detected_external_change");
        }

        Ok(())
    }
}

impl Default for EditedFileMonitor {
    fn default() -> Self {
        Self::new()
    }
}

fn spawn_event_loop(inner: Arc<EditedFileMonitorInner>, event_rx: Receiver<PathBuf>) {
    thread::spawn(move || {
        let mut pending = HashMap::<PathBuf, Instant>::new();

        loop {
            match event_rx.recv_timeout(Duration::from_millis(50)) {
                Ok(path) => {
                    pending.insert(normalize_event_path(&path), Instant::now());
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }

            let now = Instant::now();
            let ready = pending
                .iter()
                .filter_map(|(path, observed_at)| {
                    (now.duration_since(*observed_at) >= inner.debounce_duration)
                        .then_some(path.clone())
                })
                .collect::<Vec<_>>();

            for path in ready {
                pending.remove(&path);
                let monitor = EditedFileMonitor {
                    inner: Arc::clone(&inner),
                };
                if let Err(err) = monitor.process_path_change(path.clone()) {
                    tracing::debug!(path = %path.display(), error = %err, "Edited-file watcher refresh failed");
                }
            }
        }
    });
}

fn normalize_event_path(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| {
        path.parent()
            .and_then(|parent| std::fs::canonicalize(parent).ok())
            .and_then(|parent| path.file_name().map(|name| parent.join(name)))
            .unwrap_or_else(|| path.to_path_buf())
    })
}

async fn snapshot_path_async(path: PathBuf) -> Result<FileSnapshot> {
    tokio::task::spawn_blocking(move || snapshot_path_sync(&path))
        .await
        .context("Failed to join file snapshot task")?
}

fn snapshot_path_sync(path: &Path) -> Result<FileSnapshot> {
    let metadata = match std::fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Ok(FileSnapshot {
                exists: false,
                size_bytes: 0,
                modified_millis: None,
                sha256: String::new(),
                text_content: None,
            });
        }
        Err(err) => return Err(err).with_context(|| format!("Failed to read metadata for {}", path.display())),
    };

    if !metadata.is_file() {
        return Ok(FileSnapshot {
            exists: false,
            size_bytes: 0,
            modified_millis: metadata
                .modified()
                .ok()
                .and_then(system_time_to_millis),
            sha256: String::new(),
            text_content: None,
        });
    }

    let bytes = std::fs::read(path)
        .with_context(|| format!("Failed to read file bytes for {}", path.display()))?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    let digest = hasher.finalize();
    let sha256 = hex_digest(&digest);

    Ok(FileSnapshot {
        exists: true,
        size_bytes: metadata.len(),
        modified_millis: metadata
            .modified()
            .ok()
            .and_then(system_time_to_millis),
        sha256,
        text_content: String::from_utf8(bytes).ok(),
    })
}

fn system_time_to_millis(time: SystemTime) -> Option<u128> {
    time.duration_since(UNIX_EPOCH).ok().map(|duration| duration.as_millis())
}

fn hex_digest(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(&mut output, "{byte:02x}");
    }
    output
}

fn workspace_relative_display(workspace_root: &Path, path: &Path) -> String {
    if let Ok(relative) = path.strip_prefix(workspace_root) {
        return relative.to_string_lossy().to_string();
    }
    if let Ok(canonical_root) = std::fs::canonicalize(workspace_root)
        && let Ok(relative) = path.strip_prefix(canonical_root)
    {
        return relative.to_string_lossy().to_string();
    }
    path.to_string_lossy().to_string()
}

fn expand_tilde_path(path: &str) -> PathBuf {
    if let Some(stripped) = path.strip_prefix("~/")
        && let Some(home) = dirs::home_dir()
    {
        return home.join(stripped);
    }
    PathBuf::from(path)
}

pub fn conflict_override_snapshot(args: &Value) -> Option<FileSnapshot> {
    args.get(FILE_CONFLICT_OVERRIDE_ARG)
        .and_then(FileSnapshot::from_identity_value)
}

fn remove_pending_mutation(entry: &mut TrackedFileState, mutation_id: u64) -> bool {
    let Some(index) = entry
        .pending_mutations
        .iter()
        .position(|pending_id| *pending_id == mutation_id)
    else {
        return false;
    };

    let _ = entry.pending_mutations.remove(index);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn detects_same_mtime_same_size_hash_mismatch() -> Result<()> {
        let temp = TempDir::new()?;
        let file = temp.path().join("sample.txt");
        std::fs::write(&file, "before\n")?;

        let first = snapshot_path_async(file.clone()).await?;
        std::fs::write(&file, "after!\n")?;
        let second = snapshot_path_async(file.clone()).await?;

        assert_eq!(first.size_bytes, second.size_bytes);
        assert_ne!(first.sha256, second.sha256);
        Ok(())
    }

    #[tokio::test]
    async fn suppresses_self_write_event() -> Result<()> {
        let temp = TempDir::new()?;
        let file = temp.path().join("sample.txt");
        std::fs::write(&file, "before\n")?;
        let monitor = EditedFileMonitor::new();

        monitor.track_read(&file).await?;
        std::fs::write(&file, "after\n")?;
        monitor.record_agent_write_text(&file, "after\n")?;
        monitor.debug_process_path_change(&file).await?;

        assert!(monitor
            .detect_conflict(&file, Some("after\n".to_string()), None)
            .await?
            .is_none());
        Ok(())
    }

    #[tokio::test]
    async fn expected_agent_snapshot_does_not_hide_external_follow_up_change() -> Result<()> {
        let temp = TempDir::new()?;
        let file = temp.path().join("sample.txt");
        std::fs::write(&file, "before\n")?;
        let monitor = EditedFileMonitor::new();

        monitor.track_read(&file).await?;
        std::fs::write(&file, "after\n")?;
        monitor.record_agent_write_text(&file, "after\n")?;

        std::fs::write(&file, "after formatted\n")?;
        monitor.debug_process_path_change(&file).await?;

        let conflict = monitor
            .detect_conflict(&file, Some("agent\n".to_string()), None)
            .await?;
        assert!(conflict.is_some());
        Ok(())
    }

    #[tokio::test]
    async fn queues_mutations_in_order() -> Result<()> {
        let temp = TempDir::new()?;
        let file = temp.path().join("sample.txt");
        std::fs::write(&file, "hello\n")?;
        let monitor = Arc::new(EditedFileMonitor::new());

        let first = monitor.acquire_mutation(&file).await;
        let monitor_clone = Arc::clone(&monitor);
        let file_clone = file.clone();
        let waiter = tokio::spawn(async move {
            let lease = monitor_clone.acquire_mutation(&file_clone).await;
            lease.path().to_path_buf()
        });

        tokio::time::sleep(Duration::from_millis(50)).await;
        drop(first);
        let acquired = waiter.await?;
        assert_eq!(acquired, file);
        Ok(())
    }

    #[tokio::test]
    async fn detects_external_change_after_read() -> Result<()> {
        let temp = TempDir::new()?;
        let file = temp.path().join("sample.txt");
        std::fs::write(&file, "before\n")?;
        let monitor = EditedFileMonitor::new();

        monitor.track_read(&file).await?;
        std::fs::write(&file, "external\n")?;
        monitor.debug_process_path_change(&file).await?;

        let conflict = monitor
            .detect_conflict(&file, Some("agent\n".to_string()), None)
            .await?;
        assert!(conflict.is_some());
        Ok(())
    }

    #[tokio::test]
    async fn cancelled_waiter_does_not_block_following_mutation() -> Result<()> {
        let temp = TempDir::new()?;
        let file = temp.path().join("sample.txt");
        std::fs::write(&file, "hello\n")?;
        let monitor = Arc::new(EditedFileMonitor::new());

        let first = monitor.acquire_mutation(&file).await;

        let pending_monitor = Arc::clone(&monitor);
        let pending_file = file.clone();
        let pending = tokio::spawn(async move {
            let _lease = pending_monitor.acquire_mutation(&pending_file).await;
        });
        tokio::time::sleep(Duration::from_millis(50)).await;
        pending.abort();

        let next_monitor = Arc::clone(&monitor);
        let next_file = file.clone();
        let next = tokio::spawn(async move {
            let lease = next_monitor.acquire_mutation(&next_file).await;
            lease.path().to_path_buf()
        });

        drop(first);

        let acquired = tokio::time::timeout(Duration::from_secs(1), next).await??;
        assert_eq!(acquired, file);
        Ok(())
    }

    #[tokio::test]
    async fn clears_conflict_state_when_disk_returns_to_read_snapshot() -> Result<()> {
        let temp = TempDir::new()?;
        let file = temp.path().join("sample.txt");
        std::fs::write(&file, "before\n")?;
        let monitor = EditedFileMonitor::new();

        monitor.track_read(&file).await?;
        std::fs::write(&file, "external one\n")?;
        monitor.debug_process_path_change(&file).await?;

        let first_conflict = monitor
            .detect_conflict(&file, Some("agent\n".to_string()), None)
            .await?
            .expect("expected initial conflict");
        assert!(first_conflict.emit_hitl_notification);

        std::fs::write(&file, "before\n")?;
        monitor.debug_process_path_change(&file).await?;
        assert!(monitor
            .detect_conflict(&file, Some("agent\n".to_string()), None)
            .await?
            .is_none());

        std::fs::write(&file, "external two\n")?;
        monitor.debug_process_path_change(&file).await?;
        let second_conflict = monitor
            .detect_conflict(&file, Some("agent\n".to_string()), None)
            .await?
            .expect("expected renewed conflict");
        assert!(second_conflict.emit_hitl_notification);
        Ok(())
    }

    #[tokio::test]
    async fn override_requires_matching_approved_snapshot() -> Result<()> {
        let temp = TempDir::new()?;
        let file = temp.path().join("sample.txt");
        std::fs::write(&file, "before\n")?;
        let monitor = EditedFileMonitor::new();

        monitor.track_read(&file).await?;
        std::fs::write(&file, "external one\n")?;
        let approved_snapshot = snapshot_path_async(file.clone()).await?;

        std::fs::write(&file, "external two\n")?;

        let conflict = monitor
            .detect_conflict(
                &file,
                Some("agent\n".to_string()),
                Some(approved_snapshot),
            )
            .await?;
        assert!(conflict.is_some());
        Ok(())
    }

    #[test]
    fn normalizes_missing_event_paths_via_canonical_parent() -> Result<()> {
        let temp = TempDir::new()?;
        let missing = temp.path().join("missing.txt");
        let canonical_parent = std::fs::canonicalize(temp.path())?;

        assert_eq!(normalize_event_path(&missing), canonical_parent.join("missing.txt"));
        Ok(())
    }
}
