//! Sandbox preflight guards shared by pipe and PTY command launches.
//!
//! Checks the *lexical* command arguments and a symlink-resolved view of each
//! path: Landlock grants writes to a whole writable root and has no deny rules,
//! so a workspace symlink to a protected directory must be caught here.

use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, anyhow};

use crate::sandboxing::SandboxPolicy;

pub(in crate::tools::registry::executors) async fn enforce_sandbox_preflight_guards(
    requested_command: &[String],
    shell_script: Option<&str>,
    policy: &SandboxPolicy,
    working_dir: &Path,
) -> Result<()> {
    if requested_command.is_empty() {
        return Ok(());
    }

    let network_disabled = !policy.has_full_network_access() && !policy.has_network_allowlist();
    if network_disabled && command_likely_needs_network(requested_command) {
        return Err(anyhow!(
            "Command '{}' appears to require network access, but sandbox policy '{}' denies network.",
            shell_words::join(requested_command.iter().map(String::as_str)),
            policy.description()
        ));
    }

    // Landlock grants writes to a whole writable root and has no deny rules, so
    // a symlink inside the workspace (`link -> .git`, then `cat > link/config`)
    // reaches a protected file even though the lexical path looks safe. Check
    // every candidate both lexically and after symlink resolution. The canonical
    // policy uses canonical writable roots so `/private/var` vs `/var` (macOS)
    // does not turn into a false positive.
    let canonical_cwd = canonicalize_preflight_path(working_dir)
        .await?
        .unwrap_or_else(|| working_dir.to_path_buf());
    let validation_policy = canonical_validation_policy(policy, working_dir).await?;

    let mut blocked_paths = BTreeSet::new();
    for argument in requested_command.iter().skip(1) {
        let Some(candidate) = resolve_argument_path(argument, working_dir) else {
            continue;
        };
        let mut blocked = !policy.is_path_readable(&candidate);
        if let Some(canonical) = canonicalize_preflight_path(&candidate).await? {
            blocked |= !validation_policy.is_path_readable(&canonical);
        }
        if blocked {
            blocked_paths.insert(candidate.display().to_string());
        }
    }
    if !blocked_paths.is_empty() {
        let listed = blocked_paths.into_iter().take(3).collect::<Vec<_>>().join(", ");
        return Err(anyhow!(
            "Command references sensitive path(s) blocked by sandbox policy '{}': {}",
            policy.description(),
            listed
        ));
    }

    if command_likely_writes_workspace(requested_command) {
        let mut blocked_write_paths = BTreeSet::new();
        for argument in requested_command.iter().skip(1) {
            let Some(candidate) = resolve_argument_path(argument, working_dir) else {
                continue;
            };
            let mut blocked = !policy.is_path_writable(&candidate, working_dir);
            if let Some(canonical) = canonicalize_preflight_path(&candidate).await? {
                blocked |= !validation_policy.is_path_writable(&canonical, &canonical_cwd);
            }
            if blocked {
                blocked_write_paths.insert(candidate.display().to_string());
            }
        }
        if !blocked_write_paths.is_empty() {
            let listed = blocked_write_paths.into_iter().take(3).collect::<Vec<_>>().join(", ");
            return Err(anyhow!(
                "Command references path(s) blocked for writes by sandbox policy '{}': {}",
                policy.description(),
                listed
            ));
        }
    }

    // Redirections are invisible to `command_likely_writes_workspace`: `cat >
    // .git/config` looks like a read. They are also the classic editor-config
    // injection vector (`.vscode/settings.json`, format-on-save).
    if let Some(script) = shell_script {
        let mut blocked_redirection_targets = BTreeSet::new();
        for destination in crate::command_safety::shell_parser::output_redirection_destinations(script) {
            let Some(candidate) = resolve_redirection_destination(&destination, working_dir)? else {
                continue;
            };
            let mut blocked = !policy.is_path_writable(&candidate, working_dir);
            if let Some(canonical) = canonicalize_preflight_path(&candidate).await? {
                blocked |= !validation_policy.is_path_writable(&canonical, &canonical_cwd);
            }
            if blocked {
                blocked_redirection_targets.insert(candidate.display().to_string());
            }
        }
        if !blocked_redirection_targets.is_empty() {
            let listed = blocked_redirection_targets.into_iter().take(3).collect::<Vec<_>>().join(", ");
            return Err(anyhow!(
                "Command redirects output to path(s) blocked for writes by sandbox policy '{}': {}",
                policy.description(),
                listed
            ));
        }
    }

    Ok(())
}

/// Resolve a path through symlinks via the commons seam, returning the
/// canonical path only when it differs from the lexical one. Fail closed:
/// an unresolvable path (dangling symlink, permission error, loop) must not
/// be admitted by a preflight that cannot prove what the command will open.
async fn canonicalize_preflight_path(path: &Path) -> Result<Option<PathBuf>> {
    let canonical = vtcode_commons::paths::canonicalize_allow_missing(path)
        .await
        .with_context(|| format!("resolve sandbox preflight path '{}'", path.display()))?;
    Ok((canonical != path).then_some(canonical))
}

/// Rebuild a workspace-write policy whose writable roots are canonical.
///
/// Canonical candidates (e.g. `/private/var/...` on macOS) must be checked
/// against roots in the same representation. Without this, every symlinked
/// workspace would look like an out-of-root path. Same seam as
/// `normalize_additional_permissions`.
async fn canonical_validation_policy(policy: &SandboxPolicy, working_dir: &Path) -> Result<SandboxPolicy> {
    let SandboxPolicy::WorkspaceWrite {
        network_allowlist,
        sensitive_paths,
        resource_limits,
        seccomp_profile,
        exclude_tmpdir_env_var,
        exclude_slash_tmp,
        ..
    } = policy
    else {
        return Ok(policy.clone());
    };

    let mut roots = Vec::new();
    for root in policy.get_writable_roots_with_cwd(working_dir) {
        let canonical = vtcode_commons::paths::canonicalize_allow_missing(&root.root)
            .await
            .with_context(|| format!("resolve sandbox writable root '{}'", root.root.display()))?;
        roots.push(canonical);
    }
    let mut rebuilt = SandboxPolicy::workspace_write_full(
        roots,
        network_allowlist.clone(),
        sensitive_paths.clone(),
        resource_limits.clone(),
        seccomp_profile.clone(),
    );
    if let SandboxPolicy::WorkspaceWrite {
        exclude_tmpdir_env_var: exclude_tmp,
        exclude_slash_tmp: exclude_tmp_root,
        ..
    } = &mut rebuilt
    {
        *exclude_tmp = *exclude_tmpdir_env_var;
        *exclude_tmp_root = *exclude_slash_tmp;
    }
    Ok(rebuilt)
}

/// Resolve a literal output-redirection destination against the command cwd.
///
/// Unlike [`resolve_argument_path`], redirection targets are unambiguous paths
/// and include bare relative forms (`.git/config`, `settings.json`). Parent
/// traversal is rejected instead of being normalized away: upstream redirection
/// validation rejects it, and a preflight that silently canonicalized it could
/// check a different path than the one the shell opens.
fn resolve_redirection_destination(destination: &str, working_dir: &Path) -> Result<Option<PathBuf>> {
    let trimmed = destination.trim();
    if trimmed.is_empty() || trimmed == "/dev/null" {
        return Ok(None);
    }
    let path = if trimmed == "~" || trimmed.starts_with("~/") {
        vtcode_commons::paths::expand_tilde(trimmed)
    } else {
        PathBuf::from(trimmed)
    };
    if path.components().any(|component| component == Component::ParentDir) {
        return Err(anyhow!("redirection destination '{}' must not contain '..' traversal", trimmed));
    }
    let resolved = if path.is_absolute() {
        path
    } else {
        working_dir.join(path)
    };
    Ok(Some(crate::utils::path::normalize_path(&resolved)))
}

fn command_likely_needs_network(command: &[String]) -> bool {
    let Some(program) = command.first() else {
        return false;
    };
    let name = Path::new(program)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(program.as_str())
        .to_ascii_lowercase();
    if matches!(
        name.as_str(),
        "curl"
            | "wget"
            | "ping"
            | "ssh"
            | "scp"
            | "sftp"
            | "ftp"
            | "telnet"
            | "nc"
            | "ncat"
            | "nmap"
            | "dig"
            | "nslookup"
            | "host"
    ) {
        return true;
    }
    if name == "git" {
        return command.iter().skip(1).any(|arg| {
            matches!(arg.as_str(), "clone" | "fetch" | "pull" | "push" | "ls-remote" | "remote" | "submodule")
        });
    }
    false
}

pub(in crate::tools::registry::executors) fn command_likely_writes_workspace(command: &[String]) -> bool {
    let Some(program) = command.first() else {
        return false;
    };
    let name = Path::new(program)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(program.as_str())
        .to_ascii_lowercase();

    if matches!(
        name.as_str(),
        "touch"
            | "mkdir"
            | "rm"
            | "rmdir"
            | "mv"
            | "cp"
            | "chmod"
            | "chown"
            | "ln"
            | "install"
            | "truncate"
            | "tee"
            | "rustfmt"
            | "gofmt"
    ) {
        return true;
    }

    if name == "sed" {
        return command.iter().skip(1).any(|arg| arg == "-i" || arg.starts_with("-i"));
    }

    if name == "perl" {
        return command.iter().skip(1).any(|arg| arg == "-i" || arg.starts_with("-i"));
    }

    if name == "cargo" {
        return command.iter().skip(1).any(|arg| {
            matches!(arg.as_str(), "fmt" | "fix" | "build" | "check" | "clippy" | "test" | "nextest" | "clean")
        });
    }

    if matches!(name.as_str(), "npm" | "pnpm" | "yarn" | "bun") {
        return command
            .iter()
            .skip(1)
            .any(|arg| matches!(arg.as_str(), "install" | "ci" | "add" | "update"));
    }

    if name == "go" {
        return command
            .iter()
            .skip(1)
            .any(|arg| matches!(arg.as_str(), "fmt" | "test" | "build" | "mod"));
    }

    if name == "git" {
        return command.iter().skip(1).any(|arg| {
            matches!(
                arg.as_str(),
                "add"
                    | "apply"
                    | "checkout"
                    | "switch"
                    | "merge"
                    | "rebase"
                    | "cherry-pick"
                    | "commit"
                    | "stash"
                    | "reset"
                    | "restore"
                    | "rm"
                    | "mv"
            )
        });
    }

    false
}

fn resolve_argument_path(argument: &str, working_dir: &Path) -> Option<PathBuf> {
    let trimmed = argument.trim().trim_matches(|ch| ch == '"' || ch == '\'');
    if trimmed.is_empty() || trimmed.starts_with('-') || trimmed.contains("://") {
        return None;
    }

    let candidate = if trimmed.starts_with("~/") || trimmed == "~" {
        Some(vtcode_commons::paths::expand_tilde(trimmed))
    } else if trimmed.starts_with('/') {
        Some(PathBuf::from(trimmed))
    } else if trimmed.starts_with("./") || trimmed.starts_with("../") {
        Some(working_dir.join(trimmed))
    } else if let Some((_, value)) = trimmed.split_once('=') {
        if value.starts_with("~/")
            || value == "~"
            || value.starts_with('/')
            || value.starts_with("./")
            || value.starts_with("../")
        {
            Some(resolve_argument_path(value, working_dir)?)
        } else {
            None
        }
    } else {
        // Bare relative paths are the common shell form (`touch .git/config`,
        // `tee .vscode/settings.json`). Shell argv is resolved against cwd, so
        // the preflight must check the same path the program will open.
        Some(working_dir.join(trimmed))
    }?;

    Some(candidate)
}
