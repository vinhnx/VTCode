use super::{resolve_shell_preference, resolve_shell_preference_with_zsh_fork};
use crate::config::PtyConfig;
use crate::tools::shell::resolve_fallback_shell;

#[test]
fn explicit_shell_overrides_config_preference() {
    let config = PtyConfig {
        preferred_shell: Some("/bin/bash".to_string()),
        ..Default::default()
    };

    let resolved = resolve_shell_preference(Some(" /bin/zsh "), &config);
    assert_eq!(resolved, "/bin/zsh");
}

#[test]
fn config_preferred_shell_used_when_explicit_missing() {
    let config = PtyConfig {
        preferred_shell: Some("zsh".to_string()),
        ..Default::default()
    };

    let resolved = resolve_shell_preference(None, &config);
    assert_eq!(resolved, "zsh");
}

#[test]
fn blank_explicit_shell_falls_back_to_config_preference() {
    let config = PtyConfig {
        preferred_shell: Some("bash".to_string()),
        ..Default::default()
    };

    let resolved = resolve_shell_preference(Some("   "), &config);
    assert_eq!(resolved, "bash");
}

#[test]
fn blank_config_shell_falls_back_to_default_resolver() {
    let config = PtyConfig {
        preferred_shell: Some("   ".to_string()),
        ..Default::default()
    };

    let resolved = resolve_shell_preference(None, &config);
    assert_eq!(resolved, resolve_fallback_shell());
}

#[test]
fn missing_preferences_fall_back_to_default_resolver() {
    let config = PtyConfig::default();
    let resolved = resolve_shell_preference(None, &config);
    assert_eq!(resolved, resolve_fallback_shell());
}

#[test]
fn zsh_fork_disabled_uses_standard_shell_resolution() -> anyhow::Result<()> {
    let config = PtyConfig {
        preferred_shell: Some("/bin/bash".to_string()),
        ..Default::default()
    };
    let resolved = resolve_shell_preference_with_zsh_fork(None, &config)?;
    assert_eq!(resolved, "/bin/bash");
    Ok(())
}

#[test]
fn zsh_fork_missing_path_returns_error() {
    let config = PtyConfig {
        shell_zsh_fork: true,
        zsh_path: None,
        ..PtyConfig::default()
    };
    resolve_shell_preference_with_zsh_fork(Some("/bin/bash"), &config).unwrap_err();
}

#[cfg(unix)]
#[test]
fn zsh_fork_ignores_explicit_shell_and_uses_configured_path() -> anyhow::Result<()> {
    let zsh = tempfile::NamedTempFile::new()?;
    let expected = zsh.path().to_string_lossy().to_string();
    let config = PtyConfig {
        shell_zsh_fork: true,
        zsh_path: Some(expected.clone()),
        ..PtyConfig::default()
    };
    let resolved = resolve_shell_preference_with_zsh_fork(Some("/bin/bash"), &config)?;
    assert_eq!(resolved, expected);
    Ok(())
}
