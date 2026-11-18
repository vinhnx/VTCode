use anyhow::{Result, bail};
use std::path::{Component, Path, PathBuf};

/// Check if a path string is a safe relative path (no traversal, no absolute).
pub fn is_safe_relative_path(path: &str) -> bool {
    let path = path.trim();
    if path.is_empty() {
        return false;
    }

    // Reject traversal only when `..` is a whole component. A substring check
    // would drop legal names such as `v1..2.txt` on Unix/macOS.
    if path.split(['/', '\\']).any(|component| component == "..") {
        return false;
    }

    // Both separators are treated as rooted on every host (defense in depth).
    // ':' only denotes a drive or alternate data stream on Windows; on Unix it
    // is an ordinary filename character (`@notes:Q1.txt`).
    if path.starts_with(['/', '\\']) || (cfg!(windows) && path.contains(':')) {
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
        if is_windows_critical_path(path) {
            bail!("Access to Windows system directory denied");
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

/// Whether a Windows path points into a critical system directory.
///
/// Windows accepts `/` as a separator and these directories exist on every
/// drive, so compare normalized, drive-prefix-stripped components rather than
/// a hardcoded `c:\...` string prefix. Kept platform-agnostic so the logic can
/// be regression-tested on any host.
fn is_windows_critical_path(path: &str) -> bool {
    let normalized = path.replace('/', "\\").to_lowercase();
    let without_extended = normalized.strip_prefix("\\\\?\\").unwrap_or(&normalized);
    let mut chars = without_extended.chars();
    let after_drive = match (chars.next(), chars.next()) {
        (Some(drive), Some(':')) if drive.is_ascii_lowercase() => chars.as_str(),
        _ => without_extended,
    };
    let relative = after_drive.trim_start_matches('\\');
    // `windows.old` holds a previous system installation (registry hives,
    // system binaries); the old `c:\windows` string prefix covered it too.
    const CRITICAL: &[&str] = &[
        "windows",
        "windows.old",
        "program files",
        "program files (x86)",
        "system32",
    ];
    CRITICAL
        .iter()
        .any(|name| relative == *name || relative.strip_prefix(name).is_some_and(|rest| rest.starts_with('\\')))
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
    fn safe_relative_path_rejects_rooted_windows_paths() {
        assert!(!is_safe_relative_path(r"\Windows\secret.txt"));
        assert!(!is_safe_relative_path(r"\\server\share\file.txt"));
        assert!(!is_safe_relative_path("/etc/passwd"));
        assert!(is_safe_relative_path(r"src\main.rs"));
        assert!(is_safe_relative_path("src/main.rs"));
    }

    #[test]
    fn accepts_nested_relative_names() {
        assert_eq!(safe_archive_relative_path(Path::new("a/b/c.txt")).expect("safe"), PathBuf::from("a/b/c.txt"));
    }

    #[test]
    fn unix_filenames_may_contain_colons_and_double_dots() {
        if cfg!(windows) {
            return;
        }
        assert!(is_safe_relative_path("@notes:Q1.txt"));
        assert!(is_safe_relative_path("v1..2.txt"));
        assert!(!is_safe_relative_path("a/../secret.txt"));
        assert!(!is_safe_relative_path("a\\..\\secret.txt"));
    }

    #[test]
    fn windows_critical_paths_normalize_separators_and_drives() {
        for path in [
            "C:/Windows/System32/drivers/etc/hosts",
            "c:\\windows\\system32\\drivers\\etc\\hosts",
            "D:\\Windows\\System32\\foo",
            "C:/Windows.old/Windows/System32/config/SAM",
            "\\\\?\\C:\\Windows\\foo",
            "C:/Program Files/Tools/tool.exe",
            "C:/Program Files (x86)/Tools/tool.exe",
            "c:/system32/config/SAM",
        ] {
            assert!(is_windows_critical_path(path), "must be critical: {path}");
        }
        for path in [
            "C:/Users/me/Documents/windows-notes.txt",
            "C:/projects/myapp/src/main.rs",
            "D:/data/windows_backup.tar",
        ] {
            assert!(!is_windows_critical_path(path), "must not be critical: {path}");
        }
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
