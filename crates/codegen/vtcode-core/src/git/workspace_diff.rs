//! Read-only, bounded current Git state, independent of agent attribution.
use anyhow::{Context, Result, bail};
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::AsyncReadExt;
use vtcode_memory::explanation::WorkspaceDiffSnapshot;

const MAX_DIFF_BYTES: usize = 8 * 1024;
const MAX_REVIEW_DIFF_BYTES: usize = 200 * 1024;

mod baseline;
mod untracked;

struct GitOutput {
    text: String,
    truncated: bool,
    success: bool,
    exit_code: Option<i32>,
    omitted_untracked: usize,
    unborn: bool,
}

/// Capture tracked changes against HEAD without executing Git extensions.
/// An unborn branch uses its native-format empty tree; invalid Git/HEAD,
/// timeout, and I/O failures remain explicit in the snapshot.
pub async fn capture_workspace_diff(workspace: &Path) -> WorkspaceDiffSnapshot {
    capture_diff(workspace, MAX_DIFF_BYTES, false).await
}

/// Capture a larger bounded snapshot for interactive, read-only desktop review.
/// Uses the same extension suppression, redaction, and timeout as explanations.
pub async fn capture_workspace_review_diff(workspace: &Path, include_untracked: bool) -> WorkspaceDiffSnapshot {
    capture_diff(workspace, MAX_REVIEW_DIFF_BYTES, include_untracked).await
}

async fn capture_diff(workspace: &Path, max_bytes: usize, include_untracked: bool) -> WorkspaceDiffSnapshot {
    let mut snapshot = WorkspaceDiffSnapshot {
        captured_at: chrono::Utc::now().to_rfc3339(),
        text: None,
        truncated: false,
        note: String::new(),
    };
    let result = tokio::time::timeout(Duration::from_secs(2), async {
        let root = vtcode_commons::canonicalize(workspace).context("resolve workspace for current Git state")?;
        read_diff(&root, max_bytes, include_untracked).await
    })
    .await;
    snapshot.note = match result {
        Ok(Ok(output)) => {
            snapshot.text = Some(output.text);
            snapshot.truncated = output.truncated;
            let baseline = if output.unborn {
                "empty tree (unborn branch; no commits yet)"
            } else {
                "HEAD"
            };
            if include_untracked {
                format!(
                    "Tracked changes against {baseline} plus non-ignored untracked previews. Known omissions: {} untracked files (unsupported paths, unsafe or unreadable files, binary/non-UTF-8 content, or limits). Untracked capture may be skipped at the total preview limit. Ownership is not attributed to the agent.",
                    output.omitted_untracked
                )
            } else {
                format!(
                    "Tracked changes against {baseline}; untracked files excluded. Ownership is not attributed to the agent."
                )
            }
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

async fn read_diff(workspace: &Path, max_bytes: usize, include_untracked: bool) -> Result<GitOutput> {
    let baseline = baseline::resolve(workspace).await?;
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
        &baseline.revision,
        "--",
        ".",
    ]);
    // Carry beyond the visible boundary before redacting split secrets.
    let mut output = read_output(command, max_bytes + 1024).await?;
    output.unborn = baseline.unborn;
    if !output.truncated && !output.success {
        bail!("Git or HEAD is unavailable for this workspace");
    }
    if include_untracked && !output.truncated && output.text.len() <= max_bytes {
        untracked::append(workspace, &mut output, max_bytes).await?;
    }
    output.truncated |= output.text.len() > max_bytes;
    let mut text = vtcode_commons::sanitizer::redact_secrets(output.text);
    output.truncated |= text.len() > max_bytes;
    let mut end = text.len().min(max_bytes);
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
        // Read-only review must not refresh and rewrite the index stat cache.
        .env("GIT_OPTIONAL_LOCKS", "0")
        .args([
            "-c",
            "core.fsmonitor=false",
            "-c",
            "diff.autoRefreshIndex=false",
            "--no-pager",
        ])
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
        omitted_untracked: 0,
        unborn: false,
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
        let review = capture_workspace_review_diff(temp.path(), false).await;
        assert!(
            review
                .text
                .as_ref()
                .is_some_and(|text| text.contains("human edit") && !text.contains(secret))
        );
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
    async fn interactive_review_includes_staged_and_unstaged_changes_with_its_own_bound() {
        let temp = repository();
        std::fs::write(temp.path().join("staged-only.txt"), "indexed addition\n").unwrap();
        git(temp.path(), &["add", "staged-only.txt"]);
        std::fs::write(temp.path().join("tracked.txt"), "staged change\n").unwrap();
        git(temp.path(), &["add", "tracked.txt"]);
        std::fs::write(
            temp.path().join("tracked.txt"),
            format!("staged plus unstaged\n{}", "review line\n".repeat(1500)),
        )
        .unwrap();
        std::fs::write(temp.path().join("untracked.txt"), "excluded").unwrap();
        let small = capture_workspace_diff(temp.path()).await;
        assert!(small.truncated);
        let review = capture_workspace_review_diff(temp.path(), false).await;
        assert!(!review.truncated, "{}", review.note);
        let text = review.text.unwrap();
        assert!(text.contains("+staged plus unstaged"));
        assert!(text.contains("+++ b/staged-only.txt"));
        assert!(text.contains("+indexed addition"));
        assert!(text.len() > MAX_DIFF_BYTES && text.len() <= MAX_REVIEW_DIFF_BYTES);
        assert!(!text.contains("untracked.txt"));
        let folder = temp.path().join("scope");
        std::fs::create_dir(&folder).unwrap();
        std::fs::write(folder.join("scoped.txt"), "inside scope\n").unwrap();
        git(temp.path(), &["add", "scope/scoped.txt"]);
        let scoped = capture_workspace_review_diff(&folder, false).await.text.unwrap();
        assert!(scoped.contains("+inside scope"));
        assert!(!scoped.contains("tracked.txt"));
        assert!(!scoped.contains("staged-only.txt"));
        std::fs::write(temp.path().join("tracked.txt"), "large review line\n".repeat(20000)).unwrap();
        let bounded = capture_workspace_review_diff(temp.path(), false).await;
        assert!(bounded.truncated);
        assert!(bounded.text.unwrap().len() <= MAX_REVIEW_DIFF_BYTES);
    }

    #[tokio::test]
    async fn read_only_review_does_not_refresh_index_stat_cache() {
        let temp = repository();
        let tracked = std::fs::OpenOptions::new()
            .write(true)
            .open(temp.path().join("tracked.txt"))
            .unwrap();
        tracked
            .set_times(
                std::fs::FileTimes::new()
                    .set_modified(std::time::SystemTime::UNIX_EPOCH + Duration::from_secs(946_684_800)),
            )
            .unwrap();
        let index = temp.path().join(".git/index");
        let before = std::fs::read(&index).unwrap();
        let review = capture_workspace_review_diff(temp.path(), false).await;
        assert_eq!(review.text.as_deref(), Some(""), "{}", review.note);
        assert_eq!(std::fs::read(&index).unwrap(), before, "read-only capture rewrote index metadata");
        let explanation = capture_workspace_diff(temp.path()).await;
        assert_eq!(explanation.text.as_deref(), Some(""));
        assert_eq!(std::fs::read(index).unwrap(), before);
    }

    #[tokio::test]
    async fn untracked_review_is_opt_in_ignored_redacted_and_read_only() {
        let temp = repository();
        std::fs::write(temp.path().join(".gitignore"), "ignored.txt\n").unwrap();
        git(temp.path(), &["add", ".gitignore"]);
        std::fs::write(
            temp.path().join("new file.rs"),
            "new content\nOPENAI_API_KEY=sk-1234567890abcdefghijklmnopqrstuv\n",
        )
        .unwrap();
        std::fs::write(temp.path().join("empty.rs"), "").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(temp.path().join("empty.rs"), std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        std::fs::write(temp.path().join("ignored.txt"), "private ignored content").unwrap();
        let index = temp.path().join(".git/index");
        let before = std::fs::read(&index).unwrap();
        let excluded = capture_workspace_review_diff(temp.path(), false).await.text.unwrap();
        assert!(!excluded.contains("new file.rs"));
        let included = capture_workspace_review_diff(temp.path(), true).await;
        assert!(!included.truncated, "{}", included.note);
        let patch = included.text.unwrap();
        assert!(patch.contains("new file.rs") && patch.contains("+new content"));
        assert!(patch.contains("empty.rs"));
        #[cfg(unix)]
        assert!(patch.contains("diff --git a/empty.rs b/empty.rs\nnew file mode 100755\n"));
        assert!(!patch.contains("private ignored content"));
        assert!(!patch.contains("sk-1234567890abcdefghijklmnopqrstuv"));
        assert_eq!(std::fs::read(index).unwrap(), before);
        assert!(!capture_workspace_diff(temp.path()).await.text.unwrap().contains("new file.rs"));
    }

    #[tokio::test]
    async fn untracked_review_reports_unsafe_binary_and_oversized_omissions() {
        let temp = repository();
        std::fs::write(temp.path().join("binary.bin"), [0, 1, 2]).unwrap();
        std::fs::write(temp.path().join("large.txt"), "z".repeat(64 * 1024 + 1)).unwrap();
        std::fs::write(temp.path().join("bad\nname"), "header injection").unwrap();
        std::fs::write(temp.path().join("valid.txt"), "included\n").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink("tracked.txt", temp.path().join("linked.txt")).unwrap();
        let review = capture_workspace_review_diff(temp.path(), true).await;
        assert!(review.truncated);
        assert!(
            review.note.contains(if cfg!(unix) {
                "4 untracked files"
            } else {
                "3 untracked files"
            }),
            "{}",
            review.note
        );
        let patch = review.text.unwrap();
        assert!(patch.contains("+included"));
        assert!(!patch.contains("header injection"));
        assert!(!patch.contains("linked.txt"));
        assert!(!patch.contains("binary.bin"));
        assert!(!patch.contains("large.txt"));
    }

    #[tokio::test]
    async fn untracked_review_bounds_file_count_and_keeps_scoped_paths() {
        let temp = repository();
        let scope = temp.path().join("scope");
        std::fs::create_dir(&scope).unwrap();
        for index in 0..81 {
            std::fs::write(scope.join(format!("file-{index:02}.txt")), "fresh\n").unwrap();
        }
        std::fs::write(temp.path().join("outside.txt"), "outside scoped review").unwrap();
        let review = capture_workspace_review_diff(&scope, true).await;
        assert!(review.truncated);
        assert!(review.note.contains("1 untracked files"), "{}", review.note);
        let patch = review.text.unwrap();
        assert_eq!(patch.matches("new file mode").count(), 80);
        assert!(patch.contains("+++ b/scope/file-00.txt"));
        assert!(patch.contains("+++ b/scope/file-79.txt"));
        assert!(!patch.contains("file-80.txt"));
        assert!(!patch.contains("outside scoped review"));
        assert!(patch.len() <= MAX_REVIEW_DIFF_BYTES);
    }

    #[tokio::test]
    async fn unborn_review_uses_native_empty_tree_without_writing_repository_state() {
        for format in ["sha1", "sha256"] {
            let temp = tempfile::tempdir().unwrap();
            git(temp.path(), &["init", "--quiet", &format!("--object-format={format}")]);
            let empty = capture_workspace_review_diff(temp.path(), false).await;
            assert_eq!(empty.text.as_deref(), Some(""), "{}", empty.note);
            assert!(empty.note.contains("unborn branch"));
            assert!(!temp.path().join(".git/index").exists());
            std::fs::write(temp.path().join("new.txt"), "staged version\n").unwrap();
            git(temp.path(), &["add", "new.txt"]);
            std::fs::write(temp.path().join("new.txt"), "latest working version\n").unwrap();
            std::fs::write(temp.path().join("untracked.txt"), "optional addition\n").unwrap();
            let index = std::fs::read(temp.path().join(".git/index")).unwrap();
            let head = std::fs::read(temp.path().join(".git/HEAD")).unwrap();
            let object_files = || {
                let mut files = Vec::new();
                for directory in std::fs::read_dir(temp.path().join(".git/objects")).unwrap() {
                    for file in std::fs::read_dir(directory.unwrap().path()).unwrap() {
                        let path = file.unwrap().path();
                        files.push((path.clone(), std::fs::read(path).unwrap()));
                    }
                }
                files.sort();
                files
            };
            let objects = object_files();
            let tracked = capture_workspace_review_diff(temp.path(), false).await;
            let patch = tracked.text.unwrap();
            assert!(patch.contains("+latest working version"));
            assert!(!patch.contains("staged version"));
            assert!(!patch.contains("untracked.txt"));
            let all = capture_workspace_review_diff(temp.path(), true).await;
            assert!(!all.truncated, "{}", all.note);
            assert!(all.text.unwrap().contains("+optional addition"));
            assert!(
                capture_workspace_diff(temp.path())
                    .await
                    .text
                    .unwrap()
                    .contains("+latest working version")
            );
            assert_eq!(std::fs::read(temp.path().join(".git/index")).unwrap(), index);
            assert_eq!(std::fs::read(temp.path().join(".git/HEAD")).unwrap(), head);
            assert_eq!(object_files(), objects, "review wrote Git objects");
        }
    }

    #[tokio::test]
    async fn broken_head_is_not_treated_as_an_unborn_branch() {
        let temp = repository();
        git(temp.path(), &["checkout", "--detach", "--quiet", "HEAD"]);
        let detached = capture_workspace_review_diff(temp.path(), false).await;
        assert_eq!(detached.text.as_deref(), Some(""), "{}", detached.note);
        assert!(!detached.note.contains("unborn"));
        std::fs::write(temp.path().join(".git/HEAD"), format!("{}\n", "a".repeat(40))).unwrap();
        assert!(capture_workspace_review_diff(temp.path(), true).await.text.is_none());
        std::fs::write(temp.path().join(".git/HEAD"), "ref: refs/heads/broken\n").unwrap();
        std::fs::write(temp.path().join(".git/refs/heads/broken"), format!("{}\n", "a".repeat(40))).unwrap();
        assert!(capture_workspace_review_diff(temp.path(), true).await.text.is_none());
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
