use super::*;

pub(super) fn validate_git_read_only(subcommand: &str, subargs: &[String]) -> Result<()> {
    // Block dangerous flags even in read-only commands
    let dangerous_flags = ["-q", "--quiet", "--verbose", "-v"];

    for arg in subargs {
        if arg.starts_with("--") && arg.contains('=') {
            let key = arg.split('=').next().unwrap_or("");
            if key == "--format" {
                // Allow custom formats for output
                continue;
            }
        }

        if dangerous_flags.contains(&arg.as_str()) {
            // Benign flags, allow them
            continue;
        }

        // Allow common flags per subcommand
        match subcommand {
            "log" | "show" => {
                if matches!(
                    arg.as_str(),
                    "-n" | "--oneline"
                        | "--graph"
                        | "--decorate"
                        | "--all"
                        | "--grep"
                        | "-S"
                        | "-p"
                        | "-U"
                        | "--stat"
                        | "--shortstat"
                        | "--name-status"
                        | "--name-only"
                        | "--author"
                        | "--since"
                        | "--until"
                        | "--date"
                ) {
                    continue;
                }
            }
            "diff" => {
                if matches!(
                    arg.as_str(),
                    "-p" | "-U"
                        | "--stat"
                        | "--shortstat"
                        | "--name-status"
                        | "--name-only"
                        | "--no-index"
                        | "-w"
                        | "--ignore-all-space"
                        | "-b"
                        | "--ignore-space-change"
                ) {
                    continue;
                }
            }
            "branch" => {
                if matches!(arg.as_str(), "-a" | "-r" | "-v" | "--verbose") {
                    continue;
                }
            }
            _ => {
                // For other read-only commands, allow most flags
                if !arg.starts_with('-') || arg.starts_with("--") {
                    continue;
                }
            }
        }

        // Block any suspicious patterns
        if arg.contains(';') || arg.contains('|') || arg.contains('&') {
            return Err(anyhow!("git argument contains suspicious shell metacharacters"));
        }
    }

    Ok(())
}

pub(super) async fn validate_git_add(args: &[String], workspace_root: &Path, working_dir: &Path) -> Result<()> {
    // Block dangerous flags
    if args.iter().any(|a| a == "-f" || a == "--force") {
        return Err(anyhow!("git add --force is not permitted. Use regular add operations only."));
    }

    // Validate file paths if provided
    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        match arg.as_str() {
            "-u" | "--update" | "-A" | "--all" | "." => {
                // These are safe - they add all tracked or current directory
                index += 1;
            }
            "-p" | "--patch" | "-i" | "--interactive" => {
                // Interactive mode is fine
                index += 1;
            }
            "-n" | "--dry-run" => {
                index += 1;
            }
            value if value.starts_with('-') => {
                return Err(anyhow!("unsupported git add flag '{value}'"));
            }
            path => {
                // Validate the file path
                let resolved = resolve_path(workspace_root, working_dir, path).await?;
                ensure_within_workspace(workspace_root, &resolved).await?;
                index += 1;
            }
        }
    }

    Ok(())
}

pub(super) fn validate_git_commit(args: &[String]) -> Result<()> {
    let mut index = 0;

    while index < args.len() {
        let arg = &args[index];
        match arg.as_str() {
            "-m" | "--message" => {
                if index + 1 >= args.len() {
                    return Err(anyhow!("-m requires a commit message"));
                }
                index += 2;
            }
            "-F" | "--file" => {
                if index + 1 >= args.len() {
                    return Err(anyhow!("-F requires a file path"));
                }
                index += 2;
            }
            "-a" | "--all" | "-p" | "--patch" | "--amend" | "--no-verify" | "-q" | "--quiet" => {
                index += 1;
            }
            value if value.starts_with('-') => {
                return Err(anyhow!("unsupported git commit flag '{value}'"));
            }
            _ => {
                index += 1;
            }
        }
    }

    Ok(())
}

pub(super) fn validate_git_reset(args: &[String], confirm: bool) -> Result<()> {
    // Block destructive reset modes
    let is_destructive = args.iter().any(|a| a == "--hard" || a == "--merge" || a == "--keep");

    if is_destructive && !confirm {
        return Err(anyhow!(
            "git reset with --hard, --merge, or --keep is potentially destructive. Set `confirm=true` to proceed."
        ));
    }

    // Allow safe flags: --soft, --mixed, --unstage, and destructive ones with confirmation
    let safe_modes = ["--soft", "--mixed", "--unstage"];
    let allowed_destructive: Vec<&str> = if confirm {
        vec!["--hard", "--merge", "--keep"]
    } else {
        vec![]
    };

    for arg in args {
        if arg.starts_with('-') {
            let is_safe = safe_modes.iter().any(|m| arg.contains(m));
            let is_allowed_destructive = allowed_destructive.iter().any(|m| arg.contains(m));
            if !is_safe && !is_allowed_destructive {
                match arg.as_str() {
                    "-q" | "--quiet" | "-p" | "--patch" => continue,
                    _ => {
                        return Err(anyhow!(
                            "unsupported git reset flag '{arg}'. Use --soft, --mixed, or --hard (with confirm) modes."
                        ));
                    }
                }
            }
        }
    }

    Ok(())
}

pub(super) async fn validate_git_checkout(
    args: &[String],
    workspace_root: &Path,
    working_dir: &Path,
    confirm: bool,
) -> Result<()> {
    if args.is_empty() {
        return Ok(());
    }

    // Block forced checkout unless explicit confirm
    if args.iter().any(|a| a == "-f" || a == "--force") && !confirm {
        return Err(anyhow!("git checkout --force is potentially destructive; set `confirm=true` to proceed."));
    }

    // Validate paths if provided
    let mut paths_start = 0;
    for (i, arg) in args.iter().enumerate() {
        if arg == "--" {
            paths_start = i + 1;
            break;
        }
        if !arg.starts_with('-') {
            // Could be a branch or path
            paths_start = i;
            break;
        }
    }

    if paths_start > 0 {
        for path_arg in &args[paths_start..] {
            // Validate file paths
            let resolved = resolve_path(workspace_root, working_dir, path_arg).await?;
            ensure_within_workspace(workspace_root, &resolved).await?;
        }
    }

    Ok(())
}

pub(super) fn validate_git_stash(args: &[String]) -> Result<()> {
    if args.is_empty() {
        return Ok(());
    }

    let allowed_ops = ["list", "show", "pop", "apply", "drop", "clear", "create"];
    let first = args[0].as_str();

    if !allowed_ops.contains(&first) {
        return Err(anyhow!("git stash operation '{first}' is not permitted"));
    }

    // Allow flags for these operations
    for arg in &args[1..] {
        if arg.starts_with('-') {
            match arg.as_str() {
                "-q"
                | "--quiet"
                | "-p"
                | "--patch"
                | "-k"
                | "--keep-index"
                | "-u"
                | "--include-untracked"
                | "-a"
                | "--all" => continue,
                _ => return Err(anyhow!("unsupported git stash flag '{arg}'")),
            }
        }
    }

    Ok(())
}

pub(super) fn validate_git_merge(args: &[String]) -> Result<()> {
    // git merge is allowed for typical workflow
    if args.is_empty() {
        return Err(anyhow!("git merge requires a branch"));
    }

    // Block dangerous flags
    let dangerous_flags = ["--no-ff", "--squash"];
    for arg in args {
        if dangerous_flags.contains(&arg.as_str()) {
            return Err(anyhow!("git merge with {arg} flag is not permitted; use simpler merge"));
        }
    }

    Ok(())
}
