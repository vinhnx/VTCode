use std::collections::HashSet;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use once_cell::sync::Lazy;
use regex::Regex;

/// Regex pattern for Unix-style environment variables: $VAR or ${VAR}
static UNIX_ENV_PATTERN: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"\$([A-Za-z_][A-Za-z0-9_]*)|\$\{([A-Za-z_][A-Za-z0-9_]*)\}")
        .expect("valid unix env regex")
});

/// Regex pattern for Windows-style environment variables: %VAR%
static WINDOWS_ENV_PATTERN: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"%([A-Za-z_][A-Za-z0-9_]*)%").expect("valid windows env regex"));

/// Expand environment variables and home directory within a path entry.
fn expand_entry(entry: &str, workspace_root: &Path) -> Option<PathBuf> {
    let trimmed = entry.trim();
    if trimmed.is_empty() {
        return None;
    }

    let expanded_env = expand_environment_variables(trimmed);
    let mut path = if let Some(rest) = expanded_env.strip_prefix("~/") {
        dirs::home_dir().map(|home| home.join(rest))?
    } else if expanded_env == "~" {
        dirs::home_dir()?
    } else {
        PathBuf::from(expanded_env)
    };

    if path.is_relative() {
        path = workspace_root.join(path);
    }

    if path.is_dir() { Some(path) } else { None }
}

fn expand_environment_variables(input: &str) -> String {
    let unix_expanded = UNIX_ENV_PATTERN
        .replace_all(input, |caps: &regex::Captures<'_>| {
            let var_name = caps
                .get(1)
                .or_else(|| caps.get(2))
                .map(|m| m.as_str())
                .unwrap_or_default();
            // Try to get the environment variable, with special handling for HOME
            match var_name {
                "HOME" => std::env::var("HOME")
                    .or_else(|_| std::env::var("USERPROFILE"))
                    .unwrap_or_else(|_| {
                        dirs::home_dir()
                            .map(|p| p.display().to_string())
                            .unwrap_or_default()
                    }),
                _ => std::env::var(var_name).unwrap_or_default(),
            }
        })
        .into_owned();

    WINDOWS_ENV_PATTERN
        .replace_all(&unix_expanded, |caps: &regex::Captures<'_>| {
            let var_name = &caps[1];
            // Try to get the environment variable, with special handling for HOME/USERPROFILE
            match var_name {
                "HOME" | "USERPROFILE" => std::env::var("USERPROFILE")
                    .or_else(|_| std::env::var("HOME"))
                    .unwrap_or_else(|_| {
                        dirs::home_dir()
                            .map(|p| p.display().to_string())
                            .unwrap_or_default()
                    }),
                _ => std::env::var(var_name).unwrap_or_default(),
            }
        })
        .into_owned()
}

/// Compute the list of additional search paths for command execution.
pub(crate) fn compute_extra_search_paths(
    entries: &[String],
    workspace_root: &Path,
) -> Vec<PathBuf> {
    let mut results = Vec::new();
    let mut seen = HashSet::new();

    for entry in entries {
        if let Some(path) = expand_entry(entry, workspace_root) {
            if seen.insert(path.clone()) {
                results.push(path);
            }
        }
    }

    results
}

/// Attempt to resolve a program against the provided path iterator.
#[allow(dead_code)] // Function is deprecated but kept for explicit path iteration tests
pub(crate) fn resolve_program_path_from_paths(
    program: &str,
    paths: impl Iterator<Item = PathBuf>,
) -> Option<String> {
    for path_dir in paths {
        let full_path = path_dir.join(program);
        if full_path.is_file() {
            return Some(full_path.to_string_lossy().into_owned());
        }
    }
    None
}

// NOTE: Static resolution of program paths is intentionally deprecated in favor of
// always executing commands through the user's login shell (via `resolve_fallback_shell`).
// The `resolve_program_path_from_paths` helper remains for explicit path iteration tests.

/// Merge additional search paths into an existing PATH environment value.
pub(crate) fn merge_path_env(current: Option<&OsStr>, extra_paths: &[PathBuf]) -> Option<OsString> {
    if extra_paths.is_empty() && current.is_none() {
        return None;
    }

    let mut combined: Vec<PathBuf> = current
        .map(|value| std::env::split_paths(value).collect())
        .unwrap_or_default();

    // Ensure common development tool paths are included for fallback
    // These paths are often added by shell initialization files but we include them
    // to ensure development tools work even if shell initialization is incomplete
    let fallback_paths = [
        "~/.cargo/bin",               // Rust toolchain (cargo, rustc)
        "~/.local/bin",               // User-installed binaries
        "~/.nvm/versions/node/*/bin", // Node Version Manager
        "~/.bun/bin",                 // Bun package manager
        "/opt/homebrew/bin",          // Homebrew on Apple Silicon
        "/usr/local/bin",             // Local binaries
        "/opt/local/bin",             // MacPorts
    ];

    for fallback_path_pattern in &fallback_paths {
        // Expand tilde to home directory
        if let Some(home) = dirs::home_dir() {
            let expanded = fallback_path_pattern.replace("~", &home.display().to_string());

            // For glob patterns like nvm, just add the base directory if home/nvm exists
            if expanded.contains('*') {
                let base_pattern = expanded.split('*').next().unwrap_or("");
                let base_path = PathBuf::from(base_pattern.trim_end_matches('/'));
                if base_path.exists() && !combined.iter().any(|existing| existing == &base_path) {
                    combined.push(base_path);
                }
            } else {
                let path = PathBuf::from(expanded);
                if path.exists() && !combined.iter().any(|existing| existing == &path) {
                    combined.push(path);
                }
            }
        }
    }

    for path in extra_paths.iter().rev() {
        if !combined.iter().any(|existing| existing == path) {
            combined.insert(0, path.clone());
        }
    }

    if combined.is_empty() {
        return None;
    }

    std::env::join_paths(combined).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compute_extra_search_paths_expands_home_and_env() {
        let workspace = std::env::current_dir().expect("workspace");
        let temp = tempfile::tempdir().expect("tempdir");
        let temp_path = temp.path().join("bin");
        std::fs::create_dir_all(&temp_path).expect("create dir");
        unsafe {
            std::env::set_var("TEST_PATH_ENV", temp_path.display().to_string());
        }

        let entries = vec!["~/does/not/exist".to_string(), "$TEST_PATH_ENV".to_string()];
        let resolved = compute_extra_search_paths(&entries, &workspace);
        assert_eq!(resolved, vec![temp_path]);

        unsafe {
            std::env::remove_var("TEST_PATH_ENV");
        }
    }

    #[test]
    fn merge_path_env_preprends_extra_entries() {
        let extra = vec![PathBuf::from("/extra/bin"), PathBuf::from("/another/bin")];
        let current = Some(OsStr::new("/usr/bin:/bin"));
        let merged = merge_path_env(current, &extra).expect("merged path");
        let paths: Vec<PathBuf> = std::env::split_paths(&merged).collect();
        assert_eq!(paths[0], PathBuf::from("/extra/bin"));
        assert_eq!(paths[1], PathBuf::from("/another/bin"));
        assert_eq!(paths[2], PathBuf::from("/usr/bin"));
    }

    #[test]
    fn resolve_program_path_uses_extra_dirs() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let bin_dir = temp_dir.path();
        let fake = bin_dir.join("fake-tool");
        std::fs::write(&fake, b"#!/bin/sh\n").expect("write fake tool");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&fake).expect("metadata").permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&fake, perms).expect("set perms");
        }

        let resolved =
            resolve_program_path_from_paths("fake-tool", [bin_dir.to_path_buf()].into_iter());
        assert_eq!(resolved, Some(fake.to_string_lossy().to_string()))
    }
}
