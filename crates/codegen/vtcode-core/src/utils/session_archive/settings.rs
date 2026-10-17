use std::env;
#[cfg(test)]
use std::path::Path;
use std::sync::{Mutex, OnceLock};

use crate::config::{HistoryPersistence, VTCodeConfig};

pub const SESSION_DIR_ENV: &str = "VT_SESSION_DIR";
pub const SESSION_MAX_FILES_ENV: &str = "VT_SESSION_MAX_FILES";
pub const SESSION_MAX_AGE_DAYS_ENV: &str = "VT_SESSION_MAX_AGE_DAYS";
pub const SESSION_MAX_SIZE_MB_ENV: &str = "VT_SESSION_MAX_SIZE_MB";

#[derive(Debug, Clone, Copy)]
pub(crate) struct SessionHistorySettings {
    pub(crate) persistence: HistoryPersistence,
    pub(crate) max_bytes: Option<usize>,
}

impl Default for SessionHistorySettings {
    fn default() -> Self {
        Self {
            persistence: HistoryPersistence::File,
            max_bytes: None,
        }
    }
}

static SESSION_HISTORY_SETTINGS: OnceLock<Mutex<SessionHistorySettings>> = OnceLock::new();

pub(crate) fn session_history_settings() -> SessionHistorySettings {
    SESSION_HISTORY_SETTINGS
        .get()
        .map(|settings| *settings.lock().unwrap_or_else(|e| e.into_inner()))
        .unwrap_or_default()
}

pub fn apply_session_history_config_from_vtcode(config: &VTCodeConfig) {
    let settings = SessionHistorySettings {
        persistence: config.history.persistence,
        max_bytes: config.history.max_bytes,
    };
    let cell = SESSION_HISTORY_SETTINGS.get_or_init(|| Mutex::new(SessionHistorySettings::default()));
    if let Ok(mut guard) = cell.lock() {
        *guard = settings;
    }
}

pub fn history_persistence_enabled() -> bool {
    matches!(session_history_settings().persistence, HistoryPersistence::File)
}

pub(crate) fn read_env_var_os(key: &str) -> Option<std::ffi::OsString> {
    #[cfg(test)]
    if let Some(override_value) = test_env_overrides::get(key) {
        return override_value;
    }

    env::var_os(key)
}

pub(crate) fn read_env_var(key: &str) -> Option<String> {
    #[cfg(test)]
    if let Some(override_value) = test_env_overrides::get(key) {
        return override_value.map(|value| value.to_string_lossy().to_string());
    }

    env::var(key).ok()
}

#[cfg(test)]
pub(crate) fn set_test_env_override_path(key: &str, value: &Path) {
    test_env_overrides::set(key, Some(value.as_os_str().to_os_string()));
}

#[cfg(test)]
pub(crate) fn clear_test_env_override(key: &str) {
    test_env_overrides::clear(key);
}

#[cfg(test)]
pub(crate) fn override_sessions_dir_for_tests(path: &Path) {
    set_test_env_override_path(SESSION_DIR_ENV, path);
}

#[cfg(test)]
pub(crate) fn clear_sessions_dir_override_for_tests() {
    clear_test_env_override(SESSION_DIR_ENV);
}

#[cfg(test)]
mod test_env_overrides {
    use hashbrown::HashMap;
    use std::ffi::OsString;
    use std::sync::{LazyLock, Mutex};

    static OVERRIDES: LazyLock<Mutex<HashMap<String, Option<OsString>>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

    pub(super) fn get(key: &str) -> Option<Option<OsString>> {
        OVERRIDES.lock().ok().and_then(|map| map.get(key).cloned())
    }

    pub(super) fn set(key: &str, value: Option<OsString>) {
        if let Ok(mut map) = OVERRIDES.lock() {
            map.insert(key.to_owned(), value);
        }
    }

    pub(super) fn clear(key: &str) {
        if let Ok(mut map) = OVERRIDES.lock() {
            map.remove(key);
        }
    }
}
