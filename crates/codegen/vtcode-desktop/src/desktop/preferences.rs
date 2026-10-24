//! Persisted desktop preferences in `~/.vtcode/desktop/preferences.json`.
//!
//! Everything here is a UI comfort setting: a missing or corrupt file is not
//! an error and falls back to defaults.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

const MAX_RECENT_WORKSPACES: usize = 8;
const DEFAULT_SIDEBAR_WIDTH: f32 = 270.0;
const DEFAULT_INSPECTOR_WIDTH: f32 = 320.0;
pub(super) const MIN_PANE_WIDTH: f32 = 180.0;
pub(super) const MAX_PANE_WIDTH: f32 = 560.0;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(super) struct Preferences {
    /// Selected theme id.
    pub theme: Option<String>,
    /// Recently opened workspaces, most recent first.
    pub recent_workspaces: Vec<String>,
    /// Whether the context inspector was open.
    pub inspector_open: Option<bool>,
    /// Sidebar width in logical pixels.
    pub sidebar_width: Option<f32>,
    /// Inspector width in logical pixels.
    pub inspector_width: Option<f32>,
}

impl Preferences {
    pub(super) fn sidebar_width(&self) -> f32 {
        clamp_width(self.sidebar_width.unwrap_or(DEFAULT_SIDEBAR_WIDTH))
    }

    pub(super) fn inspector_width(&self) -> f32 {
        clamp_width(self.inspector_width.unwrap_or(DEFAULT_INSPECTOR_WIDTH))
    }

    /// Record a workspace as most-recently-used.
    pub(super) fn touch_workspace(&mut self, path: &str) {
        self.recent_workspaces.retain(|existing| existing != path);
        self.recent_workspaces.insert(0, path.to_string());
        self.recent_workspaces.truncate(MAX_RECENT_WORKSPACES);
    }

    /// The saved pane widths, applied without clamping surprises.
    pub(super) fn set_widths(&mut self, sidebar: f32, inspector: f32) {
        self.sidebar_width = Some(clamp_width(sidebar));
        self.inspector_width = Some(clamp_width(inspector));
    }

    fn preferences_path() -> Option<PathBuf> {
        // Test hook: keep GUI tests from touching real user preferences.
        #[cfg(test)]
        if let Some(path) = TEST_PREFERENCES_PATH.lock().ok().and_then(|guard| guard.clone()) {
            return Some(path);
        }
        dirs::home_dir().map(|home| home.join(".vtcode").join("desktop").join("preferences.json"))
    }

    /// Load preferences, returning defaults for a missing or corrupt file.
    pub(super) fn load() -> Self {
        let Some(path) = Self::preferences_path() else {
            return Self::default();
        };
        std::fs::read_to_string(path)
            .ok()
            .and_then(|content| serde_json::from_str(&content).ok())
            .unwrap_or_default()
    }

    /// Persist preferences; failures are swallowed (comfort settings only).
    pub(super) fn save(&self) {
        let Some(path) = Self::preferences_path() else {
            return;
        };
        if let Some(parent) = path.parent()
            && std::fs::create_dir_all(parent).is_ok()
            && let Ok(content) = serde_json::to_string_pretty(self)
        {
            drop(std::fs::write(path, content));
        }
    }
}

fn clamp_width(width: f32) -> f32 {
    width.clamp(MIN_PANE_WIDTH, MAX_PANE_WIDTH)
}

#[cfg(test)]
static TEST_PREFERENCES_PATH: std::sync::Mutex<Option<PathBuf>> = std::sync::Mutex::new(None);

/// Redirect preference I/O for GUI tests so they never touch the real user
/// home. Returns the sentinel path.
#[cfg(test)]
pub(super) fn install_test_preferences_path() -> PathBuf {
    let mut guard = TEST_PREFERENCES_PATH.lock().expect("test prefs lock");
    guard.clone().unwrap_or_else(|| {
        let path = std::env::temp_dir().join(format!("vtcode-desktop-test-prefs-{}.json", std::process::id()));
        *guard = Some(path.clone());
        path
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn touch_workspace_moves_to_front_and_bounds_the_list() {
        let mut preferences = Preferences::default();
        for index in 0..(MAX_RECENT_WORKSPACES + 4) {
            preferences.touch_workspace(&format!("/ws-{index}"));
        }
        assert_eq!(preferences.recent_workspaces.len(), MAX_RECENT_WORKSPACES);
        assert_eq!(preferences.recent_workspaces[0], format!("/ws-{}", MAX_RECENT_WORKSPACES + 3));

        preferences.touch_workspace("/ws-5");
        assert_eq!(preferences.recent_workspaces[0], "/ws-5");
        assert_eq!(
            preferences.recent_workspaces.iter().filter(|path| **path == "/ws-5").count(),
            1,
            "re-touching moves the entry instead of duplicating it"
        );
    }

    #[test]
    fn widths_are_clamped_to_usable_bounds() {
        let mut preferences = Preferences::default();
        assert!((preferences.sidebar_width() - DEFAULT_SIDEBAR_WIDTH).abs() < f32::EPSILON);
        assert!((preferences.inspector_width() - DEFAULT_INSPECTOR_WIDTH).abs() < f32::EPSILON);

        preferences.set_widths(40.0, 2_000.0);
        assert!((preferences.sidebar_width() - MIN_PANE_WIDTH).abs() < f32::EPSILON);
        assert!((preferences.inspector_width() - MAX_PANE_WIDTH).abs() < f32::EPSILON);
    }

    #[test]
    fn round_trips_through_json() {
        let mut preferences = Preferences {
            theme: Some("ciapre".to_string()),
            inspector_open: Some(false),
            ..Preferences::default()
        };
        preferences.touch_workspace("/tmp/ws");
        preferences.set_widths(240.0, 380.0);

        let content = serde_json::to_string(&preferences).expect("serialize");
        let parsed: Preferences = serde_json::from_str(&content).expect("deserialize");
        assert_eq!(parsed.theme.as_deref(), Some("ciapre"));
        assert_eq!(parsed.recent_workspaces, vec!["/tmp/ws"]);
        assert_eq!(parsed.inspector_open, Some(false));
        assert!((parsed.sidebar_width() - 240.0).abs() < f32::EPSILON);
        assert!((parsed.inspector_width() - 380.0).abs() < f32::EPSILON);

        let empty: Preferences = serde_json::from_str("{}").expect("defaults");
        assert_eq!(empty.theme, None);
        assert!(empty.recent_workspaces.is_empty());
    }
}
