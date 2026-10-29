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
mod tests;
