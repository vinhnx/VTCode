use std::env;
use std::io;
use std::path::{Component, Path, PathBuf};
use tokio::fs;

use anyhow::{Context, Result, anyhow};

/// Validate whether a command is allowed to run under the execution policy.
///
/// The policy is inspired by the Codex execution policy and limits commands to
/// a curated allow-list with argument validation to prevent workspace
/// breakout or destructive actions.
pub async fn validate_command(
    command: &[String],
    workspace_root: &Path,
    working_dir: &Path,
) -> Result<()> {
    if command.is_empty() {
        return Err(anyhow!("command cannot be empty"));
    }

    let program = command[0].as_str();
    let args = &command[1..];

    match program {
        "echo" => validate_echo(args),
        "ls" => validate_ls(args, workspace_root, working_dir).await,
        "cat" => validate_cat(args, workspace_root, working_dir).await,
        "cp" => validate_cp(args, workspace_root, working_dir).await,
        "head" => validate_head(args, workspace_root, working_dir).await,
        "tail" => validate_tail(args, workspace_root, working_dir).await,
        "printenv" => validate_printenv(args),
        "pwd" => validate_pwd(args),
        "rg" => validate_rg(args, workspace_root, working_dir).await,
        "grep" => validate_grep(args, workspace_root, working_dir).await,
        "sed" => validate_sed(args, workspace_root, working_dir).await,
        "which" => validate_which(args),
        "date" => validate_date(args),
        "whoami" => validate_whoami(args),
        "hostname" => validate_hostname(args),
        "uname" => validate_uname(args),
        "wc" => validate_wc(args, workspace_root, working_dir).await,
        "git" => validate_git(args, workspace_root, working_dir).await,
        "cargo" => validate_cargo(args, workspace_root, working_dir).await,
        "python" | "python3" => validate_python(args, workspace_root, working_dir).await,
        "npm" => validate_npm(args, workspace_root, working_dir).await,
        "node" => validate_node(args, workspace_root, working_dir).await,
        other => Err(anyhow!(
            "command '{}' is not permitted by the execution policy",
            other
        )),
    }
}

/// Normalize a working directory relative to the workspace root.
pub async fn sanitize_working_dir(
    workspace_root: &Path,
    working_dir: Option<&str>,
) -> Result<PathBuf> {
    let normalized_root = normalize_workspace_root(workspace_root)?;
    if let Some(dir) = working_dir {
        if dir.trim().is_empty() {
            return Ok(normalized_root);
        }
        let candidate = normalize_path(&normalized_root.join(dir));
        if !candidate.starts_with(&normalized_root) {
            return Err(anyhow!(
                "working directory '{}' escapes the workspace root",
                dir
            ));
        }
        ensure_within_workspace(&normalized_root, &candidate).await?;
        Ok(candidate)
    } else {
        Ok(normalized_root)
    }
}

fn validate_echo(args: &[String]) -> Result<()> {
    for arg in args {
        if arg.starts_with('-') {
            match arg.as_str() {
                "-n" | "-e" | "-E" => continue,
                _ => {
                    return Err(anyhow!("unsupported echo flag '{}'", arg));
                }
            }
        }
    }
    Ok(())
}

async fn validate_ls(args: &[String], workspace_root: &Path, working_dir: &Path) -> Result<()> {
    for arg in args {
        match arg.as_str() {
            "-1" | "-a" | "-l" => continue,
            value if value.starts_with('-') => {
                return Err(anyhow!("unsupported ls flag '{}'", value));
            }
            value => {
                let path = resolve_path(workspace_root, working_dir, value).await?;
                ensure_path_exists(&path)?;
            }
        }
    }
    Ok(())
}

async fn validate_cat(args: &[String], workspace_root: &Path, working_dir: &Path) -> Result<()> {
    let mut files = Vec::new();
    for arg in args {
        match arg.as_str() {
            "-b" | "-n" | "-t" => continue,
            value if value.starts_with('-') => {
                return Err(anyhow!("unsupported cat flag '{}'", value));
            }
            value => {
                let path = resolve_path(workspace_root, working_dir, value).await?;
                ensure_is_file(&path).await?;
                files.push(path);
            }
        }
    }

    if files.is_empty() {
        return Err(anyhow!("cat requires at least one readable file"));
    }

    Ok(())
}

async fn validate_cp(args: &[String], workspace_root: &Path, working_dir: &Path) -> Result<()> {
    let mut positional = Vec::new();
    let mut allow_recursive = false;

    for arg in args {
        match arg.as_str() {
            "-r" | "-R" | "--recursive" => {
                allow_recursive = true;
            }
            value if value.starts_with('-') => {
                return Err(anyhow!("unsupported cp flag '{}'", value));
            }
            value => positional.push(value.to_string()),
        }
    }

    if positional.len() < 2 {
        return Err(anyhow!("cp requires a source and destination"));
    }

    let dest_raw = positional.last().unwrap();
    let sources = &positional[..positional.len() - 1];

    for source in sources {
        let path = resolve_path(workspace_root, working_dir, source).await?;
        let metadata = fs::metadata(&path)
            .await
            .with_context(|| format!("failed to inspect source '{}'", source))?;
        if metadata.is_dir() && !allow_recursive {
            return Err(anyhow!(
                "copying directories requires the recursive flag for '{}'",
                source
            ));
        }
        if !metadata.is_file() && !metadata.is_dir() {
            return Err(anyhow!("unsupported source type for '{}'", source));
        }
    }

    let dest_path = resolve_path_allow_new(workspace_root, working_dir, dest_raw).await?;
    if let Some(parent) = dest_path.parent() {
        if !fs::try_exists(parent).await.unwrap_or(false) {
            return Err(anyhow!(
                "destination parent '{}' must exist",
                parent.display()
            ));
        }
    }

    Ok(())
}

async fn validate_head(args: &[String], workspace_root: &Path, working_dir: &Path) -> Result<()> {
    let mut positional = Vec::new();
    let mut index = 0;

    while index < args.len() {
        let current = &args[index];
        match current.as_str() {
            "-c" | "-n" => {
                let value = args
                    .get(index + 1)
                    .ok_or_else(|| anyhow!("option '{}' requires a value", current))?;
                parse_positive_int(value)
                    .with_context(|| format!("invalid value '{}' for '{}'", value, current))?;
                index += 2;
            }
            value if value.starts_with('-') => {
                return Err(anyhow!("unsupported head flag '{}'", value));
            }
            value => {
                positional.push(value);
                index += 1;
            }
        }
    }

    if positional.is_empty() {
        return Err(anyhow!("head requires at least one file"));
    }

    for file in positional {
        let path = resolve_path(workspace_root, working_dir, file).await?;
        ensure_is_file(&path).await?;
    }

    Ok(())
}

fn validate_printenv(args: &[String]) -> Result<()> {
    match args.len() {
        0 => Ok(()),
        1 => {
            let name = &args[0];
            if name.is_empty()
                || !name
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
            {
                return Err(anyhow!("invalid environment variable name '{}'", name));
            }
            Ok(())
        }
        _ => Err(anyhow!("printenv accepts zero or one argument")),
    }
}

fn validate_pwd(args: &[String]) -> Result<()> {
    if args.is_empty() {
        Ok(())
    } else {
        Err(anyhow!("pwd does not accept arguments"))
    }
}

async fn validate_rg(args: &[String], workspace_root: &Path, working_dir: &Path) -> Result<()> {
    let mut index = 0;
    let mut allow_no_pattern = false;

    while index < args.len() {
        let current = &args[index];
        if current == "--" {
            index += 1;
            break;
        }

        match current.as_str() {
            // SECURITY: Block preprocessor flags that enable arbitrary command execution
            "--pre" | "--pre-glob" => {
                return Err(anyhow!(
                    "ripgrep preprocessor flag '{}' is not permitted for security reasons. \
                     This flag enables arbitrary command execution.",
                    current
                ));
            }
            "-A" | "-B" | "-C" | "-d" | "--max-depth" | "-m" | "--max-count" => {
                let value = args
                    .get(index + 1)
                    .ok_or_else(|| anyhow!("option '{}' requires a value", current))?;
                parse_positive_int(value)
                    .with_context(|| format!("invalid value '{}' for '{}'", value, current))?;
                index += 2;
            }
            "-g" | "--glob" => {
                let value = args
                    .get(index + 1)
                    .ok_or_else(|| anyhow!("option '{}' requires a value", current))?;
                if value.is_empty() {
                    return Err(anyhow!("glob value for '{}' cannot be empty", current));
                }
                index += 2;
            }
            "-n" | "-i" | "-l" | "--files" | "--files-with-matches" | "--files-without-match" => {
                if matches!(
                    current.as_str(),
                    "--files" | "--files-with-matches" | "--files-without-match"
                ) {
                    allow_no_pattern = true;
                }
                index += 1;
            }
            value if value.starts_with('-') => {
                return Err(anyhow!("unsupported ripgrep flag '{}'", value));
            }
            _ => break,
        }
    }

    let remaining = &args[index..];
    if remaining.is_empty() && !allow_no_pattern {
        return Err(anyhow!(
            "ripgrep requires a pattern unless file listing flags are used"
        ));
    }

    let mut rem_index = 0;
    if !remaining.is_empty() {
        let pattern = &remaining[0];
        if pattern.is_empty() {
            return Err(anyhow!("ripgrep pattern cannot be empty"));
        }
        rem_index = 1;
    }

    if remaining.len() > rem_index {
        let search_root = &remaining[rem_index];
        let path = resolve_path_allow_dir(workspace_root, working_dir, search_root).await?;
        if !fs::try_exists(&path).await.unwrap_or(false) {
            return Err(anyhow!("search path '{}' does not exist", search_root));
        }
        if remaining.len() > rem_index + 1 {
            return Err(anyhow!("ripgrep accepts at most one search path"));
        }
    }

    Ok(())
}

async fn validate_sed(args: &[String], workspace_root: &Path, working_dir: &Path) -> Result<()> {
    let mut commands = Vec::new();
    let mut files = Vec::new();
    let mut index = 0;

    while index < args.len() {
        let current = &args[index];
        match current.as_str() {
            "-n" | "-u" => {
                index += 1;
            }
            "-e" => {
                let value = args
                    .get(index + 1)
                    .ok_or_else(|| anyhow!("-e requires a sed command"))?;
                ensure_safe_sed_command(value)?;
                commands.push(value.clone());
                index += 2;
            }
            value if value.starts_with('-') => {
                return Err(anyhow!("unsupported sed flag '{}'", value));
            }
            value => {
                if commands.is_empty() {
                    ensure_safe_sed_command(value)?;
                    commands.push(value.to_string());
                    index += 1;
                } else {
                    let path = resolve_path(workspace_root, working_dir, value).await?;
                    ensure_is_file(&path).await?;
                    files.push(path);
                    index += 1;
                }
            }
        }
    }

    if commands.is_empty() {
        return Err(anyhow!("sed requires at least one command"));
    }

    if files.is_empty() {
        return Err(anyhow!("sed requires at least one readable file"));
    }

    Ok(())
}

fn validate_which(args: &[String]) -> Result<()> {
    if args.is_empty() {
        return Err(anyhow!("which requires at least one program name"));
    }

    for arg in args {
        match arg.as_str() {
            "-a" | "-s" => continue,
            value if value.starts_with('-') => {
                return Err(anyhow!("unsupported which flag '{}'", value));
            }
            value => {
                if value.is_empty()
                    || value.contains('/')
                    || value.chars().any(|ch| ch.is_whitespace())
                {
                    return Err(anyhow!(
                        "program name '{}' contains unsupported characters",
                        value
                    ));
                }
            }
        }
    }

    Ok(())
}

async fn validate_git(args: &[String], workspace_root: &Path, working_dir: &Path) -> Result<()> {
    if args.is_empty() {
        return Err(anyhow!("git requires a subcommand"));
    }

    let subcommand = args[0].as_str();
    let subargs = &args[1..];

    // Tier 1: Safe read-only operations (always allowed)
    match subcommand {
        // Status and history operations
        "status" | "log" | "show" | "diff" | "branch" | "tag" | "remote" => {
            return validate_git_read_only(subcommand, subargs);
        }

        // Tree and object inspection
        "ls-tree" | "ls-files" | "cat-file" | "rev-parse" | "describe" => {
            return validate_git_read_only(subcommand, subargs);
        }

        // Config inspection (read-only)
        "config" if subargs.is_empty() || subargs.iter().all(|a| !a.starts_with("--")) => {
            return validate_git_read_only(subcommand, subargs);
        }

        // Additional inspection commands
        "blame" | "grep" | "shortlog" | "format-patch" => {
            return validate_git_read_only(subcommand, subargs);
        }

        // Stash operations (safe list/show)
        "stash"
            if matches!(
                subargs.first().map(|s| s.as_str()),
                Some("list" | "show" | "pop" | "apply" | "drop")
            ) =>
        {
            return validate_git_stash(subargs);
        }

        // Tier 2: Safe write operations (with validation)
        "add" => return validate_git_add(subargs, workspace_root, working_dir).await,
        "commit" => return validate_git_commit(subargs),
        "reset" => return validate_git_reset(subargs),
        "checkout" | "switch" => return validate_git_checkout(subargs, workspace_root, working_dir).await,
        "restore" => return validate_git_checkout(subargs, workspace_root, working_dir).await,
        "merge" => return validate_git_merge(subargs),
        "tag" if !subargs.is_empty() && !subargs[0].starts_with('-') => {
            return validate_git_read_only(subcommand, subargs);
        }

        // Tier 3: Dangerous operations (always blocked)
        "push" => {
            // Check for force flags
            if subargs
                .iter()
                .any(|a| a.contains("force") || a == "-f" || a == "--no-verify")
            {
                return Err(anyhow!(
                    "git push with force flags is not permitted. Use safe push operations only."
                ));
            }
            return validate_git_read_only(subcommand, subargs);
        }

        "force-push" => {
            return Err(anyhow!(
                "git force-push is not permitted by the execution policy"
            ));
        }

        "clean" => {
            return Err(anyhow!(
                "git clean is not permitted by the execution policy. Use explicit rm commands instead."
            ));
        }

        "gc" if subargs.iter().any(|a| a.contains("aggressive")) => {
            return Err(anyhow!("git gc with aggressive flag is not permitted"));
        }

        "filter-branch" | "rebase" | "cherry-pick" => {
            return Err(anyhow!(
                "git {} is not permitted - complex history operations require confirmation",
                subcommand
            ));
        }

        other => {
            return Err(anyhow!(
                "git subcommand '{}' is not permitted by the execution policy",
                other
            ));
        }
    }
}

fn validate_git_read_only(subcommand: &str, subargs: &[String]) -> Result<()> {
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
            return Err(anyhow!(
                "git argument contains suspicious shell metacharacters"
            ));
        }
    }

    Ok(())
}

async fn validate_git_add(
    args: &[String],
    workspace_root: &Path,
    working_dir: &Path,
) -> Result<()> {
    // Block dangerous flags
    if args.contains(&"-f".to_string()) || args.contains(&"--force".to_string()) {
        return Err(anyhow!(
            "git add --force is not permitted. Use regular add operations only."
        ));
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
                return Err(anyhow!("unsupported git add flag '{}'", value));
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

fn validate_git_commit(args: &[String]) -> Result<()> {
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
                return Err(anyhow!("unsupported git commit flag '{}'", value));
            }
            _ => {
                index += 1;
            }
        }
    }

    Ok(())
}

fn validate_git_reset(args: &[String]) -> Result<()> {
    // Block destructive reset modes
    if args.contains(&"--hard".to_string())
        || args.contains(&"--merge".to_string())
        || args.contains(&"--keep".to_string())
    {
        return Err(anyhow!(
            "git reset with --hard, --merge, or --keep is not permitted. Use --soft or --mixed instead."
        ));
    }

    // Allow safe flags: --soft, --mixed, --unstage
    let safe_modes = ["--soft", "--mixed", "--unstage"];
    for arg in args {
        if arg.starts_with('-') && !safe_modes.iter().any(|m| arg.contains(m)) {
            match arg.as_str() {
                "-q" | "--quiet" | "-p" | "--patch" => continue,
                _ => {
                    return Err(anyhow!(
                        "unsupported git reset flag '{}'. Use --soft or --mixed modes.",
                        arg
                    ));
                }
            }
        }
    }

    Ok(())
}

async fn validate_git_checkout(
    args: &[String],
    workspace_root: &Path,
    working_dir: &Path,
) -> Result<()> {
    if args.is_empty() {
        return Ok(());
    }

    // Block forced checkout
    if args.contains(&"-f".to_string()) || args.contains(&"--force".to_string()) {
        return Err(anyhow!(
            "git checkout --force is not permitted. Use regular checkout instead."
        ));
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

fn validate_git_stash(args: &[String]) -> Result<()> {
    if args.is_empty() {
        return Ok(());
    }

    let allowed_ops = ["list", "show", "pop", "apply", "drop", "clear", "create"];
    let first = args[0].as_str();

    if !allowed_ops.contains(&first) {
        return Err(anyhow!("git stash operation '{}' is not permitted", first));
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
                _ => return Err(anyhow!("unsupported git stash flag '{}'", arg)),
            }
        }
    }

    Ok(())
}

fn validate_git_merge(args: &[String]) -> Result<()> {
    // git merge is allowed for typical workflow
    if args.is_empty() {
        return Err(anyhow!("git merge requires a branch"));
    }

    // Block dangerous flags
    let dangerous_flags = ["--no-ff", "--squash"];
    for arg in args {
        if dangerous_flags.contains(&arg.as_str()) {
            return Err(anyhow!(
                "git merge with {} flag is not permitted; use simpler merge",
                arg
            ));
        }
    }

    Ok(())
}

async fn resolve_path(workspace_root: &Path, working_dir: &Path, value: &str) -> Result<PathBuf> {
    let base = build_candidate_path(workspace_root, working_dir, value).await?;
    if !fs::try_exists(&base).await.unwrap_or(false) {
        return Err(anyhow!("path '{}' does not exist", value));
    }
    if !base.starts_with(workspace_root) {
        return Err(anyhow!("path '{}' is outside the workspace root", value));
    }
    Ok(base)
}

async fn resolve_path_allow_new(
    workspace_root: &Path,
    working_dir: &Path,
    value: &str,
) -> Result<PathBuf> {
    let candidate = build_candidate_path(workspace_root, working_dir, value).await?;
    if !candidate.starts_with(workspace_root) {
        return Err(anyhow!("path '{}' is outside the workspace root", value));
    }
    Ok(candidate)
}

async fn resolve_path_allow_dir(
    workspace_root: &Path,
    working_dir: &Path,
    value: &str,
) -> Result<PathBuf> {
    let candidate = build_candidate_path(workspace_root, working_dir, value).await?;
    if !candidate.starts_with(workspace_root) {
        return Err(anyhow!("path '{}' is outside the workspace root", value));
    }
    Ok(candidate)
}

async fn build_candidate_path(
    workspace_root: &Path,
    working_dir: &Path,
    value: &str,
) -> Result<PathBuf> {
    let normalized_root = normalize_workspace_root(workspace_root)?;
    let normalized_working = normalize_path(working_dir);
    let raw_path = Path::new(value);
    let candidate = if raw_path.is_absolute() {
        normalize_path(raw_path)
    } else {
        normalize_path(&normalized_working.join(raw_path))
    };

    if !candidate.starts_with(&normalized_root) {
        return Err(anyhow!("path '{}' escapes the workspace root", value));
    }
    ensure_within_workspace(&normalized_root, &candidate).await?;
    Ok(candidate)
}

fn normalize_workspace_root(workspace_root: &Path) -> Result<PathBuf> {
    if workspace_root.is_absolute() {
        return Ok(normalize_path(workspace_root));
    }

    let cwd = env::current_dir().context("failed to resolve current working directory")?;
    Ok(normalize_path(&cwd.join(workspace_root)))
}

fn ensure_path_exists(path: &Path) -> Result<()> {
    if path.exists() {
        Ok(())
    } else {
        Err(anyhow!("path '{}' does not exist", path.display()))
    }
}

async fn ensure_is_file(path: &Path) -> Result<()> {
    let metadata = fs::metadata(path)
        .await
        .with_context(|| format!("failed to inspect '{}'", path.display()))?;
    if metadata.is_file() {
        Ok(())
    } else {
        Err(anyhow!("'{}' is not a file", path.display()))
    }
}

fn parse_positive_int(value: &str) -> Result<u64> {
    let parsed: u64 = value.parse()?;
    if parsed == 0 {
        return Err(anyhow!("value must be greater than zero"));
    }
    Ok(parsed)
}

fn ensure_safe_sed_command(value: &str) -> Result<()> {
    if value.trim().is_empty() {
        return Err(anyhow!("sed command cannot be empty"));
    }
    if value.contains([';', '|', '&', '`']) {
        return Err(anyhow!(
            "sed command contains unsupported control characters"
        ));
    }

    let mut chars = value.chars();
    if chars.next() != Some('s') {
        return Err(anyhow!("only sed substitution commands are supported"));
    }
    let delimiter = chars
        .next()
        .ok_or_else(|| anyhow!("sed substitution is missing a delimiter"))?;
    if delimiter.is_ascii_alphanumeric() || delimiter.is_ascii_whitespace() {
        return Err(anyhow!("invalid sed delimiter"));
    }

    let mut pattern = String::new();
    let mut replacement = String::new();
    let mut flags = String::new();

    parse_sed_section(&mut chars, delimiter, &mut pattern)?;
    parse_sed_section(&mut chars, delimiter, &mut replacement)?;
    collect_sed_flags(chars, &mut flags)?;

    if flags.chars().any(|ch| matches!(ch, 'e' | 'E' | 'F' | 'f')) {
        return Err(anyhow!(
            "sed execution flags are not permitted in substitution"
        ));
    }

    Ok(())
}

async fn ensure_within_workspace(normalized_root: &Path, candidate: &Path) -> Result<()> {
    let canonical_root = fs::canonicalize(normalized_root).await.with_context(|| {
        format!(
            "failed to canonicalize workspace root '{}'",
            normalized_root.display()
        )
    })?;

    if normalized_root == candidate {
        return Ok(());
    }

    let relative = candidate
        .strip_prefix(normalized_root)
        .map_err(|_| anyhow!("path '{}' escapes the workspace root", candidate.display()))?;

    let mut prefix = normalized_root.to_path_buf();
    let mut components = relative.components().peekable();

    while let Some(component) = components.next() {
        prefix.push(component.as_os_str());

        let metadata = match fs::symlink_metadata(&prefix).await {
            Ok(metadata) => metadata,
            Err(error) => {
                if error.kind() == io::ErrorKind::NotFound {
                    break;
                }
                return Err(error).with_context(|| {
                    format!("failed to inspect path component '{}'", prefix.display())
                });
            }
        };

        if metadata.file_type().is_symlink() {
            let resolved = fs::canonicalize(&prefix).await.with_context(|| {
                format!(
                    "failed to canonicalize path component '{}'",
                    prefix.display()
                )
            })?;
            if !resolved.starts_with(&canonical_root) {
                return Err(anyhow!(
                    "path '{}' escapes the workspace root via symlink '{}'",
                    candidate.display(),
                    prefix.display()
                ));
            }
        } else {
            let resolved = fs::canonicalize(&prefix).await.with_context(|| {
                format!(
                    "failed to canonicalize path component '{}'",
                    prefix.display()
                )
            })?;
            if !resolved.starts_with(&canonical_root) {
                return Err(anyhow!(
                    "path '{}' escapes the workspace root via component '{}'",
                    candidate.display(),
                    prefix.display()
                ));
            }

            if metadata.is_file() && components.peek().is_some() {
                return Err(anyhow!(
                    "path '{}' traverses through file component '{}'",
                    candidate.display(),
                    prefix.display()
                ));
            }
        }
    }

    Ok(())
}

fn parse_sed_section(
    chars: &mut std::str::Chars<'_>,
    delimiter: char,
    target: &mut String,
) -> Result<()> {
    let mut escaped = false;
    while let Some(ch) = chars.next() {
        if escaped {
            target.push(ch);
            escaped = false;
            continue;
        }
        match ch {
            '\\' => {
                escaped = true;
            }
            value if value == delimiter => {
                return Ok(());
            }
            other => target.push(other),
        }
    }
    Err(anyhow!("sed command is missing a closing delimiter"))
}

fn collect_sed_flags(chars: std::str::Chars<'_>, target: &mut String) -> Result<()> {
    for ch in chars {
        if ch.is_ascii_alphabetic() {
            target.push(ch);
        } else {
            return Err(anyhow!("sed flags contain unsupported characters"));
        }
    }
    Ok(())
}

fn normalize_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                normalized.pop();
            }
            Component::CurDir => {}
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(component.as_os_str()),
            Component::Normal(part) => normalized.push(part),
        }
    }
    normalized
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_echo() {
        assert!(validate_echo(&[]).is_ok());
        assert!(validate_echo(&["hello".to_string()]).is_ok());
        assert!(validate_echo(&["-n".to_string(), "hello".to_string()]).is_ok());
        assert!(validate_echo(&["-e".to_string(), "test".to_string()]).is_ok());
        assert!(validate_echo(&["--invalid".to_string()]).is_err());
    }

    #[test]
    fn test_validate_pwd() {
        assert!(validate_pwd(&[]).is_ok());
        assert!(validate_pwd(&["arg".to_string()]).is_err());
    }

    #[test]
    fn test_validate_printenv() {
        assert!(validate_printenv(&[]).is_ok());
        assert!(validate_printenv(&["PATH".to_string()]).is_ok());
        assert!(validate_printenv(&["MY_VAR_123".to_string()]).is_ok());
        assert!(validate_printenv(&["MY-VAR".to_string()]).is_err());
        assert!(validate_printenv(&["MY VAR".to_string()]).is_err());
        assert!(validate_printenv(&["VAR1".to_string(), "VAR2".to_string()]).is_err());
    }

    #[tokio::test]
    async fn test_validate_git_read_only() {
        // Safe read-only operations
        assert!(validate_git_read_only("status", &[]).is_ok());
        assert!(validate_git_read_only("log", &["--oneline".to_string()]).is_ok());
        assert!(validate_git_read_only("diff", &["-p".to_string()]).is_ok());
        assert!(validate_git_read_only("show", &["HEAD".to_string()]).is_ok());
        assert!(validate_git_read_only("branch", &["-a".to_string()]).is_ok());

        // Dangerous patterns blocked
        assert!(
            validate_git_read_only("log", &["--format".to_string(), "test;cat".to_string()])
                .is_err()
        );
    }

    #[test]
    fn test_validate_git_commit() {
        // Valid commits
        assert!(validate_git_commit(&["-m".to_string(), "fix: test".to_string()]).is_ok());
        assert!(validate_git_commit(&["-a".to_string()]).is_ok());
        assert!(validate_git_commit(&["--amend".to_string()]).is_ok());

        // Invalid commits
        assert!(validate_git_commit(&["-m".to_string()]).is_err()); // Missing message
        assert!(validate_git_commit(&["--invalid-flag".to_string()]).is_err());
    }

    #[test]
    fn test_validate_git_reset() {
        // Safe reset modes
        assert!(validate_git_reset(&["--soft".to_string()]).is_ok());
        assert!(validate_git_reset(&["--mixed".to_string()]).is_ok());
        assert!(validate_git_reset(&["--unstage".to_string()]).is_ok());
        assert!(validate_git_reset(&[]).is_ok());

        // Dangerous reset modes
        assert!(validate_git_reset(&["--hard".to_string()]).is_err());
        assert!(validate_git_reset(&["--merge".to_string()]).is_err());
        assert!(validate_git_reset(&["--keep".to_string()]).is_err());
    }

    #[test]
    fn test_validate_git_stash() {
        // Safe stash operations
        assert!(validate_git_stash(&["list".to_string()]).is_ok());
        assert!(validate_git_stash(&["show".to_string()]).is_ok());
        assert!(validate_git_stash(&["pop".to_string()]).is_ok());
        assert!(validate_git_stash(&["apply".to_string()]).is_ok());
        assert!(validate_git_stash(&["drop".to_string()]).is_ok());

        // Dangerous operations
        assert!(validate_git_stash(&["push".to_string()]).is_err());
        assert!(validate_git_stash(&["save".to_string()]).is_err());
    }

    #[tokio::test]
    async fn test_validate_git_safe_operations() {
        let workspace = std::path::PathBuf::from("/tmp");
        let working = std::path::PathBuf::from("/tmp");

        // Safe read-only operations should be allowed
        assert!(
            validate_git(&["status".to_string()], &workspace, &working)
                .await
                .is_ok()
        );
        assert!(
            validate_git(
                &["log".to_string(), "--oneline".to_string()],
                &workspace,
                &working
            )
            .await
            .is_ok()
        );
        assert!(
            validate_git(&["diff".to_string()], &workspace, &working)
                .await
                .is_ok()
        );
        assert!(
            validate_git(
                &["show".to_string(), "HEAD".to_string()],
                &workspace,
                &working
            )
            .await
            .is_ok()
        );
    }

    #[tokio::test]
    async fn test_validate_git_dangerous_operations_blocked() {
        let workspace = std::path::PathBuf::from("/tmp");
        let working = std::path::PathBuf::from("/tmp");

        // Dangerous operations should be blocked
        assert!(
            validate_git(
                &["push".to_string(), "--force".to_string()],
                &workspace,
                &working
            )
            .await
            .is_err()
        );
        assert!(
            validate_git(
                &["push".to_string(), "-f".to_string()],
                &workspace,
                &working
            )
            .await
            .is_err()
        );
        assert!(
            validate_git(&["clean".to_string()], &workspace, &working)
                .await
                .is_err()
        );
        assert!(
            validate_git(&["filter-branch".to_string()], &workspace, &working)
                .await
                .is_err()
        );
        assert!(
            validate_git(&["rebase".to_string()], &workspace, &working)
                .await
                .is_err()
        );
        assert!(
            validate_git(&["cherry-pick".to_string()], &workspace, &working)
                .await
                .is_err()
        );
    }

    #[test]
    fn test_validate_which() {
        assert!(validate_which(&["ls".to_string()]).is_ok());
        assert!(validate_which(&["git".to_string(), "-a".to_string()]).is_ok());
        assert!(validate_which(&[]).is_err());
        assert!(validate_which(&["/usr/bin/ls".to_string()]).is_err()); // Contains /
        assert!(validate_which(&["ls git".to_string()]).is_err()); // Contains space
    }
}

// Additional validators for common utilities
async fn validate_tail(args: &[String], workspace_root: &Path, working_dir: &Path) -> Result<()> {
    // tail is read-only, similar to head
    for arg in args {
        if !arg.starts_with('-') {
            let path = normalize_path(&working_dir.join(arg));
            ensure_within_workspace(workspace_root, &path).await?;
        }
    }
    Ok(())
}

async fn validate_grep(args: &[String], workspace_root: &Path, working_dir: &Path) -> Result<()> {
    // grep is read-only pattern search
    let mut pattern_seen = false;
    for arg in args {
        if !arg.starts_with('-') && pattern_seen {
            // Files come after pattern
            let path = normalize_path(&working_dir.join(arg));
            ensure_within_workspace(workspace_root, &path).await?;
        } else if !arg.starts_with('-') {
            pattern_seen = true;
        }
    }
    Ok(())
}

fn validate_date(args: &[String]) -> Result<()> {
    // date just displays current date/time, safe with format args
    for arg in args {
        if arg.starts_with('+') {
            // Format string is safe
            continue;
        }
    }
    Ok(())
}

fn validate_whoami(_args: &[String]) -> Result<()> {
    // whoami has no arguments, always safe
    Ok(())
}

fn validate_hostname(_args: &[String]) -> Result<()> {
    // hostname has no arguments, always safe
    Ok(())
}

fn validate_uname(args: &[String]) -> Result<()> {
    // uname only accepts specific flags
    let safe_flags = ["-a", "-s", "-n", "-r", "-v", "-m"];
    for arg in args {
        if arg.starts_with('-') && !safe_flags.contains(&arg.as_str()) {
            return Err(anyhow!("unsupported uname flag '{}'", arg));
        }
    }
    Ok(())
}

async fn validate_wc(args: &[String], workspace_root: &Path, working_dir: &Path) -> Result<()> {
    // wc is read-only, similar to head
    for arg in args {
        if !arg.starts_with('-') {
            let path = normalize_path(&working_dir.join(arg));
            ensure_within_workspace(workspace_root, &path).await?;
        }
    }
    Ok(())
}

async fn validate_cargo(args: &[String], workspace_root: &Path, working_dir: &Path) -> Result<()> {
    // Cargo commands - allow typical dev workflow operations
    if args.is_empty() {
        return Err(anyhow!("cargo requires a subcommand"));
    }

    let subcommand = args[0].as_str();
    match subcommand {
        // Safe read-only, build, and development operations
        "build" | "check" | "test" | "doc" | "clippy" | "fmt" | "run" | "bench" | "expand"
        | "tree" | "metadata" | "search" | "cache" => {
            // These are generally safe - check working directory is in workspace
            ensure_within_workspace(workspace_root, working_dir).await?;
            Ok(())
        }
        // Dangerous operations that modify system or registry
        "clean" | "install" | "uninstall" | "publish" | "yank" => {
            Err(anyhow!(
                "cargo {} is not permitted by the execution policy",
                subcommand
            ))
        }
        other => Err(anyhow!(
            "cargo subcommand '{}' is not permitted by the execution policy",
            other
        )),
    }
}

async fn validate_python(args: &[String], workspace_root: &Path, working_dir: &Path) -> Result<()> {
    // Python allows running scripts and modules - safe for workspace
    ensure_within_workspace(workspace_root, working_dir).await?;
    // Don't allow -c or eval-like flags that could be dangerous - only file/module execution
    if args.is_empty() {
        return Ok(()); // python interactive is allowed
    }
    
    let first_arg = &args[0];
    if first_arg == "-c" || first_arg == "-m" || first_arg == "-W" {
        // Allow -m (module), -W (warnings), but validate any file paths
        if first_arg != "-m" && args.len() > 1 {
            let path = normalize_path(&working_dir.join(&args[1]));
            ensure_within_workspace(workspace_root, &path).await?;
        }
    } else if !first_arg.starts_with('-') {
        // It's a script file - validate it exists in workspace
        let path = normalize_path(&working_dir.join(first_arg));
        ensure_within_workspace(workspace_root, &path).await?;
    }
    Ok(())
}

async fn validate_npm(args: &[String], workspace_root: &Path, working_dir: &Path) -> Result<()> {
    // NPM commands - allow typical dev operations
    ensure_within_workspace(workspace_root, working_dir).await?;
    if args.is_empty() {
        return Ok(());
    }

    let subcommand = args[0].as_str();
    match subcommand {
        // Dangerous operations
        "publish" | "unpublish" => {
            Err(anyhow!(
                "npm {} is not permitted by the execution policy",
                subcommand
            ))
        }
        // Allow safe and other commands by default, as npm is generally safe in workspace
        _ => Ok(()),
    }
}

async fn validate_node(args: &[String], workspace_root: &Path, working_dir: &Path) -> Result<()> {
    // Node.js script execution - safe for workspace
    ensure_within_workspace(workspace_root, working_dir).await?;
    if args.is_empty() {
        return Ok(()); // node interactive/REPL
    }

    let first_arg = &args[0];
    if !first_arg.starts_with('-') {
        // It's a script file - validate it exists in workspace
        let path = normalize_path(&working_dir.join(first_arg));
        ensure_within_workspace(workspace_root, &path).await?;
    }
    Ok(())
}
