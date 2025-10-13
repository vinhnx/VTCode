//! Dot folder configuration and cache management

use crate::config::constants::defaults;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

/// VTCode configuration stored in ~/.vtcode/
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DotConfig {
    pub version: String,
    pub last_updated: u64,
    pub preferences: UserPreferences,
    pub providers: ProviderConfigs,
    pub cache: CacheConfig,
    pub ui: UiConfig,
    #[serde(default)]
    pub workspace_trust: WorkspaceTrustStore,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserPreferences {
    pub default_model: String,
    pub default_provider: String,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
    pub auto_save: bool,
    pub theme: String,
    pub keybindings: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProviderConfigs {
    pub openai: Option<ProviderConfig>,
    pub anthropic: Option<ProviderConfig>,
    pub gemini: Option<ProviderConfig>,
    pub deepseek: Option<ProviderConfig>,
    pub openrouter: Option<ProviderConfig>,
    pub xai: Option<ProviderConfig>,
    pub ollama: Option<ProviderConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WorkspaceTrustStore {
    #[serde(default)]
    pub entries: HashMap<String, WorkspaceTrustRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceTrustRecord {
    pub level: WorkspaceTrustLevel,
    pub trusted_at: u64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceTrustLevel {
    ToolsPolicy,
    FullAuto,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProviderConfig {
    pub api_key: Option<String>,
    pub base_url: Option<String>,
    pub model: Option<String>,
    pub enabled: bool,
    pub priority: i32, // Higher priority = preferred
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheConfig {
    pub enabled: bool,
    pub max_size_mb: u64,
    pub ttl_days: u64,
    pub prompt_cache_enabled: bool,
    pub context_cache_enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiConfig {
    pub show_timestamps: bool,
    pub max_output_lines: usize,
    pub syntax_highlighting: bool,
    pub auto_complete: bool,
    pub history_size: usize,
}

impl Default for DotConfig {
    fn default() -> Self {
        Self {
            version: env!("CARGO_PKG_VERSION").to_string(),
            last_updated: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            preferences: UserPreferences::default(),
            providers: ProviderConfigs::default(),
            cache: CacheConfig::default(),
            ui: UiConfig::default(),
            workspace_trust: WorkspaceTrustStore::default(),
        }
    }
}

impl Default for UserPreferences {
    fn default() -> Self {
        Self {
            default_model: defaults::DEFAULT_MODEL.to_string(),
            default_provider: defaults::DEFAULT_PROVIDER.to_string(),
            max_tokens: Some(4096),
            temperature: Some(0.7),
            auto_save: true,
            theme: defaults::DEFAULT_THEME.to_string(),
            keybindings: HashMap::new(),
        }
    }
}

impl Default for WorkspaceTrustLevel {
    fn default() -> Self {
        Self::ToolsPolicy
    }
}

impl fmt::Display for WorkspaceTrustLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WorkspaceTrustLevel::ToolsPolicy => write!(f, "tools policy"),
            WorkspaceTrustLevel::FullAuto => write!(f, "full auto"),
        }
    }
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_size_mb: 100,
            ttl_days: 30,
            prompt_cache_enabled: true,
            context_cache_enabled: true,
        }
    }
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            show_timestamps: true,
            max_output_lines: 1000,
            syntax_highlighting: true,
            auto_complete: true,
            history_size: 1000,
        }
    }
}

/// Directory layout configuration for [`DotManager`].
#[derive(Debug, Clone)]
pub struct DotDirectoryLayout {
    /// Relative path (from the dot root) to the primary configuration file.
    pub config_file: PathBuf,
    /// Relative path to the cache root directory.
    pub cache_root_dir: PathBuf,
    /// Relative path to the prompts cache directory.
    pub prompts_cache_dir: PathBuf,
    /// Relative path to the context cache directory.
    pub context_cache_dir: PathBuf,
    /// Relative path to the models cache directory.
    pub models_cache_dir: PathBuf,
    /// Relative path to the logs directory.
    pub logs_dir: PathBuf,
    /// Relative path to the sessions directory.
    pub sessions_dir: PathBuf,
    /// Relative path to the backups directory.
    pub backups_dir: PathBuf,
    /// Additional directories to be created during initialization.
    pub additional_directories: Vec<PathBuf>,
}

impl Default for DotDirectoryLayout {
    fn default() -> Self {
        Self {
            config_file: PathBuf::from("config.toml"),
            cache_root_dir: PathBuf::from("cache"),
            prompts_cache_dir: PathBuf::from("cache/prompts"),
            context_cache_dir: PathBuf::from("cache/context"),
            models_cache_dir: PathBuf::from("cache/models"),
            logs_dir: PathBuf::from("logs"),
            sessions_dir: PathBuf::from("sessions"),
            backups_dir: PathBuf::from("backups"),
            additional_directories: Vec::new(),
        }
    }
}

impl DotDirectoryLayout {
    /// Create a new layout based on [`Default`].
    pub fn new() -> Self {
        Self::default()
    }

    /// Override the configuration file location.
    pub fn with_config_file(mut self, relative_path: impl Into<PathBuf>) -> Self {
        self.config_file = relative_path.into();
        self
    }

    /// Override the cache root directory.
    pub fn with_cache_root_dir(mut self, relative_path: impl Into<PathBuf>) -> Self {
        self.cache_root_dir = relative_path.into();
        self
    }

    /// Override the prompts cache directory.
    pub fn with_prompts_cache_dir(mut self, relative_path: impl Into<PathBuf>) -> Self {
        self.prompts_cache_dir = relative_path.into();
        self
    }

    /// Override the context cache directory.
    pub fn with_context_cache_dir(mut self, relative_path: impl Into<PathBuf>) -> Self {
        self.context_cache_dir = relative_path.into();
        self
    }

    /// Override the models cache directory.
    pub fn with_models_cache_dir(mut self, relative_path: impl Into<PathBuf>) -> Self {
        self.models_cache_dir = relative_path.into();
        self
    }

    /// Override the logs directory.
    pub fn with_logs_dir(mut self, relative_path: impl Into<PathBuf>) -> Self {
        self.logs_dir = relative_path.into();
        self
    }

    /// Override the sessions directory.
    pub fn with_sessions_dir(mut self, relative_path: impl Into<PathBuf>) -> Self {
        self.sessions_dir = relative_path.into();
        self
    }

    /// Override the backups directory.
    pub fn with_backups_dir(mut self, relative_path: impl Into<PathBuf>) -> Self {
        self.backups_dir = relative_path.into();
        self
    }

    /// Append an additional directory to create during initialization.
    pub fn with_additional_directory(mut self, relative_path: impl Into<PathBuf>) -> Self {
        self.additional_directories.push(relative_path.into());
        self
    }

    /// Validate the layout and return an error when a required path is empty.
    fn validate(&self) -> Result<(), DotError> {
        let required = [
            ("config_file", &self.config_file),
            ("cache_root_dir", &self.cache_root_dir),
            ("prompts_cache_dir", &self.prompts_cache_dir),
            ("context_cache_dir", &self.context_cache_dir),
            ("models_cache_dir", &self.models_cache_dir),
            ("logs_dir", &self.logs_dir),
            ("sessions_dir", &self.sessions_dir),
            ("backups_dir", &self.backups_dir),
        ];

        for (name, path) in required {
            if path.as_os_str().is_empty() {
                return Err(DotError::InvalidLayout(format!("{name} must not be empty")));
            }
        }

        Ok(())
    }

    fn directories(&self) -> Vec<PathBuf> {
        let mut directories = vec![
            self.cache_root_dir.clone(),
            self.prompts_cache_dir.clone(),
            self.context_cache_dir.clone(),
            self.models_cache_dir.clone(),
            self.logs_dir.clone(),
            self.sessions_dir.clone(),
            self.backups_dir.clone(),
        ];

        if let Some(parent) = self.config_file.parent() {
            if !parent.as_os_str().is_empty() {
                directories.push(parent.to_path_buf());
            }
        }

        directories.extend(self.additional_directories.iter().cloned());
        directories
    }
}

/// Strategy for recovering a configuration file when loading fails.
#[derive(Debug, Clone)]
pub struct ConfigRecoveryStrategy {
    /// Whether to scan existing backups (newest first) when attempting to
    /// recover from a corrupted configuration file.
    pub prefer_backups: bool,
    /// Whether to recreate the configuration from defaults when no usable
    /// backup is found.
    pub fallback_to_default: bool,
    /// Maximum number of backups to inspect when attempting a recovery.
    pub max_backup_attempts: usize,
    /// Whether to quarantine the corrupted configuration file by renaming it
    /// before a recovery attempt.
    pub quarantine_corrupted: bool,
}

impl Default for ConfigRecoveryStrategy {
    fn default() -> Self {
        Self {
            prefer_backups: true,
            fallback_to_default: true,
            max_backup_attempts: 5,
            quarantine_corrupted: true,
        }
    }
}

/// Outcome of loading or recovering the configuration file.
#[derive(Debug, Clone)]
pub struct ConfigLoadOutcome {
    /// The loaded configuration.
    pub config: DotConfig,
    /// Where the configuration data originated from during the load.
    pub source: ConfigLoadSource,
    /// If the existing configuration file was quarantined, this contains the
    /// path to the renamed file.
    pub quarantined_config: Option<PathBuf>,
}

/// Indicates how the configuration was obtained during a load/recovery.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigLoadSource {
    /// The configuration file existed and was loaded successfully without any
    /// recovery steps.
    Existing,
    /// The configuration file was missing and has been initialized using the
    /// default template (possibly customized via a callback).
    Initialized,
    /// A backup file was restored because the primary configuration could not
    /// be parsed.
    BackupRestored(PathBuf),
    /// No usable backups were found; the configuration was recreated from the
    /// default template (possibly customized via a callback).
    DefaultRecreated,
}

/// Dot folder manager for VTCode configuration and cache
pub struct DotManager {
    root_dir: PathBuf,
    layout: DotDirectoryLayout,
}

impl DotManager {
    pub fn new() -> Result<Self, DotError> {
        Self::with_product_name("vtcode")
    }

    /// Create a dot manager using the provided product name.
    ///
    /// The manager automatically prefixes the provided name with a leading dot
    /// (".") and stores data in `$HOME/.<product-name>/`.
    pub fn with_product_name(product_name: impl AsRef<str>) -> Result<Self, DotError> {
        Self::with_product_name_and_layout(product_name, DotDirectoryLayout::default())
    }

    /// Create a dot manager using the provided product name and directory layout.
    pub fn with_product_name_and_layout(
        product_name: impl AsRef<str>,
        layout: DotDirectoryLayout,
    ) -> Result<Self, DotError> {
        let home_dir = dirs::home_dir().ok_or(DotError::HomeDirNotFound)?;
        Self::with_home_dir_and_layout(home_dir, product_name, layout)
    }

    /// Create a dot manager rooted at the provided home directory and product
    /// name. Primarily useful for tests or host applications that wish to place
    /// the dot-folder under an alternate home directory.
    pub fn with_home_dir(
        home_dir: impl AsRef<Path>,
        product_name: impl AsRef<str>,
    ) -> Result<Self, DotError> {
        Self::with_home_dir_and_layout(home_dir, product_name, DotDirectoryLayout::default())
    }

    /// Create a dot manager rooted at the provided home directory and product
    /// name with a custom layout.
    pub fn with_home_dir_and_layout(
        home_dir: impl AsRef<Path>,
        product_name: impl AsRef<str>,
        layout: DotDirectoryLayout,
    ) -> Result<Self, DotError> {
        let root_dir_name = Self::normalize_product_dir(product_name.as_ref());
        let root_dir = home_dir.as_ref().join(root_dir_name);
        Self::with_root_dir_and_layout(root_dir, layout)
    }

    /// Create a dot manager using a fully-qualified root directory.
    pub fn with_root_dir(root_dir: impl Into<PathBuf>) -> Result<Self, DotError> {
        Self::with_root_dir_and_layout(root_dir, DotDirectoryLayout::default())
    }

    /// Create a dot manager using a fully-qualified root directory and layout.
    pub fn with_root_dir_and_layout(
        root_dir: impl Into<PathBuf>,
        layout: DotDirectoryLayout,
    ) -> Result<Self, DotError> {
        layout.validate()?;
        Ok(Self {
            root_dir: root_dir.into(),
            layout,
        })
    }

    fn normalize_product_dir(product_name: &str) -> String {
        let trimmed = product_name.trim();
        let remainder = trimmed.strip_prefix('.').unwrap_or(trimmed);

        let mut slug = String::new();

        for ch in remainder.chars() {
            if ch.is_ascii_alphanumeric() {
                slug.push(ch.to_ascii_lowercase());
            } else if matches!(ch, '-' | '_') {
                if !(ch == '-' && slug.ends_with('-')) {
                    slug.push(ch);
                }
            } else if ch.is_whitespace() {
                if !slug.ends_with('-') {
                    slug.push('-');
                }
            } else if !slug.ends_with('-') {
                slug.push('-');
            }
        }

        while slug.starts_with('-') {
            slug.remove(0);
        }

        while slug.ends_with('-') {
            slug.pop();
        }

        if slug.is_empty() {
            slug.push_str("app");
        }

        format!(".{}", slug)
    }

    /// Return the root configuration directory (the `.product` folder).
    pub fn root_dir(&self) -> &Path {
        &self.root_dir
    }

    /// Return the configured directory layout.
    pub fn layout(&self) -> &DotDirectoryLayout {
        &self.layout
    }

    fn resolve(&self, relative: &Path) -> PathBuf {
        if relative.is_absolute() {
            relative.to_path_buf()
        } else {
            self.root_dir.join(relative)
        }
    }

    /// Return the fully-qualified path to the configuration file.
    pub fn config_file_path(&self) -> PathBuf {
        self.resolve(&self.layout.config_file)
    }

    /// Return the cache root directory.
    pub fn cache_root_dir(&self) -> PathBuf {
        self.resolve(&self.layout.cache_root_dir)
    }

    fn prompts_cache_dir(&self) -> PathBuf {
        self.resolve(&self.layout.prompts_cache_dir)
    }

    fn context_cache_dir(&self) -> PathBuf {
        self.resolve(&self.layout.context_cache_dir)
    }

    fn models_cache_dir(&self) -> PathBuf {
        self.resolve(&self.layout.models_cache_dir)
    }

    /// Initialize the dot folder structure using the default [`DotConfig`].
    pub fn initialize(&self) -> Result<(), DotError> {
        self.initialize_with(|_| {})
    }

    /// Initialize the dot folder structure with a custom default configuration.
    ///
    /// The provided callback receives a mutable reference to the default
    /// [`DotConfig`] so callers can override fields before the config is
    /// persisted. The callback is only executed when the configuration file
    /// does not already exist on disk.
    pub fn initialize_with<F>(&self, configure: F) -> Result<(), DotError>
    where
        F: FnOnce(&mut DotConfig),
    {
        self.ensure_directories()?;

        let config_file = self.config_file_path();
        if config_file.exists() {
            return Ok(());
        }

        let mut config = DotConfig::default();
        configure(&mut config);
        self.save_config(&config)
    }

    /// Ensure the folder structure exists and load the configuration,
    /// creating it with the default [`DotConfig`] when missing.
    pub fn load_or_initialize(&self) -> Result<DotConfig, DotError> {
        self.load_or_initialize_with(|_| {})
    }

    /// Ensure the folder structure exists and load the configuration,
    /// creating it with a customized default when missing.
    pub fn load_or_initialize_with<F>(&self, configure: F) -> Result<DotConfig, DotError>
    where
        F: FnOnce(&mut DotConfig),
    {
        self.initialize_with(configure)?;
        self.load_config()
    }

    /// Load the configuration, attempting to recover from corrupted files
    /// using the provided strategy.
    pub fn load_or_recover(
        &self,
        strategy: ConfigRecoveryStrategy,
    ) -> Result<ConfigLoadOutcome, DotError> {
        self.load_or_recover_with(|_| {}, strategy)
    }

    /// Load the configuration, attempting to recover from corrupted files and
    /// allowing callers to customize the default template when a recreation is
    /// required.
    pub fn load_or_recover_with<F>(
        &self,
        configure: F,
        strategy: ConfigRecoveryStrategy,
    ) -> Result<ConfigLoadOutcome, DotError>
    where
        F: FnOnce(&mut DotConfig),
    {
        self.ensure_directories()?;

        let config_file = self.config_file_path();
        let mut configure = Some(configure);

        if !config_file.exists() {
            let mut config = DotConfig::default();
            if let Some(configure_fn) = configure.take() {
                configure_fn(&mut config);
            }
            self.save_config(&config)?;
            return Ok(ConfigLoadOutcome {
                config,
                source: ConfigLoadSource::Initialized,
                quarantined_config: None,
            });
        }

        match self.load_config() {
            Ok(config) => Ok(ConfigLoadOutcome {
                config,
                source: ConfigLoadSource::Existing,
                quarantined_config: None,
            }),
            Err(err) => {
                if matches!(err, DotError::TomlDe(_) | DotError::Io(_)) {
                    self.recover_corrupted_config(err, &mut configure, strategy)
                } else {
                    Err(err)
                }
            }
        }
    }

    fn ensure_directories(&self) -> Result<(), DotError> {
        fs::create_dir_all(&self.root_dir).map_err(DotError::Io)?;

        for subdir in self.layout.directories() {
            let resolved = self.resolve(&subdir);
            fs::create_dir_all(resolved).map_err(DotError::Io)?;
        }

        Ok(())
    }

    /// Load configuration from disk
    pub fn load_config(&self) -> Result<DotConfig, DotError> {
        let config_file = self.config_file_path();
        if !config_file.exists() {
            return Ok(DotConfig::default());
        }

        let content = fs::read_to_string(&config_file).map_err(DotError::Io)?;

        toml::from_str(&content).map_err(DotError::TomlDe)
    }

    /// Save configuration to disk
    pub fn save_config(&self, config: &DotConfig) -> Result<(), DotError> {
        let config_file = self.config_file_path();
        if let Some(parent) = config_file.parent() {
            fs::create_dir_all(parent).map_err(DotError::Io)?;
        }

        let content = toml::to_string_pretty(config).map_err(DotError::Toml)?;

        fs::write(&config_file, content).map_err(DotError::Io)?;

        Ok(())
    }

    /// Update configuration with new values
    pub fn update_config<F>(&self, updater: F) -> Result<(), DotError>
    where
        F: FnOnce(&mut DotConfig),
    {
        let mut config = self.load_config()?;
        updater(&mut config);
        config.last_updated = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        self.save_config(&config)
    }

    /// Get cache directory for a specific type
    pub fn cache_dir(&self, cache_type: &str) -> PathBuf {
        match cache_type {
            "prompts" => self.prompts_cache_dir(),
            "context" => self.context_cache_dir(),
            "models" => self.models_cache_dir(),
            other => self.cache_root_dir().join(other),
        }
    }

    /// Get logs directory
    pub fn logs_dir(&self) -> PathBuf {
        self.resolve(&self.layout.logs_dir)
    }

    /// Get sessions directory
    pub fn sessions_dir(&self) -> PathBuf {
        self.resolve(&self.layout.sessions_dir)
    }

    /// Get backups directory
    pub fn backups_dir(&self) -> PathBuf {
        self.resolve(&self.layout.backups_dir)
    }

    /// Clean up old cache files
    pub fn cleanup_cache(&self) -> Result<CacheCleanupStats, DotError> {
        let config = self.load_config()?;
        let max_age = std::time::Duration::from_secs(config.cache.ttl_days * 24 * 60 * 60);
        let now = std::time::SystemTime::now();

        let mut stats = CacheCleanupStats::default();

        // Clean prompt cache
        if config.cache.prompt_cache_enabled {
            let prompts_dir = self.cache_dir("prompts");
            stats.prompts_cleaned = self.cleanup_directory(&prompts_dir, max_age, now)?;
        }

        // Clean context cache
        if config.cache.context_cache_enabled {
            let context_dir = self.cache_dir("context");
            stats.context_cleaned = self.cleanup_directory(&context_dir, max_age, now)?;
        }

        // Clean model cache
        let models_dir = self.cache_dir("models");
        stats.models_cleaned = self.cleanup_directory(&models_dir, max_age, now)?;

        Ok(stats)
    }

    /// Clean up files in a directory older than max_age
    fn cleanup_directory(
        &self,
        dir: &Path,
        max_age: std::time::Duration,
        now: std::time::SystemTime,
    ) -> Result<u64, DotError> {
        if !dir.exists() {
            return Ok(0);
        }

        let mut cleaned = 0u64;

        for entry in fs::read_dir(dir).map_err(DotError::Io)? {
            let entry = entry.map_err(DotError::Io)?;
            let path = entry.path();

            if let Ok(metadata) = entry.metadata()
                && let Ok(modified) = metadata.modified()
                && let Ok(age) = now.duration_since(modified)
                && age > max_age
            {
                if path.is_file() {
                    fs::remove_file(&path).map_err(DotError::Io)?;
                    cleaned += 1;
                } else if path.is_dir() {
                    fs::remove_dir_all(&path).map_err(DotError::Io)?;
                    cleaned += 1;
                }
            }
        }

        Ok(cleaned)
    }

    fn recover_corrupted_config<F>(
        &self,
        original_error: DotError,
        configure: &mut Option<F>,
        strategy: ConfigRecoveryStrategy,
    ) -> Result<ConfigLoadOutcome, DotError>
    where
        F: FnOnce(&mut DotConfig),
    {
        let quarantined = if strategy.quarantine_corrupted {
            self.quarantine_corrupted_config()?
        } else {
            None
        };

        if strategy.prefer_backups && strategy.max_backup_attempts > 0 {
            let backups = self.list_backups()?;
            for backup_path in backups.into_iter().take(strategy.max_backup_attempts) {
                match Self::read_config_from_path(&backup_path) {
                    Ok(config) => {
                        fs::copy(&backup_path, self.config_file_path()).map_err(DotError::Io)?;
                        return Ok(ConfigLoadOutcome {
                            config,
                            source: ConfigLoadSource::BackupRestored(backup_path),
                            quarantined_config: quarantined,
                        });
                    }
                    Err(_) => continue,
                }
            }
        }

        if strategy.fallback_to_default {
            let mut config = DotConfig::default();
            if let Some(configure_fn) = configure.take() {
                configure_fn(&mut config);
            }
            self.save_config(&config)?;
            return Ok(ConfigLoadOutcome {
                config,
                source: ConfigLoadSource::DefaultRecreated,
                quarantined_config: quarantined,
            });
        }

        Err(DotError::RecoveryFailed(format!(
            "Unable to recover configuration after error: {original_error}"
        )))
    }

    fn read_config_from_path(path: &Path) -> Result<DotConfig, DotError> {
        let content = fs::read_to_string(path).map_err(DotError::Io)?;
        toml::from_str(&content).map_err(DotError::TomlDe)
    }

    fn quarantine_corrupted_config(&self) -> Result<Option<PathBuf>, DotError> {
        let config_file = self.config_file_path();
        if !config_file.exists() {
            return Ok(None);
        }

        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let file_name = config_file
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("config.toml");
        let corrupt_name = format!("{file_name}.corrupt-{timestamp}");

        let destination = config_file
            .parent()
            .map(|parent| parent.join(&corrupt_name))
            .unwrap_or_else(|| PathBuf::from(corrupt_name.clone()));

        fs::rename(&config_file, &destination).map_err(DotError::Io)?;
        Ok(Some(destination))
    }

    /// Get disk usage statistics
    pub fn disk_usage(&self) -> Result<DiskUsageStats, DotError> {
        let mut stats = DiskUsageStats::default();

        stats.config_size = self.calculate_dir_size(&self.root_dir)?;
        stats.cache_size = self.calculate_dir_size(&self.cache_root_dir())?;
        stats.logs_size = self.calculate_dir_size(&self.logs_dir())?;
        stats.sessions_size = self.calculate_dir_size(&self.sessions_dir())?;
        stats.backups_size = self.calculate_dir_size(&self.backups_dir())?;

        stats.total_size = stats.config_size
            + stats.cache_size
            + stats.logs_size
            + stats.sessions_size
            + stats.backups_size;

        Ok(stats)
    }

    /// Calculate directory size recursively
    fn calculate_dir_size(&self, dir: &Path) -> Result<u64, DotError> {
        if !dir.exists() {
            return Ok(0);
        }

        let mut size = 0u64;

        fn calculate_recursive(path: &Path, current_size: &mut u64) -> Result<(), DotError> {
            if path.is_file() {
                if let Ok(metadata) = path.metadata() {
                    *current_size += metadata.len();
                }
            } else if path.is_dir() {
                for entry in fs::read_dir(path).map_err(DotError::Io)? {
                    let entry = entry.map_err(DotError::Io)?;
                    calculate_recursive(&entry.path(), current_size)?;
                }
            }
            Ok(())
        }

        calculate_recursive(dir, &mut size)?;
        Ok(size)
    }

    /// Backup current configuration
    pub fn backup_config(&self) -> Result<PathBuf, DotError> {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let backup_name = format!("config_backup_{}.toml", timestamp);
        let backup_path = self.backups_dir().join(backup_name);

        let config_file = self.config_file_path();
        if config_file.exists() {
            fs::copy(&config_file, &backup_path).map_err(DotError::Io)?;
        }

        Ok(backup_path)
    }

    /// Legacy alias for [`backup_config`] to preserve compatibility with
    /// existing call sites while preparing the crate for extraction.
    pub fn create_backup(&self) -> Result<PathBuf, DotError> {
        self.backup_config()
    }

    /// List available backups
    pub fn list_backups(&self) -> Result<Vec<PathBuf>, DotError> {
        let backups_dir = self.backups_dir();
        if !backups_dir.exists() {
            return Ok(vec![]);
        }

        let mut backups = vec![];

        for entry in fs::read_dir(backups_dir).map_err(DotError::Io)? {
            let entry = entry.map_err(DotError::Io)?;
            if entry.path().extension().and_then(|e| e.to_str()) == Some("toml") {
                backups.push(entry.path());
            }
        }

        // Sort by modification time (newest first)
        backups.sort_by(|a, b| {
            let a_time = a.metadata().and_then(|m| m.modified()).ok();
            let b_time = b.metadata().and_then(|m| m.modified()).ok();
            b_time.cmp(&a_time)
        });

        Ok(backups)
    }

    /// Restore configuration from backup
    pub fn restore_backup(&self, backup_path: &Path) -> Result<(), DotError> {
        if !backup_path.exists() {
            return Err(DotError::BackupNotFound(backup_path.to_path_buf()));
        }

        let config_file = self.config_file_path();
        fs::copy(backup_path, &config_file).map_err(DotError::Io)?;

        Ok(())
    }
}

#[derive(Debug, Default)]
pub struct CacheCleanupStats {
    pub prompts_cleaned: u64,
    pub context_cleaned: u64,
    pub models_cleaned: u64,
}

#[derive(Debug, Default)]
pub struct DiskUsageStats {
    pub config_size: u64,
    pub cache_size: u64,
    pub logs_size: u64,
    pub sessions_size: u64,
    pub backups_size: u64,
    pub total_size: u64,
}

/// Dot folder management errors
#[derive(Debug, thiserror::Error)]
pub enum DotError {
    #[error("Home directory not found")]
    HomeDirNotFound,

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("TOML serialization error: {0}")]
    Toml(#[from] toml::ser::Error),

    #[error("TOML deserialization error: {0}")]
    TomlDe(#[from] toml::de::Error),

    #[error("Backup not found: {0}")]
    BackupNotFound(PathBuf),

    #[error("Invalid dot directory layout: {0}")]
    InvalidLayout(String),

    #[error("Config recovery failed: {0}")]
    RecoveryFailed(String),
}

use std::sync::{LazyLock, Mutex};

/// Global dot manager instance
static DOT_MANAGER: LazyLock<Mutex<DotManager>> =
    LazyLock::new(|| Mutex::new(DotManager::new().unwrap()));

/// Get global dot manager instance
pub fn get_dot_manager() -> &'static Mutex<DotManager> {
    &DOT_MANAGER
}

/// Initialize dot folder (should be called at startup)
pub fn initialize_dot_folder() -> Result<(), DotError> {
    let manager = get_dot_manager().lock().unwrap();
    manager.initialize()
}

/// Load user configuration
pub fn load_user_config() -> Result<DotConfig, DotError> {
    let manager = get_dot_manager().lock().unwrap();
    manager.load_config()
}

/// Save user configuration
pub fn save_user_config(config: &DotConfig) -> Result<(), DotError> {
    let manager = get_dot_manager().lock().unwrap();
    manager.save_config(config)
}

/// Persist the preferred UI theme in the user's dot configuration.
pub fn update_theme_preference(theme: &str) -> Result<(), DotError> {
    let manager = get_dot_manager().lock().unwrap();
    manager.update_config(|cfg| cfg.preferences.theme = theme.to_string())
}

/// Persist the preferred provider and model combination.
pub fn update_model_preference(provider: &str, model: &str) -> Result<(), DotError> {
    let manager = get_dot_manager().lock().unwrap();
    manager.update_config(|cfg| {
        cfg.preferences.default_provider = provider.to_string();
        cfg.preferences.default_model = model.to_string();
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tempfile::TempDir;

    #[test]
    fn test_dot_manager_initialization() {
        let temp_dir = TempDir::new().unwrap();
        let manager = DotManager::with_root_dir(temp_dir.path().join(".vtcode")).unwrap();

        // Test directory creation
        assert!(!manager.root_dir().exists());

        manager.initialize().unwrap();
        assert!(manager.root_dir().exists());
        assert!(manager.cache_root_dir().exists());
        assert!(manager.cache_dir("prompts").exists());
        assert!(manager.logs_dir().exists());
    }

    #[test]
    fn test_config_save_load() {
        let temp_dir = TempDir::new().unwrap();
        let manager = DotManager::with_root_dir(temp_dir.path().join(".vtcode")).unwrap();

        manager.initialize().unwrap();

        let mut config = DotConfig::default();
        config.preferences.default_model = "test-model".to_string();

        manager.save_config(&config).unwrap();
        let loaded_config = manager.load_config().unwrap();

        assert_eq!(loaded_config.preferences.default_model, "test-model");
    }

    #[test]
    fn test_product_name_normalization() {
        let temp_dir = TempDir::new().unwrap();
        let manager = DotManager::with_home_dir(temp_dir.path(), "My App 42!").unwrap();
        assert!(manager.root_dir().ends_with(Path::new(".my-app-42")));
    }

    #[test]
    fn test_custom_layout_paths() {
        let temp_dir = TempDir::new().unwrap();
        let layout = DotDirectoryLayout::new()
            .with_config_file("config/settings.toml")
            .with_cache_root_dir("runtime/cache")
            .with_prompts_cache_dir("runtime/prompt-store")
            .with_context_cache_dir("runtime/context-store")
            .with_models_cache_dir("runtime/model-store")
            .with_logs_dir("var/logs")
            .with_sessions_dir("history/sessions")
            .with_backups_dir("backups/config")
            .with_additional_directory("artifacts");

        let manager =
            DotManager::with_root_dir_and_layout(temp_dir.path().join(".custom"), layout.clone())
                .unwrap();

        manager.initialize().unwrap();

        assert!(
            manager
                .config_file_path()
                .ends_with(Path::new("config/settings.toml"))
        );
        assert!(
            manager
                .cache_root_dir()
                .ends_with(Path::new("runtime/cache"))
        );
        assert!(manager.logs_dir().ends_with(Path::new("var/logs")));
        assert!(
            manager
                .sessions_dir()
                .ends_with(Path::new("history/sessions"))
        );
        assert!(manager.backups_dir().ends_with(Path::new("backups/config")));
        assert!(
            manager
                .cache_dir("prompts")
                .ends_with(Path::new("runtime/prompt-store"))
        );
        assert!(
            manager
                .cache_dir("context")
                .ends_with(Path::new("runtime/context-store"))
        );
        assert!(
            manager
                .cache_dir("models")
                .ends_with(Path::new("runtime/model-store"))
        );

        let artifacts_dir = manager.root_dir().join("artifacts");
        assert!(artifacts_dir.exists());

        let resolved_layout = manager.layout();
        assert_eq!(resolved_layout.config_file, layout.config_file);
    }

    #[test]
    fn test_initialize_with_custom_defaults() {
        let temp_dir = TempDir::new().unwrap();
        let manager = DotManager::with_root_dir(temp_dir.path().join(".product")).unwrap();

        let calls = Arc::new(AtomicUsize::new(0));
        manager
            .initialize_with({
                let calls = Arc::clone(&calls);
                move |config| {
                    calls.fetch_add(1, Ordering::SeqCst);
                    config.preferences.default_model = "custom-model".to_string();
                    config.preferences.default_provider = "custom-provider".to_string();
                }
            })
            .unwrap();

        let config = manager.load_config().unwrap();
        assert_eq!(config.preferences.default_model, "custom-model");
        assert_eq!(config.preferences.default_provider, "custom-provider");
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        manager
            .initialize_with({
                let calls = Arc::clone(&calls);
                move |_| {
                    calls.fetch_add(1, Ordering::SeqCst);
                }
            })
            .unwrap();

        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn test_load_or_initialize_with_returns_config_and_skips_existing() {
        let temp_dir = TempDir::new().unwrap();
        let manager = DotManager::with_root_dir(temp_dir.path().join(".product")).unwrap();

        let calls = Arc::new(AtomicUsize::new(0));
        let config = manager
            .load_or_initialize_with({
                let calls = Arc::clone(&calls);
                move |cfg| {
                    calls.fetch_add(1, Ordering::SeqCst);
                    cfg.preferences.auto_save = false;
                    cfg.ui.auto_complete = false;
                }
            })
            .unwrap();

        assert!(!config.preferences.auto_save);
        assert!(!config.ui.auto_complete);
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        manager
            .update_config(|cfg| {
                cfg.preferences.auto_save = true;
                cfg.ui.auto_complete = true;
            })
            .unwrap();

        let config = manager
            .load_or_initialize_with({
                let calls = Arc::clone(&calls);
                move |_| {
                    calls.fetch_add(1, Ordering::SeqCst);
                }
            })
            .unwrap();

        assert!(config.preferences.auto_save);
        assert!(config.ui.auto_complete);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn test_load_or_recover_uses_backup_when_corrupted() {
        let temp_dir = TempDir::new().unwrap();
        let manager = DotManager::with_root_dir(temp_dir.path().join(".product")).unwrap();

        manager.initialize().unwrap();
        manager
            .update_config(|cfg| {
                cfg.preferences.default_model = "backup-model".to_string();
            })
            .unwrap();

        let backup_path = manager.create_backup().unwrap();
        fs::write(manager.config_file_path(), "not = toml").unwrap();

        let outcome = manager
            .load_or_recover(ConfigRecoveryStrategy::default())
            .unwrap();

        assert_eq!(outcome.config.preferences.default_model, "backup-model");
        assert!(matches!(
            &outcome.source,
            ConfigLoadSource::BackupRestored(restored) if restored == &backup_path
        ));
        let quarantined = outcome
            .quarantined_config
            .expect("corrupted file should be quarantined");
        assert!(quarantined.exists());
        assert!(manager.config_file_path().exists());
    }

    #[test]
    fn test_load_or_recover_recreates_when_no_backup_available() {
        let temp_dir = TempDir::new().unwrap();
        let manager = DotManager::with_root_dir(temp_dir.path().join(".product")).unwrap();

        manager.initialize().unwrap();
        fs::write(manager.config_file_path(), "bad = [").unwrap();

        let outcome = manager
            .load_or_recover_with(
                |cfg| {
                    cfg.preferences.default_model = "fallback-model".to_string();
                },
                ConfigRecoveryStrategy {
                    prefer_backups: false,
                    fallback_to_default: true,
                    max_backup_attempts: 3,
                    quarantine_corrupted: true,
                },
            )
            .unwrap();

        assert_eq!(outcome.config.preferences.default_model, "fallback-model");
        assert!(matches!(outcome.source, ConfigLoadSource::DefaultRecreated));
        assert!(
            outcome
                .quarantined_config
                .expect("expected quarantined file")
                .exists()
        );
    }
}
