//! Read-only, bounded current Git state, independent of agent attribution.
use anyhow::{Context, Result, bail};
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::AsyncReadExt;
use vtcode_memory::explanation::WorkspaceDiffSnapshot;

const MAX_DIFF_BYTES: usize = 8 * 1024;

struct GitOutput {
    text: String,
    truncated: bool,
    success: bool,
    exit_code: Option<i32>,
}

/// Capture tracked changes against HEAD without executing Git extensions.
/// Missing Git/HEAD, timeout, and I/O failures remain explicit in the snapshot.
pub async fn capture_workspace_diff(workspace: &Path) -> WorkspaceDiffSnapshot {
    let mut snapshot = WorkspaceDiffSnapshot {
        captured_at: chrono::Utc::now().to_rfc3339(),
        text: None,
        truncated: false,
        note: String::new(),
    };
    let result = tokio::time::timeout(Duration::from_secs(2), async {
        let root = vtcode_commons::canonicalize(workspace).context("resolve workspace for current Git state")?;
        read_diff(&root).await
    })
    .await;
    snapshot.note = match result {
        Ok(Ok(output)) => {
            snapshot.text = Some(output.text);
            snapshot.truncated = output.truncated;
            "Tracked changes against HEAD; untracked files excluded. Ownership is not attributed to the agent."
                .to_owned()
        }
        Ok(Err(error)) => format!("Current Git diff unavailable: {error:#}"),
        Err(_) => "Current Git diff unavailable: capture timed out".to_owned(),
    };
    snapshot.note = vtcode_commons::sanitizer::redact_secrets(snapshot.note)
        .chars()
        .filter(|c| !c.is_control())
        .take(2048)
        .collect();
    snapshot
}

async fn read_diff(workspace: &Path) -> Result<GitOutput> {
    // Git diff can run clean/process filters even with --no-textconv.
    // Override every configured driver, or fail closed when inspection fails.
    let (_config_directory, mut config) = super::worktree::git_command_at(workspace)?;
    configure_read_only(&mut config);
    config.args([
        "config",
        "--null",
        "--get-regexp",
        r"^filter\..*\.(clean|process|required)$",
    ]);
    let filters = read_output(config, MAX_DIFF_BYTES).await?;
    if filters.truncated || !filters.success && !(filters.exit_code == Some(1) && filters.text.is_empty()) {
        bail!("Git filter configuration could not be safely inspected");
    }
    let (_directory, mut command) = super::worktree::git_command_at(workspace)?;
    configure_read_only(&mut command);
    for record in filters.text.split('\0').filter(|record| !record.is_empty()) {
        let (key, _) = record.split_once('\n').context("invalid Git filter configuration")?;
        if key.len() > 256 || !key.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_')) {
            bail!("Git filter name cannot be safely overridden");
        }
        command.args([
            "-c",
            &format!("{key}={}", if key.ends_with(".required") { "false" } else { "" }),
        ]);
    }
    command.args([
        "diff",
        "--no-ext-diff",
        "--no-textconv",
        "--no-renames",
        "--color=never",
        "HEAD",
        "--",
        ".",
    ]);
    // Carry beyond the visible boundary before redacting split secrets.
    let mut output = read_output(command, MAX_DIFF_BYTES + 1024).await?;
    if !output.truncated && !output.success {
        bail!("Git or HEAD is unavailable for this workspace");
    }
    output.truncated |= output.text.len() > MAX_DIFF_BYTES;
    let mut text = vtcode_commons::sanitizer::redact_secrets(output.text);
    let mut end = text.len().min(MAX_DIFF_BYTES);
    while !text.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    text.truncate(end);
    output.text = text.chars().filter(|c| !c.is_control() || *c == '\n' || *c == '\t').collect();
    Ok(output)
}

fn configure_read_only(command: &mut std::process::Command) {
    command.env_clear();
    for key in ["PATH", "SystemRoot"] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
    command
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .args(["-c", "core.fsmonitor=false", "--no-pager"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .stdin(Stdio::null());
}

async fn read_output(command: std::process::Command, limit: usize) -> Result<GitOutput> {
    let mut command = tokio::process::Command::from(command);
    command.kill_on_drop(true);
    let mut child = command.spawn().context("launch read-only Git diff")?;
    let stdout = child.stdout.take().context("Git diff stdout unavailable")?;
    let mut bytes = Vec::with_capacity(limit + 1);
    stdout
        .take((limit + 1) as u64)
        .read_to_end(&mut bytes)
        .await
        .context("read Git diff")?;
    let truncated = bytes.len() > limit;
    if truncated {
        child.kill().await.context("stop bounded Git diff")?;
    }
    let status = child.wait().await.context("wait for Git diff")?;
    Ok(GitOutput {
        text: String::from_utf8_lossy(&bytes).into_owned(),
        truncated,
        success: status.success(),
        exit_code: status.code(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git(workspace: &Path, args: &[&str]) {
        let output = std::process::Command::new("git")
            .current_dir(workspace)
            .args(args)
            .output()
            .unwrap();
        assert!(output.status.success(), "Git setup failed: {}", String::from_utf8_lossy(&output.stderr));
    }

    fn repository() -> tempfile::TempDir {
        let temp = tempfile::tempdir().unwrap();
        git(temp.path(), &["init", "--quiet"]);
        std::fs::write(temp.path().join("tracked.txt"), "original\n").unwrap();
        std::fs::write(temp.path().join(".gitattributes"), "tracked.txt filter=probe diff=probe\n").unwrap();
        git(temp.path(), &["add", "."]);
        git(
            temp.path(),
            &[
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.invalid",
                "commit",
                "--quiet",
                "-m",
                "fixture",
            ],
        );
        temp
    }

    #[tokio::test]
    async fn workspace_snapshot_is_separate_bounded_redacted_and_disables_extensions() {
        let temp = repository();
        for (key, value) in [
            ("filter.probe.clean", "touch filter-ran; cat"),
            ("filter.probe.process", "touch process-ran; cat"),
            ("filter.probe.required", "true"),
            ("diff.probe.textconv", "touch textconv-ran; cat"),
            ("diff.external", "touch external-ran"),
            ("core.fsmonitor", "touch fsmonitor-ran"),
        ] {
            git(temp.path(), &["config", key, value]);
        }
        let secret = concat!("sk-", "test1234567890abcdefghijklmnop");
        std::fs::write(temp.path().join("tracked.txt"), format!("human edit {secret}\n{}", "界\n".repeat(10000)))
            .unwrap();
        std::fs::write(temp.path().join("untracked.txt"), "untracked human edit").unwrap();
        let snapshot = capture_workspace_diff(temp.path()).await;
        let text = snapshot
            .text
            .as_ref()
            .unwrap_or_else(|| panic!("snapshot unavailable: {}", snapshot.note));
        assert!(text.contains("human edit"));
        assert!(!text.contains(secret));
        assert!(text.len() <= MAX_DIFF_BYTES);
        assert!(snapshot.truncated);
        assert!(!text.contains("untracked.txt"));
        assert!(snapshot.note.contains("Ownership is not attributed"));
        for name in [
            "filter-ran",
            "process-ran",
            "textconv-ran",
            "external-ran",
            "fsmonitor-ran",
        ] {
            assert!(!temp.path().join(name).exists(), "Git extension executed: {name}");
        }
    }

    #[tokio::test]
    async fn clean_and_unavailable_workspaces_remain_distinguishable() {
        let temp = repository();
        let clean = capture_workspace_diff(temp.path()).await;
        assert_eq!(clean.text.as_deref(), Some(""), "{}", clean.note);
        assert!(!clean.truncated);
        let other = tempfile::tempdir().unwrap();
        let unavailable = capture_workspace_diff(other.path()).await;
        assert!(unavailable.text.is_none());
        assert!(unavailable.note.contains("unavailable"));
    }
}
