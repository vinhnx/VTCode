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
            if path.starts_with(prefix) {
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
