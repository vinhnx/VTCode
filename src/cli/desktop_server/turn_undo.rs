//! Safe per-turn file-change reversal for the desktop transcript.
//!
//! Undo reverses exactly the changes a turn recorded in the session event
//! log — never a whole-workspace checkpoint restore. Every refusal is a
//! successful call with `undone: false` and a reason; only malformed
//! requests and active-turn races are transport errors.
//!
//! Safety contract (two phases, fail closed):
//!
//! 1. Gather: collect the turn's completed `FileChange` items from the
//!    event tail, cross-check each recorded file against its strict
//!    per-file diff, and reverse-apply every op to in-memory content.
//! 2. Flush: re-read each file immediately before its mutation and abort
//!    on any drift (later edits, external writes). Applied files are
//!    reported honestly when a later file aborts the flush.
//!
//! Non-guarantees: protection assumes no adversary racing the filesystem
//! (external edits are detected via revalidation, not locked out), and
//! recreated files receive default permissions since pre-change modes are
//! not recorded. Turns are refused when the log shows mutating shell
//! activity outside the file pipeline, but MCP-side mutations carry no
//! activity stamp and legacy events predate terminator attestation: both
//! degrade to documented refusals only where the evidence allows.

use super::*;
use std::collections::HashMap;
use std::ffi::OsString;
use std::io::ErrorKind;

use vtcode_commons::paths::{canonicalize_async, ensure_path_within_workspace};
use vtcode_core::exec::events::{
    CommandActivity, FileChangeItem, PatchApplyStatus, PatchChangeKind, ThreadEvent, ThreadItemDetails,
};
use vtcode_diff::{DiffHunk, DiffLineKind, UnifiedFile, reverse_apply_hunks, split_unified_files};

/// Event bytes scanned for one undo: mirrors session restore.
const EVENT_TAIL_BYTES: usize = 4 * 1024 * 1024;
/// Identifier bound for session and turn ids.
const MAX_ID_BYTES: usize = 256;
/// Refusal bounds for pathological change sets.
const MAX_UNDO_FILES: usize = 256;
const MAX_UNDO_HUNKS_PER_FILE: usize = 2048;
/// Total in-memory plan budget (original plus reversed bytes held while
/// planning): per-file caps alone cannot bound 256 large files.
const MAX_UNDO_PLAN_BYTES: usize = 64 * 1024 * 1024;

pub(super) async fn handle_undo_turn(context: &ServiceContext, request: &Request) -> Result<(Value, bool)> {
    let params: protocol::TurnUndoParams = parse_params(&request.params)?;
    if params.session_id.is_empty()
        || params.session_id.len() > MAX_ID_BYTES
        || params.turn_id.is_empty()
        || params.turn_id.len() > MAX_ID_BYTES
    {
        bail!("invalid turn undo identity");
    }
    if context.state.lock().await.session_id.as_deref() != Some(params.session_id.as_str()) {
        bail!("undo is only available for the selected session");
    }
    ensure_no_active_turn(context)
        .await
        .map_err(|_cause| anyhow!("cancel the active turn before undoing file changes"))?;
    let (undone, files, reason) = match undo_turn(&context.workspace, &params.session_id, &params.turn_id).await {
        Ok(files) => (true, files, String::new()),
        Err(refusal) => (false, refusal.applied, refusal.reason),
    };
    let result = protocol::TurnUndoResult {
        session_id: params.session_id,
        turn_id: params.turn_id,
        undone,
        files,
        reason,
    };
    Ok((serde_json::to_value(result)?, false))
}

/// One recorded file change with its strict per-file hunks.
struct UndoFileOp {
    path: String,
    kind: PatchChangeKind,
    hunks: Vec<DiffHunk>,
}

/// Refusal with honestly reported partial progress.
struct UndoRefusal {
    reason: String,
    applied: Vec<protocol::TurnUndoFile>,
}

impl UndoRefusal {
    fn clean(reason: impl Into<String>) -> Self {
        Self { reason: reason.into(), applied: Vec::new() }
    }
}

async fn undo_turn(
    workspace: &Path,
    session_id: &str,
    turn_id: &str,
) -> Result<Vec<protocol::TurnUndoFile>, UndoRefusal> {
    let ops = gather_undo_ops(workspace, session_id, turn_id).map_err(UndoRefusal::clean)?;
    apply_undo_ops(workspace, &ops).await
}

fn gather_undo_ops(workspace: &Path, session_id: &str, turn_id: &str) -> Result<Vec<UndoFileOp>, String> {
    let tail = match vtcode_memory::snapshot::read_event_tail(workspace, session_id, EVENT_TAIL_BYTES) {
        Ok(Some(tail)) => tail,
        Ok(None) => return Err("no recorded history for this session; undo is unavailable".to_string()),
        Err(error) => return Err(public_tail_reason(&error)),
    };
    if tail.truncated {
        return Err("recorded history is incomplete; undo is unavailable".to_string());
    }
    let mut ops = Vec::new();
    let mut unattributed = 0usize;
    let mut shell_mutation = false;
    for event in tail.events {
        let ThreadEvent::ItemCompleted(completed) = event.into_event() else {
            continue;
        };
        let item = completed.item;
        let Some(context) = item.context.as_deref() else {
            if matches!(item.details, ThreadItemDetails::FileChange(_)) {
                unattributed += 1;
            }
            continue;
        };
        if context.turn_id != turn_id {
            continue;
        }
        // Mutating shell activity runs outside the file pipeline, so its
        // bytes are invisible to reversal: the turn cannot be undone
        // exactly. File tools never carry this stamp (only command-session
        // tools and exec output are classified), so file-only turns pass.
        if context.activity == Some(CommandActivity::Mutation)
            && matches!(item.details, ThreadItemDetails::CommandExecution(_) | ThreadItemDetails::ToolInvocation(_))
        {
            shell_mutation = true;
            continue;
        }
        let ThreadItemDetails::FileChange(change) = item.details else {
            continue;
        };
        collect_item_ops(&change, &mut ops)?;
    }
    if shell_mutation {
        return Err("turn ran mutating commands outside the file pipeline; undo is unavailable".to_string());
    }
    if ops.is_empty() {
        if unattributed > 0 {
            return Err("recorded file changes lack turn attribution; undo is unavailable".to_string());
        }
        return Err("no completed file changes recorded for this turn".to_string());
    }
    if ops.len() > MAX_UNDO_FILES {
        return Err("turn has too many file changes to undo safely".to_string());
    }
    Ok(ops)
}

fn public_tail_reason(error: &vtcode_memory::SessionStoreError) -> String {
    match error {
        vtcode_memory::SessionStoreError::Io { source, .. } if source.kind() == ErrorKind::InvalidInput => {
            "unknown session; undo is unavailable".to_string()
        }
        vtcode_memory::SessionStoreError::Io { source, .. } if source.kind() == ErrorKind::Unsupported => {
            "recorded history cannot be read on this platform; undo is unavailable".to_string()
        }
        _ => "recorded history cannot be read; undo is unavailable".to_string(),
    }
}

fn collect_item_ops(change: &FileChangeItem, ops: &mut Vec<UndoFileOp>) -> Result<(), String> {
    if change.status != PatchApplyStatus::Completed {
        return Err("turn has a file change that failed to apply; undo is unavailable".to_string());
    }
    if change.diff_incomplete == Some(true) {
        return Err("turn has an incomplete recorded diff; undo is unavailable".to_string());
    }
    if change.changes.is_empty() {
        return Err("turn has a file change without recorded files; undo is unavailable".to_string());
    }
    let diff = change.unified_diff.as_deref().filter(|diff| !diff.trim().is_empty());
    let Some(diff) = diff else {
        // An empty recording is evidence only for whole-file Add/Delete of
        // an empty file (no bytes, hence no terminators to lose): anything
        // else is indistinguishable from a missing recording.
        if change.changes.len() == 1 && matches!(change.changes[0].kind, PatchChangeKind::Add | PatchChangeKind::Delete)
        {
            let only = &change.changes[0];
            ops.push(UndoFileOp {
                path: only.path.clone(),
                kind: only.kind,
                hunks: Vec::new(),
            });
            return Ok(());
        }
        return Err("turn has a file change without a recorded diff; undo is unavailable".to_string());
    };
    let files =
        split_unified_files(diff).map_err(|error| format!("recorded diff cannot be reversed exactly: {error}"))?;
    let mut matched = vec![false; files.len()];
    for change in &change.changes {
        // Normalized terminators cannot reproduce the original bytes: the
        // unverified Delete path additionally refuses unknown (legacy)
        // recordings, while Update/Add fall back on exact content checks.
        if change.terminators_normalized == Some(true) {
            return Err(format!(
                "{} has a recorded diff with normalized line endings; undo is unavailable",
                change.path
            ));
        }
        if change.kind == PatchChangeKind::Delete && change.terminators_normalized != Some(false) {
            return Err(format!(
                "turn deleted {}, whose original bytes cannot be verified; undo is unavailable",
                change.path
            ));
        }
        let index = match_file_section(&files, &matched, &change.path, change.kind)
            .ok_or_else(|| "recorded file list does not match the recorded diff".to_string())?;
        matched[index] = true;
        if files[index].hunks.len() > MAX_UNDO_HUNKS_PER_FILE {
            return Err(format!("{} changed too much to undo safely", change.path));
        }
        ops.push(UndoFileOp {
            path: change.path.clone(),
            kind: change.kind,
            hunks: files[index].hunks.clone(),
        });
    }
    if matched.iter().any(|matched| !matched) {
        return Err("recorded file list does not match the recorded diff".to_string());
    }
    Ok(())
}

/// Matches one recorded change against its strict diff section. `Add` is
/// keyed by the new-side path, `Delete` by the old-side path, and `Update`
/// by either; a headerless single-file section attributes to a lone change.
fn match_file_section(files: &[UnifiedFile], matched: &[bool], path: &str, kind: PatchChangeKind) -> Option<usize> {
    debug_assert_eq!(matched.len(), files.len(), "match flags track sections one to one");
    if files.len() == 1 && files[0].old_path.is_none() && files[0].new_path.is_none() {
        return (!matched[0]).then_some(0);
    }
    files.iter().enumerate().position(|(index, file)| {
        if matched[index] {
            return false;
        }
        let keyed = match kind {
            PatchChangeKind::Add => file.new_path.as_deref(),
            PatchChangeKind::Delete => file.old_path.as_deref(),
            PatchChangeKind::Update => file.new_path.as_deref().or(file.old_path.as_deref()),
        };
        keyed == Some(path)
    })
}

/// In-memory reversal plan for one path: original disk state plus the
/// fully reversed outcome. `None` means absent.
struct UndoFilePlan {
    /// Workspace-relative path, for reporting.
    path: String,
    /// Contained absolute path; ancestors verified against symlink escape.
    absolute: PathBuf,
    original: Option<String>,
    reversed: Option<String>,
    /// Reversed ops in recorded order, for reporting.
    reports: Vec<protocol::TurnUndoFile>,
}

async fn apply_undo_ops(workspace: &Path, ops: &[UndoFileOp]) -> Result<Vec<protocol::TurnUndoFile>, UndoRefusal> {
    // Group by path, preserving first-seen order; ops per path stay in
    // recorded order so reversal below runs them back to front.
    let mut order: Vec<&str> = Vec::new();
    let mut groups: HashMap<&str, Vec<&UndoFileOp>> = HashMap::new();
    for op in ops {
        groups
            .entry(op.path.as_str())
            .or_insert_with(|| {
                order.push(op.path.as_str());
                Vec::new()
            })
            .push(op);
    }
    let canonical_workspace = canonicalize_async(workspace)
        .await
        .map_err(|error| UndoRefusal::clean(public_io_reason(error)))?;
    let mut plans = Vec::with_capacity(order.len());
    let mut planned_bytes = 0usize;
    for path in &order {
        let Some(group) = groups.get(path) else {
            continue;
        };
        let plan = plan_file_undo(workspace, &canonical_workspace, path, group)
            .await
            .map_err(UndoRefusal::clean)?;
        planned_bytes = planned_bytes.saturating_add(plan_held_bytes(&plan));
        if !plan_fits_budget(planned_bytes) {
            return Err(UndoRefusal::clean("turn holds too much content to undo safely"));
        }
        plans.push(plan);
    }

    // Flush: re-read each file immediately before its mutation. An abort
    // reports the files already flushed; each mutation below is atomic
    // (tmp-file rename or single unlink), so no file is left half-written.
    let mut applied = Vec::new();
    for plan in &plans {
        match flush_file_undo(plan).await {
            Ok(()) => applied.extend(plan.reports.clone()),
            Err(reason) => {
                let done = applied.len();
                let total: usize = plans.iter().map(|plan| plan.reports.len()).sum();
                return Err(UndoRefusal {
                    reason: format!("{reason} ({done} of {total} file changes reverted)"),
                    applied,
                });
            }
        }
    }
    Ok(applied)
}

async fn plan_file_undo(
    workspace: &Path,
    canonical_workspace: &Path,
    path: &str,
    group: &[&UndoFileOp],
) -> Result<UndoFilePlan, String> {
    if path.is_empty() {
        return Err("recorded an empty file path; undo is unavailable".to_string());
    }
    let absolute = resolve_undo_path(workspace, canonical_workspace, path).await?;
    let original = read_undo_state(path, &absolute).await?;
    let mut state = original.clone();
    for op in group.iter().rev() {
        state = reverse_op_state(path, op, state)?;
    }
    let reports = group
        .iter()
        .map(|op| {
            let (additions, deletions) = hunk_counts(&op.hunks);
            protocol::TurnUndoFile {
                path: op.path.clone(),
                kind: op.kind,
                additions,
                deletions,
            }
        })
        .collect();
    Ok(UndoFilePlan {
        path: path.to_owned(),
        absolute,
        original,
        reversed: state,
        reports,
    })
}

fn reverse_op_state(path: &str, op: &UndoFileOp, state: Option<String>) -> Result<Option<String>, String> {
    match op.kind {
        PatchChangeKind::Update => {
            let Some(current) = state else {
                return Err(format!("{path} was deleted since the turn; undo is unavailable"));
            };
            reverse_apply_hunks(&current, &op.hunks)
                .map(Some)
                .map_err(|error| format!("{path} cannot be reversed exactly ({error}); undo is unavailable"))
        }
        PatchChangeKind::Add => {
            let Some(current) = state else {
                return Err(format!("{path} was deleted since the turn; undo is unavailable"));
            };
            let added: String = op.hunks.iter().map(|hunk| hunk.new_side_text()).collect();
            if current != added {
                return Err(format!("{path} changed since the turn; undo is unavailable"));
            }
            Ok(None)
        }
        PatchChangeKind::Delete => {
            if state.is_some() {
                return Err(format!("{path} changed since the turn; undo is unavailable"));
            }
            let restored: String = op.hunks.iter().map(|hunk| hunk.old_side_text()).collect();
            Ok(Some(restored))
        }
    }
}

fn hunk_counts(hunks: &[DiffHunk]) -> (u64, u64) {
    let mut additions = 0u64;
    let mut deletions = 0u64;
    for hunk in hunks {
        for line in &hunk.lines {
            match line.kind {
                DiffLineKind::Addition => additions = additions.saturating_add(1),
                DiffLineKind::Deletion => deletions = deletions.saturating_add(1),
                DiffLineKind::Context => {}
            }
        }
    }
    (additions, deletions)
}

/// Resolves a recorded relative path to a contained absolute path, refusing
/// lexical escapes, absolute paths, and symlink ancestors. The final
/// component itself must not be a symlink either (checked on read).
async fn resolve_undo_path(workspace: &Path, canonical_workspace: &Path, path: &str) -> Result<PathBuf, String> {
    let candidate = workspace.join(path);
    let absolute = ensure_path_within_workspace(&candidate, workspace)
        .map_err(|_cause| format!("{path} escapes the workspace; undo is unavailable"))?;
    // Walk every level from the file up to the workspace root: intermediate
    // symlinks are resolved by the OS, so only a level-by-level check sees
    // them. Unreadable levels refuse (fail closed); missing levels cannot
    // be links. The nearest existing ancestor must canonicalize inside the
    // workspace, so a symlinked directory can never redirect the mutation.
    let mut level = absolute.as_path();
    let mut nearest_existing = None;
    loop {
        match tokio::fs::symlink_metadata(level).await {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() {
                    return Err(format!("{path} resolves through a symlink; undo is unavailable"));
                }
                if nearest_existing.is_none() {
                    nearest_existing = Some(level);
                }
            }
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => return Err(public_io_reason(error)),
        }
        if level == workspace {
            break;
        }
        let Some(parent) = level.parent() else {
            break;
        };
        level = parent;
    }
    let Some(existing) = nearest_existing else {
        return Err(public_io_reason(std::io::Error::new(ErrorKind::NotFound, "workspace is missing")));
    };
    let canonical_ancestor = canonicalize_async(existing).await.map_err(public_io_reason)?;
    if !canonical_ancestor.starts_with(canonical_workspace) {
        return Err(format!("{path} escapes the workspace; undo is unavailable"));
    }
    Ok(absolute)
}

/// Shared budget predicate so the flush honors the same bound the plan
/// was built under: total held bytes must fit [`MAX_UNDO_PLAN_BYTES`].
fn plan_fits_budget(held_bytes: usize) -> bool {
    held_bytes <= MAX_UNDO_PLAN_BYTES
}

fn plan_held_bytes(plan: &UndoFilePlan) -> usize {
    plan.original
        .as_ref()
        .map_or(0, String::len)
        .saturating_add(plan.reversed.as_ref().map_or(0, String::len))
}

/// Reads current disk state: `None` when absent. Refuses symlinks,
/// non-regular files, oversized files (before allocating the read), and
/// non-UTF-8 bytes.
async fn read_undo_state(path: &str, absolute: &Path) -> Result<Option<String>, String> {
    match tokio::fs::symlink_metadata(absolute).await {
        Ok(metadata) => {
            let file_type = metadata.file_type();
            if file_type.is_symlink() {
                return Err(format!("{path} is a symlink; undo is unavailable"));
            }
            if !file_type.is_file() {
                return Err(format!("{path} is not a regular file; undo is unavailable"));
            }
            if metadata.len() > vtcode_diff::MAX_REVERSE_FILE_BYTES as u64 {
                return Err(format!("{path} is too large to verify safely; undo is unavailable"));
            }
        }
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(public_io_reason(error)),
    }
    let bytes = tokio::fs::read(absolute).await.map_err(public_io_reason)?;
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|_cause| format!("{path} is not valid UTF-8; undo is unavailable"))
}

async fn flush_file_undo(plan: &UndoFilePlan) -> Result<(), String> {
    // Revalidate immediately before mutation: the file must still hold
    // exactly the bytes the plan was built from.
    let current = read_undo_state(&plan.path, &plan.absolute).await?;
    if current != plan.original {
        return Err(format!("{} changed during undo", plan.path));
    }
    match &plan.reversed {
        Some(content) => atomic_write_file(&plan.absolute, content).await,
        None => tokio::fs::remove_file(&plan.absolute).await.map_err(public_io_reason),
    }
}

/// Writes through a temp file plus rename so a crash never leaves a
/// half-written file; preserves the original permissions when present.
async fn atomic_write_file(absolute: &Path, content: &str) -> Result<(), String> {
    if let Some(parent) = absolute.parent() {
        tokio::fs::create_dir_all(parent).await.map_err(public_io_reason)?;
    }
    let permissions = tokio::fs::symlink_metadata(absolute)
        .await
        .ok()
        .filter(|metadata| metadata.file_type().is_file())
        .map(|metadata| metadata.permissions());
    let temp = undo_temp_path(absolute);
    let write = async {
        tokio::fs::write(&temp, content.as_bytes()).await?;
        if let Some(permissions) = permissions {
            tokio::fs::set_permissions(&temp, permissions).await?;
        }
        tokio::fs::rename(&temp, absolute).await?;
        Ok::<(), std::io::Error>(())
    }
    .await;
    if let Err(error) = write {
        let _ = tokio::fs::remove_file(&temp).await;
        return Err(public_io_reason(error));
    }
    Ok(())
}

fn undo_temp_path(absolute: &Path) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or(0);
    let mut temp: OsString = absolute.as_os_str().to_owned();
    temp.push(format!(".vtcode-undo-{}-{nanos}.tmp", std::process::id()));
    PathBuf::from(temp)
}

fn public_io_reason(error: std::io::Error) -> String {
    format!("file update failed ({}); undo is unavailable", sanitize_public_error(&error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_budget_accepts_both_sides_of_the_bound() {
        assert!(plan_fits_budget(0));
        assert!(plan_fits_budget(MAX_UNDO_PLAN_BYTES));
        assert!(!plan_fits_budget(MAX_UNDO_PLAN_BYTES.saturating_add(1)));
    }
}
