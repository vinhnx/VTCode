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
        "printenv" => validate_printenv(args),
        "pwd" => validate_pwd(args),
        "rg" => validate_rg(args, workspace_root, working_dir).await,
        "sed" => validate_sed(args, workspace_root, working_dir).await,
        "which" => validate_which(args),
        "git" if args.first().map(|s| s.as_str()) == Some("diff") => Err(anyhow!(
            "command 'git diff' is not permitted by the execution policy. Use structured file reading tools instead."
        )),
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
