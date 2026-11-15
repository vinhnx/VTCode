//! Path, sensitivity, and sandbox policy for the filesystem workspace.
//!
//! Every path the workspace touches is validated, screened against the
//! sensitive-file policy, and checked against the sandbox policy before
//! any command runs.

use super::*;

pub(super) fn validate_relative_path(path: &str) -> Result<PathBuf> {
    if path.is_empty() || path.len() > 4096 || path.contains('\0') {
        return Err(WebmcpError::PathRejected(path.to_string()));
    }
    let path = Path::new(path);
    if path.is_absolute() {
        return Err(WebmcpError::PathRejected(path.display().to_string()));
    }
    let mut validated = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => validated.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(WebmcpError::PathRejected(path.display().to_string()));
            }
        }
    }
    if validated.as_os_str().is_empty() {
        return Err(WebmcpError::PathRejected(path.display().to_string()));
    }
    Ok(validated)
}

pub(super) fn reject_sensitive_relative_path(relative: &Path, original: &str) -> Result<()> {
    if is_sensitive_relative_path(relative) {
        return Err(WebmcpError::PathRejected(format!("sensitive workspace path is not exposed: {original}")));
    }
    Ok(())
}

/// Classify recorded paths before exposing change evidence. This lexical filter
/// does not authorize filesystem access; reads still require bound directory handles.
pub fn is_public_evidence_path(path: &str) -> bool {
    validate_relative_path(path).is_ok_and(|relative| {
        !is_sensitive_relative_path(&relative)
            && !relative.components().any(|component| {
                matches!(component, Component::Normal(name) if name.to_str().is_none_or(|name| IGNORED_DIRECTORY_NAMES.iter().any(|ignored| name.eq_ignore_ascii_case(ignored))))
            })
    })
}

pub(super) fn is_sensitive_relative_path(relative: &Path) -> bool {
    relative.components().any(|component| {
        let Component::Normal(name) = component else {
            return false;
        };
        let Some(name) = name.to_str() else {
            return true;
        };
        is_sensitive_file(name)
            || SENSITIVE_DIRECTORY_NAMES
                .iter()
                .any(|sensitive| name.eq_ignore_ascii_case(sensitive))
    })
}

#[cfg(unix)]
pub(super) fn has_multiple_hard_links(metadata: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;

    metadata.nlink() > 1
}

#[cfg(not(unix))]
const fn has_multiple_hard_links(_metadata: &std::fs::Metadata) -> bool {
    false
}

pub(super) fn reject_hard_links(metadata: &std::fs::Metadata, path: &str) -> Result<()> {
    if has_multiple_hard_links(metadata) {
        return Err(WebmcpError::PathRejected(format!("hard-linked file is not allowed: {path}")));
    }
    Ok(())
}

pub(super) fn digest_text(content: &str) -> String {
    use std::fmt::Write as _;

    let digest = Sha256::digest(content.as_bytes());
    let mut encoded = String::with_capacity(7 + digest.len() * 2);
    encoded.push_str("sha256:");
    // `write!` appends into the pre-sized buffer: the previous per-byte
    // `format!` allocated a fresh `String` 32 times per digest.
    for byte in digest {
        let _ignored = write!(encoded, "{byte:02x}");
    }
    encoded
}

pub(super) fn list_files_blocking(
    root: &Path,
    root_dir: &std::fs::File,
    limits: FilesystemLimits,
) -> Result<Vec<WorkspaceFile>> {
    let mut files = Vec::new();
    let mut total_bytes = 0usize;
    let mut visited_directories = 0usize;
    visit_directory(root, root_dir, root, 0, limits, &mut total_bytes, &mut visited_directories, &mut files)?;
    files.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(files)
}

pub(super) fn visit_directory(
    root: &Path,
    root_dir: &std::fs::File,
    directory: &Path,
    depth: usize,
    limits: FilesystemLimits,
    total_bytes: &mut usize,
    visited_directories: &mut usize,
    files: &mut Vec<WorkspaceFile>,
) -> Result<()> {
    if depth > MAX_DIRECTORY_DEPTH {
        return Err(WebmcpError::LimitExceeded);
    }
    *visited_directories = visited_directories.saturating_add(1);
    if *visited_directories > MAX_VISITED_DIRECTORIES {
        return Err(WebmcpError::LimitExceeded);
    }
    let entries = std::fs::read_dir(directory)?;
    for entry in entries {
        if files.len() >= limits.max_files {
            return Err(WebmcpError::LimitExceeded);
        }
        let entry = entry?;
        let path = entry.path();
        let relative_path = path
            .strip_prefix(root)
            .map_err(|_error| WebmcpError::PathRejected(path.display().to_string()))?;
        if is_sensitive_relative_path(relative_path) {
            continue;
        }
        let metadata = std::fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() {
            continue;
        }
        if metadata.is_dir() {
            if entry
                .file_name()
                .to_str()
                .is_some_and(|name| IGNORED_DIRECTORY_NAMES.contains(&name))
            {
                continue;
            }
            visit_directory(root, root_dir, &path, depth + 1, limits, total_bytes, visited_directories, files)?;
            continue;
        }
        if !metadata.is_file() {
            continue;
        }
        if has_multiple_hard_links(&metadata) {
            continue;
        }
        let max_file_bytes = u64::try_from(limits.max_file_bytes).unwrap_or(u64::MAX);
        if metadata.len() > max_file_bytes {
            continue;
        }
        let file = open_workspace_file(root, root_dir, relative_path, false)?;
        let opened_metadata = file.metadata()?;
        if !opened_metadata.is_file()
            || has_multiple_hard_links(&opened_metadata)
            || opened_metadata.len() > max_file_bytes
        {
            continue;
        }
        let mut reader = file.take(max_file_bytes.saturating_add(1));
        let mut bytes = Vec::with_capacity(limits.max_file_bytes.min(64 * 1024));
        let _bytes_read = reader.read_to_end(&mut bytes)?;
        if bytes.len() > limits.max_file_bytes {
            continue;
        }
        let Ok(content) = String::from_utf8(bytes) else {
            continue;
        };
        *total_bytes = total_bytes.saturating_add(content.len());
        if *total_bytes > limits.max_total_bytes {
            return Err(WebmcpError::LimitExceeded);
        }
        let relative = relative_path.to_string_lossy().replace(std::path::MAIN_SEPARATOR, "/");
        files.push(WorkspaceFile {
            path: relative,
            size_bytes: content.len() as u64,
            digest: digest_text(&content),
        });
    }
    Ok(())
}

pub(super) fn trusted_toolchain_directory(path: &Path, workspace: &Path) -> Option<PathBuf> {
    let canonical = vtcode_commons::canonicalize(path).ok()?;
    (canonical.is_dir() && !canonical.starts_with(workspace)).then_some(canonical)
}

pub(super) fn resolve_check_executable(program: &str, workspace: &Path) -> Result<PathBuf> {
    let executable_name = if cfg!(windows) {
        format!("{program}.exe")
    } else {
        program.to_string()
    };
    let mut candidates = Vec::new();

    #[cfg(unix)]
    {
        if program == "cargo" {
            if let Some(home) = std::env::var_os("HOME") {
                candidates.push(PathBuf::from(home).join(".cargo/bin").join(&executable_name));
            }
        }
        for directory in ["/usr/local/bin", "/opt/homebrew/bin", "/usr/bin", "/bin"] {
            candidates.push(PathBuf::from(directory).join(&executable_name));
        }
    }

    #[cfg(windows)]
    {
        if program == "cargo"
            && let Some(profile) = std::env::var_os("USERPROFILE")
        {
            candidates.push(PathBuf::from(profile).join(".cargo/bin").join(&executable_name));
        }
        if let Some(system_root) = std::env::var_os("SystemRoot") {
            candidates.push(PathBuf::from(system_root).join("System32").join(&executable_name));
        }
    }

    for candidate in candidates {
        let Ok(canonical) = vtcode_commons::canonicalize(&candidate) else {
            continue;
        };
        if canonical.starts_with(workspace) {
            continue;
        }
        let Ok(metadata) = std::fs::metadata(&canonical) else {
            continue;
        };
        if metadata.is_file() && is_executable_file(&metadata) {
            return Ok(canonical);
        }
    }

    Err(WebmcpError::InvalidRequest(format!(
        "allowlisted check executable is not installed in a trusted location: {program}"
    )))
}

pub(super) fn is_executable_file(metadata: &std::fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        metadata.is_file()
    }
}

pub(super) fn trusted_executable_path(executable: &Path) -> Result<String> {
    let mut directories = Vec::new();
    if let Some(parent) = executable.parent() {
        directories.push(parent.to_path_buf());
    }
    #[cfg(unix)]
    directories.extend([
        PathBuf::from("/usr/local/bin"),
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/usr/bin"),
        PathBuf::from("/bin"),
    ]);
    #[cfg(windows)]
    if let Some(system_root) = std::env::var_os("SystemRoot") {
        directories.push(PathBuf::from(system_root).join("System32"));
    }
    directories.dedup();
    std::env::join_paths(directories)
        .map(|path| path.to_string_lossy().into_owned())
        .map_err(|error| WebmcpError::Adapter(format!("failed to build trusted check PATH: {error}")))
}

pub(super) fn sensitive_path_for_policy(path: &Path) -> Result<SensitivePath> {
    let path_string = path.to_str().ok_or_else(|| {
        WebmcpError::Adapter(format!("cannot sandbox a non-UTF-8 sensitive path: {}", path.display()))
    })?;
    if path_string
        .chars()
        .any(|character| character == '"' || character == '\\' || character.is_control())
    {
        return Err(WebmcpError::Adapter(format!(
            "cannot sandbox a sensitive path containing an unsafe character: {path_string}"
        )));
    }
    Ok(SensitivePath::new(path_string))
}

pub(super) fn collect_workspace_sensitive_paths(root: &Path) -> Result<Vec<SensitivePath>> {
    let mut sensitive_paths = Vec::new();
    let mut pending = vec![(root.to_path_buf(), 0usize)];
    let mut visited_directories = 0usize;

    while let Some((directory, depth)) = pending.pop() {
        if depth > MAX_DIRECTORY_DEPTH {
            return Err(WebmcpError::LimitExceeded);
        }
        visited_directories = visited_directories.saturating_add(1);
        if visited_directories > MAX_VISITED_DIRECTORIES {
            return Err(WebmcpError::LimitExceeded);
        }

        for entry in std::fs::read_dir(&directory)? {
            let entry = entry?;
            let path = entry.path();
            let relative_path = path
                .strip_prefix(root)
                .map_err(|_error| WebmcpError::PathRejected(path.display().to_string()))?;
            if is_sensitive_relative_path(relative_path) {
                if sensitive_paths.len() >= MAX_CHECK_SENSITIVE_PATHS {
                    return Err(WebmcpError::LimitExceeded);
                }
                sensitive_paths.push(sensitive_path_for_policy(&path)?);
                continue;
            }

            let metadata = std::fs::symlink_metadata(&path)?;
            if metadata.file_type().is_symlink() {
                continue;
            }
            if metadata.is_dir() {
                pending.push((path, depth + 1));
            }
        }
    }

    Ok(sensitive_paths)
}

pub(super) fn check_sandbox_policy(root: &Path) -> Result<SandboxPolicy> {
    let mut sensitive_paths = default_sensitive_paths();
    for name in SENSITIVE_FILES.iter().copied().chain(SENSITIVE_DIRECTORY_NAMES.iter().copied()) {
        sensitive_paths.push(sensitive_path_for_policy(&root.join(name))?);
    }
    sensitive_paths.extend(collect_workspace_sensitive_paths(root)?);
    Ok(SandboxPolicy::workspace_write_with_sensitive_paths(vec![root.to_path_buf()], sensitive_paths))
}

pub(super) fn parse_safe_command(command: &str, allowed_commands: &HashSet<String>) -> Result<Vec<String>> {
    if command.len() > 512
        || command
            .chars()
            .any(|character| matches!(character, ';' | '|' | '&' | '$' | '`' | '>' | '<' | '\n' | '\r'))
    {
        return Err(WebmcpError::InvalidRequest("check command contains shell syntax".to_string()));
    }
    let args = shell_words::split(command)
        .map_err(|error| WebmcpError::InvalidRequest(format!("invalid check command: {error}")))?;
    let Some(program) = args.first() else {
        return Err(WebmcpError::InvalidRequest("check command cannot be empty".to_string()));
    };
    if Path::new(program).components().count() != 1 || !allowed_commands.contains(program) {
        return Err(WebmcpError::InvalidRequest("check executable is not allowlisted".to_string()));
    }
    if args.iter().any(|argument| argument.contains('\0')) {
        return Err(WebmcpError::InvalidRequest("check command contains an invalid argument".to_string()));
    }
    match program.as_str() {
        "cargo"
            if args.get(1).is_some_and(|subcommand| subcommand == "check")
                && args.iter().skip(2).all(|argument| {
                    matches!(
                        argument.as_str(),
                        "--locked" | "--offline" | "--workspace" | "--all-targets" | "--all-features"
                    )
                }) =>
        {
            Ok(args)
        }
        "printf" => Ok(args),
        _ => Err(WebmcpError::InvalidRequest(
            "only the bounded cargo check and printf commands are supported".to_string(),
        )),
    }
}

pub(super) async fn read_process_output<R>(mut reader: R) -> Result<Vec<u8>>
where
    R: AsyncRead + Unpin,
{
    let mut captured = Vec::with_capacity(MAX_CHECK_OUTPUT_BYTES.min(8192));
    let mut buffer = [0u8; 8192];
    let mut exceeded = false;
    loop {
        let bytes_read = reader.read(&mut buffer).await?;
        if bytes_read == 0 {
            break;
        }
        let remaining = MAX_CHECK_OUTPUT_BYTES.saturating_sub(captured.len());
        exceeded |= bytes_read > remaining;
        if remaining > 0 {
            let chunk = buffer
                .get(..bytes_read.min(remaining))
                .ok_or_else(|| WebmcpError::Adapter("check output read exceeded its buffer".to_string()))?;
            captured.extend_from_slice(chunk);
        }
    }
    if exceeded {
        Err(WebmcpError::LimitExceeded)
    } else {
        Ok(captured)
    }
}
