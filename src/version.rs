use std::path::PathBuf;

/// Get the application version including git information
pub fn version() -> String {
    // Try to get git info from build script, fallback to just version if not available
    let commit_hash = option_env!("VT_CODE_GIT_INFO").unwrap_or("unknown");
    let version = env!("CARGO_PKG_VERSION");

    format!(
        "{} ({})",
        version,
        commit_hash
    )
}

/// Get the config directory path for the application
pub fn get_config_dir() -> Option<PathBuf> {
    dirs::config_dir().map(|dir| dir.join("vtcode"))
}

/// Get the data directory path for the application
pub fn get_data_dir() -> Option<PathBuf> {
    dirs::data_dir().map(|dir| dir.join("vtcode"))
}