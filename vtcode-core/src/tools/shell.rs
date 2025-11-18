use std::path::Path;

/// Resolve the fallback shell for command execution when program is not found.
/// Prefers the user's configured shell, respecting environment variables and system detection.
pub(crate) fn resolve_fallback_shell() -> String {
    // Try SHELL environment variable first (set by login shells)
    if let Ok(shell) = std::env::var("SHELL") {
        let trimmed = shell.trim();
        if !trimmed.is_empty() && Path::new(trimmed).exists() {
            return trimmed.to_string();
        }
    }

    // Detect available shells on the system
    const SHELL_CANDIDATES: &[&str] = &[
        "/bin/zsh",
        "/usr/bin/zsh",
        "/bin/bash",
        "/usr/bin/bash",
        "/bin/sh",
        "/usr/bin/sh",
    ];

    for shell_path in SHELL_CANDIDATES {
        if Path::new(shell_path).exists() {
            return shell_path.to_string();
        }
    }

    // Final fallback
    "/bin/sh".to_string()
}
