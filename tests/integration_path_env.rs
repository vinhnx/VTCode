#![allow(
    missing_docs,
    reason = "Intentional compatibility, platform, test, or API-shape suppression."
)]
//! Integration tests for PATH environment inheritance.
//!
//! Every test here has an oracle: either an independently derived assertion on
//! the process environment or a subprocess result. Print-only "documentation"
//! cases were removed because they passed regardless of the behavior they
//! claimed to demonstrate.

#[cfg(test)]
mod path_environment_tests {
    use std::env;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    fn path_entries() -> Vec<PathBuf> {
        env::var_os("PATH")
            .map(|value| env::split_paths(&value).collect())
            .unwrap_or_default()
    }

    /// Component-aware membership check: a prefix such as `.cargo/bin-evil`
    /// must not satisfy an entry of `.cargo/bin`.
    fn contains_path_entry(entries: &[PathBuf], candidate: &Path) -> bool {
        entries.iter().any(|entry| entry == candidate)
    }

    #[test]
    fn path_is_present_and_points_at_real_directories() {
        let raw = env::var_os("PATH").expect("PATH must be set");
        assert!(!raw.is_empty(), "PATH must not be empty");

        let entries = path_entries();
        assert!(!entries.is_empty(), "PATH must contain at least one directory");
        assert!(
            entries.iter().any(|entry| entry.is_dir()),
            "at least one PATH entry must be an existing directory: {entries:?}"
        );
    }

    #[test]
    fn path_entry_matching_uses_whole_components() {
        let entries = vec![PathBuf::from("/home/tester/.cargo/bin")];
        assert!(contains_path_entry(&entries, Path::new("/home/tester/.cargo/bin")));
        assert!(!contains_path_entry(&entries, Path::new("/home/tester/.cargo/bin-evil")));
        assert!(!contains_path_entry(&entries, Path::new("/home/tester/.cargo")));
        assert!(!contains_path_entry(&entries, Path::new("/home/other/.cargo/bin")));
    }

    #[test]
    fn home_is_present_and_absolute() {
        let home = env::var_os("HOME").expect("HOME must be set for user-local tool paths");
        let home_path = PathBuf::from(home);
        assert!(home_path.is_absolute(), "HOME must be absolute: {}", home_path.display());
    }

    #[test]
    fn path_has_multiple_entries() {
        let entries = path_entries();
        assert!(entries.len() >= 2, "a usable PATH has multiple entries: {entries:?}");
    }

    #[test]
    fn cargo_is_reachable_through_inherited_path() {
        // This suite is launched by Cargo, so Cargo is necessarily on PATH.
        let output = Command::new("cargo")
            .arg("--version")
            .output()
            .expect("Cargo launched this test and must remain reachable");
        assert!(output.status.success(), "cargo --version failed: {}", output.status);
        let version = String::from_utf8_lossy(&output.stdout);
        assert!(version.contains("cargo"), "unexpected cargo --version output: {version}");
    }
}
