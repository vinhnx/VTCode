use anyhow::{Context, Result, anyhow};
use hashbrown::HashMap;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use super::{ConfigManager, VTCodeConfig};

/// Configuration watcher that monitors config files for changes
/// and automatically reloads them when modifications are detected.
pub struct ConfigWatcher {
    workspace_path: PathBuf,
    last_load_time: Arc<Mutex<Instant>>,
    current_config: Arc<Mutex<Option<VTCodeConfig>>>,
    watcher: Option<RecommendedWatcher>,
    debounce_duration: Duration,
    last_event_time: Arc<Mutex<Instant>>,
}

impl ConfigWatcher {
    /// Create a new ConfigWatcher for the given workspace.
    #[must_use]
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

    /// Initialize the file watcher and load initial configuration.
    ///
    /// # Errors
    ///
    /// Returns an error when the initial config load fails or when the watcher
    /// cannot subscribe to config parent directories.
    pub async fn initialize(&mut self) -> Result<()> {
        self.load_config().await?;

        let workspace_path = self.workspace_path.clone();
        let last_event_time = Arc::clone(&self.last_event_time);
        let debounce_duration = self.debounce_duration;

        let mut watcher = RecommendedWatcher::new(
            move |res: Result<notify::Event, notify::Error>| {
                if let Ok(event) = res {
                    let now = Instant::now();
                    if let Ok(mut last_time) = last_event_time.lock()
                        && now.duration_since(*last_time) >= debounce_duration
                    {
                        *last_time = now;
                        if is_relevant_config_event(&event, &workspace_path) {
                            tracing::debug!("Config file changed: {:?}", event);
                        }
                    }
                }
            },
            notify::Config::default(),
        )?;

        for path in get_config_file_paths(&self.workspace_path) {
            if let Some(parent) = path.parent() {
                watcher
                    .watch(parent, RecursiveMode::NonRecursive)
                    .with_context(|| format!("Failed to watch config directory: {:?}", parent))?;
            }
        }

        self.watcher = Some(watcher);
        Ok(())
    }

    /// Load or reload configuration.
    ///
    /// # Errors
    ///
    /// Returns an error when internal watcher state cannot be updated.
    pub async fn load_config(&mut self) -> Result<()> {
        let config = ConfigManager::load_from_workspace(&self.workspace_path)
            .ok()
            .map(|manager| manager.config().clone());

        let mut current = self
            .current_config
            .lock()
            .map_err(|_| anyhow!("config watcher state lock poisoned"))?;
        *current = config;
        drop(current);

        let mut last_load = self
            .last_load_time
            .lock()
            .map_err(|_| anyhow!("config watcher timestamp lock poisoned"))?;
        *last_load = Instant::now();

        Ok(())
    }

    /// Get the current configuration, reloading if the watcher detected changes.
    pub async fn get_config(&mut self) -> Option<VTCodeConfig> {
        if self.should_reload().await
            && let Err(err) = self.load_config().await
        {
            tracing::warn!("Failed to reload config: {}", err);
        }

        self.current_config
            .lock()
            .ok()
            .and_then(|current| current.clone())
    }

    async fn should_reload(&self) -> bool {
        let Ok(last_event) = self.last_event_time.lock() else {
            return false;
        };
        let Ok(last_load) = self.last_load_time.lock() else {
            return false;
        };

        *last_event > *last_load
    }

    /// Get the last load time for debugging.
    #[must_use]
    pub async fn last_load_time(&self) -> Instant {
        self.last_load_time
            .lock()
            .map(|instant| *instant)
            .unwrap_or_else(|_| Instant::now())
    }
}

/// Simple config watcher that polls file mtimes instead of using filesystem events.
pub struct SimpleConfigWatcher {
    workspace_path: PathBuf,
    last_load_time: Instant,
    last_check_time: Instant,
    check_interval: Duration,
    last_modified_times: HashMap<PathBuf, SystemTime>,
    debounce_duration: Duration,
    last_reload_attempt: Option<Instant>,
}

impl SimpleConfigWatcher {
    #[must_use]
    pub fn new(workspace_path: PathBuf) -> Self {
        Self {
            workspace_path,
            last_load_time: Instant::now(),
            last_check_time: Instant::now(),
            check_interval: Duration::from_secs(10),
            last_modified_times: HashMap::new(),
            debounce_duration: Duration::from_millis(1000),
            last_reload_attempt: None,
        }
    }

    pub fn should_reload(&mut self) -> bool {
        let now = Instant::now();

        if now.duration_since(self.last_check_time) < self.check_interval {
            return false;
        }
        self.last_check_time = now;

        let mut saw_change = false;
        for target in get_config_file_paths(&self.workspace_path) {
            if !target.exists() {
                continue;
            }

            if let Some(current_modified) = latest_modified(&target) {
                let previous = self.last_modified_times.get(&target).copied();
                self.last_modified_times.insert(target, current_modified);
                if let Some(last_modified) = previous
                    && current_modified > last_modified
                {
                    saw_change = true;
                }
            }
        }

        if !saw_change {
            return false;
        }

        if let Some(last_attempt) = self.last_reload_attempt
            && now.duration_since(last_attempt) < self.debounce_duration
        {
            return false;
        }
        self.last_reload_attempt = Some(now);
        true
    }

    pub fn load_config(&mut self) -> Option<VTCodeConfig> {
        let config = ConfigManager::load_from_workspace(&self.workspace_path)
            .ok()
            .map(|manager| manager.config().clone());

        self.last_load_time = Instant::now();
        self.last_modified_times.clear();
        for target in get_config_file_paths(&self.workspace_path) {
            if let Some(modified) = latest_modified(&target) {
                self.last_modified_times.insert(target, modified);
            }
        }

        config
    }

    pub fn set_check_interval(&mut self, seconds: u64) {
        self.check_interval = Duration::from_secs(seconds);
    }

    pub fn set_debounce_duration(&mut self, millis: u64) {
        self.debounce_duration = Duration::from_millis(millis);
    }
}

fn is_relevant_config_event(event: &notify::Event, _workspace_path: &Path) -> bool {
    let relevant_files = ["vtcode.toml", ".vtcode.toml", "config.toml", "theme.toml"];
    let relevant_dirs = ["config", "theme"];

    match &event.kind {
        notify::EventKind::Create(_)
        | notify::EventKind::Modify(_)
        | notify::EventKind::Remove(_) => event.paths.iter().any(|path| {
            path.file_name()
                .and_then(|file_name| file_name.to_str())
                .is_some_and(|file_name| {
                    relevant_files.contains(&file_name) || relevant_dirs.contains(&file_name)
                })
        }),
        _ => false,
    }
}

fn get_config_file_paths(workspace_path: &Path) -> Vec<PathBuf> {
    let mut paths = vec![
        workspace_path.join("vtcode.toml"),
        workspace_path.join(".vtcode.toml"),
        workspace_path.join(".vtcode").join("theme.toml"),
        workspace_path.join("config"),
        workspace_path.join("theme"),
        workspace_path.join(".vtcode").join("config"),
        workspace_path.join(".vtcode").join("theme"),
    ];

    if let Some(home_dir) = dirs::home_dir() {
        paths.push(home_dir.join(".vtcode.toml"));
    }

    paths
}

fn latest_modified(path: &Path) -> Option<SystemTime> {
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
