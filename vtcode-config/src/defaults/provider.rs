use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use once_cell::sync::Lazy;
use vtcode_commons::paths::WorkspacePaths;

const DEFAULT_CONFIG_FILE_NAME: &str = "vtcode.toml";
const DEFAULT_CONFIG_DIR_NAME: &str = ".vtcode";
const DEFAULT_SYNTAX_THEME: &str = "base16-ocean.dark";

static DEFAULT_SYNTAX_LANGUAGES: Lazy<Vec<String>> = Lazy::new(|| {
    vec![
        "rust",
        "python",
        "javascript",
        "typescript",
        "go",
        "java",
        "cpp",
        "c",
        "php",
        "html",
        "css",
        "sql",
        "csharp",
        "bash",
    ]
    .into_iter()
    .map(String::from)
    .collect()
});

static CONFIG_DEFAULTS: Lazy<RwLock<Arc<dyn ConfigDefaultsProvider>>> =
    Lazy::new(|| RwLock::new(Arc::new(DefaultConfigDefaults)));

/// Provides access to filesystem and syntax defaults used by the configuration
/// loader.
pub trait ConfigDefaultsProvider: Send + Sync {
    /// Returns the primary configuration file name expected in a workspace.
    fn config_file_name(&self) -> &str {
        DEFAULT_CONFIG_FILE_NAME
    }

    /// Creates a [`WorkspacePaths`] implementation for the provided workspace
    /// root.
    fn workspace_paths_for(&self, workspace_root: &Path) -> Box<dyn WorkspacePaths>;

    /// Returns the fallback configuration locations searched outside the
    /// workspace.
    fn home_config_paths(&self, config_file_name: &str) -> Vec<PathBuf>;

    /// Returns the default syntax highlighting theme identifier.
    fn syntax_theme(&self) -> String;

    /// Returns the default list of syntax highlighting languages.
    fn syntax_languages(&self) -> Vec<String>;
}

#[derive(Debug, Default)]
struct DefaultConfigDefaults;

impl ConfigDefaultsProvider for DefaultConfigDefaults {
    fn workspace_paths_for(&self, workspace_root: &Path) -> Box<dyn WorkspacePaths> {
        Box::new(DefaultWorkspacePaths::new(workspace_root.to_path_buf()))
    }

    fn home_config_paths(&self, config_file_name: &str) -> Vec<PathBuf> {
        default_home_paths(config_file_name)
    }

    fn syntax_theme(&self) -> String {
        DEFAULT_SYNTAX_THEME.to_string()
    }

    fn syntax_languages(&self) -> Vec<String> {
        default_syntax_languages()
    }
}

/// Installs a new [`ConfigDefaultsProvider`], returning the previous provider.
pub fn install_config_defaults_provider(
    provider: Arc<dyn ConfigDefaultsProvider>,
) -> Arc<dyn ConfigDefaultsProvider> {
    let mut guard = CONFIG_DEFAULTS
        .write()
        .expect("config defaults provider lock poisoned");
    std::mem::replace(&mut *guard, provider)
}

/// Restores the built-in defaults provider.
pub fn reset_to_default_config_defaults() {
    let _ = install_config_defaults_provider(Arc::new(DefaultConfigDefaults));
}

/// Executes the provided function with the currently installed provider.
pub fn with_config_defaults<F, R>(operation: F) -> R
where
    F: FnOnce(&dyn ConfigDefaultsProvider) -> R,
{
    let guard = CONFIG_DEFAULTS
        .read()
        .expect("config defaults provider lock poisoned");
    operation(guard.as_ref())
}

/// Returns the currently installed provider as an [`Arc`].
pub fn current_config_defaults() -> Arc<dyn ConfigDefaultsProvider> {
    let guard = CONFIG_DEFAULTS
        .read()
        .expect("config defaults provider lock poisoned");
    Arc::clone(&*guard)
}

pub fn with_config_defaults_provider_for_test<F, R>(
    provider: Arc<dyn ConfigDefaultsProvider>,
    action: F,
) -> R
where
    F: FnOnce() -> R,
{
    use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};

    let previous = install_config_defaults_provider(provider);
    let result = catch_unwind(AssertUnwindSafe(action));
    let _ = install_config_defaults_provider(previous);

    match result {
        Ok(value) => value,
        Err(payload) => resume_unwind(payload),
    }
}

fn default_home_paths(config_file_name: &str) -> Vec<PathBuf> {
    dirs::home_dir()
        .map(|home| home.join(DEFAULT_CONFIG_DIR_NAME).join(config_file_name))
        .into_iter()
        .collect()
}

fn default_syntax_languages() -> Vec<String> {
    DEFAULT_SYNTAX_LANGUAGES.clone()
}

#[derive(Debug, Clone)]
struct DefaultWorkspacePaths {
    root: PathBuf,
}

impl DefaultWorkspacePaths {
    fn new(root: PathBuf) -> Self {
        Self { root }
    }

    fn config_dir_path(&self) -> PathBuf {
        self.root.join(DEFAULT_CONFIG_DIR_NAME)
    }
}

impl WorkspacePaths for DefaultWorkspacePaths {
    fn workspace_root(&self) -> &Path {
        &self.root
    }

    fn config_dir(&self) -> PathBuf {
        self.config_dir_path()
    }

    fn cache_dir(&self) -> Option<PathBuf> {
        Some(self.config_dir_path().join("cache"))
    }

    fn telemetry_dir(&self) -> Option<PathBuf> {
        Some(self.config_dir_path().join("telemetry"))
    }
}

/// Adapter that maps an existing [`WorkspacePaths`] implementation into a
/// [`ConfigDefaultsProvider`].
#[derive(Debug, Clone)]
pub struct WorkspacePathsDefaults<P>
where
    P: WorkspacePaths + ?Sized,
{
    paths: Arc<P>,
    config_file_name: String,
    home_paths: Option<Vec<PathBuf>>,
    syntax_theme: String,
    syntax_languages: Vec<String>,
}

impl<P> WorkspacePathsDefaults<P>
where
    P: WorkspacePaths + 'static,
{
    /// Creates a defaults provider that delegates to the supplied
    /// [`WorkspacePaths`] implementation.
    pub fn new(paths: Arc<P>) -> Self {
        Self {
            paths,
            config_file_name: DEFAULT_CONFIG_FILE_NAME.to_string(),
            home_paths: None,
            syntax_theme: DEFAULT_SYNTAX_THEME.to_string(),
            syntax_languages: default_syntax_languages(),
        }
    }

    /// Overrides the configuration file name returned by the provider.
    pub fn with_config_file_name(mut self, file_name: impl Into<String>) -> Self {
        self.config_file_name = file_name.into();
        self
    }

    /// Overrides the fallback configuration search paths returned by the provider.
    pub fn with_home_paths(mut self, home_paths: Vec<PathBuf>) -> Self {
        self.home_paths = Some(home_paths);
        self
    }

    /// Overrides the default syntax theme returned by the provider.
    pub fn with_syntax_theme(mut self, theme: impl Into<String>) -> Self {
        self.syntax_theme = theme.into();
        self
    }

    /// Overrides the default syntax languages returned by the provider.
    pub fn with_syntax_languages(mut self, languages: Vec<String>) -> Self {
        self.syntax_languages = languages;
        self
    }

    /// Consumes the builder, returning a boxed provider implementation.
    pub fn build(self) -> Box<dyn ConfigDefaultsProvider> {
        Box::new(self)
    }
}

impl<P> ConfigDefaultsProvider for WorkspacePathsDefaults<P>
where
    P: WorkspacePaths + 'static,
{
    fn config_file_name(&self) -> &str {
        &self.config_file_name
    }

    fn workspace_paths_for(&self, _workspace_root: &Path) -> Box<dyn WorkspacePaths> {
        Box::new(WorkspacePathsWrapper {
            inner: Arc::clone(&self.paths),
        })
    }

    fn home_config_paths(&self, config_file_name: &str) -> Vec<PathBuf> {
        self.home_paths
            .clone()
            .unwrap_or_else(|| default_home_paths(config_file_name))
    }

    fn syntax_theme(&self) -> String {
        self.syntax_theme.clone()
    }

    fn syntax_languages(&self) -> Vec<String> {
        self.syntax_languages.clone()
    }
}

#[derive(Debug, Clone)]
struct WorkspacePathsWrapper<P>
where
    P: WorkspacePaths + ?Sized,
{
    inner: Arc<P>,
}

impl<P> WorkspacePaths for WorkspacePathsWrapper<P>
where
    P: WorkspacePaths + ?Sized,
{
    fn workspace_root(&self) -> &Path {
        self.inner.workspace_root()
    }

    fn config_dir(&self) -> PathBuf {
        self.inner.config_dir()
    }

    fn cache_dir(&self) -> Option<PathBuf> {
        self.inner.cache_dir()
    }

    fn telemetry_dir(&self) -> Option<PathBuf> {
        self.inner.telemetry_dir()
    }
}
