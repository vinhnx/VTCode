use anyhow::{Result, bail};

/// Validates that a path is safe to use.
/// Preventing traversal, absolute system paths, and dangerous characters.
pub fn validate_path_safety(path: &str) -> Result<()> {
    // Reject path traversal attempts
    if path.contains("..") {
        bail!("Path traversal attempt detected ('..')");
    }

    // Additional traversal patterns
    if path.contains("~/../") || path.contains("/.../") {
        bail!("Advanced path traversal detected");
    }

    // Reject absolute paths outside workspace
    // Note: We can't strictly block all absolute paths as the agent might need to access
    // explicitly allowed directories, but we can block obvious system critical paths.
    const UNIX_CRITICAL: &[&str] = &["/etc", "/usr", "/bin", "/sbin", "/var", "/boot", "/root", "/dev"];
    for prefix in UNIX_CRITICAL {
        if path.starts_with(prefix) {
            bail!("Access to system directory denied: {}", prefix);
        }
    }

    // Windows critical paths
    #[cfg(windows)]
    {
        let path_lower = path.to_lowercase();
        const WIN_CRITICAL: &[&str] = &["c:\\windows", "c:\\program files", "c:\\system32"];
        for prefix in WIN_CRITICAL {
            if path_lower.starts_with(prefix) {
                bail!("Access to Windows system directory denied");
            }
        }
    }

    // Reject dangerous shell characters in paths (including null byte)
    const DANGEROUS_CHARS: &[char] = &['$', '`', '|', ';', '&', '\n', '\r', '>', '<', '\0'];
    if path.contains(DANGEROUS_CHARS) {
        bail!("Path contains dangerous shell characters");
    }

    Ok(())
}
