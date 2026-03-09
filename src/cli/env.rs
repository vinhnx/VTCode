use std::path::{Path, PathBuf};

pub fn set_workspace_env(workspace: &Path) {
    unsafe {
        std::env::set_var("VTCODE_WORKSPACE", workspace);
    }
}

pub fn set_additional_dirs_env(additional_dirs: &[PathBuf]) {
    let dirs_str = additional_dirs
        .iter()
        .map(|path| path.to_string_lossy().to_string())
        .collect::<Vec<_>>()
        .join(":");
    unsafe {
        std::env::set_var("VTCODE_ADDITIONAL_DIRS", dirs_str);
    }
}
