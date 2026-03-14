use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::Utc;

use crate::utils::session_debug::sanitize_debug_component;

const TASKS_DIR: &str = ".vtcode/tasks";
const CURRENT_BLOCKED_FILE: &str = "current_blocked.md";
const BLOCKERS_DIR: &str = "blockers";
const CURRENT_TASK_FILE: &str = "current_task.md";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockedHandoffArtifacts {
    pub current_path: PathBuf,
    pub archive_path: PathBuf,
}

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
    let archive_name = format!(
        "{}-{}.md",
        sanitize_debug_component(session_id, "session"),
        timestamp.format("%Y%m%dT%H%M%SZ")
    );
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

    fs::write(&current_path, &markdown)
        .with_context(|| format!("failed to write {}", current_path.display()))?;
    fs::write(&archive_path, markdown)
        .with_context(|| format!("failed to write {}", archive_path.display()))?;

    Ok(BlockedHandoffArtifacts {
        current_path,
        archive_path,
    })
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_current_and_archived_blocked_handoffs() {
        let temp = tempfile::tempdir().expect("temp dir");
        let tasks_dir = temp.path().join(".vtcode/tasks");
        fs::create_dir_all(&tasks_dir).expect("tasks dir");
        fs::write(
            tasks_dir.join("current_task.md"),
            "# Current Task\n\n- [ ] investigate blocker\n",
        )
        .expect("tracker");

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
}
