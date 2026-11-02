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

/// Holds this session's checkpoint lock until the complete agent turn has
/// settled.
///
/// Same-session begins and rewinds serialize on this lock; other sessions in
/// the workspace proceed concurrently with their own locks and disjoint
/// checkpoint records.
#[derive(Debug)]
pub struct PromptCheckpointLease {
    key: String,
    _lock: fs::File,
    skipped_paths: Option<SnapshotSkippedPaths>,
}

impl PromptCheckpointLease {
    /// Paths the turn-start capture left out, if any, so the caller can warn
    /// that this turn's rewind may be incomplete.
    pub fn skipped_paths(&self) -> Option<&SnapshotSkippedPaths> {
        self.skipped_paths.as_ref()
    }
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

/// One filesnap drop reason as a short human phrase.
fn drop_reason_phrase(reason: filesnap::DropReason) -> String {
    match reason {
        filesnap::DropReason::OverSizeLimit => {
            let mib = filesnap::ScanLimits::default().max_file_bytes / (1024 * 1024);
            format!("larger than the {mib} MiB snapshot size limit")
        }
        filesnap::DropReason::Unreadable => "unreadable".to_string(),
        filesnap::DropReason::NotARegularFile => "not a regular file".to_string(),
    }
}

/// Record what a capture left out: the exact dropped count plus filesnap's
/// bounded sample, relativized for display. Stored on the snapshot and
/// carried by the lease so both the turn-start warning and a later rewind
/// can stay honest about coverage.
pub(super) fn skipped_paths_record(workspace: &Path, checkpoint: &filesnap::Checkpoint) -> SnapshotSkippedPaths {
    SnapshotSkippedPaths {
        total: checkpoint.stats.dropped,
        sample: checkpoint
            .stats
            .sample
            .iter()
            .map(|(path, reason)| SnapshotSkippedPath {
                path: path
                    .strip_prefix(workspace)
                    .unwrap_or(path)
                    .to_string_lossy()
                    .replace('\\', "/"),
                reason: drop_reason_phrase(*reason),
            })
            .collect(),
    }
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
        let key = session_filename_key(session)?;
        Ok(self.storage_dir.join(format!("branch_{key}.json")))
    }

    /// This session's checkpoint lock file: the `branch_{key}.json`
    /// navigation file's `rewind-{key}.lock` sibling.
    ///
    /// The [`PromptCheckpointLease`] holds it for the whole turn and
    /// rewind/recover take it for their duration, so one session can never
    /// capture and restore at once — while other sessions use their own
    /// lock files concurrently.
    fn session_lock_path(&self, session: &str) -> Result<PathBuf> {
        let key = session_filename_key(session)?;
        Ok(self.storage_dir.join(format!("rewind-{key}.lock")))
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
    /// stack, and reclaims this session's lock file.
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
        remove_lock_file_if_unheld(&self.session_lock_path(session)?);
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
    /// blocking worker retains the session lock through atomic publication and
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
        // all belong on the blocking worker. Its lease retains the session
        // lock through pruning and the complete agent turn, including on
        // cancellation. Other sessions hold their own locks concurrently.
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
        // Validate before touching any lock so invalid ids fail fast, then
        // take this session's lock. Same-session overlap fails here (a second
        // turn or rewind in this session); other sessions proceed on their
        // own locks — the filesnap engine serializes per session and gives
        // each capture a truthful record of the workspace.
        let lock_path = self.session_lock_path(session)?;
        let lock = acquire_verified_rewind_lock(&lock_path).context(SESSION_BUSY_CONTEXT)?;
        let mut state = self.navigation(session)?;
        anyhow::ensure!(state.pending.is_none(), "{INTERRUPTED_REWIND_MARKER}; run /rewind-recover before continuing");
        let workspace = self.canonical_workspace.clone();
        let storage = self.storage_dir.clone();
        let engine = format!("vt-{}", uuid::Uuid::new_v4());
        let watch = format!("vt-watch-{session}");
        let active = Active {
            workspace: workspace.clone(),
            storage: storage.clone(),
            record: self.snapshot_path_for_session(session, turn)?,
            engine: engine.clone(),
            watch: watch.clone(),
        };
        let (files, skipped_paths) = {
            let store = filesnap::WorkspaceStore::open(&storage, &workspace)?;
            store.note_turn(&watch, &engine)?;
            let watched: Vec<_> = store
                .declared_paths(&watch, filesnap::DeclaredWindow::default())?
                .into_iter()
                .collect();
            store.declare_paths(&engine, &engine, &watched)?;
            let checkpoint = filesnap::capture_turn(&store, &engine, &engine, &filesnap::TurnScope::at(&workspace))?;
            // Skipped paths degrade the checkpoint instead of blocking the
            // turn: the exclusions ride the snapshot record and the lease so
            // the turn-start warning and a later rewind stay honest.
            let skipped_paths = (checkpoint.stats.dropped != 0).then(|| skipped_paths_record(&workspace, &checkpoint));
            let files = file_records(&checkpoint.manifest, &workspace, &engine)?;
            for file in &files {
                SnapshotManager::checked_file_path(&workspace, &storage, Path::new(&file.path))?;
            }
            (files, skipped_paths)
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
            skipped_paths: skipped_paths.clone(),
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
        Ok(PromptCheckpointLease { key: session.into(), _lock: lock, skipped_paths })
    }
    /// Enumerate only checkpoints on this session's current conversation branch.
    pub async fn rewind_points(&self, session: &str) -> Result<Vec<SnapshotMetadata>> {
        let state = self.navigation(session)?;
        let mut points = Vec::new();
        for turn in state.active.iter().rev() {
            if let Some(stored) = self.load_snapshot_for_session(session, *turn).await? {
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
        // Serialize against this session's own turns; other sessions rewind
        // concurrently on their own locks.
        let _lock = acquire_verified_rewind_lock(&self.session_lock_path(session)?)
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
            anyhow::ensure!(turn.is_none(), "{INTERRUPTED_REWIND_MARKER}; run /rewind-recover");
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
                targets.push(
                    self.load_snapshot_for_session(session, *id)
                        .await?
                        .context("Checkpoint expired")?,
                );
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
        // The rescue capture below fails closed on drops, so its coverage is
        // complete by construction: the destination's exclusions must not ride
        // along and cry wolf on redo.
        rescue.metadata.skipped_paths = None;
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
            if checkpoint.stats.dropped != 0 {
                let skipped = skipped_paths_record(&workspace, &checkpoint);
                anyhow::bail!("Could not save recovery files ({}); rewind cancelled", skipped.summary());
            }
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
        let _lock = acquire_verified_rewind_lock(&self.session_lock_path(session)?)
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
mod tests;
