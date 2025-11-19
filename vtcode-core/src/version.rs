/// Get the application version including git information
pub fn version() -> &'static str {
    // Try to get git info from build script, fallback to just version if not available
    let commit_hash = option_env!("VT_CODE_GIT_INFO").unwrap_or("unknown");
    let version = env!("CARGO_PKG_VERSION");

    // Use Box::leak to convert String to &'static str
    let version_string = format!(
        "{} ({})",
        version,
        commit_hash
    );

    Box::leak(version_string.into_boxed_str())
}