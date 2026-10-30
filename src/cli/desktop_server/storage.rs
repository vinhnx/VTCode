use super::*;
use vtcode_commons::VtCodePaths;
use vtcode_core::git::WorktreeManager;
use vtcode_core::utils::session_archive::{delete_session_archive, sessions_dir};

/// Caps for storage accounting walks; worktrees hold full checkouts.
const MAX_WALK_ENTRIES: usize = 200_000;
const MAX_WALK_DEPTH: usize = 64;

pub(super) async fn handle_storage_request(context: &ServiceContext, request: &Request) -> Result<(Value, bool)> {
    match request.method.as_str() {
        protocol::method::STORAGE_SUMMARY => {
            let summary = storage_summary(&context.workspace).await?;
            Ok((serde_json::to_value(summary)?, false))
        }
        protocol::method::SESSION_DELETE => {
            let params: protocol::SessionIdParams = parse_params(&request.params)?;
            if context.state.lock().await.session_id.as_deref() == Some(params.session_id.as_str()) {
                bail!("cannot delete the selected session");
            }
            let deleted = delete_session_archive(&params.session_id).await?;
            let result = protocol::SessionDeleteResult { session_id: params.session_id, deleted };
            Ok((serde_json::to_value(result)?, false))
        }
        protocol::method::WORKTREE_LIST => {
            let workspace = context.workspace.clone();
            let entries = tokio::task::spawn_blocking(move || list_worktrees(&workspace)).await??;
            Ok((serde_json::to_value(entries)?, false))
        }
        protocol::method::WORKTREE_REMOVE => {
            let params: protocol::WorktreeRemoveParams = parse_params(&request.params)?;
            let task_workspace = context.workspace.clone();
            let task_name = params.name.clone();
            tokio::task::spawn_blocking(move || WorktreeManager::new(task_workspace).remove(&task_name)).await??;
            let result = protocol::WorktreeRemoveResult { name: params.name, removed: true };
            Ok((serde_json::to_value(result)?, false))
        }
        _ => bail!("unsupported storage method {}", request.method),
    }
}

async fn storage_summary(workspace: &Path) -> Result<protocol::StorageSummary> {
    let sessions = sessions_dir().await?;
    let state_dir = VtCodePaths::resolve()?.ensure_state_dir()?.to_path_buf();
    let workspace = workspace.to_path_buf();
    tokio::task::spawn_blocking(move || {
        let sessions_usage = dir_usage(&sessions, 1, None);
        let worktrees_dir = workspace.join(".vtcode").join("worktrees");
        let worktrees_bytes = dir_usage(&worktrees_dir, MAX_WALK_DEPTH, None).bytes;
        let memory_bytes = dir_usage(&workspace.join(".vtcode").join("sessions"), MAX_WALK_DEPTH, None).bytes;
        let other_bytes = dir_usage(&state_dir, MAX_WALK_DEPTH, Some(&sessions)).bytes;
        let total_bytes = sessions_usage
            .bytes
            .saturating_add(worktrees_bytes)
            .saturating_add(memory_bytes)
            .saturating_add(other_bytes);
        Ok(protocol::StorageSummary {
            state_dir: state_dir.display().to_string(),
            workspace: workspace.display().to_string(),
            sessions_bytes: sessions_usage.bytes,
            sessions_count: sessions_usage.files,
            worktrees_bytes,
            worktrees_count: child_dir_count(&worktrees_dir),
            memory_bytes,
            other_bytes,
            total_bytes,
        })
    })
    .await?
}

fn list_worktrees(workspace: &Path) -> Result<Vec<protocol::WorktreeEntry>> {
    Ok(WorktreeManager::new(workspace)
        .list()?
        .into_iter()
        .map(|info| protocol::WorktreeEntry {
            name: info.name,
            head: info.head,
            is_dirty: info.is_dirty,
        })
        .collect())
}

struct DirUsage {
    bytes: u64,
    files: u64,
}

/// Bounded recursive size walk. Symlinks are never followed and unreadable
/// entries are skipped, matching the fail-soft retention scans.
fn dir_usage(root: &Path, max_depth: usize, skip: Option<&Path>) -> DirUsage {
    let mut usage = DirUsage { bytes: 0, files: 0 };
    let mut budgeted = 0usize;
    let mut stack = vec![(root.to_path_buf(), 0usize)];
    while let Some((dir, depth)) = stack.pop() {
        if depth >= max_depth || skip.is_some_and(|skipped| dir == skipped) {
            continue;
        }
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        for entry in entries {
            if budgeted >= MAX_WALK_ENTRIES {
                return usage;
            }
            budgeted += 1;
            let entry = match entry {
                Ok(entry) => entry,
                Err(_) => continue,
            };
            let metadata = match std::fs::symlink_metadata(entry.path()) {
                Ok(metadata) => metadata,
                Err(_) => continue,
            };
            if metadata.file_type().is_symlink() {
                continue;
            }
            if metadata.is_dir() {
                stack.push((entry.path(), depth + 1));
            } else if metadata.is_file() {
                usage.bytes = usage.bytes.saturating_add(metadata.len());
                usage.files = usage.files.saturating_add(1);
            }
        }
    }
    usage
}

fn child_dir_count(dir: &Path) -> u64 {
    let mut count = 0u64;
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            if std::fs::symlink_metadata(entry.path()).is_ok_and(|metadata| metadata.is_dir()) {
                count = count.saturating_add(1);
            }
        }
    }
    count
}
