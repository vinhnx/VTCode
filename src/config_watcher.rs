use anyhow::{Context, Result};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use vtcode_core::config::loader::{ConfigManager, VTCodeConfig};

/// Configuration watcher that monitors config files for changes
/// and automatically reloads them when modifications are detected
pub struct ConfigWatcher {
    workspace_path: PathBuf,
    last_load_time: Arc<Mutex<Instant>>,
    current_config: Arc<Mutex<Option<VTCodeConfig>>>,
    watcher: Option<RecommendedWatcher>,
    debounce_duration: Duration,
    last_event_time: Arc<Mutex<Instant>>,
}

impl ConfigWatcher {
    /// Create a new ConfigWatcher for the given workspace
    pub fn new(workspace_path: PathBuf) -> Self {
        Self {
            workspace_path,
            last_load_time: Arc::new(Mutex::new(Instant::now())),
            current_config: Arc::new(Mutex::new(None)),
            watcher: None,
            debounce_duration: Duration::from_millis(500),
            last_event_time: Arc::new(Mutex::new(Instant::now())),
        }
    }

    /// Initialize the file watcher and load initial configuration
    pub async fn initialize(&mut self) -> Result<()> {
        // Load initial configuration
        self.load_config().await?;

        // Set up file watcher
        let workspace_path = self.workspace_path.clone();
        let last_event_time = self.last_event_time.clone();
        let debounce_duration = self.debounce_duration;

        let mut watcher = RecommendedWatcher::new(
            move |res: Result<notify::Event, notify::Error>| {
                if let Ok(event) = res {
                    let now = Instant::now();
                    let mut last_time = last_event_time.blocking_lock();

                    // Debounce rapid file changes
                    if now.duration_since(*last_time) >= debounce_duration {
                        *last_time = now;

                        // Check if the event is relevant to our config files
                        if is_relevant_config_event(&event, &workspace_path) {
                            tracing::debug!("Config file changed: {:?}", event);
                        }
                    }
                }
            },
            notify::Config::default(),
        )?;

        // Watch the workspace directory for config file changes
        let config_paths = get_config_file_paths(&self.workspace_path);
        for path in config_paths {
            if let Some(parent) = path.parent() {
                watcher
                    .watch(parent, RecursiveMode::NonRecursive)
                    .with_context(|| format!("Failed to watch config directory: {:?}", parent))?;
            }
        }

        self.watcher = Some(watcher);
        Ok(())
    }

    /// Load or reload configuration
    pub async fn load_config(&mut self) -> Result<()> {
        let config = ConfigManager::load_from_workspace(&self.workspace_path)
            .ok()
            .map(|manager| manager.config().clone());

        let mut current = self.current_config.lock().await;
        *current = config;

        let mut last_load = self.last_load_time.lock().await;
        *last_load = Instant::now();

        Ok(())
    }

    /// Get the current configuration, reload if changed
    pub async fn get_config(&mut self) -> Option<VTCodeConfig> {
        // Check if we need to reload based on file changes
        if self.should_reload().await
            && let Err(err) = self.load_config().await
        {
            tracing::warn!("Failed to reload config: {}", err);
        }

        self.current_config.lock().await.clone()
    }

    /// Check if configuration should be reloaded based on file changes
    async fn should_reload(&self) -> bool {
        let last_event = self.last_event_time.lock().await;
        let last_load = self.last_load_time.lock().await;

        // Reload if there were recent file events after our last load
        *last_event > *last_load
    }

    /// Get the last load time for debugging
    pub async fn last_load_time(&self) -> Instant {
        *self.last_load_time.lock().await
    }
}

/// Check if a file event is relevant to VT Code configuration
fn is_relevant_config_event(event: &notify::Event, _workspace_path: &Path) -> bool {
    // Look for events on common config file names
    let relevant_files = ["vtcode.toml", ".vtcode.toml", "config.toml", "theme.toml"];
    let relevant_dirs = ["config", "theme"];

    // Check the event kind (note: notify::Event has a single `kind` field, not `kinds`)
    match &event.kind {
        notify::EventKind::Create(_)
        | notify::EventKind::Modify(_)
        | notify::EventKind::Remove(_) => {
            for path in &event.paths {
                if let Some(file_name) = path.file_name()
                    && let Some(file_name_str) = file_name.to_str()
                    && (relevant_files.contains(&file_name_str)
                        || relevant_dirs.contains(&file_name_str))
                {
                    return true;
                }
            }
        }
        _ => {}
    }

    false
}

/// Get all potential config file paths to watch
fn get_config_file_paths(workspace_path: &Path) -> Vec<PathBuf> {
    let mut paths = Vec::new();

    // Core config files
    paths.push(workspace_path.join("vtcode.toml"));
    paths.push(workspace_path.join(".vtcode.toml"));
    paths.push(workspace_path.join(".vtcode").join("theme.toml"));

    // Live-edit directories (workspace-level and dot-folder level)
    paths.push(workspace_path.join("config"));
    paths.push(workspace_path.join("theme"));
    paths.push(workspace_path.join(".vtcode").join("config"));
    paths.push(workspace_path.join(".vtcode").join("theme"));

    // Global config (if applicable)
    if let Some(home_dir) = home::home_dir() {
        paths.push(home_dir.join(".vtcode.toml"));
    }

    paths
}

/// Simple config watcher that doesn't use file system events
/// Useful for environments where file watching isn't available
pub struct SimpleConfigWatcher {
    workspace_path: PathBuf,
    last_load_time: Instant,
    last_check_time: Instant,
    check_interval: Duration,
    last_modified_times: HashMap<PathBuf, std::time::SystemTime>,
    debounce_duration: Duration,
    last_reload_attempt: Option<Instant>,
}

impl SimpleConfigWatcher {
    pub fn new(workspace_path: PathBuf) -> Self {
        Self {
            workspace_path,
            last_load_time: Instant::now(),
            last_check_time: Instant::now(),
            check_interval: Duration::from_secs(10), // Reduced frequency from 5s to 10s
            last_modified_times: HashMap::new(),
            debounce_duration: Duration::from_millis(1000), // 1 second debounce
            last_reload_attempt: None,
        }
    }

    pub fn should_reload(&mut self) -> bool {
        let now = Instant::now();

        // Only check periodically
        if now.duration_since(self.last_check_time) >= self.check_interval {
            self.last_check_time = now;

            // Check all config/theme targets.
            let watch_targets = get_config_file_paths(&self.workspace_path);
            let mut saw_change = false;

            for target in &watch_targets {
                // Check if path exists.
                if !target.exists() {
                    continue;
                }

                if let Some(current_modified) = latest_modified(target) {
                    let previous = self.last_modified_times.get(target).copied();
                    self.last_modified_times
                        .insert(target.clone(), current_modified);
                    if let Some(last_modified) = previous
                        && current_modified > last_modified
                    {
                        saw_change = true;
                    }
                }
            }

            if saw_change {
                // Respect debounce period for burst edits while typing/saving.
                if let Some(last_attempt) = self.last_reload_attempt
                    && now.duration_since(last_attempt) < self.debounce_duration
                {
                    return false;
                }
                self.last_reload_attempt = Some(now);
                return true;
            }
        }

        false
    }

    pub fn load_config(&mut self) -> Option<VTCodeConfig> {
        let config = ConfigManager::load_from_workspace(&self.workspace_path)
            .ok()
            .map(|manager| manager.config().clone());

        self.last_load_time = Instant::now();

        // Refresh modified-time baseline after successful load.
        self.last_modified_times.clear();
        for target in get_config_file_paths(&self.workspace_path) {
            if let Some(modified) = latest_modified(&target) {
                self.last_modified_times.insert(target, modified);
            }
        }

        config
    }

    /// Set custom check interval (in seconds)
    pub fn set_check_interval(&mut self, seconds: u64) {
        self.check_interval = Duration::from_secs(seconds);
    }

    /// Set custom debounce duration (in milliseconds)
    pub fn set_debounce_duration(&mut self, millis: u64) {
        self.debounce_duration = Duration::from_millis(millis);
    }
}

fn latest_modified(path: &Path) -> Option<std::time::SystemTime> {
    if path.is_file() {
        return std::fs::metadata(path).ok()?.modified().ok();
    }

    if !path.is_dir() {
        return None;
    }

    let mut newest = None;
    for entry in walkdir::WalkDir::new(path)
        .into_iter()
        .filter_map(|item| item.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        let Ok(modified) = metadata.modified() else {
            continue;
        };
        newest = match newest {
            Some(current) if modified <= current => Some(current),
            _ => Some(modified),
        };
    }
    newest
}
