#![expect(
    unused_results,
    reason = "Path normalization intentionally ignores the boolean returned by pop after a validated component check."
)]

use std::path::{Component, Path, PathBuf};

/// Normalize a path by resolving `.` and `..` components lexically.
pub fn normalize_path(path: &Path) -> PathBuf {
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

/// Expand a leading `~` or `~/` to the user's home directory. The function is
/// intentionally forgiving: paths that don't start with `~` are returned as-is,
/// and when the home directory cannot be determined the original path is
/// preserved so callers can surface a downstream error rather than panicking.
///
/// This is the canonical implementation used by the tool registry and the
/// sandbox runtime; both call sites previously carried near-identical copies.
pub fn expand_tilde(path: &str) -> PathBuf {
    if path == "~" {
        return dirs::home_dir().unwrap_or_else(|| PathBuf::from(path));
    }
    if let Some(rest) = path.strip_prefix("~/")
        && let Some(home) = dirs::home_dir()
    {
        return home.join(rest);
    }
    PathBuf::from(path)
}

/// Normalize identifiers to ASCII alphanumerics with lowercase output.
pub fn normalize_ascii_identifier(value: &str) -> String {
    let mut normalized = String::with_capacity(value.len());
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            normalized.push(ch.to_ascii_lowercase());
        }
    }
    normalized
}

/// Extract the filename from a path, with fallback to the full path.
pub fn file_name_from_path(path: &str) -> String {
    Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| path.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn expand_tilde_passes_through_absolute_paths() {
        let absolute = "/etc/hosts";
        assert_eq!(expand_tilde(absolute), PathBuf::from(absolute));
    }

    #[test]
    fn expand_tilde_passes_through_relative_paths() {
        let relative = "src/main.rs";
        assert_eq!(expand_tilde(relative), PathBuf::from(relative));
    }

    #[test]
    fn expand_tilde_resolves_bare_tilde_to_home() {
        if let Some(home) = dirs::home_dir() {
            assert_eq!(expand_tilde("~"), home);
        }
    }

    #[test]
    fn expand_tilde_resolves_tilde_slash_prefix() {
        if let Some(home) = dirs::home_dir() {
            let resolved = expand_tilde("~/projects/vtcode");
            assert_eq!(resolved, home.join("projects/vtcode"));
        }
    }
}
