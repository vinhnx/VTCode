use std::path::PathBuf;
use std::sync::Arc;

use tempfile::tempdir;
use vtcode_commons::reference::StaticWorkspacePaths;
use vtcode_config::ConfigManager;
use vtcode_config::defaults::{
    ConfigDefaultsProvider, WorkspacePathsDefaults, install_config_defaults_provider,
};

fn main() -> anyhow::Result<()> {
    // Create a disposable workspace that mimics a downstream project layout.
    let workspace = tempdir()?;
    let workspace_root = workspace.path().to_path_buf();
    let config_dir = workspace_root.join("config");

    std::fs::create_dir_all(&config_dir)?;
    std::fs::write(
        config_dir.join("settings.toml"),
        r#"
            [agent]
            provider = "openai"
            default_model = "gpt-4.1-mini"
        "#
        .trim(),
    )?;

    // Wrap the custom workspace paths with a defaults provider so the loader
    // searches the new directories and uses the desired syntax defaults.
    let static_paths = StaticWorkspacePaths::new(&workspace_root, &config_dir)
        .with_cache_dir(config_dir.join("cache"))
        .with_telemetry_dir(config_dir.join("telemetry"));

    let defaults = WorkspacePathsDefaults::new(Arc::new(static_paths))
        .with_config_file_name("settings.toml")
        .with_home_paths(Vec::<PathBuf>::new())
        .with_syntax_theme("tokyonight_night")
        .with_syntax_languages(vec![
            "rust".to_string(),
            "python".to_string(),
            "typescript".to_string(),
        ]);
    let defaults: Arc<dyn ConfigDefaultsProvider> = Arc::new(defaults);

    // Swap the provider during the configuration load so downstream adopters
    // can bootstrap without touching global `.vtcode` directories.
    let previous = install_config_defaults_provider(Arc::clone(&defaults));
    let manager = ConfigManager::load_from_workspace(&workspace_root)?;

    println!(
        "Agent provider: {}\nSyntax theme: {}\nLanguages: {}",
        manager.config().agent.provider,
        defaults.syntax_theme(),
        defaults.syntax_languages().join(", "),
    );

    // Restore the original provider before exiting.
    install_config_defaults_provider(previous);
    Ok(())
}
