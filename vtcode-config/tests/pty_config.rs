use anyhow::Result;
use vtcode_config::{PtyConfig, PtyEmulationBackend, VTCodeConfig};

#[test]
fn shell_zsh_fork_disabled_allows_missing_zsh_path() {
    let config = PtyConfig::default();
    assert!(config.validate().is_ok());
}

#[test]
fn pty_defaults_to_ghostty_backend() {
    let config = PtyConfig::default();
    assert_eq!(config.emulation_backend, PtyEmulationBackend::Ghostty);
}

#[test]
fn pty_deserializes_ghostty_backend() -> Result<()> {
    let config: VTCodeConfig = toml::from_str(
        r#"
[pty]
emulation_backend = "ghostty"
"#,
    )?;

    assert_eq!(config.pty.emulation_backend, PtyEmulationBackend::Ghostty);
    Ok(())
}

#[test]
fn shell_zsh_fork_enabled_requires_zsh_path() {
    let config = PtyConfig {
        shell_zsh_fork: true,
        zsh_path: None,
        ..PtyConfig::default()
    };
    assert!(config.validate().is_err());
}

#[cfg(unix)]
#[test]
fn shell_zsh_fork_enabled_rejects_relative_zsh_path() {
    let config = PtyConfig {
        shell_zsh_fork: true,
        zsh_path: Some("zsh".to_string()),
        ..PtyConfig::default()
    };
    assert!(config.validate().is_err());
}

#[cfg(unix)]
#[test]
fn shell_zsh_fork_enabled_accepts_existing_absolute_file() -> Result<()> {
    let temp_file = tempfile::NamedTempFile::new()?;
    let config = PtyConfig {
        shell_zsh_fork: true,
        zsh_path: Some(temp_file.path().to_string_lossy().to_string()),
        ..PtyConfig::default()
    };
    assert!(config.validate().is_ok());
    Ok(())
}
