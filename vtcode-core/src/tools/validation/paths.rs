use anyhow::{Result, bail};

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
        static UNIX_CRITICAL: &[&str] = &[
            "/etc", "/usr", "/bin", "/sbin", "/var", "/boot", "/root", "/dev",
        ];
        for prefix in UNIX_CRITICAL {
            let is_var_temp_exception = *prefix == "/var"
                && (path.starts_with("/var/folders/")
                    || path == "/var/folders"
                    || path.starts_with("/var/tmp/")
                    || path == "/var/tmp");

            if !is_var_temp_exception && matches_critical_prefix(path, prefix) {
                bail!("Access to system directory denied: {}", prefix);
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
    path == prefix
        || path
            .strip_prefix(prefix)
            .is_some_and(|rest| rest.starts_with('/'))
}

#[cfg(test)]
mod tests {
    use super::validate_path_safety;

    #[test]
    fn allows_macos_temp_paths_under_var_folders() {
        assert!(validate_path_safety("/var/folders/ab/cd/tmp123/file.txt").is_ok());
    }

    #[test]
    fn still_blocks_sensitive_var_paths() {
        assert!(validate_path_safety("/var/db/shadow").is_err());
        assert!(validate_path_safety("/var").is_err());
    }

    #[test]
    fn allows_non_critical_prefix_matches() {
        assert!(validate_path_safety("/varnish/cache/file").is_ok());
    }

    #[test]
    fn allows_var_tmp_paths() {
        assert!(validate_path_safety("/var/tmp/vtcode/run.log").is_ok());
    }
}
