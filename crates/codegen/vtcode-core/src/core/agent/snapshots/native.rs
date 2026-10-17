use super::*;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

#[derive(Clone)]
struct Active {
    workspace: PathBuf,
    storage: PathBuf,
    record: PathBuf,
    engine: String,
    watch: String,
}
static ACTIVE: OnceLock<Mutex<HashMap<String, Arc<Mutex<Active>>>>> = OnceLock::new();
fn active_map() -> &'static Mutex<HashMap<String, Arc<Mutex<Active>>>> {
    ACTIVE.get_or_init(Mutex::default)
}

/// Holds exclusive workspace access until the complete agent turn has settled.
pub struct PromptCheckpointLease {
    key: String,
    _lock: fs::File,
}
impl Drop for PromptCheckpointLease {
    fn drop(&mut self) {
        if let Ok(mut active) = active_map().lock() {
            active.remove(&self.key);
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
struct Recovery {
    #[serde(default)]
    policy: String,
    snapshot: String,
    active: Vec<usize>,
}
#[derive(Clone, Default, Serialize, Deserialize)]
struct Navigation {
    active: Vec<usize>,
    redo: Vec<Recovery>,
    pending: Option<Recovery>,
}

fn atomic_json(path: &Path, value: &impl Serialize) -> Result<()> {
    use std::io::Write;
    let temp = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    let mut options = fs::OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temp)?;
    file.write_all(&serde_json::to_vec(value)?)?;
    file.sync_all()?;
    fs::rename(temp, path)?;
    Ok(())
}

/// Whether a locked fd still refers to the file currently at `lock_path`.
///
/// The completion path unlinks `rewind.lock` while holding its lock. A process
/// that opened the old inode just before that unlink can `try_lock` the ghost
/// after the holder drops — while another process creates a fresh file — and
/// both would believe they hold the lock. Comparing inos closes that window.
#[cfg(unix)]
fn locked_file_matches_path(file: &fs::File, lock_path: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    let Ok(locked) = file.metadata() else {
        return false;
    };
    let Ok(current) = fs::metadata(lock_path) else {
        return false;
    };
    locked.ino() == current.ino()
}

#[cfg(not(unix))]
fn locked_file_matches_path(_: &fs::File, _: &Path) -> bool {
    true
}

/// Acquire the workspace rewind lock, verifying the locked inode is still the
/// file at `lock_path`. A ghost acquisition (locked inode replaced under us)
/// is dropped and retried against the current file, so mutual exclusion holds
/// across the completion path's unlink-while-held cleanup.
fn acquire_verified_rewind_lock(lock_path: &Path) -> std::io::Result<fs::File> {
    for _ in 0..3 {
        let file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(lock_path)?;
        file.try_lock()?;
        if locked_file_matches_path(&file, lock_path) {
            return Ok(file);
        }
        drop(file);
    }
    Err(std::io::Error::other("rewind.lock kept changing identity; retry the turn"))
}

fn file_records(manifest: &filesnap::Manifest, workspace: &Path, engine: &str) -> Result<Vec<FileSnapshot>> {
    manifest
        .entries
        .keys()
        .chain(manifest.absent.iter())
        .map(|path| {
            let relative = Path::new(path)
                .strip_prefix(workspace)
                .context("Checkpoint escaped the workspace")?;
            Ok(FileSnapshot {
                path: relative.to_string_lossy().replace('\\', "/"),
                deleted: manifest.absent.contains(path),
                encoding: Some(FileEncoding::Filesnap),
                data: Some(engine.to_owned()),
            })
        })
        .collect()
}

// Only literal POSIX-shell redirects whose cwd stays unchanged can provide
// reliable preimages. Never execute or expand shell text to discover a path.
fn canonicalize_for_strip(path: &Path) -> PathBuf {
    if let Ok(canonical) = canonicalize(path) {
        return canonical;
    }
    // The target may not exist yet (creation). Walk up to the nearest
    // existing ancestor so symlinked workspaces (`/var` vs `/private/var`)
    // still resolve inside the workspace instead of being skipped.
    let mut ancestor = path.parent();
    while let Some(dir) = ancestor {
        if dir.as_os_str().is_empty() {
            break;
        }
        if let Ok(canonical_parent) = canonicalize(dir)
            && let Ok(stripped) = path.strip_prefix(dir)
        {
            let mut joined = canonical_parent;
            joined.push(stripped);
            return joined;
        }
        ancestor = dir.parent();
    }
    path.to_path_buf()
}

fn recovery_path(storage: &Path, snapshot: &str) -> PathBuf {
    storage.join(format!("turn_recovery_{snapshot}.json"))
}

fn shell_redirect_paths(args: &serde_json::Value) -> BTreeSet<PathBuf> {
    use crate::command_safety::shell_parser::contains_dynamic_shell_syntax;

    let collect = || -> Option<BTreeSet<PathBuf>> {
        if cfg!(windows) {
            return None;
        }
        if let Some(shell) = args.get("shell").and_then(serde_json::Value::as_str) {
            let name = Path::new(shell).file_name()?.to_str()?;
            if !matches!(name, "bash" | "sh" | "dash" | "zsh" | "ksh") {
                return None;
            }
        }
        let script = args
            .get("raw_command")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
            .or_else(|| crate::tools::command_args::raw_command_text(args))?;
        if !script.contains('>') {
            return None;
        }
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&tree_sitter_bash::LANGUAGE.into()).ok()?;
        let tree = parser.parse(&script, None)?;
        if tree.root_node().has_error() {
            return None;
        }
        let mut paths = BTreeSet::new();
        let mut pending = vec![tree.root_node()];
        while let Some(node) = pending.pop() {
            match node.kind() {
                "program" | "list" | "pipeline" | "redirected_statement" => {
                    let mut cursor = node.walk();
                    pending.extend(node.named_children(&mut cursor));
                }
                "command" => {
                    let name = node.child_by_field_name("name")?.utf8_text(script.as_bytes()).ok()?;
                    if contains_dynamic_shell_syntax(name) {
                        return None;
                    }
                    let words = shell_words::split(name).ok()?;
                    if words.len() != 1
                        || matches!(
                            words.first()?.as_str(),
                            "cd" | "pushd"
                                | "popd"
                                | "source"
                                | "."
                                | "eval"
                                | "exec"
                                | "command"
                                | "builtin"
                                | "alias"
                                | "unalias"
                                | "trap"
                                | "enable"
                                | "shopt"
                        )
                    {
                        return None;
                    }
                    let mut cursor = node.walk();
                    pending.extend(
                        node.named_children(&mut cursor)
                            .filter(|child| matches!(child.kind(), "file_redirect" | "heredoc_redirect")),
                    );
                }
                "file_redirect" => {
                    let text = node.utf8_text(script.as_bytes()).ok()?;
                    let operator = text.trim_start_matches(|c: char| c.is_ascii_digit());
                    if !operator.starts_with('>') && !operator.starts_with("&>") {
                        continue;
                    }
                    let mut cursor = node.walk();
                    for destination in node.children_by_field_name("destination", &mut cursor) {
                        let raw = destination.utf8_text(script.as_bytes()).ok()?;
                        if contains_dynamic_shell_syntax(raw) {
                            return None;
                        }
                        let words = shell_words::split(raw).ok()?;
                        if words.len() != 1 {
                            return None;
                        }
                        let path = words.first()?;
                        if operator.starts_with(">&") && (path == "-" || path.chars().all(|c| c.is_ascii_digit())) {
                            continue;
                        }
                        if path.is_empty() || path.starts_with('~') {
                            return None;
                        }
                        paths.insert(PathBuf::from(path));
                    }
                }
                // Here-doc contents and comments are data, not shell commands.
                "heredoc_redirect" | "comment" => {}
                // Functions, loops, subshells and dynamic cwd changes need a
                // richer execution model; do not guess their target paths.
                _ => return None,
            }
        }
        Some(paths)
    };
    collect().unwrap_or_default()
}

/// Capture final local write arguments after host hook rewriting and before tool execution.
pub async fn declare_prompt_edit(session: String, name: String, args: serde_json::Value) -> Result<()> {
    let active = active_map()
        .lock()
        .map_err(|error| anyhow::anyhow!("Checkpoint lock poisoned: {error}"))?
        .get(&session)
        .cloned();
    let Some(active) = active else {
        return Ok(());
    };
    let is_shell = crate::tools::tool_intent::is_command_run_tool_call(&name, &args);
    let operation = args.get("action").and_then(serde_json::Value::as_str).unwrap_or(&name);
    if !is_shell
        && !["write", "edit", "patch", "delete", "remove", "create", "move", "rename"]
            .iter()
            .any(|verb| operation.contains(verb))
    {
        return Ok(());
    }
    tokio::task::spawn_blocking(move || -> Result<()> {
        let active = active
            .lock()
            .map_err(|error| anyhow::anyhow!("Checkpoint lock poisoned: {error}"))?;
        let mut paths = BTreeSet::new();
        if is_shell {
            let redirects = shell_redirect_paths(&args);
            if redirects.is_empty() {
                return Ok(());
            }
            let cwd = crate::tools::command_args::working_dir_text(&args)
                .map_or_else(|| active.workspace.clone(), |path| active.workspace.join(path));
            // A missing workdir must not abort capture; fall back so the tool
            // still runs and other paths are still tracked.
            let cwd = canonicalize(&cwd).unwrap_or(cwd);
            paths.extend(redirects.into_iter().map(|path| cwd.join(path)));
        } else {
            for key in [
                "path",
                "file_path",
                "destination",
                "destination_path",
                "new_path",
                "source",
            ] {
                if let Some(path) = args.get(key).and_then(serde_json::Value::as_str) {
                    paths.insert(PathBuf::from(path));
                }
            }
            for key in ["patch", "input", "patch_text"] {
                if let Some(patch) = args.get(key).and_then(serde_json::Value::as_str) {
                    for line in patch.lines() {
                        for prefix in [
                            "*** Add File: ",
                            "*** Update File: ",
                            "*** Delete File: ",
                            "*** Move to: ",
                        ] {
                            if let Some(path) = line.strip_prefix(prefix) {
                                paths.insert(PathBuf::from(path));
                            }
                        }
                    }
                }
            }
        }
        if paths.is_empty() {
            return Ok(());
        }
        let store = filesnap::WorkspaceStore::open(&active.storage, &active.workspace)?;
        let target = store.target_for_turn(&active.engine)?.context("Missing active checkpoint")?;
        let before = store.manifest(target.manifest_id())?;
        let ignore = filesnap::load_ignore(&active.workspace);
        for path in paths {
            if path.components().any(|part| part == Component::ParentDir) {
                continue;
            }
            // Canonicalize absolute targets the same way `normalize_path` does
            // so macOS `/var` vs `/private/var` and symlinked workspaces do not
            // fail checkpointing. Fall back to the raw path for not-yet-created
            // files. Anything still outside the workspace is skipped, never fatal.
            let relative = if path.is_absolute() {
                let canonical = canonicalize_for_strip(&path);
                if let Ok(relative) = canonical.strip_prefix(&active.workspace) {
                    relative.to_path_buf()
                } else if let Ok(relative) = path.strip_prefix(&active.workspace) {
                    relative.to_path_buf()
                } else {
                    continue;
                }
            } else {
                path
            };
            let path = match SnapshotManager::checked_file_path(&active.workspace, &active.storage, &relative) {
                Ok(path) => path,
                Err(error) => {
                    tracing::debug!(%error, "Skipping checkpoint pre-image outside restore authority");
                    continue;
                }
            };
            if filesnap::is_ignored(&ignore, &path) {
                continue;
            }
            if let Err(error) =
                store.declare_paths(&active.watch, &active.engine, std::slice::from_ref(&path))
            {
                tracing::debug!(%error, "Skipping checkpoint watch declaration");
                continue;
            }
            let key = path.to_string_lossy();
            // The first preimage owns the turn boundary. A second write must
            // never replace a recorded absence with the newly created bytes.
            if before.entries.contains_key(key.as_ref()) || before.absent.contains(key.as_ref()) {
                continue;
            }
            let image = match fs::read(&path) {
                Ok(bytes) => filesnap::PreEditImage::Existed(bytes),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => filesnap::PreEditImage::DidNotExist,
                Err(error) => {
                    // Directories and other non-file targets have no pre-image;
                    // skip them rather than failing the turn.
                    tracing::debug!(%error, path = %path.display(), "Skipping checkpoint pre-image for non-file target");
                    continue;
                }
            };
            if let Err(error) = filesnap::declare_edits(
                &store,
                &active.engine,
                &active.engine,
                &filesnap::TurnScope::at(&active.workspace),
                vec![(path, image)],
            ) {
                tracing::debug!(%error, "Skipping checkpoint pre-image declaration");
                continue;
            }
        }
        let target = store.target_for_turn(&active.engine)?.context("Missing active checkpoint")?;
        let manifest = store.manifest(target.manifest_id())?;
        let mut stored: StoredSnapshot = serde_json::from_slice(&fs::read(&active.record)?)?;
        stored.files = file_records(&manifest, &active.workspace, &active.engine)?;
        stored.metadata.file_count = stored.files.len();
        atomic_json(&active.record, &stored)
    })
    .await?
}

impl SnapshotManager {
    async fn retire_recovery_record(&self, snapshot: &str) {
        if uuid::Uuid::parse_str(snapshot).is_err() {
            return;
        }
        let record = recovery_path(&self.storage_dir, snapshot);
        match self.retire_snapshot(&record).await {
            Ok(()) => {}
            Err(error) => {
                let is_missing = error
                    .downcast_ref::<std::io::Error>()
                    .is_some_and(|io| io.kind() == std::io::ErrorKind::NotFound);
                if !is_missing {
                    tracing::warn!(%error, "Failed to retire consumed recovery record");
                }
            }
        }
    }

    fn navigation_path(&self, session: &str) -> Result<PathBuf> {
        anyhow::ensure!(session.len() <= 80 && !session.is_empty(), "Invalid session ID");
        let key: String = session.as_bytes().iter().map(|b| format!("{b:02x}")).collect();
        Ok(self.storage_dir.join(format!("branch_{key}.json")))
    }
    fn navigation(&self, session: &str) -> Result<Navigation> {
        match fs::read(self.navigation_path(session)?) {
            Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Navigation::default()),
            Err(error) => Err(error.into()),
        }
    }

    /// Bound a finished session's navigation record so it cannot pin every turn
    /// snapshot for the full retention window. Keeps the newest
    /// [`REWIND_ACTIVE_KEEP`] active turns for a possible resume, drops the
    /// rest (those `turn_*.json` files become prune-eligible), clears the redo
    /// stack, and releases the workspace rewind lock.
    pub async fn complete_session_navigation(&self, session: &str) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }
        let nav_path = self.navigation_path(session)?;
        let mut state = self.navigation(session)?;
        let had_record = nav_path.exists();
        if state.active.len() > REWIND_ACTIVE_KEEP {
            let drop_count = state.active.len() - REWIND_ACTIVE_KEEP;
            state.active.drain(..drop_count);
        }
        for entry in std::mem::take(&mut state.redo) {
            self.retire_recovery_record(&entry.snapshot).await;
        }
        // Avoid inventing an empty branch file for checkpoint-less sessions.
        if had_record || !state.active.is_empty() || state.pending.is_some() {
            atomic_json(&nav_path, &state)?;
        }
        let lock_path = self.storage_dir.join("rewind.lock");
        // Only remove the lock while we hold it. Deleting an flocked file that
        // another process holds would let later `try_lock` calls create a fresh
        // inode and break mutual exclusion.
        match fs::OpenOptions::new().read(true).write(true).open(&lock_path) {
            Ok(file) => {
                // Only unlink while holding a lock on the file that is
                // currently at the path; a ghost lock (path replaced under
                // us) must not delete the replacement's lock file.
                if file.try_lock().is_ok() && locked_file_matches_path(&file, &lock_path) {
                    if let Err(error) = fs::remove_file(&lock_path)
                        && error.kind() != std::io::ErrorKind::NotFound
                    {
                        tracing::debug!(%error, "failed to remove rewind.lock after session completion");
                    }
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                tracing::debug!(%error, "failed to open rewind.lock after session completion");
            }
        }
        // Newly unpinned turns can be reclaimed now rather than waiting for the
        // next prompt's budget prune.
        if let Err(error) = self.prune_snapshot_budget().await {
            tracing::debug!(%error, "checkpoint budget prune failed after session completion");
        }
        Ok(())
    }

    fn build_ignore(&self, policy: &str) -> Result<filesnap::Gitignore> {
        anyhow::ensure!(policy.len() <= 1024 * 1024, "Ignore policy is too large");
        let mut builder = filesnap::GitignoreBuilder::new(&self.canonical_workspace);
        for line in policy.lines() {
            builder.add_line(None, line)?;
        }
        Ok(builder.build()?)
    }
    /// Save the exact conversation prefix and workspace before sending a prompt.
    pub async fn begin_prompt(
        &self,
        turn: usize,
        session: &str,
        prompt: &str,
        conversation: &[SessionMessage],
    ) -> Result<PromptCheckpointLease> {
        self.begin_prompt_with_cancellation(
            turn,
            session,
            prompt,
            conversation,
            tokio_util::sync::CancellationToken::new(),
        )
        .await?
        .context("checkpoint preparation cancelled")
    }

    /// Prepare a checkpoint while allowing the caller to stop waiting. The
    /// blocking worker retains the rewind lock through atomic publication and
    /// drops its lease on cancellation; no tool may run without that lease.
    pub async fn begin_prompt_with_cancellation(
        &self,
        turn: usize,
        session: &str,
        prompt: &str,
        conversation: &[SessionMessage],
        cancellation: tokio_util::sync::CancellationToken,
    ) -> Result<Option<PromptCheckpointLease>> {
        let manager = self.clone();
        let session = session.to_owned();
        let prompt = prompt.to_owned();
        let conversation = conversation.to_vec();
        // Lock acquisition, scanning, serialization and durable publication
        // all belong on the blocking worker. Its lease retains the same lock
        // through pruning and the complete agent turn, including on cancellation.
        let worker_cancel = cancellation.clone();
        let worker = tokio::task::spawn_blocking(move || -> Result<Option<PromptCheckpointLease>> {
            if worker_cancel.is_cancelled() {
                return Ok(None);
            }
            let lease = manager.begin_prompt_blocking(turn, &session, &prompt, &conversation)?;
            if worker_cancel.is_cancelled() {
                Ok(None)
            } else {
                Ok(Some(lease))
            }
        });
        // Cancel the worker's publication ownership even if this future is
        // dropped by a caller racing a stop request.
        let cancel_on_drop = cancellation.clone().drop_guard();
        let lease = tokio::select! {
            biased;
            _ = cancellation.cancelled() => return Ok(None),
            result = worker => result.context("checkpoint preparation worker failed")??,
        };
        if lease.is_some() {
            tokio::select! {
                biased;
                _ = cancellation.cancelled() => return Ok(None),
                result = self.prune_snapshot_budget() => {
                    if let Err(error) = result { tracing::debug!(%error, "checkpoint budget prune failed"); }
                }
            }
        }
        let _ = cancel_on_drop.disarm();
        Ok(lease)
    }

    fn begin_prompt_blocking(
        &self,
        turn: usize,
        session: &str,
        prompt: &str,
        conversation: &[SessionMessage],
    ) -> Result<PromptCheckpointLease> {
        let lock = acquire_verified_rewind_lock(&self.storage_dir.join("rewind.lock"))
            .context("Another turn or rewind is using this workspace")?;
        let mut state = self.navigation(session)?;
        anyhow::ensure!(state.pending.is_none(), "Interrupted rewind; run /rewind-recover before continuing");
        let workspace = self.canonical_workspace.clone();
        let storage = self.storage_dir.clone();
        let engine = format!("vt-{}", uuid::Uuid::new_v4());
        let watch = format!("vt-watch-{session}");
        let active = Active {
            workspace: workspace.clone(),
            storage: storage.clone(),
            record: self.snapshot_path(turn),
            engine: engine.clone(),
            watch: watch.clone(),
        };
        let files = {
            let store = filesnap::WorkspaceStore::open(&storage, &workspace)?;
            store.note_turn(&watch, &engine)?;
            let watched: Vec<_> = store
                .declared_paths(&watch, filesnap::DeclaredWindow::default())?
                .into_iter()
                .collect();
            store.declare_paths(&engine, &engine, &watched)?;
            let checkpoint = filesnap::capture_turn(&store, &engine, &engine, &filesnap::TurnScope::at(&workspace))?;
            anyhow::ensure!(checkpoint.stats.dropped == 0, "Checkpoint skipped paths; cannot safely start turn");
            let files = file_records(&checkpoint.manifest, &workspace, &engine)?;
            for file in &files {
                SnapshotManager::checked_file_path(&workspace, &storage, Path::new(&file.path))?;
            }
            files
        };
        let metadata = SnapshotMetadata {
            id: format!("turn_{turn}"),
            turn_number: turn,
            created_at: Self::current_timestamp()?,
            description: Self::truncate_description(prompt),
            message_count: conversation.len(),
            file_count: files.len(),
            touched_files: vec![],
            prompt_text: Some(prompt.into()),
            prompt_message_index: None,
            session_id: Some(session.into()),
            runtime_turn_id: None,
            session_turn_number: Some(turn),
            turn_diagnostics: None,
        };
        atomic_json(
            &active.record,
            &StoredSnapshot {
                metadata,
                conversation: conversation.to_vec(),
                files,
                schema_version: Some(SNAPSHOT_SCHEMA_VERSION),
            },
        )?;
        state.active.push(turn);
        for entry in std::mem::take(&mut state.redo) {
            if uuid::Uuid::parse_str(&entry.snapshot).is_ok() {
                let record = recovery_path(&self.storage_dir, &entry.snapshot);
                let retired = record.with_extension(format!("retired-{}", uuid::Uuid::new_v4()));
                if let Err(error) = fs::rename(&record, retired)
                    && error.kind() != std::io::ErrorKind::NotFound
                {
                    tracing::warn!(%error, "Failed to retire discarded recovery record");
                }
            }
        }
        atomic_json(&self.navigation_path(session)?, &state)?;
        active_map()
            .lock()
            .map_err(|error| anyhow::anyhow!("Checkpoint lock poisoned: {error}"))?
            .insert(session.into(), Arc::new(Mutex::new(active)));
        Ok(PromptCheckpointLease { key: session.into(), _lock: lock })
    }
    /// Enumerate only checkpoints on this session's current conversation branch.
    pub async fn rewind_points(&self, session: &str) -> Result<Vec<SnapshotMetadata>> {
        let state = self.navigation(session)?;
        let mut points = Vec::new();
        for turn in state.active.iter().rev() {
            if let Some(stored) = self.load_snapshot(*turn).await? {
                points.push(stored.metadata);
            }
        }
        Ok(points)
    }
    /// Restore files and return their matching conversation, with persistent multi-level redo.
    pub async fn navigate_prompt(
        &self,
        turn: Option<usize>,
        scope: RevertScope,
        session: &str,
        conversation: &[SessionMessage],
    ) -> Result<CheckpointRestore> {
        let _lock = acquire_verified_rewind_lock(&self.storage_dir.join("rewind.lock"))
            .context("Wait for the current turn to finish")?;
        let policy = match fs::read_to_string(self.canonical_workspace.join(".filesnapignore")) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(error) => return Err(error.into()),
        };
        let ignore = self.build_ignore(&policy)?;
        let mut state = self.navigation(session)?;
        let load_recovery = |saved: &Recovery| -> Result<StoredSnapshot> {
            uuid::Uuid::parse_str(&saved.snapshot)?;
            Ok(serde_json::from_slice(&fs::read(recovery_path(&self.storage_dir, &saved.snapshot))?)?)
        };
        if let Some(pending) = state.pending.clone() {
            anyhow::ensure!(turn.is_none(), "Interrupted rewind; run /rewind-recover");
            let restored = self
                .restore_stored_snapshot_with_ignore(
                    load_recovery(&pending)?,
                    RevertScope::Both,
                    &self.build_ignore(&pending.policy)?,
                )
                .await?;
            state.pending = None;
            atomic_json(&self.navigation_path(session)?, &state)?;
            self.retire_recovery_record(&pending.snapshot).await;
            return Ok(restored);
        }
        let (targets, next_active) = if let Some(turn) = turn {
            let index = state
                .active
                .iter()
                .position(|id| *id == turn)
                .context("Checkpoint is not on this branch")?;
            let mut targets = Vec::new();
            for id in state.active[index..].iter().rev() {
                targets.push(self.load_snapshot(*id).await?.context("Checkpoint expired")?);
            }
            (targets, state.active[..index].to_vec())
        } else {
            let saved = state.redo.last().context("Nothing to redo; new prompts clear redo")?;
            (vec![load_recovery(saved)?], saved.active.clone())
        };
        let destination = targets.last().context("No restore target")?;
        let mut rescue = destination.clone();
        rescue.conversation = conversation.to_vec();
        rescue.metadata.prompt_text = None;
        rescue.metadata.prompt_message_index = None;
        rescue.metadata.message_count = conversation.len();
        let paths: BTreeSet<String> = targets
            .iter()
            .flat_map(|target| target.files.iter().map(|f| f.path.clone()))
            .collect();
        let workspace = self.canonical_workspace.clone();
        let storage = self.storage_dir.clone();
        rescue.files = tokio::task::spawn_blocking(move || -> Result<Vec<FileSnapshot>> {
            let paths = paths
                .iter()
                .map(|p| Self::checked_file_path(&workspace, &storage, Path::new(p)))
                .collect::<Result<Vec<_>>>()?;
            let engine = format!("vt-{}", uuid::Uuid::new_v4());
            let store = filesnap::WorkspaceStore::open(&storage, &workspace)?;
            let checkpoint = store.checkpoint(&engine, &engine, paths)?;
            anyhow::ensure!(checkpoint.stats.dropped == 0, "Could not save recovery files; rewind cancelled");
            file_records(&checkpoint.manifest, &workspace, &engine)
        })
        .await??;
        let saved = Recovery {
            policy,
            snapshot: uuid::Uuid::new_v4().to_string(),
            active: state.active.clone(),
        };
        atomic_json(&recovery_path(&self.storage_dir, &saved.snapshot), &rescue)?;
        state.pending = Some(saved.clone());
        atomic_json(&self.navigation_path(session)?, &state)?;
        let destination = CheckpointRestore {
            metadata: destination.metadata.clone(),
            conversation: destination.conversation.clone(),
        };
        for target in targets {
            if let Err(error) = self.restore_stored_snapshot_with_ignore(target, scope, &ignore).await {
                self.restore_stored_snapshot_with_ignore(rescue, RevertScope::Both, &ignore)
                    .await
                    .context(format!("Rewind failed ({error}); recovery failed; run /rewind-recover"))?;
                let failed = saved.snapshot.clone();
                state.pending = None;
                atomic_json(&self.navigation_path(session)?, &state)?;
                self.retire_recovery_record(&failed).await;
                return Err(error.context("Rewind failed; original files recovered"));
            }
        }
        state.pending = None;
        state.active = next_active;
        if turn.is_some() {
            state.redo.push(saved);
        } else {
            // Redo writes a fresh rescue record for crash recovery before
            // restoring. On success neither the consumed redo entry nor the
            // transient rescue is needed, so retire both instead of leaking.
            self.retire_recovery_record(&saved.snapshot).await;
            if let Some(used) = state.redo.pop() {
                self.retire_recovery_record(&used.snapshot).await;
            }
        }
        atomic_json(&self.navigation_path(session)?, &state)?;
        Ok(destination)
    }

    /// Complete an interrupted rewind only. Unlike [`Self::navigate_prompt`],
    /// this never touches the redo stack, so `/rewind-recover` cannot overwrite
    /// edits made after a completed rewind when there is nothing to recover.
    pub async fn recover_pending_rewind(&self, session: &str) -> Result<CheckpointRestore> {
        let _lock = acquire_verified_rewind_lock(&self.storage_dir.join("rewind.lock"))
            .context("Wait for the current turn to finish")?;
        // Fail closed when no recovery is pending; never fall back to redo.
        let state = self.navigation(session)?;
        let pending = state.pending.clone().context("No interrupted rewind to recover")?;
        let stored: StoredSnapshot = {
            uuid::Uuid::parse_str(&pending.snapshot)?;
            serde_json::from_slice(&fs::read(recovery_path(&self.storage_dir, &pending.snapshot))?)?
        };
        let policy = self.build_ignore(&pending.policy)?;
        let restored = self
            .restore_stored_snapshot_with_ignore(stored, RevertScope::Both, &policy)
            .await?;
        let mut state = self.navigation(session)?;
        // Only clear if the same pending is still present; a concurrent rewind
        // must not lose its recovery record.
        if state
            .pending
            .as_ref()
            .is_some_and(|current| current.snapshot == pending.snapshot)
        {
            state.pending = None;
            atomic_json(&self.navigation_path(session)?, &state)?;
            self.retire_recovery_record(&pending.snapshot).await;
        }
        Ok(restored)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::provider::MessageRole;
    use tempfile::TempDir;

    #[test]
    #[cfg(unix)]
    fn rewind_lock_acquire_verifies_and_blocks_concurrent_holders() {
        let temp = TempDir::new().expect("tempdir");
        let lock_path = temp.path().join("rewind.lock");

        let first = acquire_verified_rewind_lock(&lock_path).expect("first acquire");
        assert!(
            locked_file_matches_path(&first, &lock_path),
            "a fresh acquisition must cover the current path inode"
        );

        // A live holder blocks other acquirers (WouldBlock), preserving the
        // pre-existing mutual-exclusion behavior.
        let second = acquire_verified_rewind_lock(&lock_path);
        let error = second.expect_err("held lock must block");
        assert_eq!(error.kind(), std::io::ErrorKind::WouldBlock);

        drop(first);
        let third = acquire_verified_rewind_lock(&lock_path).expect("re-acquire after release");
        assert!(locked_file_matches_path(&third, &lock_path));
    }

    #[test]
    #[cfg(unix)]
    fn rewind_lock_detects_replaced_path_inode() {
        let temp = TempDir::new().expect("tempdir");
        let lock_path = temp.path().join("rewind.lock");
        fs::write(&lock_path, b"").expect("create lock file");
        let ghost = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&lock_path)
            .expect("open old inode");

        // The completion path's unlink-while-held replaced the file: the old
        // fd is now a ghost and must not be treated as covering the path.
        fs::remove_file(&lock_path).expect("unlink");
        fs::write(&lock_path, b"").expect("recreate lock file");
        assert!(
            !locked_file_matches_path(&ghost, &lock_path),
            "a ghost fd must not verify against the replaced path"
        );

        // The acquire helper never accepts a ghost: it retries onto the
        // current file even while the ghost is still open.
        let verified = acquire_verified_rewind_lock(&lock_path).expect("acquire onto current inode");
        assert!(locked_file_matches_path(&verified, &lock_path));
    }

    #[test]
    #[cfg(unix)]
    fn shell_redirects_only_capture_literal_targets_with_a_known_cwd() {
        let paths = |script: &str| shell_redirect_paths(&serde_json::json!({"cmd": script}));
        assert_eq!(paths("printf 'hello' > hello.py && cat hello.py"), BTreeSet::from([PathBuf::from("hello.py")]));
        assert_eq!(
            paths("cat > 'hello world.py' <<'PY'\nprint('> not-a-path')\nPY"),
            BTreeSet::from([PathBuf::from("hello world.py")])
        );
        assert_eq!(paths("printf hi >> log.txt 2>&1"), BTreeSet::from([PathBuf::from("log.txt")]));
        for script in [
            "cat hello.py",
            "cat < input.txt",
            "printf '> innocent.txt'",
            "echo hi 2>&1",
            "echo hi > $OUTPUT",
            "echo hi > $(pwd)/hello.py",
            "echo hi > ~/hello.py",
            "cd nested && echo hi > hello.py",
            "command cd nested; echo hi > hello.py",
            "eval 'cd nested'; echo hi > hello.py",
            "f() { echo hi > hello.py; }; f",
            "echo hi >",
        ] {
            assert!(paths(script).is_empty(), "must not guess paths for {script}");
        }
    }

    #[tokio::test]
    async fn cancelled_prompt_preparation_never_admits_a_lease_and_a_fresh_prompt_can_start() -> Result<()> {
        let dir = TempDir::new()?;
        let manager = SnapshotManager::new(SnapshotConfig::new(dir.path().into()))?;
        let session = uuid::Uuid::new_v4().to_string();
        let cancellation = tokio_util::sync::CancellationToken::new();
        cancellation.cancel();
        assert!(
            manager
                .begin_prompt_with_cancellation(1, &session, "cancelled", &[], cancellation)
                .await?
                .is_none()
        );
        assert!(!active_map().lock().unwrap().contains_key(&session));
        let lease = manager.begin_prompt(2, &session, "fresh", &[]).await?;
        assert!(active_map().lock().unwrap().contains_key(&session));
        assert!(manager.begin_prompt(3, &session, "must not overlap", &[]).await.is_err());
        drop(lease);
        assert!(!active_map().lock().unwrap().contains_key(&session));
        let _next_lease = manager.begin_prompt(3, &session, "after release", &[]).await?;
        Ok(())
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn shell_preimages_respect_ignore_and_workspace_boundaries() -> Result<()> {
        let dir = TempDir::new()?;
        let outside = TempDir::new()?;
        fs::write(dir.path().join(".filesnapignore"), "ignored.txt\n")?;
        let manager = SnapshotManager::new(SnapshotConfig::new(dir.path().into()))?;
        let session = uuid::Uuid::new_v4().to_string();
        let lease = manager.begin_prompt(1, &session, "create files", &[]).await?;
        let outside_file = outside.path().join("outside.txt");
        let script =
            format!("echo hi > ignored.txt; echo hi > {}", shell_words::quote(&outside_file.to_string_lossy()));
        declare_prompt_edit(session.clone(), "exec_command".into(), serde_json::json!({"cmd":script})).await?;
        fs::write(dir.path().join("ignored.txt"), "ignored")?;
        fs::write(&outside_file, "outside")?;
        // No input or display text should be mistaken for a shell write.
        declare_prompt_edit(session.clone(), "exec_command".into(), serde_json::json!({"cmd":"cat ignored.txt"}))
            .await?;
        assert!(manager.load_snapshot(1).await?.expect("checkpoint").files.is_empty());
        drop(lease);
        manager.navigate_prompt(Some(1), RevertScope::Both, &session, &[]).await?;
        assert_eq!(fs::read_to_string(dir.path().join("ignored.txt"))?, "ignored");
        assert_eq!(fs::read_to_string(outside_file)?, "outside");
        let _lease = manager.begin_prompt(2, &session, "next prompt", &[]).await?;
        assert!(manager.load_snapshot(2).await?.expect("next checkpoint").files.is_empty());
        Ok(())
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn shell_creation_rewind_redo_then_rewind_creation_removes_file() -> Result<()> {
        let dir = TempDir::new()?;
        let manager = SnapshotManager::new(SnapshotConfig::new(dir.path().into()))?;
        let session = uuid::Uuid::new_v4().to_string();
        let nested = dir.path().join("nested");
        fs::create_dir(&nested)?;
        let file = nested.join("hello.py");
        let neighbor = dir.path().join("hello.py");
        fs::write(&neighbor, "unrelated existing file")?;
        let original = vec![SessionMessage::new(MessageRole::User, "pwd")];
        let created = vec![SessionMessage::new(MessageRole::User, "add a hello world py")];
        let current = vec![SessionMessage::new(MessageRole::User, "add greetings")];
        let hello = "print(\"Hello, World!\")\n";
        let greetings = "print(\"Hello, Ada!\")\n";

        let lease = manager.begin_prompt(1, &session, "add a hello world py", &original).await?;
        let script = "printf 'print(\"Hello, World!\")\\n' > hello.py && cat hello.py";
        declare_prompt_edit(
            session.clone(),
            "exec_command".into(),
            serde_json::json!({"cmd":script,"workdir":"nested"}),
        )
        .await?;
        let output = tokio::process::Command::new("sh")
            .args(["-c", script])
            .current_dir(&nested)
            .output()
            .await?;
        assert!(output.status.success());
        assert_eq!(fs::read_to_string(&file)?, hello);
        // A subsequent write in the same prompt must retain the first absence.
        declare_prompt_edit(
            session.clone(),
            "exec_command".into(),
            serde_json::json!({"cmd":script,"workdir":"nested"}),
        )
        .await?;
        assert!(
            manager
                .load_snapshot(1)
                .await?
                .expect("creation checkpoint")
                .files
                .iter()
                .any(|f| f.path == "nested/hello.py" && f.deleted)
        );
        drop(lease);

        let lease = manager.begin_prompt(2, &session, "add greetings", &created).await?;
        let script = "cat > hello.py <<'PY'\nprint(\"Hello, Ada!\")\nPY";
        declare_prompt_edit(
            session.clone(),
            "unified_exec".into(),
            serde_json::json!({"action":"run","command":script,"working_dir":nested}),
        )
        .await?;
        let output = tokio::process::Command::new("sh")
            .args(["-c", script])
            .current_dir(&nested)
            .output()
            .await?;
        assert!(output.status.success());
        assert_eq!(fs::read_to_string(&file)?, greetings);
        drop(lease);
        // A file never declared or captured must not be swept away by rewind.
        fs::write(dir.path().join("unrelated.txt"), "leave me alone")?;

        let restored = manager.navigate_prompt(Some(2), RevertScope::Both, &session, &current).await?;
        assert_eq!(restored.conversation, created);
        assert_eq!(fs::read_to_string(&file)?, hello);
        let restored = manager
            .navigate_prompt(None, RevertScope::Both, &session, &restored.conversation)
            .await?;
        assert_eq!(restored.conversation, current);
        assert_eq!(fs::read_to_string(&file)?, greetings);
        let restored = manager
            .navigate_prompt(Some(1), RevertScope::Both, &session, &restored.conversation)
            .await?;
        assert_eq!(restored.conversation, original);
        assert!(!file.exists());
        assert_eq!(fs::read_to_string(&neighbor)?, "unrelated existing file");
        assert_eq!(fs::read_to_string(dir.path().join("unrelated.txt"))?, "leave me alone");
        let resumed = SnapshotManager::new(SnapshotConfig::new(dir.path().into()))?;
        let restored = resumed
            .navigate_prompt(None, RevertScope::Both, &session, &restored.conversation)
            .await?;
        assert_eq!(restored.conversation, current);
        assert_eq!(fs::read_to_string(&file)?, greetings);
        Ok(())
    }

    #[tokio::test]
    async fn native_history_restores_prompt_prefix_binary_assets_and_nested_redo() -> Result<()> {
        let dir = TempDir::new()?;
        let manager = SnapshotManager::new(SnapshotConfig::new(dir.path().into()))?;
        let session = uuid::Uuid::new_v4().to_string();
        let asset = dir.path().join("asset.bin");
        fs::write(&asset, [0, 255, 7])?;
        let original = vec![SessionMessage::new(MessageRole::User, "original")];
        let lease = manager.begin_prompt(1, &session, "first", &original).await?;
        fs::write(&asset, "first result")?;
        drop(lease);
        let second = vec![SessionMessage::new(MessageRole::User, "second")];
        let lease = manager.begin_prompt(2, &session, "second", &second).await?;
        declare_prompt_edit(session.clone(), "write_file".into(), serde_json::json!({"path":".created"})).await?;
        fs::write(dir.path().join(".created"), "second result")?;
        drop(lease);
        let current = vec![SessionMessage::new(MessageRole::User, "current")];
        let restored = manager.navigate_prompt(Some(2), RevertScope::Both, &session, &current).await?;
        assert_eq!(restored.conversation, second);
        assert!(!dir.path().join(".created").exists());
        let restored = manager
            .navigate_prompt(Some(1), RevertScope::Both, &session, &restored.conversation)
            .await?;
        assert_eq!(restored.conversation, original);
        assert_eq!(fs::read(&asset)?, vec![0, 255, 7]);
        let restored = manager
            .navigate_prompt(None, RevertScope::Both, &session, &restored.conversation)
            .await?;
        assert_eq!(restored.conversation, second);
        let restored = manager
            .navigate_prompt(None, RevertScope::Both, &session, &restored.conversation)
            .await?;
        assert_eq!(restored.conversation, current);
        assert_eq!(fs::read_to_string(dir.path().join(".created"))?, "second result");
        assert!(
            manager
                .navigate_prompt(None, RevertScope::Both, &session, &current)
                .await
                .is_err()
        );
        // One jump across both prompts must also reverse later file creation.
        let restored = manager.navigate_prompt(Some(1), RevertScope::Both, &session, &current).await?;
        assert_eq!(restored.conversation, original);
        assert!(!dir.path().join(".created").exists());
        let resumed = SnapshotManager::new(SnapshotConfig::new(dir.path().into()))?;
        let restored = resumed.navigate_prompt(None, RevertScope::Both, &session, &original).await?;
        assert_eq!(restored.conversation, current);
        Ok(())
    }

    fn live_recovery_files(workspace: &Path) -> Vec<PathBuf> {
        let storage = workspace.join(".vtcode").join("checkpoints");
        fs::read_dir(&storage)
            .map(|entries| {
                entries
                    .filter_map(|entry| entry.ok().map(|entry| entry.path()))
                    .filter(|path| {
                        path.file_name()
                            .and_then(|name| name.to_str())
                            .is_some_and(|name| name.starts_with("turn_recovery_") && name.ends_with(".json"))
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    #[tokio::test]
    async fn consumed_recovery_records_are_retired_not_leaked() -> Result<()> {
        let dir = TempDir::new()?;
        let manager = SnapshotManager::new(SnapshotConfig::new(dir.path().into()))?;
        let session = uuid::Uuid::new_v4().to_string();
        let original = vec![SessionMessage::new(MessageRole::User, "original")];
        let lease = manager.begin_prompt(1, &session, "first", &original).await?;
        drop(lease);
        let second = vec![SessionMessage::new(MessageRole::User, "second")];
        let lease = manager.begin_prompt(2, &session, "second", &second).await?;
        drop(lease);
        let current = vec![SessionMessage::new(MessageRole::User, "current")];

        assert!(live_recovery_files(dir.path()).is_empty());
        let restored = manager.navigate_prompt(Some(2), RevertScope::Both, &session, &current).await?;
        assert_eq!(live_recovery_files(dir.path()).len(), 1);

        let restored = manager
            .navigate_prompt(None, RevertScope::Both, &session, &restored.conversation)
            .await?;
        assert_eq!(restored.conversation, current);
        assert!(live_recovery_files(dir.path()).is_empty(), "consumed redo must retire its recovery record");

        // A new rewind followed by a new prompt must also retire the discarded redo.
        let restored = manager.navigate_prompt(Some(2), RevertScope::Both, &session, &current).await?;
        assert_eq!(live_recovery_files(dir.path()).len(), 1);
        let _lease = manager.begin_prompt(3, &session, "third", &restored.conversation).await?;
        assert!(live_recovery_files(dir.path()).is_empty(), "begin_prompt must retire discarded redo records");
        Ok(())
    }

    #[tokio::test]
    async fn recover_pending_fails_closed_without_touching_redo() -> Result<()> {
        let dir = TempDir::new()?;
        let manager = SnapshotManager::new(SnapshotConfig::new(dir.path().into()))?;
        let session = uuid::Uuid::new_v4().to_string();
        let original = vec![SessionMessage::new(MessageRole::User, "original")];
        let lease = manager.begin_prompt(1, &session, "first", &original).await?;
        drop(lease);
        let current = vec![SessionMessage::new(MessageRole::User, "current")];

        // No interrupted rewind: recovery must fail and must not consume redo.
        assert!(manager.recover_pending_rewind(&session).await.is_err());
        let restored = manager.navigate_prompt(Some(1), RevertScope::Both, &session, &current).await?;
        assert!(manager.recover_pending_rewind(&session).await.is_err());
        // Redo is still intact for the completed rewind.
        let redone = manager
            .navigate_prompt(None, RevertScope::Both, &session, &restored.conversation)
            .await?;
        assert_eq!(redone.conversation, current);
        Ok(())
    }

    #[tokio::test]
    async fn complete_session_navigation_trims_active_and_clears_redo() -> Result<()> {
        let dir = TempDir::new()?;
        let manager = SnapshotManager::new(SnapshotConfig::new(dir.path().into()))?;
        let storage = dir.path().join(".vtcode").join("checkpoints");
        let session = uuid::Uuid::new_v4().to_string();
        let key: String = session.as_bytes().iter().map(|b| format!("{b:02x}")).collect();
        let branch_path = storage.join(format!("branch_{key}.json"));
        let lock_path = storage.join("rewind.lock");
        fs::write(&lock_path, b"")?;

        // 20 protected turns plus a redo entry: the completion trim must keep
        // only the newest REWIND_ACTIVE_KEEP actives and drop redo entirely.
        let active: Vec<usize> = (1435..=1454).collect();
        let recovery = Recovery {
            policy: String::new(),
            snapshot: uuid::Uuid::new_v4().to_string(),
            active: vec![1435],
        };
        let recovery_path = recovery_path(&storage, &recovery.snapshot);
        atomic_json(&recovery_path, &serde_json::json!({}))?;
        let state = Navigation {
            active: active.clone(),
            redo: vec![recovery],
            pending: None,
        };
        atomic_json(&branch_path, &state)?;

        manager.complete_session_navigation(&session).await?;

        let saved: Navigation = serde_json::from_slice(&fs::read(&branch_path)?)?;
        assert_eq!(
            saved.active,
            active[active.len() - REWIND_ACTIVE_KEEP..].to_vec(),
            "only the newest rewind window stays pinned"
        );
        assert!(saved.redo.is_empty(), "completion must clear redo");
        assert!(saved.pending.is_none());
        assert!(!lock_path.exists(), "completion must release the workspace rewind lock");
        assert!(!recovery_path.exists(), "completion must retire redo recovery records");
        Ok(())
    }
}
