#![expect(
    clippy::indexing_slicing,
    reason = "Command validators inspect byte-oriented option tokens after checking their lengths and delimiters."
)]

use std::env;
use std::path::{Path, PathBuf};
use tokio::fs;

use anyhow::{Context, Result, anyhow};
use vtcode_commons::paths::normalize_path;

mod git;

use git::{
    validate_git_add, validate_git_checkout, validate_git_commit, validate_git_merge, validate_git_read_only,
    validate_git_reset, validate_git_stash,
};

/// Helper to validate flags in arguments (reduces duplication in 10+ validators)
/// Returns error if any flag not in allowed_flags is found
fn validate_allowed_flags(args: &[String], allowed_flags: &[&str], command_name: &str) -> Result<()> {
    for arg in args {
        if arg.starts_with('-') && !allowed_flags.contains(&arg.as_str()) {
            return Err(anyhow!("unsupported {command_name} flag '{arg}'"));
        }
    }
    Ok(())
}

/// Helper to validate that a command accepts no arguments
fn validate_no_args(args: &[String], command_name: &str) -> Result<()> {
    if args.is_empty() {
        Ok(())
    } else {
        Err(anyhow!("{command_name} does not accept arguments"))
    }
}

/// Validate whether a command is allowed to run under the execution policy.
///
/// The policy is inspired by the Codex execution policy and limits commands to
/// a curated allow-list with argument validation to prevent workspace
/// breakout or destructive actions.
pub async fn validate_command(
    command: &[String],
    workspace_root: &Path,
    working_dir: &Path,
    confirm: bool,
) -> Result<()> {
    if command.is_empty() {
        return Err(anyhow!("command cannot be empty"));
    }

    let (program, args) = command
        .split_first()
        .ok_or_else(|| anyhow!("command cannot be empty (unexpected)"))?;
    let program = program.as_str();

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
        "git" => validate_git(args, workspace_root, working_dir, confirm).await,
        "cargo" => validate_cargo(args, workspace_root, working_dir, confirm).await,
        "python" | "python3" => validate_python(args, workspace_root, working_dir).await,
        "npm" => validate_npm(args, workspace_root, working_dir).await,
        "node" => validate_node(args, workspace_root, working_dir).await,
        other => Err(anyhow!("command '{other}' is not permitted by the execution policy")),
    }
}

/// Normalize a working directory relative to the workspace root.
pub async fn sanitize_working_dir(workspace_root: &Path, working_dir: Option<&str>) -> Result<PathBuf> {
    let normalized_root = normalize_workspace_root(workspace_root)?;
    if let Some(dir) = working_dir {
        if dir.trim().is_empty() {
            return Ok(normalized_root);
        }
        let candidate = normalize_path(&normalized_root.join(dir));
        if !candidate.starts_with(&normalized_root) {
            return Err(anyhow!("working directory '{dir}' escapes the workspace root"));
        }
        ensure_within_workspace(&normalized_root, &candidate).await?;
        Ok(candidate)
    } else {
        Ok(normalized_root)
    }
}

fn validate_echo(args: &[String]) -> Result<()> {
    validate_allowed_flags(args, &["-n", "-e", "-E"], "echo")
}

async fn validate_ls(args: &[String], workspace_root: &Path, working_dir: &Path) -> Result<()> {
    let allowed_ls_flags = &["-1", "-a", "-l"];
    for arg in args {
        if arg.starts_with('-') {
            if !allowed_ls_flags.contains(&arg.as_str()) {
                return Err(anyhow!("unsupported ls flag '{arg}'"));
            }
        } else {
            let path = resolve_path(workspace_root, working_dir, arg).await?;
            ensure_path_exists(&path)?;
        }
    }
    Ok(())
}

async fn validate_cat(args: &[String], workspace_root: &Path, working_dir: &Path) -> Result<()> {
    let allowed_cat_flags = &["-b", "-n", "-t"];
    let mut files = Vec::with_capacity(args.len());

    for arg in args {
        if arg.starts_with('-') {
            if !allowed_cat_flags.contains(&arg.as_str()) {
                return Err(anyhow!("unsupported cat flag '{arg}'"));
            }
        } else {
            let path = resolve_path(workspace_root, working_dir, arg).await?;
            ensure_is_file(&path).await?;
            files.push(path);
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
                return Err(anyhow!("unsupported cp flag '{value}'"));
            }
            value => positional.push(value.to_owned()),
        }
    }

    if positional.len() < 2 {
        return Err(anyhow!("cp requires a source and destination"));
    }

    let dest_raw = positional
        .last()
        .ok_or_else(|| anyhow!("cp command missing destination path"))?;
    let sources = &positional[..positional.len() - 1];

    for source in sources {
        let path = resolve_path(workspace_root, working_dir, source).await?;
        let metadata = fs::metadata(&path)
            .await
            .with_context(|| format!("failed to inspect source '{source}'"))?;
        if metadata.is_dir() && !allow_recursive {
            return Err(anyhow!("copying directories requires the recursive flag for '{source}'"));
        }
        if !metadata.is_file() && !metadata.is_dir() {
            return Err(anyhow!("unsupported source type for '{source}'"));
        }
    }

    let dest_path = resolve_path_allow_new(workspace_root, working_dir, dest_raw).await?;
    if let Some(parent) = dest_path.parent()
        && !fs::try_exists(parent).await.unwrap_or(false)
    {
        return Err(anyhow!("destination parent '{}' must exist", parent.display()));
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
                    .ok_or_else(|| anyhow!("option '{current}' requires a value"))?;
                let _parsed =
                    parse_positive_int(value).with_context(|| format!("invalid value '{value}' for '{current}'"))?;
                index += 2;
            }
            value if value.starts_with('-') => {
                return Err(anyhow!("unsupported head flag '{value}'"));
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
            if name.is_empty() || !name.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '_') {
                return Err(anyhow!("invalid environment variable name '{name}'"));
            }
            Ok(())
        }
        _ => Err(anyhow!("printenv accepts zero or one argument")),
    }
}

fn validate_pwd(args: &[String]) -> Result<()> {
    validate_no_args(args, "pwd")
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
                    "ripgrep preprocessor flag '{current}' is not permitted for security reasons. \
                     This flag enables arbitrary command execution."
                ));
            }
            "-A" | "-B" | "-C" | "-d" | "--max-depth" | "-m" | "--max-count" => {
                let value = args
                    .get(index + 1)
                    .ok_or_else(|| anyhow!("option '{current}' requires a value"))?;
                let _parsed =
                    parse_positive_int(value).with_context(|| format!("invalid value '{value}' for '{current}'"))?;
                index += 2;
            }
            "-g" | "--glob" => {
                let value = args
                    .get(index + 1)
                    .ok_or_else(|| anyhow!("option '{current}' requires a value"))?;
                if value.is_empty() {
                    return Err(anyhow!("glob value for '{current}' cannot be empty"));
                }
                index += 2;
            }
            "-n" | "-i" | "-l" | "--files" | "--files-with-matches" | "--files-without-match" => {
                if matches!(current.as_str(), "--files" | "--files-with-matches" | "--files-without-match") {
                    allow_no_pattern = true;
                }
                index += 1;
            }
            value if value.starts_with('-') => {
                return Err(anyhow!("unsupported ripgrep flag '{value}'"));
            }
            _ => break,
        }
    }

    let remaining = &args[index..];
    if remaining.is_empty() && !allow_no_pattern {
        return Err(anyhow!("ripgrep requires a pattern unless file listing flags are used"));
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
            return Err(anyhow!("search path '{search_root}' does not exist"));
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
                let value = args.get(index + 1).ok_or_else(|| anyhow!("-e requires a sed command"))?;
                ensure_safe_sed_command(value)?;
                commands.push(value.clone());
                index += 2;
            }
            value if value.starts_with('-') => {
                return Err(anyhow!("unsupported sed flag '{value}'"));
            }
            value => {
                if commands.is_empty() {
                    ensure_safe_sed_command(value)?;
                    commands.push(value.to_owned());
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
                return Err(anyhow!("unsupported which flag '{value}'"));
            }
            value => {
                if value.is_empty() || value.contains('/') || value.chars().any(|ch| ch.is_whitespace()) {
                    return Err(anyhow!("program name '{value}' contains unsupported characters"));
                }
            }
        }
    }

    Ok(())
}

async fn validate_git(args: &[String], workspace_root: &Path, working_dir: &Path, confirm: bool) -> Result<()> {
    if args.is_empty() {
        return Err(anyhow!("git requires a subcommand"));
    }

    let subcommand = args[0].as_str();
    let subargs = &args[1..];

    // Tier 1: Safe read-only operations (always allowed)
    match subcommand {
        // Status and history operations
        "status" | "log" | "show" | "diff" | "branch" | "tag" | "remote" => {
            // For tag command, add specific validation
            if subcommand == "tag" && !subargs.is_empty() && !subargs[0].starts_with('-') {
                // This condition was originally in the unreachable pattern
                // but now it's handled within the general case for "tag"
            }
            validate_git_read_only(subcommand, subargs)
        }

        // Tree and object inspection
        "ls-tree" | "ls-files" | "cat-file" | "rev-parse" | "describe" => validate_git_read_only(subcommand, subargs),

        // Config inspection (read-only)
        "config" if subargs.is_empty() || subargs.iter().all(|a| !a.starts_with("--")) => {
            validate_git_read_only(subcommand, subargs)
        }

        // Additional inspection commands
        "blame" | "grep" | "shortlog" | "format-patch" => validate_git_read_only(subcommand, subargs),

        // Stash operations (safe list/show)
        "stash" if matches!(subargs.first().map(|s| s.as_str()), Some("list" | "show" | "pop" | "apply" | "drop")) => {
            validate_git_stash(subargs)
        }

        // Tier 2: Safe write operations (with validation)
        "add" => validate_git_add(subargs, workspace_root, working_dir).await,
        "commit" => validate_git_commit(subargs),
        "reset" => validate_git_reset(subargs, confirm),
        "checkout" | "switch" => validate_git_checkout(subargs, workspace_root, working_dir, confirm).await,
        "restore" => validate_git_checkout(subargs, workspace_root, working_dir, confirm).await,
        "merge" => validate_git_merge(subargs),

        // Tier 3: Dangerous operations (always blocked)
        "push" => {
            // Check for force flags
            if subargs.iter().any(|a| a.contains("force") || a == "-f" || a == "--no-verify") {
                Err(anyhow!("git push with force flags is not permitted. Use safe push operations only."))
            } else {
                validate_git_read_only(subcommand, subargs)
            }
        }

        "force-push" => Err(anyhow!("git force-push is not permitted by the execution policy")),

        "clean" => {
            Err(anyhow!("git clean is not permitted by the execution policy. Use explicit rm commands instead."))
        }

        "gc" if subargs.iter().any(|a| a.contains("aggressive")) => {
            Err(anyhow!("git gc with aggressive flag is not permitted"))
        }

        "filter-branch" | "rebase" | "cherry-pick" => {
            Err(anyhow!("git {subcommand} is not permitted - complex history operations require confirmation"))
        }

        other => Err(anyhow!("git subcommand '{other}' is not permitted by the execution policy")),
    }
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
        return Err(anyhow!("sed command contains unsupported control characters"));
    }

    let mut chars = value.chars();
    if chars.next() != Some('s') {
        return Err(anyhow!("only sed substitution commands are supported"));
    }
    let delimiter = chars.next().ok_or_else(|| anyhow!("sed substitution is missing a delimiter"))?;
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
        return Err(anyhow!("sed execution flags are not permitted in substitution"));
    }

    Ok(())
}

async fn ensure_within_workspace(normalized_root: &Path, candidate: &Path) -> Result<()> {
    vtcode_commons::paths::ensure_path_within_workspace_resolved(candidate, normalized_root)
        .await
        .map(|_| ())
}

fn parse_sed_section(chars: &mut std::str::Chars<'_>, delimiter: char, target: &mut String) -> Result<()> {
    let mut escaped = false;
    for ch in chars.by_ref() {
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
            return Err(anyhow!("unsupported uname flag '{arg}'"));
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

async fn validate_cargo(args: &[String], workspace_root: &Path, working_dir: &Path, confirm: bool) -> Result<()> {
    // Cargo commands - allow typical dev workflow operations
    if args.is_empty() {
        return Err(anyhow!("cargo requires a subcommand"));
    }

    let subcommand = args[0].as_str();
    match subcommand {
        // Safe read-only, build, and development operations
        "build" | "check" | "test" | "doc" | "clippy" | "fmt" | "run" | "bench" | "expand" | "tree" | "metadata"
        | "search" | "cache" => {
            // These are generally safe - check working directory is in workspace
            ensure_within_workspace(workspace_root, working_dir).await?;
            Ok(())
        }
        // Operations that may be destructive or publish to remote registries
        "clean" | "install" | "uninstall" | "publish" | "yank" => {
            if confirm {
                // Allow with explicit confirmation
                ensure_within_workspace(workspace_root, working_dir).await?;
                Ok(())
            } else {
                Err(anyhow!("cargo {subcommand} is potentially destructive; set `confirm=true` to proceed."))
            }
        }
        other => Err(anyhow!("cargo subcommand '{other}' is not permitted by the execution policy")),
    }
}

async fn validate_python(args: &[String], workspace_root: &Path, working_dir: &Path) -> Result<()> {
    // Python allows running scripts and modules - safe for workspace
    ensure_within_workspace(workspace_root, working_dir).await?;
    if args.is_empty() {
        return Ok(()); // python interactive is allowed
    }

    let first_arg = &args[0];

    // Reject -c (execute code string) - arbitrary code execution risk
    if first_arg == "-c" {
        return Err(anyhow!(
            "python -c is not permitted for security reasons: \
             it enables arbitrary code execution outside the workspace sandbox"
        ));
    }

    // Reject -e and -c variants that bypass file-based execution
    if first_arg == "-e" || first_arg == "-exec" {
        return Err(anyhow!("python {first_arg} is not permitted: it enables arbitrary code execution"));
    }

    if first_arg == "-m" || first_arg == "-W" {
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
        "publish" | "unpublish" => Err(anyhow!("npm {subcommand} is not permitted by the execution policy")),
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

    // Reject -e/--eval (execute code string) - arbitrary code execution risk
    if first_arg == "-e" || first_arg == "--eval" {
        return Err(anyhow!(
            "node {first_arg} is not permitted for security reasons: \
             it enables arbitrary code execution outside the workspace sandbox"
        ));
    }

    if !first_arg.starts_with('-') {
        // It's a script file - validate it exists in workspace
        let path = normalize_path(&working_dir.join(first_arg));
        ensure_within_workspace(workspace_root, &path).await?;
    }
    Ok(())
}

async fn resolve_path(workspace_root: &Path, working_dir: &Path, value: &str) -> Result<PathBuf> {
    let base = build_candidate_path(workspace_root, working_dir, value).await?;
    if !fs::try_exists(&base).await.unwrap_or(false) {
        return Err(anyhow!("path '{value}' does not exist"));
    }
    if !base.starts_with(workspace_root) {
        return Err(anyhow!("path '{value}' is outside the workspace root"));
    }
    Ok(base)
}

async fn resolve_path_allow_new(workspace_root: &Path, working_dir: &Path, value: &str) -> Result<PathBuf> {
    let candidate = build_candidate_path(workspace_root, working_dir, value).await?;
    if !candidate.starts_with(workspace_root) {
        return Err(anyhow!("path '{value}' is outside the workspace root"));
    }
    Ok(candidate)
}

async fn resolve_path_allow_dir(workspace_root: &Path, working_dir: &Path, value: &str) -> Result<PathBuf> {
    let candidate = build_candidate_path(workspace_root, working_dir, value).await?;
    if !candidate.starts_with(workspace_root) {
        return Err(anyhow!("path '{value}' is outside the workspace root"));
    }
    Ok(candidate)
}

async fn build_candidate_path(workspace_root: &Path, working_dir: &Path, value: &str) -> Result<PathBuf> {
    let normalized_root = normalize_workspace_root(workspace_root)?;
    let normalized_working = normalize_path(working_dir);
    let raw_path = Path::new(value);
    let candidate = if raw_path.is_absolute() {
        normalize_path(raw_path)
    } else {
        normalize_path(&normalized_working.join(raw_path))
    };

    if !candidate.starts_with(&normalized_root) {
        return Err(anyhow!("path '{value}' escapes the workspace root"));
    }
    ensure_within_workspace(&normalized_root, &candidate).await?;
    Ok(candidate)
}

#[cfg(test)]
mod tests;
