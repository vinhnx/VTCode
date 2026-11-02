use anyhow::{Result, bail};
use std::path::{Component, Path, PathBuf};

/// Check if a path string is a safe relative path (no traversal, no absolute).
pub fn is_safe_relative_path(path: &str) -> bool {
    let path = path.trim();
    if path.is_empty() {
        return false;
    }

    // Check for path traversal attempts
    if path.contains("..") {
        return false;
    }

    // Block absolute paths for security
    if path.starts_with('/') || path.contains(':') {
        return false;
    }

    true
}

/// Validates that a path is safe to use.
/// Preventing traversal, absolute system paths, and dangerous characters.
///
/// Optimization: Uses early returns and byte-level checks for common patterns
pub fn validate_path_safety(path: &str) -> Result<()> {
    // Optimization: Fast path for empty or very short paths
    if path.is_empty() {
        return Ok(());
    }

    // Reject path traversal attempts
    // Optimization: Use contains on bytes for simple patterns
    if path.contains("..") {
        bail!("Path traversal attempt detected ('..')");
    }

    // Additional traversal patterns
    if path.contains("~/../") || path.contains("/.../") {
        bail!("Advanced path traversal detected");
    }

    // Optimization: Only check Unix critical paths if path starts with '/'
    if path.starts_with('/') {
        // Reject absolute paths outside workspace
        // Note: We can't strictly block all absolute paths as the agent might need to access
        // explicitly allowed directories, but we can block obvious system critical paths.
        static UNIX_CRITICAL: &[&str] = &["/etc", "/usr", "/bin", "/sbin", "/var", "/boot", "/root", "/dev"];
        for prefix in UNIX_CRITICAL {
            let is_var_temp_exception = *prefix == "/var"
                && (path.starts_with("/var/folders/")
                    || path == "/var/folders"
                    || path.starts_with("/var/tmp/")
                    || path == "/var/tmp");

            if !is_var_temp_exception && matches_critical_prefix(path, prefix) {
                bail!("Access to system directory denied: {prefix}");
            }
        }
    }

    // Windows critical paths
    #[cfg(windows)]
    {
        let path_lower = path.to_lowercase();
        static WIN_CRITICAL: &[&str] = &["c:\\windows", "c:\\program files", "c:\\system32"];
        for prefix in WIN_CRITICAL {
            if path_lower.starts_with(prefix) {
                bail!("Access to Windows system directory denied");
            }
        }
    }

    // Reject dangerous shell characters in paths (including null byte)
    // Optimization: Check bytes directly for faster character detection
    static DANGEROUS_CHARS: &[u8] = b"$`|;&\n\r><\0";
    for &c in path.as_bytes() {
        if DANGEROUS_CHARS.contains(&c) {
            bail!("Path contains dangerous shell characters");
        }
    }

    Ok(())
}

fn matches_critical_prefix(path: &str, prefix: &str) -> bool {
    path == prefix || path.strip_prefix(prefix).is_some_and(|rest| rest.starts_with('/'))
}

/// Validate an archive entry name and return it as a destination-relative path.
///
/// Lexical only (no filesystem access): rejects absolute paths, Windows
/// prefixes, and parent components (`..`), and collapses `.` components.
/// Unlike a substring check for `..`, legitimate names such as
/// `v1..2/README.md` are accepted. An empty result means the name resolves to
/// the extraction destination itself (e.g. `.`); callers decide whether to
/// skip or reject that.
///
/// # Errors
///
/// Returns an error when the name contains a component that would escape the
/// extraction destination.
pub fn safe_archive_relative_path(name: &Path) -> Result<PathBuf> {
    let mut safe = PathBuf::new();
    for component in name.components() {
        match component {
            Component::Normal(part) => safe.push(part),
            Component::CurDir => {}
            Component::ParentDir => {
                bail!("unsafe archive path ('..' escapes destination): {}", name.display());
            }
            Component::RootDir | Component::Prefix(_) => {
                bail!("unsafe archive path (absolute): {}", name.display());
            }
        }
    }
    Ok(safe)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_nested_relative_names() {
        assert_eq!(safe_archive_relative_path(Path::new("a/b/c.txt")).expect("safe"), PathBuf::from("a/b/c.txt"));
    }

    #[test]
    fn collapses_current_dir_components() {
        assert_eq!(
            safe_archive_relative_path(Path::new("./nested/./file.txt")).expect("safe"),
            PathBuf::from("nested/file.txt")
        );
    }

    #[test]
    fn accepts_dot_dot_inside_a_single_component() {
        // Substring `..` checks reject this legitimate name; component
        // analysis must accept it.
        assert_eq!(
            safe_archive_relative_path(Path::new("v1..2/README.md")).expect("safe"),
            PathBuf::from("v1..2/README.md")
        );
    }

    #[test]
    fn rejects_parent_components() {
        let err = safe_archive_relative_path(Path::new("../../etc/passwd")).expect_err("traversal");
        assert!(err.to_string().contains("unsafe archive path"), "unexpected error: {err}");
    }

    #[test]
    fn rejects_absolute_names() {
        let err = safe_archive_relative_path(Path::new("/abs.txt")).expect_err("absolute");
        assert!(err.to_string().contains("unsafe archive path"), "unexpected error: {err}");
    }

    #[test]
    fn resolves_bare_current_dir_to_empty() {
        assert!(safe_archive_relative_path(Path::new(".")).expect("safe").as_os_str().is_empty());
    }

    #[cfg(windows)]
    #[test]
    fn rejects_windows_prefixes() {
        let err = safe_archive_relative_path(Path::new("C:\\evil.txt")).expect_err("prefix");
        assert!(err.to_string().contains("unsafe archive path"), "unexpected error: {err}");
    }
}
