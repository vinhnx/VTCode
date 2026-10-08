use std::fs;
use std::io::{Read as _, Write as _};
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, ensure};
use chrono::Utc;
use uuid::Uuid;
use vtcode_commons::canonicalize;

use crate::utils::session_archive::VerifiedSessionArchiveIdentifier;
use crate::utils::session_debug::sanitize_debug_component;

const TASKS_DIR: &str = ".vtcode/tasks";
const CURRENT_BLOCKED_FILE: &str = "current_blocked.md";
const BLOCKERS_DIR: &str = "blockers";

/// Plan-mode note appended to the handoff's Actionable Next Steps when the
/// blocked turn ran with planning active. Kept as one generic paragraph
/// (rather than the transcript's mutation/generic split) so the persisted
/// file stays useful to a resumed session without duplicating the
/// caller-side block-kind matcher.
const PLAN_MODE_HANDOFF_GUIDANCE: &str = "- Plan mode was active (read-only) when this turn blocked: `Mutation blocked` means the edit was stopped by policy, not by a failing check. To keep planning, type `continue` to resume with retained history; to implement, approve the plan or run `/mode build` (`/mode auto` for unattended).";

struct BlockedHandoffPaths<'a> {
    current: &'a Path,
    archive: &'a Path,
}

/// Artifacts produced by [`write_blocked_handoff`], containing paths to the
/// current and archived handoff files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockedHandoffArtifacts {
    /// Path to the current blocked handoff markdown file.
    pub current_path: PathBuf,
    /// Path to the archived blocked handoff markdown file.
    pub archive_path: PathBuf,
}

/// Resume metadata for a blocked handoff.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockedHandoffResume<'a> {
    /// The identifier came from a verified, persisted session archive.
    Available(&'a VerifiedSessionArchiveIdentifier),
    /// No durable archive can be advertised for this handoff.
    Unavailable(&'a str),
}

/// Write a blocked-handoff artifact when the agent hits an unrecoverable blocker.
///
/// Creates both a `current_blocked.md` file and a timestamped archive under
/// `.vtcode/tasks/blockers/`. The handoff includes the blocker summary and
/// explicitly declines to attribute the workspace-global tracker to a session. Resume commands are added only by
/// [`write_blocked_handoff_with_resume`] after a caller verifies an
/// archive identifier.
pub fn write_blocked_handoff(
    workspace: &Path,
    session_id: &str,
    outcome_code: &str,
    blocker_summary: &str,
    relevant_paths: &[PathBuf],
) -> Result<BlockedHandoffArtifacts> {
    write_blocked_handoff_with_resume(
        workspace,
        session_id,
        outcome_code,
        blocker_summary,
        relevant_paths,
        BlockedHandoffResume::Unavailable(
            "Resume is unavailable because this compatibility entry point has no verified session archive.",
        ),
        false,
    )
}

/// Write a blocked handoff with resume metadata supplied through the typed
/// archive-verification boundary.
///
/// `planning_active` records whether the blocked turn ran with planning
/// active so the persisted handoff carries the same mode guidance as the
/// transcript (a resumed session reads the file, not the old transcript).
pub fn write_blocked_handoff_with_resume(
    workspace: &Path,
    session_id: &str,
    outcome_code: &str,
    blocker_summary: &str,
    relevant_paths: &[PathBuf],
    resume: BlockedHandoffResume<'_>,
    planning_active: bool,
) -> Result<BlockedHandoffArtifacts> {
    let (workspace, tasks_dir, blockers_dir) = safe_handoff_directories(workspace)?;
    fs::create_dir_all(&blockers_dir)
        .with_context(|| format!("failed to create blockers dir {}", blockers_dir.display()))?;

    let current_path = tasks_dir.join(CURRENT_BLOCKED_FILE);
    let timestamp = Utc::now();
    let archive_name = format!(
        "{}-{}-{}.md",
        sanitize_debug_component(session_id, "session"),
        timestamp.format("%Y%m%dT%H%M%SZ"),
        Uuid::new_v4()
    );
    let archive_path = blockers_dir.join(archive_name);

    let markdown = render_blocked_handoff(
        &workspace,
        session_id,
        outcome_code,
        blocker_summary,
        BlockedHandoffPaths { current: &current_path, archive: &archive_path },
        relevant_paths,
        timestamp.to_rfc3339(),
        resume,
        planning_active,
    );

    write_handoff_file(&archive_path, &markdown, false)?;
    write_handoff_file(&current_path, &markdown, true)?;

    // Preserve forensic artifacts and pin the session so ordinary retention
    // cannot erase evidence while this blocker remains unresolved.
    copy_blocker_session_forensics(&workspace, session_id, &archive_path);
    pin_blocker_session_retention(&workspace, session_id);

    Ok(BlockedHandoffArtifacts { current_path, archive_path })
}

/// Copy session `events.jsonl` / ATIF into a forensics dir beside the archive.
/// Best-effort: missing sources must never fail the handoff write.
fn copy_blocker_session_forensics(workspace: &Path, session_id: &str, archive_path: &Path) {
    let session_dir = vtcode_memory::session_directory(workspace, session_id);
    if !session_dir.is_dir() {
        return;
    }
    copy_forensics_from_dir(&session_dir, archive_path);
}

fn copy_forensics_from_dir(session_dir: &Path, archive_path: &Path) {
    let Some(stem) = archive_path.file_stem().and_then(|s| s.to_str()) else {
        return;
    };
    let Some(parent) = archive_path.parent() else {
        return;
    };
    let forensics_dir = parent.join(format!("{stem}-forensics"));
    if fs::create_dir_all(&forensics_dir).is_err() {
        return;
    }
    for rel in ["events.jsonl", "derived/atif-trajectory.json"] {
        let src = session_dir.join(rel);
        if !src.is_file() {
            continue;
        }
        let dest_name = rel.replace('/', "__");
        let _ = fs::copy(&src, forensics_dir.join(dest_name));
    }
}

/// Pin the session directory against retention while the blocker is open.
fn pin_blocker_session_retention(workspace: &Path, session_id: &str) {
    let session_dir = vtcode_memory::session_directory(workspace, session_id);
    if !session_dir.is_dir() {
        return;
    }
    let _ = vtcode_memory::pin_session_retention(&session_dir, "unresolved-blocker");
}

/// Clear retention pins when a blocker archive for `session_id` is resolved.
/// Keeps the pin if another unresolved archive still references the session.
fn unpin_resolved_blocker_session(workspace: &Path, session_id: &str) {
    let Ok((_, _, blockers_dir)) = safe_handoff_directories(workspace) else {
        return;
    };
    if let Ok(entries) = fs::read_dir(&blockers_dir) {
        let resolution_marker = format!("resolved_by_session: {session_id}");
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("md") {
                continue;
            }
            let Ok(content) = fs::read_to_string(&path) else {
                continue;
            };
            if parse_blocked_handoff_content(&content).is_some_and(|info| info.session_id == session_id)
                && !content.lines().any(|line| line.trim() == resolution_marker)
            {
                // Another unresolved archive still references this session.
                return;
            }
        }
    }
    let session_dir = vtcode_memory::session_directory(workspace, session_id);
    let _ = vtcode_memory::unpin_session_retention(&session_dir);
}

/// Parsed information from a blocked handoff file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockedHandoffInfo {
    pub session_id: String,
    pub outcome_code: String,
    pub blocker_summary: String,
    pub created_at: Option<String>,
    pub resume_command: Option<String>,
}

/// Reads `.vtcode/tasks/current_blocked.md` if it exists, parsing the front-matter
/// and blocker summary.
pub fn read_current_blocked_handoff(workspace: &Path) -> Option<BlockedHandoffInfo> {
    let (_, tasks_dir) = safe_handoff_tasks_dir(workspace).ok()?;
    let current_path = tasks_dir.join(CURRENT_BLOCKED_FILE);
    ensure_not_symlink(&current_path).ok()?;
    let content = fs::read_to_string(&current_path).ok()?;
    parse_blocked_handoff_content(&content)
}

/// Fallback when the live pointer is missing but an archived blocker exists
/// for the session (e.g. pointer cleared by a fork or stale workspace).
/// Returns the most recently modified matching archive, if any.
pub fn find_latest_archived_blocker_for_session(workspace: &Path, session_id: &str) -> Option<BlockedHandoffInfo> {
    let (_, _, blockers_dir) = safe_handoff_directories(workspace).ok()?;
    let entries = fs::read_dir(&blockers_dir).ok()?;
    let needle = session_id.to_ascii_lowercase();
    let mut best: Option<(std::time::SystemTime, BlockedHandoffInfo)> = None;
    for entry in entries.flatten() {
        let path = entry.path();
        let file_name = path.file_name()?.to_string_lossy().to_ascii_lowercase();
        if !file_name.contains(&needle) && !needle.contains(file_name.as_str()) {
            // Also match sanitized session prefix (blocker files lowercase the id).
            if !file_name.contains("session-") {
                continue;
            }
            // Fall through to content check: session_id is in front-matter.
        }
        ensure_not_symlink(&path).ok()?;
        let content = fs::read_to_string(&path).ok()?;
        let info = parse_blocked_handoff_content(&content)?;
        if info.session_id != session_id
            && !session_id.contains(&info.session_id)
            && !info.session_id.contains(session_id)
        {
            continue;
        }
        let modified = entry.metadata().and_then(|meta| meta.modified()).ok()?;
        let replace = best.as_ref().is_none_or(|(best_time, _)| modified > *best_time);
        if replace {
            best = Some((modified, info));
        }
    }
    best.map(|(_, info)| info)
}

fn parse_blocked_handoff_content(content: &str) -> Option<BlockedHandoffInfo> {
    let mut lines = content.lines();
    if lines.next()?.trim() != "---" {
        return None;
    }

    let mut session_id = None;
    let mut outcome_code = None;
    let mut created_at = None;
    let mut resume_command = None;

    let mut in_front_matter = true;
    let mut body_lines = Vec::new();

    for line in lines {
        if in_front_matter {
            let trimmed = line.trim();
            if trimmed == "---" {
                in_front_matter = false;
                continue;
            }
            if let Some((key, val)) = trimmed.split_once(':') {
                let key = key.trim();
                let val = val.trim().trim_matches('"').trim_matches('\'').trim().to_string();
                match key {
                    "session_id" => session_id = Some(val),
                    "outcome" => outcome_code = Some(val),
                    "created_at" => created_at = Some(val),
                    "resume_command" => resume_command = Some(val),
                    _ => {}
                }
            }
        } else {
            body_lines.push(line);
        }
    }

    let session_id = session_id?;
    let outcome_code = outcome_code.unwrap_or_else(|| "blocked".to_string());

    let mut blocker_summary = String::new();
    let mut in_summary = false;
    for line in body_lines {
        let trimmed = line.trim();
        if trimmed == "# Blocker Summary" {
            in_summary = true;
            continue;
        }
        if in_summary {
            if trimmed.starts_with('#') {
                break;
            }
            blocker_summary.push_str(line);
            blocker_summary.push('\n');
        }
    }
    let blocker_summary = blocker_summary.trim().to_string();

    Some(BlockedHandoffInfo {
        session_id,
        outcome_code,
        blocker_summary,
        created_at,
        resume_command,
    })
}

fn safe_handoff_tasks_dir(workspace: &Path) -> Result<(PathBuf, PathBuf)> {
    let canonical_workspace =
        canonicalize(workspace).with_context(|| format!("failed to canonicalize {}", workspace.display()))?;
    let tasks_dir = canonical_workspace.join(TASKS_DIR);
    ensure_no_symlinked_components(&canonical_workspace, &tasks_dir)?;
    Ok((canonical_workspace, tasks_dir))
}

fn safe_handoff_directories(workspace: &Path) -> Result<(PathBuf, PathBuf, PathBuf)> {
    let (canonical_workspace, tasks_dir) = safe_handoff_tasks_dir(workspace)?;
    let blockers_dir = tasks_dir.join(BLOCKERS_DIR);
    ensure_no_symlinked_components(&canonical_workspace, &blockers_dir)?;
    Ok((canonical_workspace, tasks_dir, blockers_dir))
}

fn ensure_no_symlinked_components(workspace: &Path, path: &Path) -> Result<()> {
    let relative = path
        .strip_prefix(workspace)
        .with_context(|| format!("handoff path escaped {}", workspace.display()))?;
    let mut current = workspace.to_path_buf();
    for component in relative.components() {
        let Component::Normal(component) = component else {
            anyhow::bail!("handoff path contains an unsafe component: {}", path.display());
        };
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(metadata) => {
                ensure!(
                    !metadata.file_type().is_symlink(),
                    "refusing symlinked handoff directory {}",
                    current.display()
                );
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => break,
            Err(err) => {
                return Err(err).with_context(|| format!("failed to inspect handoff directory {}", current.display()));
            }
        }
    }
    Ok(())
}

fn ensure_not_symlink(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            ensure!(!metadata.file_type().is_symlink(), "refusing symlinked handoff {}", path.display());
            Ok(())
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err).with_context(|| format!("failed to inspect handoff {}", path.display())),
    }
}

fn ensure_safe_handoff_target(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            ensure!(!metadata.file_type().is_symlink(), "refusing symlinked handoff {}", path.display());
            ensure!(metadata.is_file(), "refusing non-file handoff target {}", path.display());
            #[cfg(windows)]
            let is_single_linked = {
                let file = fs::File::open(path)
                    .with_context(|| format!("failed to open handoff target {} for inspection", path.display()))?;
                single_link_file(&file)
            };
            #[cfg(not(windows))]
            let is_single_linked = single_link_file(&metadata);
            ensure!(is_single_linked, "refusing hard-linked handoff target {}", path.display());
            Ok(())
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err).with_context(|| format!("failed to inspect handoff target {}", path.display())),
    }
}

fn write_handoff_file(path: &Path, contents: &str, replace_existing: bool) -> Result<()> {
    if replace_existing {
        return write_replaced_handoff_file(path, contents);
    }

    write_new_handoff_file(path, contents)
}

fn write_new_handoff_file(path: &Path, contents: &str) -> Result<()> {
    let mut options = fs::OpenOptions::new();
    options.write(true);
    options.create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;

        options.custom_flags(libc::O_NOFOLLOW);
    }
    let mut file = options
        .open(path)
        .with_context(|| format!("failed to open handoff {} for writing", path.display()))?;
    file.write_all(contents.as_bytes())
        .with_context(|| format!("failed to write handoff {}", path.display()))?;
    file.sync_data()
        .with_context(|| format!("failed to sync handoff {}", path.display()))?;
    Ok(())
}

fn write_replaced_handoff_file(path: &Path, contents: &str) -> Result<()> {
    ensure_safe_handoff_target(path)?;
    let parent = path.parent().context("handoff target has no parent directory")?;
    let target_name = path.file_name().and_then(|name| name.to_str()).unwrap_or("handoff");
    let temporary_path = parent.join(format!(".{target_name}.{}.tmp", Uuid::new_v4()));

    if let Err(error) = write_new_handoff_file(&temporary_path, contents) {
        let _ = fs::remove_file(&temporary_path);
        return Err(error);
    }

    #[cfg(unix)]
    let replacement = fs::rename(&temporary_path, path);
    #[cfg(not(unix))]
    let replacement = match fs::rename(&temporary_path, path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            ensure_safe_handoff_target(path)?;
            fs::remove_file(path).and_then(|()| fs::rename(&temporary_path, path))
        }
        Err(error) => Err(error),
    };

    if let Err(error) = replacement {
        let _ = fs::remove_file(&temporary_path);
        return Err(error).with_context(|| format!("failed to replace handoff {}", path.display()));
    }
    Ok(())
}

/// Clears `.vtcode/tasks/current_blocked.md` if it exists.
///
/// Returns `Ok(true)` if the file was deleted, or `Ok(false)` if it did not exist.
pub fn clear_current_blocked_handoff(workspace: &Path) -> Result<bool> {
    let (_, tasks_dir) = safe_handoff_tasks_dir(workspace)?;
    let current_path = tasks_dir.join(CURRENT_BLOCKED_FILE);
    ensure_not_symlink(&current_path)?;
    match fs::remove_file(&current_path) {
        Ok(()) => Ok(true),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(err) => Err(err).with_context(|| format!("failed to remove {}", current_path.display())),
    }
}

/// Clears `.vtcode/tasks/current_blocked.md` only when it belongs to `session_id`.
///
/// Missing or malformed handoffs are left intact so one session cannot clear
/// another session's recovery pointer.
pub fn clear_current_blocked_handoff_for_session(workspace: &Path, session_id: &str) -> Result<bool> {
    let (workspace, tasks_dir) = safe_handoff_tasks_dir(workspace)?;
    let current_path = tasks_dir.join(CURRENT_BLOCKED_FILE);
    ensure_not_symlink(&current_path)?;
    let claim_path = current_path.with_file_name(format!(
        ".{CURRENT_BLOCKED_FILE}.{}.{}.resolving",
        std::process::id(),
        Uuid::new_v4()
    ));
    match fs::rename(&current_path, &claim_path) {
        Ok(()) => {}
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(err) => {
            return Err(err).with_context(|| {
                format!("failed to claim blocked handoff {} as {}", current_path.display(), claim_path.display())
            });
        }
    }

    let result = (|| {
        let content = fs::read_to_string(&claim_path)
            .with_context(|| format!("failed to read claimed handoff {}", claim_path.display()))?;
        let Some(info) = parse_blocked_handoff_content(&content) else {
            return Ok(false);
        };
        if info.session_id != session_id {
            return Ok(false);
        }
        mark_archived_handoff_resolved(&workspace, &content, session_id)?;
        fs::remove_file(&claim_path)
            .with_context(|| format!("failed to remove claimed handoff {}", claim_path.display()))?;
        Ok(true)
    })();

    if !matches!(result, Ok(true)) {
        restore_claim_without_overwrite(&claim_path, &current_path)?;
    }
    result
}

/// Restore an unconsumed claim without overwriting a handoff concurrently
/// written by another session. A hard link provides create-if-absent behavior;
/// the private claim can then be unlinked independently.
fn restore_claim_without_overwrite(claim_path: &Path, current_path: &Path) -> Result<()> {
    match fs::hard_link(claim_path, current_path) {
        Ok(()) => {}
        Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(err) => {
            return Err(err)
                .with_context(|| format!("failed to restore blocked handoff claim {}", claim_path.display()));
        }
    };
    fs::remove_file(claim_path)
        .with_context(|| format!("failed to release blocked handoff claim {}", claim_path.display()))
}

fn front_matter_value<'a>(content: &'a str, expected_key: &str) -> Option<&'a str> {
    let mut lines = content.lines();
    if lines.next()?.trim() != "---" {
        return None;
    }
    for line in lines {
        let trimmed = line.trim();
        if trimmed == "---" {
            return None;
        }
        let Some((key, value)) = trimmed.split_once(':') else {
            continue;
        };
        if key.trim() == expected_key {
            return Some(value.trim().trim_matches('"').trim_matches('\''));
        }
    }
    None
}

fn mark_archived_handoff_resolved(workspace: &Path, current_content: &str, session_id: &str) -> Result<()> {
    let archive_file =
        front_matter_value(current_content, "archive_file").context("blocked handoff archive_file is missing")?;
    let archive_component = Path::new(archive_file);
    let mut components = archive_component.components();
    ensure!(
        matches!(components.next(), Some(Component::Normal(_))) && components.next().is_none(),
        "blocked handoff archive_file must be a single path component"
    );

    let (canonical_workspace, _, blockers_dir) = safe_handoff_directories(workspace)?;
    let archive_path = blockers_dir.join(archive_component);
    let canonical_blockers =
        canonicalize(&blockers_dir).with_context(|| format!("failed to canonicalize {}", blockers_dir.display()))?;
    ensure!(
        canonical_blockers.starts_with(&canonical_workspace),
        "blocked handoff blockers directory escaped {}",
        canonical_workspace.display()
    );
    let canonical_archive =
        canonicalize(&archive_path).with_context(|| format!("failed to canonicalize {}", archive_path.display()))?;
    ensure!(
        canonical_archive.parent() == Some(canonical_blockers.as_path()),
        "blocked handoff archive escaped {}",
        canonical_blockers.display()
    );

    let mut archive = open_archive_for_resolution(&canonical_archive)?;
    let metadata = archive
        .metadata()
        .with_context(|| format!("failed to stat {}", canonical_archive.display()))?;
    ensure!(metadata.is_file(), "blocked handoff archive is not a regular file");
    #[cfg(windows)]
    let is_single_linked = single_link_file(&archive);
    #[cfg(not(windows))]
    let is_single_linked = single_link_file(&metadata);
    ensure!(is_single_linked, "blocked handoff archive has unexpected hard links");

    let mut archive_content = String::new();
    archive
        .read_to_string(&mut archive_content)
        .with_context(|| format!("failed to read {}", canonical_archive.display()))?;
    ensure!(
        parse_blocked_handoff_content(&archive_content).is_some_and(|info| info.session_id == session_id),
        "blocked handoff archive session does not match {session_id}"
    );
    let resolution_marker = format!("resolved_by_session: {session_id}");
    if archive_content.lines().any(|line| line.trim() == resolution_marker) {
        // Already resolved: still release the pin when no other unresolved
        // archive references this session.
        unpin_resolved_blocker_session(workspace, session_id);
        return Ok(());
    }

    write!(
        archive,
        "\n# Resolution\n\n{resolution_marker}\nresolved_at: {}\nresolution_scope: blocked turn recovered; task may remain incomplete\n",
        Utc::now().to_rfc3339()
    )
    .with_context(|| format!("failed to append resolution to {}", canonical_archive.display()))?;
    archive
        .sync_data()
        .with_context(|| format!("failed to sync {}", canonical_archive.display()))?;
    unpin_resolved_blocker_session(workspace, session_id);
    Ok(())
}

fn open_archive_for_resolution(path: &Path) -> Result<fs::File> {
    let mut options = fs::OpenOptions::new();
    options.read(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;

        options.custom_flags(libc::O_NOFOLLOW);
    }
    options.open(path).with_context(|| format!("failed to open {}", path.display()))
}

#[cfg(unix)]
fn single_link_file(metadata: &fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;

    metadata.nlink() == 1
}

#[cfg(windows)]
#[expect(
    unsafe_code,
    reason = "Windows has no stable standard-library hard-link count API; query the owning file handle through Win32"
)]
fn single_link_file(file: &fs::File) -> bool {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle};

    let mut information = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: `file` owns a valid handle for the duration of the call and
    // `information` points to writable storage of the expected Win32 type.
    let query_succeeded = unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut information) != 0 };
    query_succeeded && information.nNumberOfLinks == 1
}

#[cfg(not(any(unix, windows)))]
const fn single_link_file(_metadata: &fs::Metadata) -> bool {
    // Refuse resolution on platforms without a stable link-count API rather
    // than appending to a file whose identity cannot be checked.
    false
}

fn render_blocked_handoff(
    workspace: &Path,
    session_id: &str,
    outcome_code: &str,
    blocker_summary: &str,
    handoff_paths: BlockedHandoffPaths<'_>,
    relevant_paths: &[PathBuf],
    created_at: String,
    resume: BlockedHandoffResume<'_>,
    planning_active: bool,
) -> String {
    let mut paths = vec![
        workspace.to_path_buf(),
        handoff_paths.current.to_path_buf(),
        handoff_paths.archive.to_path_buf(),
    ];
    for path in relevant_paths {
        if !paths.iter().any(|existing| existing == path) {
            paths.push(path.clone());
        }
    }

    let relevant_paths_section = paths
        .iter()
        .map(|path| format!("- `{}`", path.display()))
        .collect::<Vec<_>>()
        .join("\n");

    let (resume_front_matter, resume_metadata, resume_actionable) = match resume {
        BlockedHandoffResume::Available(identifier) => (
            format!("resume_command: \"vtcode --resume {}\"\n", identifier.as_str()),
            format!("- Resume command: `vtcode --resume {}`\n", identifier.as_str()),
            format!("- From terminal: Run `vtcode --resume {}`\n", identifier.as_str()),
        ),
        BlockedHandoffResume::Unavailable(explanation) => {
            (String::new(), format!("- Resume unavailable: {}\n", explanation.trim()), String::new())
        }
    };

    let actionable_steps = format!(
        "## Actionable Next Steps\n\n- In this session: Type `continue` to retry with retained history, or provide alternative instructions.\n{resume_actionable}- Archived details: `{}`.\n- Live pointer: `{}` may be cleared after this session recovers successfully.{plan_mode_actionable}",
        handoff_paths.archive.display(),
        handoff_paths.current.display(),
        plan_mode_actionable = if planning_active {
            format!("\n{PLAN_MODE_HANDOFF_GUIDANCE}")
        } else {
            String::new()
        },
    );
    let archive_file = handoff_paths
        .archive
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("unknown");

    format!(
        "---\nsession_id: {session_id}\noutcome: {outcome_code}\ncreated_at: {created_at}\nworkspace: {}\narchive_file: {archive_file}\n{resume_front_matter}---\n\n# Blocker Summary\n\n{}\n\n{}\n\n# Session Tracker Snapshot\n\n_Tracker snapshot unavailable: `.vtcode/tasks/current_task.md` is workspace-global and is not safe to attribute to this session._\n\n# Relevant Paths\n\n{}\n\n# Resume Metadata\n\n- Session ID: `{session_id}`\n- Outcome: `{outcome_code}`\n{resume_metadata}",
        workspace.display(),
        blocker_summary.trim(),
        actionable_steps,
        relevant_paths_section,
    )
}

/// Artifacts produced by [`write_async_approval_blocker`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AsyncApprovalArtifacts {
    /// Path to the async approval blocker markdown file.
    pub current_path: PathBuf,
    /// Unique token used to approve or reject this request via CLI.
    pub approval_token: String,
}

/// Write an async (deferred) approval blocker file.
///
/// Unlike [`write_blocked_handoff`] which signals a hard stop, this writes a
/// blocker that can be resolved out-of-band via CLI (`vtcode approve <token>`).
/// The blocker includes the approval question, tool details, and a unique token.
pub fn write_async_approval_blocker(
    workspace: &Path,
    session_id: &str,
    approval_question: &str,
    tool_name: &str,
    args: &serde_json::Value,
    estimated_cost: Option<f64>,
    notify_command: Option<&str>,
) -> Result<AsyncApprovalArtifacts> {
    let (_, _, blockers_dir) = safe_handoff_directories(workspace)?;
    fs::create_dir_all(&blockers_dir)
        .with_context(|| format!("failed to create blockers dir {}", blockers_dir.display()))?;

    let approval_token = Uuid::new_v4().to_string();
    let timestamp = Utc::now();
    let archive_name = format!(
        "async-{}-{}-{}.md",
        sanitize_debug_component(session_id, "session"),
        timestamp.format("%Y%m%dT%H%M%SZ"),
        Uuid::new_v4()
    );
    let current_path = blockers_dir.join(archive_name);

    let cost_line = estimated_cost.map(|c| format!("Estimated cost: ${c:.4}")).unwrap_or_default();

    let notify_line = notify_command.map(|cmd| format!("Notify command: `{cmd}`")).unwrap_or_default();

    let markdown = format!(
        "---\ntoken: {approval_token}\nsession_id: {session_id}\ntool: {tool_name}\ncreated_at: {created_at}\ntype: async_approval\n---\n\n\
         # Async Approval Request\n\n\
         ## Question\n\n{approval_question}\n\n\
         ## Tool\n- Name: `{tool_name}`\n- Arguments: ```json\n{args_json}\n```\n\
         {cost_line}\n{notify_line}\n\n\
         ## How to Approve\n\n\
         ```\nvtcode approve {approval_token}\nvtcode reject {approval_token}\nvtcode approve list\n```\n",
        created_at = timestamp.to_rfc3339(),
        args_json = serde_json::to_string_pretty(args).unwrap_or_else(|_| args.to_string()),
    );

    write_handoff_file(&current_path, &markdown, false)?;

    Ok(AsyncApprovalArtifacts { current_path, approval_token })
}

#[cfg(test)]
mod tests {
    use crate::utils::session_archive::VerifiedSessionArchiveIdentifier;

    use super::*;

    #[test]
    fn writes_current_and_archived_blocked_handoffs() {
        let temp = tempfile::TempDir::new().expect("temp");
        // Provide a session store so forensics/pin paths resolve.
        let session_dir = vtcode_memory::session_directory(temp.path(), "session-a");
        fs::create_dir_all(session_dir.join("derived")).expect("session dir");
        fs::write(session_dir.join("events.jsonl"), "{\"type\":\"thread.started\"}\n").expect("events");
        fs::write(session_dir.join("derived/atif-trajectory.json"), "{\"schema_version\":\"ATIF-v1.4\",\"steps\":[]}")
            .expect("atif");

        let artifacts =
            write_blocked_handoff(temp.path(), "session-a", "blocked", "first", &[]).expect("write first handoff");

        assert!(artifacts.archive_path.is_file());
        let stem = artifacts.archive_path.file_stem().unwrap().to_string_lossy().to_string();
        let forensics = artifacts
            .archive_path
            .parent()
            .expect("parent")
            .join(format!("{stem}-forensics"));
        assert!(forensics.join("events.jsonl").is_file(), "forensics events copy missing");
        assert!(forensics.join("derived__atif-trajectory.json").is_file(), "forensics ATIF copy missing");
        assert!(
            vtcode_memory::session_retention_pinned(&session_dir),
            "unresolved blocker must pin session retention"
        );

        let artifacts2 =
            write_blocked_handoff(temp.path(), "session-a", "blocked", "second", &[]).expect("write second handoff");
        assert_ne!(artifacts.archive_path, artifacts2.archive_path);
    }

    #[test]
    fn resolution_clears_retention_pin_when_no_other_open_blocker() {
        let temp = tempfile::TempDir::new().expect("temp");
        let session_dir = vtcode_memory::session_directory(temp.path(), "session-pin");
        fs::create_dir_all(&session_dir).expect("session dir");

        let artifacts =
            write_blocked_handoff(temp.path(), "session-pin", "blocked", "stall", &[]).expect("write handoff");
        assert!(vtcode_memory::session_retention_pinned(&session_dir));

        clear_current_blocked_handoff_for_session(temp.path(), "session-pin").expect("resolve");
        assert!(
            !vtcode_memory::session_retention_pinned(&session_dir),
            "resolution must clear retention pin when no other unresolved archive remains"
        );
        assert!(artifacts.archive_path.is_file());
    }

    #[test]
    fn blocked_handoff_archives_have_unique_paths() {
        let temp = tempfile::tempdir().expect("temp dir");

        let first =
            write_blocked_handoff(temp.path(), "session-a", "blocked", "first", &[]).expect("write first handoff");
        let second =
            write_blocked_handoff(temp.path(), "session-a", "blocked", "second", &[]).expect("write second handoff");

        assert_ne!(first.archive_path, second.archive_path);
        assert!(first.archive_path.exists());
        assert!(second.archive_path.exists());
        assert!(
            fs::read_to_string(first.archive_path)
                .expect("read first archive")
                .contains("first")
        );
    }

    #[test]
    fn replacing_current_handoff_does_not_leave_temporary_files() {
        let temp = tempfile::tempdir().expect("temp dir");
        let tasks_dir = temp.path().join(TASKS_DIR);
        fs::create_dir_all(&tasks_dir).expect("tasks dir");
        let current = tasks_dir.join(CURRENT_BLOCKED_FILE);
        fs::write(&current, "old handoff").expect("write old handoff");

        write_handoff_file(&current, "new handoff", true).expect("replace handoff");

        assert_eq!(fs::read_to_string(&current).expect("read replacement"), "new handoff");
        assert_eq!(
            fs::read_dir(tasks_dir)
                .expect("read tasks dir")
                .filter_map(Result::ok)
                .filter(|entry| entry.file_name().to_string_lossy().starts_with(".current_blocked.md."))
                .count(),
            0
        );
    }

    #[test]
    fn async_approval_blockers_have_unique_paths() {
        let temp = tempfile::tempdir().expect("temp dir");
        let first = write_async_approval_blocker(
            temp.path(),
            "session-a",
            "approve the first request",
            "exec_command",
            &serde_json::json!({"command": "true"}),
            None,
            None,
        )
        .expect("write first approval blocker");
        let second = write_async_approval_blocker(
            temp.path(),
            "session-a",
            "approve the second request",
            "exec_command",
            &serde_json::json!({"command": "false"}),
            None,
            None,
        )
        .expect("write second approval blocker");

        assert_ne!(first.current_path, second.current_path);
        assert!(first.current_path.exists());
        assert!(second.current_path.exists());
    }

    #[test]
    fn writes_blocked_handoff_without_resume_when_archive_is_unavailable() {
        let temp = tempfile::tempdir().expect("temp dir");

        let artifacts = write_blocked_handoff_with_resume(
            temp.path(),
            "runtime-session",
            "blocked",
            "History persistence is disabled.",
            &[temp.path().join("src/lib.rs")],
            BlockedHandoffResume::Unavailable("Resume is unavailable because the session archive was not persisted."),
            false,
        )
        .expect("write handoff");

        let current = fs::read_to_string(&artifacts.current_path).expect("current handoff");
        assert!(!current.contains("resume_command:"));
        assert!(!current.contains("vtcode --resume"));
        assert!(current.contains("Resume is unavailable because the session archive was not persisted."));
    }

    #[test]
    fn uses_verified_archive_identifier_for_resume_command() {
        let temp = tempfile::tempdir().expect("temp dir");

        let verified_identifier = VerifiedSessionArchiveIdentifier("session-archive-id".to_owned());
        let artifacts = write_blocked_handoff_with_resume(
            temp.path(),
            "runtime-session",
            "blocked",
            "Execution stalled on a loop.",
            &[],
            BlockedHandoffResume::Available(&verified_identifier),
            false,
        )
        .expect("write handoff");

        let current = fs::read_to_string(&artifacts.current_path).expect("current handoff");
        assert!(current.contains("vtcode --resume session-archive-id"));
        assert!(!current.contains("vtcode --resume runtime-session"));
    }

    #[test]
    fn planning_handoff_carries_mode_guidance_in_actionable_steps() {
        let temp = tempfile::tempdir().expect("temp dir");

        let planning = write_blocked_handoff_with_resume(
            temp.path(),
            "plan-session",
            "blocked",
            "Mutation blocked until verification: 1 mutating command(s) await a verifier.",
            &[],
            BlockedHandoffResume::Unavailable("Resume unavailable in this test."),
            true,
        )
        .expect("write planning handoff");
        let planning_content = fs::read_to_string(&planning.current_path).expect("planning handoff");
        assert!(planning_content.contains("Plan mode was active (read-only)"));
        assert!(planning_content.contains("/mode build"));
        // Both the live pointer and the archive carry the same markdown.
        let planning_archive = fs::read_to_string(&planning.archive_path).expect("planning archive");
        assert!(planning_archive.contains("Plan mode was active (read-only)"));

        let non_planning = write_blocked_handoff_with_resume(
            temp.path(),
            "build-session",
            "blocked",
            "Mutation blocked until verification: 1 mutating command(s) await a verifier.",
            &[],
            BlockedHandoffResume::Unavailable("Resume unavailable in this test."),
            false,
        )
        .expect("write non-planning handoff");
        let non_planning_content = fs::read_to_string(&non_planning.current_path).expect("non-planning handoff");
        assert!(!non_planning_content.contains("Plan mode was active (read-only)"));
    }

    #[test]
    fn write_async_approval_blocker_creates_file_with_token() {
        let temp = tempfile::tempdir().expect("temp dir");
        let tasks_dir = temp.path().join(".vtcode/tasks");
        fs::create_dir_all(&tasks_dir).expect("tasks dir");

        let artifacts = write_async_approval_blocker(
            temp.path(),
            "session-456",
            "Push 50 commits to main?",
            "git_push",
            &serde_json::json!({"force": true, "branch": "main"}),
            Some(0.50),
            Some("/usr/local/bin/notify"),
        )
        .expect("write async blocker");

        assert!(!artifacts.approval_token.is_empty());
        assert!(artifacts.current_path.exists());

        let content = fs::read_to_string(&artifacts.current_path).expect("read blocker");
        assert!(content.contains("Push 50 commits to main?"));
        assert!(content.contains("git_push"));
        assert!(content.contains("Estimated cost: $0.50"));
        assert!(content.contains("vtcode approve"));
        assert!(content.contains(&artifacts.approval_token));
    }

    #[test]
    fn write_async_approval_blocker_handles_minimal_input() {
        let temp = tempfile::tempdir().expect("temp dir");
        let tasks_dir = temp.path().join(".vtcode/tasks");
        fs::create_dir_all(&tasks_dir).expect("tasks dir");

        let artifacts = write_async_approval_blocker(
            temp.path(),
            "session-789",
            "Delete the file?",
            "delete_file",
            &serde_json::json!({"path": "/tmp/x"}),
            None,
            None,
        )
        .expect("write async blocker");

        assert!(!artifacts.approval_token.is_empty());
        assert!(artifacts.current_path.exists());

        let content = fs::read_to_string(&artifacts.current_path).expect("read blocker");
        assert!(content.contains("Delete the file?"));
        assert!(content.contains("delete_file"));
        // No cost or notify section
        assert!(!content.contains("Estimated cost:"));
        assert!(!content.contains("Notify command:"));
    }

    #[test]
    fn test_read_and_clear_current_blocked_handoff() {
        let temp = tempfile::tempdir().expect("temp dir");

        // When file does not exist
        assert_eq!(read_current_blocked_handoff(temp.path()), None);
        assert!(!clear_current_blocked_handoff(temp.path()).unwrap());

        // Write a blocked handoff
        let verified_identifier = VerifiedSessionArchiveIdentifier("session-archive-id".to_owned());
        let _artifacts = write_blocked_handoff_with_resume(
            temp.path(),
            "test-session-123",
            "blocked",
            "Tool call failed repeatedly with permission errors.",
            &[],
            BlockedHandoffResume::Available(&verified_identifier),
            false,
        )
        .expect("write handoff");

        // Read it back
        let info = read_current_blocked_handoff(temp.path()).expect("read info");
        assert_eq!(info.session_id, "test-session-123");
        assert_eq!(info.outcome_code, "blocked");
        assert_eq!(info.blocker_summary, "Tool call failed repeatedly with permission errors.");
        assert!(info.created_at.is_some());
        assert_eq!(info.resume_command.as_deref(), Some("vtcode --resume session-archive-id"));

        // Clear it
        assert!(clear_current_blocked_handoff(temp.path()).unwrap());
        // Should no longer exist
        assert_eq!(read_current_blocked_handoff(temp.path()), None);
        assert!(!clear_current_blocked_handoff(temp.path()).unwrap());
    }

    #[test]
    fn session_scoped_clear_preserves_another_sessions_handoff() {
        let temp = tempfile::tempdir().expect("temp dir");
        let artifacts =
            write_blocked_handoff(temp.path(), "session-a", "blocked", "stalled", &[]).expect("write handoff");

        assert!(!clear_current_blocked_handoff_for_session(temp.path(), "session-b").expect("scoped clear"));
        assert_eq!(
            read_current_blocked_handoff(temp.path()).map(|info| info.session_id),
            Some("session-a".to_string())
        );
        assert!(clear_current_blocked_handoff_for_session(temp.path(), "session-a").expect("scoped clear"));
        assert_eq!(read_current_blocked_handoff(temp.path()), None);
        let archive = fs::read_to_string(artifacts.archive_path).expect("read resolved archive");
        assert!(archive.contains("# Resolution"));
        assert!(archive.contains("resolved_by_session: session-a"));
        assert!(
            archive.contains("resolution_scope: blocked turn recovered; task may remain incomplete"),
            "resolution states turn scope, not task completion"
        );
    }

    #[test]
    fn session_scoped_clear_preserves_malformed_handoff() {
        let temp = tempfile::tempdir().expect("temp dir");
        let tasks_dir = temp.path().join(TASKS_DIR);
        fs::create_dir_all(&tasks_dir).expect("tasks dir");
        let current = tasks_dir.join(CURRENT_BLOCKED_FILE);
        fs::write(&current, "not a handoff").expect("write malformed handoff");

        assert!(!clear_current_blocked_handoff_for_session(temp.path(), "session-a").expect("scoped clear"));
        assert!(current.exists());
    }

    #[test]
    fn session_scoped_clear_rejects_archive_path_traversal() {
        let temp = tempfile::tempdir().expect("temp dir");
        let tasks_dir = temp.path().join(TASKS_DIR);
        fs::create_dir_all(&tasks_dir).expect("tasks dir");
        let current = tasks_dir.join(CURRENT_BLOCKED_FILE);
        fs::write(
            &current,
            "---\nsession_id: session-a\noutcome: blocked\narchive_file: ../../outside.md\n---\n\n# Blocker Summary\n\nstalled\n",
        )
        .expect("write traversal handoff");

        assert!(clear_current_blocked_handoff_for_session(temp.path(), "session-a").is_err());
        assert!(current.exists());
        assert!(!temp.path().join("outside.md").exists());
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn session_scoped_clear_rejects_hardlinked_archive() {
        let temp = tempfile::tempdir().expect("temp dir");
        let artifacts =
            write_blocked_handoff(temp.path(), "session-a", "blocked", "stalled", &[]).expect("write handoff");
        let external = temp.path().join("outside.md");
        fs::copy(&artifacts.archive_path, &external).expect("copy archive");
        fs::remove_file(&artifacts.archive_path).expect("remove original archive");
        fs::hard_link(&external, &artifacts.archive_path).expect("create hard link");

        assert!(clear_current_blocked_handoff_for_session(temp.path(), "session-a").is_err());
        assert!(read_current_blocked_handoff(temp.path()).is_some());
        assert!(
            !fs::read_to_string(external)
                .expect("read external file")
                .contains("# Resolution")
        );
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn write_blocked_handoff_rejects_hardlinked_current_pointer() {
        let temp = tempfile::tempdir().expect("workspace temp dir");
        let tasks_dir = temp.path().join(TASKS_DIR);
        fs::create_dir_all(&tasks_dir).expect("tasks dir");
        let external = temp.path().join("external.md");
        fs::write(&external, "must remain unchanged").expect("external file");
        fs::hard_link(&external, tasks_dir.join(CURRENT_BLOCKED_FILE)).expect("hard link current pointer");

        assert!(write_blocked_handoff(temp.path(), "session-a", "blocked", "stalled", &[]).is_err());
        assert_eq!(fs::read_to_string(external).expect("read external file"), "must remain unchanged");
    }

    #[cfg(unix)]
    #[test]
    fn session_scoped_clear_rejects_blockers_directory_outside_workspace() {
        let temp = tempfile::tempdir().expect("temp dir");
        let artifacts =
            write_blocked_handoff(temp.path(), "session-a", "blocked", "stalled", &[]).expect("write handoff");
        let blockers_dir = temp.path().join(TASKS_DIR).join(BLOCKERS_DIR);
        let outside_temp = tempfile::tempdir().expect("outside temp dir");
        let outside_dir = outside_temp.path().join("outside-blockers");
        fs::create_dir(&outside_dir).expect("outside directory");
        let outside_archive = outside_dir.join(artifacts.archive_path.file_name().expect("archive file name"));
        fs::rename(&artifacts.archive_path, &outside_archive).expect("move archive outside");
        fs::remove_dir(&blockers_dir).expect("remove blockers directory");
        std::os::unix::fs::symlink(&outside_dir, &blockers_dir).expect("link blockers directory");

        assert!(clear_current_blocked_handoff_for_session(temp.path(), "session-a").is_err());
        assert!(read_current_blocked_handoff(temp.path()).is_some());
        assert!(
            !fs::read_to_string(outside_archive)
                .expect("read outside archive")
                .contains("# Resolution")
        );
    }

    #[cfg(unix)]
    #[test]
    fn handoff_paths_reject_symlinked_vtcode_parent() {
        let temp = tempfile::tempdir().expect("workspace temp dir");
        let outside = tempfile::tempdir().expect("outside temp dir");
        let outside_tasks = outside.path().join(TASKS_DIR);
        fs::create_dir_all(&outside_tasks).expect("outside tasks dir");
        std::os::unix::fs::symlink(outside.path().join(".vtcode"), temp.path().join(".vtcode"))
            .expect("symlink vtcode parent");

        assert!(write_blocked_handoff(temp.path(), "session-a", "blocked", "stalled", &[]).is_err());
        assert!(
            write_async_approval_blocker(
                temp.path(),
                "session-a",
                "approve this",
                "exec_command",
                &serde_json::json!({}),
                None,
                None,
            )
            .is_err()
        );
        assert!(clear_current_blocked_handoff(temp.path()).is_err());
        assert!(!outside_tasks.join(CURRENT_BLOCKED_FILE).exists());
    }

    #[test]
    fn restoring_claim_does_not_overwrite_concurrent_handoff() {
        let temp = tempfile::tempdir().expect("temp dir");
        let current = temp.path().join(CURRENT_BLOCKED_FILE);
        let claim = temp.path().join(".current_blocked.md.claim");
        fs::write(&claim, "session-a").expect("write claim");
        fs::write(&current, "session-b").expect("write replacement");

        restore_claim_without_overwrite(&claim, &current).expect("restore without overwrite");

        assert_eq!(fs::read_to_string(&current).expect("read current"), "session-b");
        assert!(!claim.exists());
    }
}
