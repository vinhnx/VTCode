//! Persisted desktop preferences in `~/.vtcode/desktop/preferences.json`.
//!
//! A missing or corrupt file falls back to defaults. Permission choices are
//! explicit per-workspace overrides of the CLI confirmation flag.

use std::path::PathBuf;

use gpui_kit::component::theme::ThemeMode;
use gpui_kit::{App, Global};
use serde::{Deserialize, Serialize};

use super::localization::UiLanguage;

const MAX_RECENT_WORKSPACES: usize = 8;
const DEFAULT_SIDEBAR_WIDTH: f32 = 270.0;
const DEFAULT_INSPECTOR_WIDTH: f32 = 320.0;
pub(super) const MIN_PANE_WIDTH: f32 = 180.0;
pub(super) const MAX_PANE_WIDTH: f32 = 560.0;
/// Smallest UI base size: keeps the tiniest labels (`text_xs` = 0.75rem) at
/// 10.5px, the smallest functional size on macOS.
pub(super) const MIN_UI_FONT_SIZE: u8 = 14;
pub(super) const MAX_UI_FONT_SIZE: u8 = 24;
/// Smallest mono size for diffs, code blocks, and terminal output.
pub(super) const MIN_CODE_FONT_SIZE: u8 = 11;
pub(super) const MAX_CODE_FONT_SIZE: u8 = 24;

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

#[derive(Clone, Copy)]
struct SystemMotionBaseline(bool);

impl Global for SystemMotionBaseline {}

/// Captures the GPUI Kit system setting before any desktop override is applied.
pub(super) fn system_motion_baseline(cx: &mut App) -> bool {
    if let Some(baseline) = cx.try_global::<SystemMotionBaseline>() {
        return baseline.0;
    }

    let current = cx.reduce_motion();
    cx.set_global(SystemMotionBaseline(current));
    current
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
pub(super) struct TypographyProfile {
    pub ui_font_family: Option<String>,
    pub code_font_family: Option<String>,
    pub ui_font_size: Option<u8>,
    pub code_font_size: Option<u8>,
}

#[derive(Clone, Copy)]
pub(super) struct TypographySettings<'a> {
    pub ui_font_family: Option<&'a str>,
    pub code_font_family: Option<&'a str>,
    pub ui_font_size: u8,
    pub code_font_size: u8,
}

impl TypographySettings<'_> {
    fn to_profile(self) -> TypographyProfile {
        TypographyProfile {
            ui_font_family: self.ui_font_family.map(str::to_owned),
            code_font_family: self.code_font_family.map(str::to_owned),
            ui_font_size: Some(self.ui_font_size),
            code_font_size: Some(self.code_font_size),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(super) struct Preferences {
    /// Selected desktop UI language. Unknown values fall back to English.
    pub ui_language: Option<String>,
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
    /// Keep independent theme and typography choices for light and dark mode.
    pub separate_theme_profiles: bool,
    pub light_theme: Option<String>,
    pub dark_theme: Option<String>,
    pub light_typography: Option<TypographyProfile>,
    pub dark_typography: Option<TypographyProfile>,
    /// None preserves legacy explicit theme choices; new profiles follow the CLI.
    pub follow_cli_theme: Option<bool>,
    /// Recently opened workspaces, most recent first.
    pub recent_workspaces: Vec<String>,
    /// Explicit confirmation choices, retained alongside recent workspace paths.
    pub workspace_permissions: std::collections::BTreeMap<String, vtcode_desktop_protocol::PermissionMode>,
    /// Last inspector state, retained for compatibility; launch always starts collapsed.
    pub inspector_open: Option<bool>,
    /// Place the project and thread navigation on the right side of the workspace.
    /// Missing values keep existing installs on the new right-side default.
    pub navigation_on_right: Option<bool>,
    /// Show the project and thread navigation panel. Its actions remain in the header when hidden.
    pub navigation_visible: Option<bool>,
    /// Sidebar width in logical pixels.
    pub sidebar_width: Option<f32>,
    /// Inspector width in logical pixels.
    pub inspector_width: Option<f32>,
}

impl Preferences {
    pub(super) fn ui_language(&self) -> UiLanguage {
        UiLanguage::from_saved(self.ui_language.as_deref())
    }

    pub(super) fn set_ui_language(&mut self, language: UiLanguage) {
        self.ui_language = Some(language.locale().to_owned());
    }

    pub(super) fn ui_font_size(&self) -> u8 {
        self.ui_font_size.unwrap_or(16).clamp(MIN_UI_FONT_SIZE, MAX_UI_FONT_SIZE)
    }
    pub(super) fn code_font_size(&self) -> u8 {
        self.code_font_size.unwrap_or(14).clamp(MIN_CODE_FONT_SIZE, MAX_CODE_FONT_SIZE)
    }

    pub(super) fn navigation_on_right(&self) -> bool {
        self.navigation_on_right.unwrap_or(true)
    }

    pub(super) fn navigation_visible(&self) -> bool {
        self.navigation_visible.unwrap_or(true)
    }

    pub(super) fn typography_for(&self, mode: ThemeMode) -> TypographySettings<'_> {
        let profile = if self.separate_theme_profiles {
            match mode {
                ThemeMode::Light => self.light_typography.as_ref(),
                ThemeMode::Dark => self.dark_typography.as_ref(),
            }
        } else {
            None
        };
        match profile {
            Some(profile) => TypographySettings {
                ui_font_family: profile.ui_font_family.as_deref(),
                code_font_family: profile.code_font_family.as_deref(),
                ui_font_size: profile.ui_font_size.unwrap_or(16).clamp(MIN_UI_FONT_SIZE, MAX_UI_FONT_SIZE),
                code_font_size: profile
                    .code_font_size
                    .unwrap_or(14)
                    .clamp(MIN_CODE_FONT_SIZE, MAX_CODE_FONT_SIZE),
            },
            None => TypographySettings {
                ui_font_family: self.ui_font_family.as_deref(),
                code_font_family: self.code_font_family.as_deref(),
                ui_font_size: self.ui_font_size(),
                code_font_size: self.code_font_size(),
            },
        }
    }

    pub(super) fn set_separate_theme_profiles(&mut self, enabled: bool, mode: ThemeMode) {
        if enabled == self.separate_theme_profiles {
            return;
        }
        if enabled {
            let shared = TypographyProfile {
                ui_font_family: self.ui_font_family.clone(),
                code_font_family: self.code_font_family.clone(),
                ui_font_size: self.ui_font_size,
                code_font_size: self.code_font_size,
            };
            self.light_typography = Some(shared.clone());
            self.dark_typography = Some(shared);
        } else {
            let shared = self.typography_for(mode).to_profile();
            self.ui_font_family = shared.ui_font_family;
            self.code_font_family = shared.code_font_family;
            self.ui_font_size = shared.ui_font_size;
            self.code_font_size = shared.code_font_size;
            self.light_typography = None;
            self.dark_typography = None;
        }
        self.separate_theme_profiles = enabled;
    }

    pub(super) fn set_ui_font_family(&mut self, mode: ThemeMode, family: Option<String>) {
        if self.separate_theme_profiles {
            self.typography_profile_mut(mode).ui_font_family = family;
        } else {
            self.ui_font_family = family;
        }
    }

    pub(super) fn set_code_font_family(&mut self, mode: ThemeMode, family: Option<String>) {
        if self.separate_theme_profiles {
            self.typography_profile_mut(mode).code_font_family = family;
        } else {
            self.code_font_family = family;
        }
    }

    pub(super) fn set_ui_font_size(&mut self, mode: ThemeMode, size: u8) {
        if self.separate_theme_profiles {
            self.typography_profile_mut(mode).ui_font_size = Some(size);
        } else {
            self.ui_font_size = Some(size);
        }
    }

    pub(super) fn set_code_font_size(&mut self, mode: ThemeMode, size: u8) {
        if self.separate_theme_profiles {
            self.typography_profile_mut(mode).code_font_size = Some(size);
        } else {
            self.code_font_size = Some(size);
        }
    }

    fn typography_profile_mut(&mut self, mode: ThemeMode) -> &mut TypographyProfile {
        let shared = TypographyProfile {
            ui_font_family: self.ui_font_family.clone(),
            code_font_family: self.code_font_family.clone(),
            ui_font_size: self.ui_font_size,
            code_font_size: self.code_font_size,
        };
        match mode {
            ThemeMode::Light => self.light_typography.get_or_insert(shared),
            ThemeMode::Dark => self.dark_typography.get_or_insert(shared),
        }
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
        self.workspace_permissions
            .retain(|path, _| self.recent_workspaces.contains(path));
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
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("vtcode-desktop-test-prefs-{}-{nonce}.json", std::process::id()));
        *guard = Some(path.clone());
        path
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ui_language_round_trips_and_invalid_saved_values_preserve_other_preferences() {
        let old: Preferences = serde_json::from_str(r#"{"theme":"ciapre"}"#).unwrap();
        assert_eq!(old.ui_language(), UiLanguage::English);

        let mut preferences = old;
        preferences.set_ui_language(UiLanguage::Vietnamese);
        let serialized = serde_json::to_string(&preferences).unwrap();
        let loaded: Preferences = serde_json::from_str(&serialized).unwrap();
        assert_eq!(loaded.ui_language(), UiLanguage::Vietnamese);
        assert_eq!(loaded.theme.as_deref(), Some("ciapre"));

        for language in UiLanguage::ALL {
            preferences.set_ui_language(language);
            let serialized = serde_json::to_string(&preferences).unwrap();
            let loaded: Preferences = serde_json::from_str(&serialized).unwrap();
            assert_eq!(loaded.ui_language(), language);
        }

        let invalid: Preferences = serde_json::from_str(r#"{"theme":"ciapre","uiLanguage":"unsupported"}"#).unwrap();
        assert_eq!(invalid.ui_language(), UiLanguage::English);
        assert_eq!(invalid.theme.as_deref(), Some("ciapre"));
    }

    #[gpui_kit::test]
    fn system_motion_baseline_initializes_once_without_redundant_global_notifications(
        cx: &mut gpui_kit::TestAppContext,
    ) {
        let notifications = std::rc::Rc::new(std::cell::Cell::new(0));
        let notifications_for_observer = notifications.clone();
        let observer = cx.update(|cx| {
            cx.observe_global::<SystemMotionBaseline>(move |_| {
                notifications_for_observer.set(notifications_for_observer.get() + 1);
            })
        });

        cx.update(|cx| {
            cx.set_reduce_motion(true);
            assert!(system_motion_baseline(cx));
        });
        assert_eq!(notifications.get(), 1);

        cx.update(|cx| {
            cx.set_reduce_motion(false);
            assert!(system_motion_baseline(cx), "the original system setting remains authoritative");
        });
        assert_eq!(notifications.get(), 1, "reading the existing baseline does not notify observers");
        drop(observer);
    }

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
        assert_eq!((settings.ui_font_size(), settings.code_font_size()), (14, 24));
        let parsed: Preferences = serde_json::from_str(&serde_json::to_string(&settings).unwrap()).unwrap();
        assert_eq!((parsed.ui_font_size(), parsed.code_font_size()), (14, 24));
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
        assert!(defaults.light_typography.is_none() && defaults.dark_typography.is_none());

        let selected = Preferences {
            separate_theme_profiles: true,
            light_theme: Some("solarized-light".into()),
            dark_theme: Some("ciapre".into()),
            light_typography: Some(TypographyProfile {
                ui_font_size: Some(19),
                ..TypographyProfile::default()
            }),
            dark_typography: Some(TypographyProfile {
                ui_font_size: Some(16),
                ..TypographyProfile::default()
            }),
            ..defaults
        };
        let loaded: Preferences = serde_json::from_str(&serde_json::to_string(&selected).unwrap()).unwrap();
        assert!(loaded.separate_theme_profiles);
        assert_eq!(loaded.light_theme.as_deref(), Some("solarized-light"));
        assert_eq!(loaded.dark_theme.as_deref(), Some("ciapre"));
        assert_eq!(loaded.typography_for(ThemeMode::Light).ui_font_size, 19);
        assert_eq!(loaded.typography_for(ThemeMode::Dark).ui_font_size, 16);
    }

    #[test]
    fn theme_profiles_keep_typography_independent_and_merge_active_profile_on_disable() {
        let mut preferences = Preferences {
            ui_font_family: Some("Helvetica Neue".into()),
            code_font_family: Some("Monaco".into()),
            ui_font_size: Some(16),
            code_font_size: Some(14),
            ..Preferences::default()
        };
        preferences.set_separate_theme_profiles(true, ThemeMode::Dark);
        preferences.set_ui_font_size(ThemeMode::Light, 19);
        preferences.set_code_font_family(ThemeMode::Dark, Some("Menlo".into()));
        preferences.set_ui_font_family(ThemeMode::Light, None);

        let light = preferences.typography_for(ThemeMode::Light);
        let dark = preferences.typography_for(ThemeMode::Dark);
        assert_eq!(light.ui_font_size, 19);
        assert_eq!(dark.ui_font_size, 16);
        assert_eq!(light.ui_font_family, None);
        assert_eq!(dark.ui_font_family, Some("Helvetica Neue"));
        assert_eq!(light.code_font_family, Some("Monaco"));
        assert_eq!(dark.code_font_family, Some("Menlo"));

        preferences.set_separate_theme_profiles(false, ThemeMode::Light);
        assert!(!preferences.separate_theme_profiles);
        assert_eq!(preferences.ui_font_size(), 19);
        assert_eq!(preferences.ui_font_family, None);
        assert_eq!(preferences.code_font_family.as_deref(), Some("Monaco"));
        assert!(preferences.light_typography.is_none() && preferences.dark_typography.is_none());
    }

    #[test]
    fn profile_sizes_are_clamped_and_legacy_shared_typography_is_inherited() {
        let mut preferences: Preferences =
            serde_json::from_str(r#"{"separateThemeProfiles":true,"uiFontSize":18,"codeFontSize":13}"#).unwrap();
        assert_eq!(preferences.typography_for(ThemeMode::Light).ui_font_size, 18);
        assert_eq!(preferences.typography_for(ThemeMode::Dark).code_font_size, 13);
        preferences.set_ui_font_size(ThemeMode::Dark, 0);
        preferences.set_code_font_size(ThemeMode::Dark, 255);
        let dark = preferences.typography_for(ThemeMode::Dark);
        assert_eq!((dark.ui_font_size, dark.code_font_size), (14, 24));
        let light = preferences.typography_for(ThemeMode::Light);
        assert_eq!((light.ui_font_size, light.code_font_size), (18, 13));
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
            navigation_on_right: Some(false),
            navigation_visible: Some(false),
            ..Preferences::default()
        };
        preferences.touch_workspace("/tmp/ws");
        preferences.set_widths(240.0, 380.0);

        let content = serde_json::to_string(&preferences).expect("serialize");
        let parsed: Preferences = serde_json::from_str(&content).expect("deserialize");
        assert_eq!(parsed.theme.as_deref(), Some("ciapre"));
        assert_eq!(parsed.recent_workspaces, vec!["/tmp/ws"]);
        assert_eq!(parsed.inspector_open, Some(false));
        assert!(!parsed.navigation_on_right());
        assert!(!parsed.navigation_visible());
        assert!((parsed.sidebar_width() - 240.0).abs() < f32::EPSILON);
        assert!((parsed.inspector_width() - 380.0).abs() < f32::EPSILON);

        let empty: Preferences = serde_json::from_str("{}").expect("defaults");
        assert_eq!(empty.theme, None);
        assert!(empty.recent_workspaces.is_empty());
        assert!(empty.navigation_on_right(), "new and legacy preferences default navigation to the right");
        assert!(empty.navigation_visible(), "new and legacy preferences show navigation by default");
    }
}
