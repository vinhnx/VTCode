use anyhow::{Result, bail};

/// Validates that a path is safe to use.
/// preventing traversal, absolute system paths, and dangerous characters.
pub fn validate_path_safety(path: &str) -> Result<()> {
    // Reject path traversal attempts
    if path.contains("..") {
        bail!("Path traversal attempt detected ('..')");
    }
    
    // Reject absolute paths outside workspace
    // Note: We can't strictly block all absolute paths as the agent might need to access
    // explicitly allowed directories, but we can block obvious system critical paths.
    if path.starts_with("/etc") || path.starts_with("/usr") || path.starts_with("/bin") 
        || path.starts_with("/sbin") || path.starts_with("/var") || path.starts_with("/boot") {
        bail!("Access to system directory denied");
    }
    
    // Reject dangerous shell characters in paths
    const DANGEROUS_CHARS: &[char] = &['$', '`', '|', ';', '&', '\n', '\r', '>', '<'];
    if path.contains(DANGEROUS_CHARS) {
        bail!("Path contains dangerous shell characters");
    }
    
    Ok(())
}
