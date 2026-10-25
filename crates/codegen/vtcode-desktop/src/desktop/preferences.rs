//! Persisted desktop preferences in `~/.vtcode/desktop/preferences.json`.
//!
//! Everything here is a UI comfort setting: a missing or corrupt file is not
//! an error and falls back to defaults.

use std::path::PathBuf;

use gpui_kit::{App, Global};
use serde::{Deserialize, Serialize};

const MAX_RECENT_WORKSPACES: usize = 8;
const DEFAULT_SIDEBAR_WIDTH: f32 = 270.0;
const DEFAULT_INSPECTOR_WIDTH: f32 = 320.0;
pub(super) const MIN_PANE_WIDTH: f32 = 180.0;
pub(super) const MAX_PANE_WIDTH: f32 = 560.0;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) enum ReducedMotionPreference {
    #[default]
    System,
    On,
    Off,
}

impl ReducedMotionPreference {
    pub(super) const ALL: [Self; 3] = [Self::System, Self::On, Self::Off];

    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::System => "System",
            Self::On => "On",
            Self::Off => "Off",
        }
    }

    pub(super) const fn id(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::On => "on",
            Self::Off => "off",
        }
    }
}

#[derive(Default)]
struct SystemMotionBaseline(Option<bool>);

impl Global for SystemMotionBaseline {}

/// Captures the GPUI Kit system setting before any desktop override is applied.
pub(super) fn system_motion_baseline(cx: &mut App) -> bool {
    let current = cx.reduce_motion();
    *cx.default_global::<SystemMotionBaseline>().0.get_or_insert(current)
}

/// Applies a saved override or returns GPUI to the current OS preference.
pub(super) fn apply_reduced_motion(preference: ReducedMotionPreference, system_baseline: bool, cx: &mut App) {
    match preference {
        ReducedMotionPreference::System => {
            cx.set_reduce_motion(system_baseline);
            gpui_kit::base::apply_system_reduce_motion(cx);
        }
        ReducedMotionPreference::On => cx.set_reduce_motion(true),
        ReducedMotionPreference::Off => cx.set_reduce_motion(false),
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(super) struct Preferences {
    /// The optional diagnostics footer is hidden on a fresh installation.
    pub show_status_bar: bool,
    pub reduced_motion: ReducedMotionPreference,
    pub shortcuts: std::collections::BTreeMap<String, String>,
    pub ui_font_family: Option<String>,
    pub code_font_family: Option<String>,
    pub ui_font_size: Option<u8>,
    pub code_font_size: Option<u8>,
    /// Selected theme id.
    pub theme: Option<String>,
    /// When enabled, the toolbar theme control switches between these saved profiles.
    pub separate_theme_profiles: bool,
    pub light_theme: Option<String>,
    pub dark_theme: Option<String>,
    /// None preserves legacy explicit theme choices; new profiles follow the CLI.
    pub follow_cli_theme: Option<bool>,
    /// Recently opened workspaces, most recent first.
    pub recent_workspaces: Vec<String>,
    /// Last inspector state, retained for compatibility; launch always starts collapsed.
    pub inspector_open: Option<bool>,
    /// Sidebar width in logical pixels.
    pub sidebar_width: Option<f32>,
    /// Inspector width in logical pixels.
    pub inspector_width: Option<f32>,
}

impl Preferences {
    pub(super) fn ui_font_size(&self) -> u8 {
        self.ui_font_size.unwrap_or(16).clamp(12, 24)
    }
    pub(super) fn code_font_size(&self) -> u8 {
        self.code_font_size.unwrap_or(14).clamp(10, 24)
    }

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

    /// Best-effort saves for existing comfort controls.
    pub(super) fn save(&self) {
        drop(self.try_save());
    }

    pub(super) fn try_save(&self) -> Result<(), String> {
        let path = Self::preferences_path().ok_or("Preferences directory is unavailable")?;
        let content = serde_json::to_vec_pretty(self).map_err(|error| error.to_string())?;
        vtcode_commons::VtCodePaths::write_private_file_atomic(path, &content)
            .map_err(|error| format!("Could not save desktop preferences: {error}"))
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
    fn font_families_round_trip_independently_and_legacy_defaults_remain_optional() {
        let old: Preferences = serde_json::from_str("{}").unwrap();
        assert!(old.ui_font_family.is_none() && old.code_font_family.is_none());
        let selected = Preferences {
            ui_font_family: Some("Helvetica Neue".into()),
            code_font_family: Some("Menlo".into()),
            ..old
        };
        let loaded: Preferences = serde_json::from_str(&serde_json::to_string(&selected).unwrap()).unwrap();
        assert_eq!(loaded.ui_font_family.as_deref(), Some("Helvetica Neue"));
        assert_eq!(loaded.code_font_family.as_deref(), Some("Menlo"));
    }

    #[test]
    fn font_sizes_default_clamp_and_round_trip_independently() {
        let old: Preferences = serde_json::from_str("{\"theme\":\"ciapre\"}").unwrap();
        assert_eq!((old.ui_font_size(), old.code_font_size()), (16, 14));
        let settings = Preferences {
            ui_font_size: Some(0),
            code_font_size: Some(255),
            ..old
        };
        assert_eq!((settings.ui_font_size(), settings.code_font_size()), (12, 24));
        let parsed: Preferences = serde_json::from_str(&serde_json::to_string(&settings).unwrap()).unwrap();
        assert_eq!((parsed.ui_font_size(), parsed.code_font_size()), (12, 24));
    }

    #[test]
    fn reduced_motion_defaults_to_system_and_persists_explicit_choices() {
        let defaults: Preferences = serde_json::from_str("{}").unwrap();
        assert_eq!(defaults.reduced_motion, ReducedMotionPreference::System);

        let selected = Preferences {
            reduced_motion: ReducedMotionPreference::On,
            ..defaults
        };
        let loaded: Preferences = serde_json::from_str(&serde_json::to_string(&selected).unwrap()).unwrap();
        assert_eq!(loaded.reduced_motion, ReducedMotionPreference::On);
    }

    #[test]
    fn separate_theme_profiles_default_off_and_round_trip_independently() {
        let defaults: Preferences = serde_json::from_str("{}").unwrap();
        assert!(!defaults.separate_theme_profiles);
        assert!(defaults.light_theme.is_none() && defaults.dark_theme.is_none());

        let selected = Preferences {
            separate_theme_profiles: true,
            light_theme: Some("solarized-light".into()),
            dark_theme: Some("ciapre".into()),
            ..defaults
        };
        let loaded: Preferences = serde_json::from_str(&serde_json::to_string(&selected).unwrap()).unwrap();
        assert!(loaded.separate_theme_profiles);
        assert_eq!(loaded.light_theme.as_deref(), Some("solarized-light"));
        assert_eq!(loaded.dark_theme.as_deref(), Some("ciapre"));
    }

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
