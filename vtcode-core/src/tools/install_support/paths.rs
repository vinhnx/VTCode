use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

pub(crate) fn vtcode_state_dir() -> Result<PathBuf> {
    dirs::home_dir()
        .map(|home| vtcode_state_dir_from_home(&home))
        .context("Cannot determine home directory for VT Code-managed install")
}

pub(crate) fn vtcode_state_dir_or_default() -> PathBuf {
    dirs::home_dir()
        .map(|home| vtcode_state_dir_from_home(&home))
        .unwrap_or_else(|| PathBuf::from(".vtcode"))
}

pub(crate) fn vtcode_state_dir_from_home(home: &Path) -> PathBuf {
    home.join(".vtcode")
}
