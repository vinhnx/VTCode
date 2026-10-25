//! Resolve the captured tree, distinguishing unborn branches from invalid HEAD.

use super::{configure_read_only, read_output};
use anyhow::{Result, bail};
use std::path::Path;

pub(super) struct Baseline {
    pub(super) revision: String,
    pub(super) unborn: bool,
}

async fn query(workspace: &Path, args: &[&str]) -> Result<super::GitOutput> {
    let (_directory, mut command) = crate::git::worktree::git_command_at(workspace)?;
    configure_read_only(&mut command);
    command.args(args);
    read_output(command, 4096).await
}

fn object_id(output: super::GitOutput) -> Result<String> {
    let id = output.text.trim_end_matches('\n');
    if !output.success || output.truncated || !matches!(id.len(), 40 | 64) || !id.bytes().all(|b| b.is_ascii_hexdigit())
    {
        bail!("Git baseline object could not be resolved");
    }
    Ok(id.to_owned())
}

pub(super) async fn resolve(workspace: &Path) -> Result<Baseline> {
    let head = query(workspace, &["rev-parse", "--verify", "--quiet", "HEAD^{tree}"]).await?;
    if head.success {
        return Ok(Baseline { revision: object_id(head)?, unborn: false });
    }
    if head.truncated || head.exit_code != Some(1) {
        bail!("Git or HEAD is unavailable for this workspace");
    }
    let branch = query(workspace, &["symbolic-ref", "--quiet", "HEAD"]).await?;
    let name = branch.text.trim_end_matches('\n');
    if !branch.success || branch.truncated || !name.starts_with("refs/heads/") || name.chars().any(char::is_control) {
        bail!("HEAD is not a valid unborn branch");
    }
    let existing = query(workspace, &["show-ref", "--verify", "--quiet", name]).await?;
    if existing.truncated || existing.exit_code != Some(1) {
        bail!("HEAD branch exists but its tree is unavailable");
    }
    // stdin is /dev/null and -w is deliberately absent: compute the repository's
    // native-format empty tree ID without writing objects or touching the index.
    let empty = query(workspace, &["hash-object", "-t", "tree", "--stdin"]).await?;
    Ok(Baseline { revision: object_id(empty)?, unborn: true })
}
