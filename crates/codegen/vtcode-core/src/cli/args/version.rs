use super::*;

pub(super) fn parse_workspace_directory(raw: &str) -> Result<PathBuf, String> {
    let candidate = PathBuf::from(raw);
    if !candidate.exists() {
        return Err(format!(
            "'{}' is not a valid workspace path or subcommand.\n\
             Run `vtcode --help` to see available commands and options.",
            raw
        ));
    }

    Ok(candidate)
}

/// Multi-line `--version` report whose first line is the caller-supplied
/// build stamp (`{version}+{sha}[.dirty]` from the root build script, or the
/// bare crate version). The stamp is a parameter — not `option_env!` — because
/// `cargo:rustc-env` never crosses package boundaries, so only the root crate
/// can read what its own build script sets. Keep the stamp to a single token:
/// the desktop updater parses this first line strictly.
pub fn long_version(git_info: &str) -> String {
    let path_diagnostics = match vtcode_commons::VtCodePaths::resolve() {
        Ok(paths) => format!(
            "Config directory: {}\nData directory: {}\nState directory: {}\nCache directory: {}\nRuntime directory: {}\nExecutable directory: {}\nLegacy directory: {}\nMigration marker: {}\nMigration report: {}",
            paths.config_dir().display(),
            paths.data_dir().display(),
            paths.state_dir().display(),
            paths.cache_dir().display(),
            paths.runtime_dir().display(),
            paths.executable_dir().display(),
            paths.legacy_dir().display(),
            paths.migration_marker_path().display(),
            paths.migration_report_path().display(),
        ),
        Err(error) => format!("Storage paths: unavailable ({error:#})"),
    };
    let environment = [
        "HOME",
        "VTCODE_HOME",
        "VTCODE_CONFIG",
        "VTCODE_CONFIG_PATH",
        "VTCODE_DATA",
        "CODEX_HOME",
        "XDG_CONFIG_HOME",
        "XDG_DATA_HOME",
        "XDG_STATE_HOME",
        "XDG_CACHE_HOME",
        "XDG_RUNTIME_DIR",
        "XDG_BIN_HOME",
        "XDG_CONFIG_DIRS",
        "XDG_DATA_DIRS",
    ]
    .iter()
    .map(|name| {
        let value = env::var_os(name)
            .map(|value| value.to_string_lossy().into_owned())
            .unwrap_or_else(|| "<not set>".to_string());
        format!("  {name}={value}")
    })
    .collect::<Vec<_>>()
    .join("\n");

    format!(
        "{}\n\nAuthors: {}\n{}\n\nEnvironment variables:\n{}",
        git_info,
        env!("CARGO_PKG_AUTHORS"),
        path_diagnostics,
        environment,
    )
}
