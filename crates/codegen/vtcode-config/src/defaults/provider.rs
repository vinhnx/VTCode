use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use std::sync::LazyLock;
use vtcode_commons::VtCodePaths;
use vtcode_commons::paths::WorkspacePaths;

const DEFAULT_CONFIG_FILE_NAME: &str = "vtcode.toml";
const DEFAULT_CONFIG_DIR_NAME: &str = ".vtcode";
const DEFAULT_SYNTAX_THEME: &str = "base16-ocean.dark";

/// Empty by default — all ~250 syntect grammars are enabled.
/// Users can set `enabled_languages` in vtcode.toml to restrict to a subset.
static DEFAULT_SYNTAX_LANGUAGES: LazyLock<Vec<String>> = LazyLock::new(Vec::new);

static CONFIG_DEFAULTS: LazyLock<RwLock<Arc<dyn ConfigDefaultsProvider>>> =
    LazyLock::new(|| RwLock::new(Arc::new(DefaultConfigDefaults)));

fn read_env_var(key: &str) -> Option<String> {
    crate::env_helpers::read_env_var(key)
}

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

    /// Returns the canonical user configuration directory.
    ///
    /// The default preserves custom providers by deriving it from their
    /// highest-precedence home configuration path.
    fn canonical_user_config_dir(&self) -> anyhow::Result<Option<PathBuf>> {
        Ok(self
            .canonical_user_config_path(self.config_file_name())?
            .and_then(|path| path.parent().map(Path::to_path_buf)))
    }

    /// Returns the canonical user configuration file for `config_file_name`.
    ///
    /// Existing providers remain compatible: their last home path has always
    /// represented the highest-precedence user layer.
    fn canonical_user_config_path(&self, config_file_name: &str) -> anyhow::Result<Option<PathBuf>> {
        Ok(self.home_config_paths(config_file_name).into_iter().last())
    }

    /// Returns system configuration files from lowest to highest precedence.
    fn system_config_paths(&self, _config_file_name: &str) -> anyhow::Result<Vec<PathBuf>> {
        Ok(Vec::new())
    }

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

    fn canonical_user_config_dir(&self) -> anyhow::Result<Option<PathBuf>> {
        Ok(Some(VtCodePaths::resolve()?.config_dir().to_path_buf()))
    }

    fn canonical_user_config_path(&self, config_file_name: &str) -> anyhow::Result<Option<PathBuf>> {
        Ok(Some(VtCodePaths::resolve()?.config_path(config_file_name)?))
    }

    fn system_config_paths(&self, config_file_name: &str) -> anyhow::Result<Vec<PathBuf>> {
        VtCodePaths::resolve()?.system_config_paths(config_file_name)
    }

    fn syntax_theme(&self) -> String {
        DEFAULT_SYNTAX_THEME.to_string()
    }

    fn syntax_languages(&self) -> Vec<String> {
        default_syntax_languages()
    }
}

/// Installs a new [`ConfigDefaultsProvider`], returning the previous provider.
pub fn install_config_defaults_provider(provider: Arc<dyn ConfigDefaultsProvider>) -> Arc<dyn ConfigDefaultsProvider> {
    let mut guard = CONFIG_DEFAULTS.write().unwrap_or_else(|poisoned| {
        tracing::warn!("config defaults provider lock poisoned while installing provider; recovering");
        poisoned.into_inner()
    });
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
    let guard = CONFIG_DEFAULTS.read().unwrap_or_else(|poisoned| {
        tracing::warn!("config defaults provider lock poisoned while reading provider; recovering");
        poisoned.into_inner()
    });
    operation(guard.as_ref())
}

/// Returns the currently installed provider as an [`Arc`].
pub fn current_config_defaults() -> Arc<dyn ConfigDefaultsProvider> {
    let guard = CONFIG_DEFAULTS.read().unwrap_or_else(|poisoned| {
        tracing::warn!("config defaults provider lock poisoned while cloning provider; recovering");
        poisoned.into_inner()
    });
    Arc::clone(&*guard)
}

pub fn with_config_defaults_provider_for_test<F, R>(provider: Arc<dyn ConfigDefaultsProvider>, action: F) -> R
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

/// Resolves the canonical configuration directory through the shared path
/// policy. The legacy directory remains a read-only compatibility candidate;
/// callers writing configuration must use the canonical path.
///
/// Returns `None` if no suitable directory can be determined.
pub fn get_config_dir() -> Option<PathBuf> {
    resolve_vtcode_paths().ok().map(|paths| paths.config_dir().to_path_buf())
}

/// Returns the canonical VT Code data directory. The legacy root is only a
/// migration/read-compatibility source.
///
/// Returns `None` if no suitable directory can be determined.
pub fn get_data_dir() -> Option<PathBuf> {
    resolve_vtcode_paths().ok().map(|paths| paths.data_dir().to_path_buf())
}

fn resolve_vtcode_paths() -> anyhow::Result<VtCodePaths> {
    #[cfg(test)]
    {
        // The config crate's test environment helper intentionally avoids
        // mutating the process environment. Feed those overrides through the
        // shared resolver so the public compatibility APIs keep their test
        // semantics without duplicating path parsing here.
        let mut environment: Vec<(String, String)> = std::env::vars().collect();
        for key in ["VTCODE_CONFIG", "VTCODE_DATA"] {
            if crate::env_helpers::test_env_overrides::is_overridden(key) {
                environment.retain(|(name, _)| name != key);
                if let Some(value) = read_env_var(key) {
                    environment.push((key.to_string(), value));
                }
            }
        }
        let environment_refs: Vec<(&str, &str)> =
            environment.iter().map(|(key, value)| (key.as_str(), value.as_str())).collect();
        VtCodePaths::from_environment(&environment_refs)
    }

    #[cfg(not(test))]
    {
        VtCodePaths::resolve()
    }
}

fn default_home_paths(config_file_name: &str) -> Vec<PathBuf> {
    if let Ok(resolver) = VtCodePaths::resolve() {
        let mut paths = vec![resolver.legacy_home_dir().join(config_file_name)];
        if let Ok(canonical_path) = resolver.config_path(config_file_name)
            && !paths.iter().any(|path| path == &canonical_path)
        {
            paths.push(canonical_path);
        }
        return paths;
    }

    let mut paths = Vec::with_capacity(2);

    // 1. Legacy fallback (lower precedence) — the historical VTCODE_HOME file.
    if let Some(home_dir) = dirs::home_dir() {
        paths.push(home_dir.join(DEFAULT_CONFIG_DIR_NAME).join(config_file_name));
    }

    // 2. Canonical platform config path (higher precedence).
    if let Some(config_dir) = get_config_dir() {
        let xdg_path = config_dir.join(config_file_name);
        if !paths.iter().any(|p| p == &xdg_path) {
            paths.push(xdg_path);
        }
    }

    paths
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
    system_paths: Option<Vec<PathBuf>>,
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
            system_paths: None,
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

    /// Overrides the system configuration search paths returned by the provider.
    pub fn with_system_config_paths(mut self, system_paths: Vec<PathBuf>) -> Self {
        self.system_paths = Some(system_paths);
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
        Box::new(WorkspacePathsWrapper { inner: Arc::clone(&self.paths) })
    }

    fn home_config_paths(&self, config_file_name: &str) -> Vec<PathBuf> {
        self.home_paths.clone().unwrap_or_else(|| default_home_paths(config_file_name))
    }

    fn system_config_paths(&self, config_file_name: &str) -> anyhow::Result<Vec<PathBuf>> {
        let _ = config_file_name;
        Ok(self.system_paths.clone().unwrap_or_default())
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

#[cfg(test)]
mod tests {
    use super::{get_config_dir, get_data_dir};
    use serial_test::serial;
    use std::path::PathBuf;

    fn with_env_var<F>(key: &str, value: Option<&str>, f: F)
    where
        F: FnOnce(),
    {
        let previous = crate::env_helpers::test_env_overrides::get(key);
        crate::env_helpers::test_env_overrides::set(key, value);

        f();

        crate::env_helpers::test_env_overrides::restore(key, previous);
    }

    #[test]
    #[serial]
    fn get_config_dir_uses_env_override() {
        with_env_var("VTCODE_CONFIG", Some("/tmp/vtcode-config-test"), || {
            assert_eq!(get_config_dir(), Some(PathBuf::from("/tmp/vtcode-config-test")));
        });
    }

    #[test]
    #[serial]
    fn get_data_dir_uses_env_override() {
        with_env_var("VTCODE_DATA", Some("/tmp/vtcode-data-test"), || {
            assert_eq!(get_data_dir(), Some(PathBuf::from("/tmp/vtcode-data-test")));
        });
    }

    #[test]
    #[serial]
    fn get_config_dir_ignores_blank_env_override() {
        with_env_var("VTCODE_CONFIG", Some("   "), || {
            let resolved = get_config_dir();
            assert!(resolved.is_some());
            assert_ne!(resolved, Some(PathBuf::from("   ")));
            assert_ne!(resolved, Some(PathBuf::new()));
        });
    }

    #[test]
    #[serial]
    fn get_data_dir_ignores_blank_env_override() {
        with_env_var("VTCODE_DATA", Some("   "), || {
            let resolved = get_data_dir();
            assert!(resolved.is_some());
            assert_ne!(resolved, Some(PathBuf::from("   ")));
            assert_ne!(resolved, Some(PathBuf::new()));
        });
    }

    #[test]
    #[serial]
    fn env_guard_restores_original_value() {
        let key = "VTCODE_CONFIG";
        let initial = super::read_env_var(key);
        with_env_var(key, Some("/tmp/vtcode-config-test"), || {
            assert_eq!(super::read_env_var(key), Some("/tmp/vtcode-config-test".to_string()));
        });
        assert_eq!(super::read_env_var(key), initial);
    }
}
