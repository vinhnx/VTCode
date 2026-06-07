use anyhow::Result;
use vtcode_config::{PtyConfig, VTCodeConfig};

#[test]
fn shell_zsh_fork_disabled_allows_missing_zsh_path() {
    let config = PtyConfig::default();
    assert!(config.validate().is_ok());
}

#[test]
fn pty_config_deserializes_with_defaults() -> Result<()> {
    let config: VTCodeConfig = toml::from_str(
        r#"
[pty]
scrollback_lines = 500
"#,
    )?;

    assert_eq!(config.pty.scrollback_lines, 500);
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
