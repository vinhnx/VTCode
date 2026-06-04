use std::path::Path;

/// Infer a sensible default verify command for a workspace by inspecting
/// well-known project manifests.
///
/// Returns at most one command: the first matching language toolchain in
/// priority order — Rust, Python, Node.js — or an empty vector when no
/// supported manifest is found.
pub fn infer_default_verify_commands(workspace_root: &Path) -> Vec<String> {
    if workspace_root.join("Cargo.toml").exists() {
        return vec!["cargo check".to_string()];
    }
    if workspace_root.join("pytest.ini").exists()
        || workspace_root.join("pyproject.toml").exists()
        || workspace_root.join("setup.py").exists()
    {
        return vec!["pytest".to_string()];
    }
    if workspace_root.join("package.json").exists() {
        return vec!["npm test".to_string()];
    }
    Vec::new()
}
