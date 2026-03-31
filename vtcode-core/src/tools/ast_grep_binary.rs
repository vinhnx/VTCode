use std::path::{Path, PathBuf};
use std::sync::Mutex;

use once_cell::sync::Lazy;

pub const AST_GREP_BIN_ENV: &str = "VTCODE_AST_GREP_BIN";
pub const AST_GREP_INSTALL_COMMAND: &str = "vtcode dependencies install ast-grep";

static AST_GREP_OVERRIDE: Lazy<Mutex<AstGrepBinaryOverride>> =
    Lazy::new(|| Mutex::new(AstGrepBinaryOverride::System));

#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug, Clone, Default)]
enum AstGrepBinaryOverride {
    #[default]
    System,
    Missing,
    Path(PathBuf),
}

#[doc(hidden)]
#[must_use]
pub struct AstGrepBinaryOverrideGuard {
    previous: AstGrepBinaryOverride,
}

impl Drop for AstGrepBinaryOverrideGuard {
    fn drop(&mut self) {
        *AST_GREP_OVERRIDE
            .lock()
            .expect("ast-grep override mutex must not be poisoned") = self.previous.clone();
    }
}

#[doc(hidden)]
pub fn set_ast_grep_binary_override_for_tests(path: Option<PathBuf>) -> AstGrepBinaryOverrideGuard {
    let mut state = AST_GREP_OVERRIDE
        .lock()
        .expect("ast-grep override mutex must not be poisoned");
    let previous = state.clone();
    *state = match path {
        Some(path) => AstGrepBinaryOverride::Path(path),
        None => AstGrepBinaryOverride::Missing,
    };
    AstGrepBinaryOverrideGuard { previous }
}

pub fn managed_ast_grep_bin_dir() -> PathBuf {
    dirs::home_dir()
        .map(|home| managed_ast_grep_bin_dir_from_home(&home))
        .unwrap_or_else(|| PathBuf::from(".vtcode/bin"))
}

pub fn managed_ast_grep_binary_path() -> PathBuf {
    managed_ast_grep_bin_dir().join(canonical_ast_grep_binary_name())
}

pub fn managed_ast_grep_alias_path() -> Option<PathBuf> {
    alias_ast_grep_binary_name().map(|name| managed_ast_grep_bin_dir().join(name))
}

pub fn managed_ast_grep_candidates() -> Vec<PathBuf> {
    let mut candidates = vec![managed_ast_grep_binary_path()];
    if let Some(alias) = managed_ast_grep_alias_path() {
        candidates.push(alias);
    }
    candidates
}

pub fn resolve_ast_grep_binary_from_env_and_fs() -> Option<PathBuf> {
    match AST_GREP_OVERRIDE
        .lock()
        .expect("ast-grep override mutex must not be poisoned")
        .clone()
    {
        AstGrepBinaryOverride::System => {}
        AstGrepBinaryOverride::Missing => return None,
        AstGrepBinaryOverride::Path(path) => return Some(path),
    }

    let env_override = std::env::var_os(AST_GREP_BIN_ENV)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from);

    resolve_ast_grep_binary_with_sources(
        env_override,
        managed_ast_grep_candidates(),
        resolve_ast_grep_binary_on_path(),
    )
}

pub fn resolve_ast_grep_binary_on_path() -> Option<PathBuf> {
    which::which(canonical_ast_grep_binary_name())
        .ok()
        .or_else(|| alias_ast_grep_binary_name().and_then(|alias| which::which(alias).ok()))
}

pub fn canonical_ast_grep_binary_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "ast-grep.exe"
    } else {
        "ast-grep"
    }
}

pub fn alias_ast_grep_binary_name() -> Option<&'static str> {
    if cfg!(target_os = "linux") {
        None
    } else if cfg!(target_os = "windows") {
        Some("sg.exe")
    } else {
        Some("sg")
    }
}

pub fn missing_ast_grep_message(suffix: &str) -> String {
    let extra = if suffix.is_empty() {
        String::new()
    } else {
        format!(" {suffix}")
    };
    format!(
        "ast-grep is not available; run `{AST_GREP_INSTALL_COMMAND}` or install `ast-grep` manually.{extra}"
    )
}

fn managed_ast_grep_bin_dir_from_home(home: &Path) -> PathBuf {
    home.join(".vtcode").join("bin")
}

fn resolve_ast_grep_binary_with_sources(
    env_override: Option<PathBuf>,
    managed_candidates: Vec<PathBuf>,
    path_candidate: Option<PathBuf>,
) -> Option<PathBuf> {
    env_override
        .filter(|path| path.exists())
        .or_else(|| {
            managed_candidates
                .into_iter()
                .find(|candidate| candidate.exists())
        })
        .or(path_candidate)
}

#[cfg(test)]
mod tests {
    use super::{
        alias_ast_grep_binary_name, canonical_ast_grep_binary_name,
        managed_ast_grep_bin_dir_from_home, resolve_ast_grep_binary_from_env_and_fs,
        resolve_ast_grep_binary_with_sources, set_ast_grep_binary_override_for_tests,
    };
    use std::path::{Path, PathBuf};
    use tempfile::TempDir;

    #[test]
    fn managed_bin_dir_uses_vtcode_home_bin() {
        let path = managed_ast_grep_bin_dir_from_home(Path::new("/tmp/example-home"));
        assert_eq!(path, Path::new("/tmp/example-home/.vtcode/bin"));
    }

    #[test]
    fn canonical_binary_name_matches_platform() {
        if cfg!(target_os = "windows") {
            assert_eq!(canonical_ast_grep_binary_name(), "ast-grep.exe");
        } else {
            assert_eq!(canonical_ast_grep_binary_name(), "ast-grep");
        }
    }

    #[test]
    fn alias_binary_name_skips_linux() {
        if cfg!(target_os = "linux") {
            assert_eq!(alias_ast_grep_binary_name(), None);
        } else if cfg!(target_os = "windows") {
            assert_eq!(alias_ast_grep_binary_name(), Some("sg.exe"));
        } else {
            assert_eq!(alias_ast_grep_binary_name(), Some("sg"));
        }
    }

    #[test]
    fn resolution_prefers_env_override() {
        let temp_dir = TempDir::new().expect("temp dir");
        let env_path = temp_dir.path().join("custom-ast-grep");
        std::fs::write(&env_path, "binary").expect("env binary");

        let managed_path = temp_dir.path().join("managed-ast-grep");
        std::fs::write(&managed_path, "binary").expect("managed binary");

        let path_fallback = temp_dir.path().join("path-ast-grep");
        std::fs::write(&path_fallback, "binary").expect("path binary");

        let resolved = resolve_ast_grep_binary_with_sources(
            Some(env_path.clone()),
            vec![managed_path],
            Some(path_fallback),
        );

        assert_eq!(resolved, Some(env_path));
    }

    #[test]
    fn resolution_prefers_managed_binary_before_path() {
        let temp_dir = TempDir::new().expect("temp dir");
        let managed_path = temp_dir.path().join("managed-ast-grep");
        std::fs::write(&managed_path, "binary").expect("managed binary");

        let path_fallback = temp_dir.path().join("path-ast-grep");
        std::fs::write(&path_fallback, "binary").expect("path binary");

        let resolved = resolve_ast_grep_binary_with_sources(
            None,
            vec![managed_path.clone()],
            Some(path_fallback),
        );

        assert_eq!(resolved, Some(managed_path));
    }

    #[test]
    fn resolution_uses_path_fallback_when_needed() {
        let temp_dir = TempDir::new().expect("temp dir");
        let path_fallback = temp_dir.path().join("path-ast-grep");
        std::fs::write(&path_fallback, "binary").expect("path binary");

        let resolved = resolve_ast_grep_binary_with_sources(
            Some(PathBuf::from("/missing/env-ast-grep")),
            vec![PathBuf::from("/missing/managed-ast-grep")],
            Some(path_fallback.clone()),
        );

        assert_eq!(resolved, Some(path_fallback));
    }

    #[test]
    fn resolution_uses_test_override_when_present() {
        let temp_dir = TempDir::new().expect("temp dir");
        let override_path = temp_dir.path().join("override-ast-grep");
        std::fs::write(&override_path, "binary").expect("override binary");
        let _guard = set_ast_grep_binary_override_for_tests(Some(override_path.clone()));

        let resolved = resolve_ast_grep_binary_from_env_and_fs();

        assert_eq!(resolved, Some(override_path));
    }

    #[test]
    fn resolution_can_be_forced_missing_in_tests() {
        let _guard = set_ast_grep_binary_override_for_tests(None);

        let resolved = resolve_ast_grep_binary_from_env_and_fs();

        assert_eq!(resolved, None);
    }
}
