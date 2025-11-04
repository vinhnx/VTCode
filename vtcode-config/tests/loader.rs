use anyhow::Result;
use assert_fs::TempDir;
use serial_test::serial;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use vtcode_commons::paths::WorkspacePaths;
use vtcode_config::ConfigManager;
use vtcode_config::constants::defaults;
use vtcode_config::defaults::provider::with_config_defaults_provider_for_test;
use vtcode_config::defaults::{ConfigDefaultsProvider, WorkspacePathsDefaults};

#[derive(Clone)]
struct TestPaths {
    root: PathBuf,
    config_dir: PathBuf,
}

impl TestPaths {
    fn new(root: PathBuf, config_dir: PathBuf) -> Self {
        Self { root, config_dir }
    }
}

impl WorkspacePaths for TestPaths {
    fn workspace_root(&self) -> &Path {
        &self.root
    }

    fn config_dir(&self) -> PathBuf {
        self.config_dir.clone()
    }
}

fn with_test_defaults<T>(
    workspace_root: &Path,
    config_dir: PathBuf,
    home_paths: Vec<PathBuf>,
    action: impl FnOnce() -> T,
) -> T {
    let workspace_paths = TestPaths::new(workspace_root.to_path_buf(), config_dir);
    let provider = WorkspacePathsDefaults::new(Arc::new(workspace_paths))
        .with_home_paths(home_paths)
        .build();
    let provider: Arc<dyn ConfigDefaultsProvider> = provider.into();

    with_config_defaults_provider_for_test(provider, action)
}

fn write_config(path: &Path, provider: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let contents = format!(
        "[agent]\nprovider = \"{}\"\nmax_conversation_turns = 5\n",
        provider
    );
    fs::write(path, contents)?;
    Ok(())
}

#[test]
#[serial]
fn loads_config_from_workspace_root_before_config_dir() -> Result<()> {
    let workspace = TempDir::new()?;
    let workspace_root = workspace.path();
    let config_dir = workspace_root.join(".vtcode");
    fs::create_dir_all(&config_dir)?;

    let root_config = workspace_root.join("vtcode.toml");
    let config_dir_config = config_dir.join("vtcode.toml");
    let home_config = workspace_root.join("home").join("vtcode.toml");

    write_config(&root_config, "workspace-root")?;
    write_config(&config_dir_config, "config-dir")?;
    write_config(&home_config, "home")?;

    let manager = with_test_defaults(
        workspace_root,
        config_dir,
        vec![home_config.clone()],
        || ConfigManager::load_from_workspace(workspace_root),
    )?;

    assert_eq!(manager.config().agent.provider, "workspace-root");
    assert_eq!(manager.config_path(), Some(root_config.as_path()));

    Ok(())
}

#[test]
#[serial]
fn loads_config_from_config_dir_when_root_missing() -> Result<()> {
    let workspace = TempDir::new()?;
    let workspace_root = workspace.path();
    let config_dir = workspace_root.join(".vtcode");
    fs::create_dir_all(&config_dir)?;

    let config_dir_config = config_dir.join("vtcode.toml");
    let home_config = workspace_root.join("home").join("vtcode.toml");

    write_config(&config_dir_config, "config-dir")?;
    write_config(&home_config, "home")?;

    let manager = with_test_defaults(
        workspace_root,
        config_dir,
        vec![home_config.clone()],
        || ConfigManager::load_from_workspace(workspace_root),
    )?;

    assert_eq!(manager.config().agent.provider, "config-dir");
    assert_eq!(manager.config_path(), Some(config_dir_config.as_path()));

    Ok(())
}

#[test]
#[serial]
fn loads_config_from_home_directory_when_workspace_missing() -> Result<()> {
    let workspace = TempDir::new()?;
    let workspace_root = workspace.path();
    let config_dir = workspace_root.join(".vtcode");
    fs::create_dir_all(&config_dir)?;

    let home_config = workspace_root.join("home").join("vtcode.toml");
    write_config(&home_config, "home")?;

    let manager = with_test_defaults(
        workspace_root,
        config_dir,
        vec![home_config.clone()],
        || ConfigManager::load_from_workspace(workspace_root),
    )?;

    assert_eq!(manager.config().agent.provider, "home");
    assert_eq!(manager.config_path(), Some(home_config.as_path()));

    Ok(())
}

#[test]
#[serial]
fn falls_back_to_default_config_when_no_files_found() -> Result<()> {
    let workspace = TempDir::new()?;
    let workspace_root = workspace.path();
    let config_dir = workspace_root.join(".vtcode");
    fs::create_dir_all(&config_dir)?;

    let manager = with_test_defaults(workspace_root, config_dir, Vec::new(), || {
        ConfigManager::load_from_workspace(workspace_root)
    })?;

    assert!(manager.config_path().is_none());
    assert_eq!(manager.config().agent.provider, defaults::DEFAULT_PROVIDER);

    Ok(())
}
