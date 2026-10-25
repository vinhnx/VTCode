//! Bounded untracked previews, opened through the shared no-follow file reader.

use super::{GitOutput, configure_read_only, read_output};
use anyhow::{Context, Result, bail};
use std::io::Read;
use std::path::Path;

const MAX_FILES: usize = 80;
const MAX_FILE_BYTES: usize = 64 * 1024;

pub(super) async fn append(workspace: &Path, output: &mut GitOutput, max_bytes: usize) -> Result<()> {
    let (_directory, mut command) = crate::git::worktree::git_command_at(workspace)?;
    configure_read_only(&mut command);
    command.args(["ls-files", "--others", "--exclude-standard", "-z", "--", "."]);
    let listing = read_output(command, 64 * 1024).await?;
    if listing.truncated || !listing.success {
        bail!("Untracked file listing unavailable or exceeds the review limit");
    }
    let (_directory, mut command) = crate::git::worktree::git_command_at(workspace)?;
    configure_read_only(&mut command);
    command.args(["rev-parse", "--show-prefix"]);
    let prefix = read_output(command, 4096).await?;
    if prefix.truncated || !prefix.success {
        bail!("Untracked file prefix unavailable");
    }
    let prefix = prefix.text.strip_suffix('\n').unwrap_or(&prefix.text);
    for (index, path) in listing.text.split('\0').filter(|path| !path.is_empty()).enumerate() {
        let remaining = max_bytes.saturating_sub(output.text.len());
        let patch = if index >= MAX_FILES || remaining == 0 {
            None
        } else {
            let root = workspace.to_path_buf();
            let path = path.to_owned();
            let display_path = format!("{prefix}{path}");
            tokio::task::spawn_blocking(move || preview(&root, &path, &display_path, remaining))
                .await
                .context("read untracked preview worker")?
        };
        if let Some(patch) = patch {
            output.text.push_str(&patch);
        } else {
            output.omitted_untracked = output.omitted_untracked.saturating_add(1);
            output.truncated = true;
        }
    }
    Ok(())
}

fn preview(root: &Path, path: &str, display_path: &str, remaining: usize) -> Option<String> {
    // Do not let a filename inject patch headers or ambiguous quoted paths.
    if display_path
        .chars()
        .any(|c| c.is_control() || matches!(c, '\\' | '"' | '\u{fffd}'))
    {
        return None;
    }
    let file = vtcode_commons::fs::bound_file::open_file_beneath(root, Path::new(path)).ok()?;
    #[cfg(unix)]
    let executable = {
        use std::os::unix::fs::PermissionsExt;
        file.metadata().ok()?.permissions().mode() & 0o111 != 0
    };
    #[cfg(not(unix))]
    let executable = false;
    let mode = if executable { "100755" } else { "100644" };
    let mut bytes = Vec::new();
    file.take((MAX_FILE_BYTES + 1) as u64).read_to_end(&mut bytes).ok()?;
    if bytes.len() > MAX_FILE_BYTES || bytes.contains(&0) {
        return None;
    }
    let text = vtcode_commons::sanitizer::redact_secrets(String::from_utf8(bytes).ok()?);
    let label = format!("b/{display_path}");
    let mut patch = format!("diff --git a/{display_path} b/{display_path}\nnew file mode {mode}\n");
    patch.push_str(&vtcode_diff::format_unified_diff(
        "",
        &text,
        vtcode_diff::DiffOptions {
            old_label: Some("/dev/null"),
            new_label: Some(&label),
            ..Default::default()
        },
    ));
    (patch.len() <= remaining).then_some(patch)
}
