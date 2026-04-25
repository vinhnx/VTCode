use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

use super::constants::SUBAGENT_PREVIEW_LINES;
use super::types::{PersistedBackgroundRecord, PersistedBackgroundState};
use crate::utils::session_archive::{SessionListing, SessionSnapshot};

const BACKGROUND_DEMO_AGENT: &str = "background-demo";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BackgroundLaunchSpec {
    pub command: Vec<String>,
    pub use_pty: bool,
}

// ─── Background State Persistence ──────────────────────────────────────────

pub(crate) fn background_state_path(workspace_root: &Path) -> PathBuf {
    workspace_root
        .join(".vtcode")
        .join("state")
        .join("background_subagents.json")
}

pub(crate) fn load_background_state(workspace_root: &Path) -> Result<PersistedBackgroundState> {
    let path = background_state_path(workspace_root);
    if !path.exists() {
        return Ok(PersistedBackgroundState::default());
    }

    let raw = std::fs::read_to_string(&path)
        .with_context(|| format!("Failed to read {}", path.display()))?;
    serde_json::from_str(&raw).with_context(|| format!("Failed to parse {}", path.display()))
}

pub(crate) fn persist_background_state(
    workspace_root: &Path,
    records: Vec<PersistedBackgroundRecord>,
) -> Result<()> {
    let path = background_state_path(workspace_root);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create {}", parent.display()))?;
    }
    let payload = serde_json::to_string_pretty(&PersistedBackgroundState { records })?;
    std::fs::write(&path, payload)
        .with_context(|| format!("Failed to write {}", path.display()))?;
    Ok(())
}

// ─── Background Command Building ───────────────────────────────────────────

pub(crate) fn build_background_launch_spec(
    workspace_root: &Path,
    agent_name: &str,
    parent_session_id: &str,
    session_id: &str,
    prompt: &str,
    max_turns: Option<usize>,
    model_override: Option<&str>,
    reasoning_override: Option<&str>,
) -> Result<BackgroundLaunchSpec> {
    if agent_name == BACKGROUND_DEMO_AGENT {
        let script = workspace_root
            .join("scripts")
            .join("demo-background-subagent.sh");
        return Ok(BackgroundLaunchSpec {
            command: vec![script.to_string_lossy().into_owned()],
            use_pty: false,
        });
    }

    Ok(BackgroundLaunchSpec {
        command: build_background_subagent_command(
            workspace_root,
            agent_name,
            parent_session_id,
            session_id,
            prompt,
            max_turns,
            model_override,
            reasoning_override,
        )?,
        use_pty: true,
    })
}

pub fn build_background_subagent_command(
    workspace_root: &Path,
    agent_name: &str,
    parent_session_id: &str,
    session_id: &str,
    prompt: &str,
    max_turns: Option<usize>,
    model_override: Option<&str>,
    reasoning_override: Option<&str>,
) -> Result<Vec<String>> {
    let executable = std::env::current_exe().context("Failed to resolve current vtcode binary")?;
    let executable =
        resolve_background_subagent_executable_for_workspace(workspace_root, &executable);
    let mut command = vec![
        executable.to_string_lossy().into_owned(),
        "background-subagent".to_string(),
        "--workspace".to_string(),
        workspace_root.to_string_lossy().into_owned(),
        "--agent-name".to_string(),
        agent_name.to_string(),
        "--parent-session-id".to_string(),
        parent_session_id.to_string(),
        "--session-id".to_string(),
        session_id.to_string(),
        "--prompt".to_string(),
        prompt.to_string(),
    ];

    if let Some(max_turns) = max_turns {
        command.push("--max-turns".to_string());
        command.push(max_turns.to_string());
    }
    if let Some(model_override) = model_override
        && !model_override.trim().is_empty()
    {
        command.push("--model-override".to_string());
        command.push(model_override.to_string());
    }
    if let Some(reasoning_override) = reasoning_override
        && !reasoning_override.trim().is_empty()
    {
        command.push("--reasoning-override".to_string());
        command.push(reasoning_override.to_string());
    }

    Ok(command)
}

fn resolve_background_subagent_executable_for_workspace(
    workspace_root: &Path,
    current_exe: &Path,
) -> PathBuf {
    let workspace_target = workspace_root.join("target");
    if current_exe.starts_with(&workspace_target) {
        return current_exe.to_path_buf();
    }

    let binary_name = format!("vtcode{}", std::env::consts::EXE_SUFFIX);
    for profile in ["debug", "release"] {
        let candidate = workspace_target.join(profile).join(&binary_name);
        if candidate.is_file() {
            return candidate;
        }
    }

    current_exe.to_path_buf()
}

pub fn background_record_id(agent_name: &str) -> String {
    format!("background-{}", sanitize_component(agent_name))
}

fn sanitize_component(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

// ─── Preview Utilities ─────────────────────────────────────────────────────

pub fn extract_tail_lines(content: &str, max_lines: usize) -> String {
    let lines: Vec<_> = content.lines().collect();
    let start = lines.len().saturating_sub(max_lines);
    lines[start..].join("\n")
}

pub fn load_archive_preview(path: &Path) -> Result<String> {
    let listing = load_session_listing(path)?;
    Ok(extract_tail_lines(
        &listing.snapshot.transcript.join("\n"),
        SUBAGENT_PREVIEW_LINES,
    ))
}

fn load_session_listing(path: &Path) -> Result<SessionListing> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read session archive {}", path.display()))?;
    let snapshot: SessionSnapshot = serde_json::from_str(&raw)
        .with_context(|| format!("Failed to parse session archive {}", path.display()))?;
    Ok(SessionListing {
        path: path.to_path_buf(),
        snapshot,
    })
}

#[must_use]
pub(crate) fn exec_session_is_running(session: &crate::tools::types::VTCodeExecSession) -> bool {
    matches!(
        session.lifecycle_state,
        Some(crate::tools::types::VTCodeSessionLifecycleState::Running)
    )
}

pub fn subagent_display_label(spec: &vtcode_config::SubagentSpec) -> String {
    spec.nickname_candidates
        .first()
        .cloned()
        .unwrap_or_else(|| spec.name.clone())
}

#[cfg(test)]
mod tests {
    use super::{
        build_background_launch_spec, resolve_background_subagent_executable_for_workspace,
    };
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    #[test]
    fn prefers_workspace_built_binary_when_current_exe_is_external() {
        let temp_dir = TempDir::new().expect("temp dir");
        let workspace_root = temp_dir.path();
        let candidate = workspace_root.join("target/debug/vtcode");
        fs::create_dir_all(candidate.parent().expect("parent")).expect("mkdir");
        fs::write(&candidate, b"binary").expect("write candidate");

        let resolved = resolve_background_subagent_executable_for_workspace(
            workspace_root,
            Path::new("/usr/local/bin/vtcode"),
        );

        assert_eq!(resolved, candidate);
    }

    #[test]
    fn keeps_current_exe_when_already_running_workspace_binary() {
        let temp_dir = TempDir::new().expect("temp dir");
        let workspace_root = temp_dir.path();
        let current = workspace_root.join("target/debug/vtcode");
        fs::create_dir_all(current.parent().expect("parent")).expect("mkdir");
        fs::write(&current, b"binary").expect("write current");

        let resolved =
            resolve_background_subagent_executable_for_workspace(workspace_root, &current);

        assert_eq!(resolved, current);
    }

    #[test]
    fn background_demo_launch_uses_direct_script_pipe_session() {
        let temp_dir = TempDir::new().expect("temp dir");
        let workspace_root = temp_dir.path();

        let launch = build_background_launch_spec(
            workspace_root,
            "background-demo",
            "parent",
            "child",
            "Report readiness once.",
            Some(4),
            None,
            None,
        )
        .expect("background demo launch");

        assert!(!launch.use_pty);
        assert_eq!(
            launch.command,
            vec![
                workspace_root
                    .join("scripts")
                    .join("demo-background-subagent.sh")
                    .to_string_lossy()
                    .into_owned()
            ]
        );
    }
}
