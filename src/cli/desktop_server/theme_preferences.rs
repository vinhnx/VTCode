//! Persist Desktop theme selections through the CLI's layered config writer.
use std::path::PathBuf;

use anyhow::{Context, Result};
use vtcode_core::config::loader::ConfigManager;

pub(super) async fn persist(workspace: PathBuf, theme: String) -> Result<String> {
    validate_theme(&theme)?;
    tokio::task::spawn_blocking(move || {
        persist_sync(&workspace, &theme)?;
        Ok::<_, anyhow::Error>(theme)
    })
    .await
    .context("CLI theme persistence task failed")?
}

fn validate_theme(theme: &str) -> Result<()> {
    vtcode_ui::theme::ensure_theme(theme).context("unsupported built-in VT Code theme")?;
    Ok(())
}

fn persist_sync(workspace: &std::path::Path, theme: &str) -> Result<()> {
    let mut manager =
        ConfigManager::load_from_workspace(workspace).context("failed to load CLI config for theme synchronization")?;
    let mut config = manager.config().clone();
    if config.agent.theme != theme {
        config.agent.theme = theme.to_string();
        manager
            .save_config(&config)
            .context("failed to save CLI theme to configuration")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_registered_cli_themes_can_be_saved() {
        assert!(validate_theme("ciapre-blue").is_ok());
        assert!(validate_theme("system-dark").is_err());
        assert!(validate_theme("../../vtcode.toml").is_err());
    }

    #[tokio::test]
    async fn desktop_theme_sync_uses_the_cli_config_writer_without_discarding_comments() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("vtcode.toml");
        std::fs::write(&path, "# local user note\n[agent]\ntheme = \"ciapre\"\n").expect("initial config");
        let selected = persist(directory.path().to_path_buf(), "ciapre-blue".into())
            .await
            .expect("save theme through CLI workspace writer");
        assert_eq!(selected, "ciapre-blue");

        let saved = std::fs::read_to_string(path).expect("saved config");
        assert!(saved.contains("# local user note"));
        let reloaded = ConfigManager::load_from_workspace(directory.path()).expect("reload workspace config");
        assert_eq!(reloaded.config().agent.theme, "ciapre-blue");
    }
}
