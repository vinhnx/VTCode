use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::Utc;
use uuid::Uuid;

use crate::utils::session_debug::sanitize_debug_component;

const TASKS_DIR: &str = ".vtcode/tasks";
const CURRENT_BLOCKED_FILE: &str = "current_blocked.md";
const BLOCKERS_DIR: &str = "blockers";
const CURRENT_TASK_FILE: &str = "current_task.md";

/// Artifacts produced by [`write_blocked_handoff`], containing paths to the
/// current and archived handoff files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockedHandoffArtifacts {
    /// Path to the current blocked handoff markdown file.
    pub current_path: PathBuf,
    /// Path to the archived blocked handoff markdown file.
    pub archive_path: PathBuf,
}

/// Write a blocked-handoff artifact when the agent hits an unrecoverable blocker.
///
/// Creates both a `current_blocked.md` file and a timestamped archive under
/// `.vtcode/tasks/blockers/`. The handoff includes the blocker summary, current
/// tracker snapshot, and a resume command.
pub fn write_blocked_handoff(
    workspace: &Path,
    session_id: &str,
    outcome_code: &str,
    blocker_summary: &str,
    relevant_paths: &[PathBuf],
) -> Result<BlockedHandoffArtifacts> {
    let tasks_dir = workspace.join(TASKS_DIR);
    let blockers_dir = tasks_dir.join(BLOCKERS_DIR);
    fs::create_dir_all(&blockers_dir)
        .with_context(|| format!("failed to create blockers dir {}", blockers_dir.display()))?;

    let tracker_path = tasks_dir.join(CURRENT_TASK_FILE);
    let current_path = tasks_dir.join(CURRENT_BLOCKED_FILE);
    let timestamp = Utc::now();
    let archive_name =
        format!("{}-{}.md", sanitize_debug_component(session_id, "session"), timestamp.format("%Y%m%dT%H%M%SZ"));
    let archive_path = blockers_dir.join(archive_name);

    let markdown = render_blocked_handoff(
        workspace,
        session_id,
        outcome_code,
        blocker_summary,
        &tracker_path,
        &current_path,
        &archive_path,
        relevant_paths,
        timestamp.to_rfc3339(),
    );

    fs::write(&current_path, &markdown).with_context(|| format!("failed to write {}", current_path.display()))?;
    fs::write(&archive_path, markdown).with_context(|| format!("failed to write {}", archive_path.display()))?;

    Ok(BlockedHandoffArtifacts { current_path, archive_path })
}

fn render_blocked_handoff(
    workspace: &Path,
    session_id: &str,
    outcome_code: &str,
    blocker_summary: &str,
    tracker_path: &Path,
    current_path: &Path,
    archive_path: &Path,
    relevant_paths: &[PathBuf],
    created_at: String,
) -> String {
    let tracker_snapshot = fs::read_to_string(tracker_path)
        .ok()
        .filter(|content| !content.trim().is_empty())
        .unwrap_or_else(|| "_No current tracker snapshot found._".to_string());

    let mut paths = vec![
        workspace.to_path_buf(),
        tracker_path.to_path_buf(),
        current_path.to_path_buf(),
        archive_path.to_path_buf(),
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

    format!(
        "---\nsession_id: {session_id}\noutcome: {outcome_code}\ncreated_at: {created_at}\nworkspace: {}\nresume_command: \"vtcode --resume {session_id}\"\n---\n\n# Blocker Summary\n\n{}\n\n# Current Tracker Snapshot\n\n{}\n\n# Relevant Paths\n\n{}\n\n# Resume Metadata\n\n- Session ID: `{session_id}`\n- Outcome: `{outcome_code}`\n- Resume command: `vtcode --resume {session_id}`\n",
        workspace.display(),
        blocker_summary.trim(),
        tracker_snapshot,
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
    let tasks_dir = workspace.join(TASKS_DIR);
    let blockers_dir = tasks_dir.join(BLOCKERS_DIR);
    fs::create_dir_all(&blockers_dir)
        .with_context(|| format!("failed to create blockers dir {}", blockers_dir.display()))?;

    let approval_token = Uuid::new_v4().to_string();
    let timestamp = Utc::now();
    let archive_name = format!(
        "async-{}-{}.md",
        sanitize_debug_component(session_id, "session"),
        timestamp.format("%Y%m%dT%H%M%SZ")
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

    fs::write(&current_path, &markdown)
        .with_context(|| format!("failed to write async blocker {}", current_path.display()))?;

    Ok(AsyncApprovalArtifacts { current_path, approval_token })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_current_and_archived_blocked_handoffs() {
        let temp = tempfile::tempdir().expect("temp dir");
        let tasks_dir = temp.path().join(".vtcode/tasks");
        fs::create_dir_all(&tasks_dir).expect("tasks dir");
        fs::write(tasks_dir.join("current_task.md"), "# Current Task\n\n- [ ] investigate blocker\n").expect("tracker");

        let artifacts = write_blocked_handoff(
            temp.path(),
            "session-123",
            "loop_detected",
            "Execution stalled on a loop.",
            &[temp.path().join("src/lib.rs")],
        )
        .expect("write handoff");

        let current = fs::read_to_string(&artifacts.current_path).expect("current handoff");
        let archive = fs::read_to_string(&artifacts.archive_path).expect("archive handoff");

        assert_eq!(current, archive);
        assert!(current.contains("session_id: session-123"));
        assert!(current.contains("# Blocker Summary"));
        assert!(current.contains("Execution stalled on a loop."));
        assert!(current.contains("# Current Task"));
        assert!(current.contains("vtcode --resume session-123"));
        assert!(current.contains("src/lib.rs"));
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
}
