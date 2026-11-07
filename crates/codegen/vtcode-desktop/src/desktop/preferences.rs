//! Persisted desktop preferences in `~/.vtcode/desktop/preferences.json`.
//!
//! A missing or corrupt file falls back to defaults. Permission choices are
//! explicit per-workspace overrides of the CLI confirmation flag.

use std::path::PathBuf;

#[cfg(not(test))]
use objc2_app_kit::{NSFont, NSFontTextStyleBody};
#[cfg(not(test))]
use objc2_foundation::NSDictionary;

use gpui_kit::SharedString;
use gpui_kit::component::IndexPath;
use gpui_kit::component::diff::DiffMode;
use gpui_kit::component::searchable_list::SearchableListItem;
use gpui_kit::component::select::{SearchableVec, SelectState};
use gpui_kit::component::theme::ThemeMode;
use gpui_kit::{App, Global};
use serde::{Deserialize, Serialize};

use super::diff_preferences;
use super::files::ExternalEditor;
use super::localization::UiLanguage;
use super::themes;

const MAX_RECENT_WORKSPACES: usize = 8;
const DEFAULT_SIDEBAR_WIDTH: f32 = 270.0;
/// Fits the five-tab inspector rail plus its trailing close control without
/// scrolling at the default UI size (the rail needs ~407px plus its insets,
/// the rail gap, and the close button); narrower widths stay usable through
/// the tab scroller.
pub(super) const DEFAULT_INSPECTOR_WIDTH: f32 = 474.0;
pub(super) const MIN_PANE_WIDTH: f32 = 180.0;
pub(super) const MAX_PANE_WIDTH: f32 = 560.0;
/// Smallest UI base size: keeps the tiniest labels (`text_xs` = 0.75rem) at
/// 10.5px, the smallest functional size on macOS.
pub(super) const MIN_UI_FONT_SIZE: u8 = 14;
pub(super) const MAX_UI_FONT_SIZE: u8 = 24;
/// Baseline UI size when the user has not pinned one: follows the macOS
/// accessibility text setting through [`system_text_scale`].
const DEFAULT_UI_FONT_SIZE: u8 = 16;
/// The macOS system default body size that the accessibility text slider
/// scales away from (System Settings → Accessibility → Display → Text).
#[cfg_attr(test, allow(dead_code, reason = "the test stub returns a constant scale"))]
const SYSTEM_BODY_FONT_SIZE: f32 = 13.0;

/// The macOS accessibility text setting as a multiplier over the system
/// default body size: 1.0 at the system default, larger when the user opted
/// into bigger system text. An unset app size follows it; an explicit size
/// overrides it. Applies at launch and whenever typography is re-applied.
fn system_text_scale() -> f32 {
    #[cfg(not(test))]
    {
        #[expect(
            unsafe_code,
            reason = "Reading the macOS accessibility text size requires the AppKit preferred-font query; \
                it is a process-global main-thread lookup with default options"
        )]
        // SAFETY: main-thread AppKit query; the preferred-font lookup returns a valid font object.
        unsafe {
            let font = NSFont::preferredFontForTextStyle_options(NSFontTextStyleBody, &NSDictionary::dictionary());
            #[allow(
                clippy::cast_possible_truncation,
                reason = "system font sizes are small positives; f32 is exact"
            )]
            ((font.pointSize() as f32) / SYSTEM_BODY_FONT_SIZE).clamp(1.0, 1.5)
        }
    }
    #[cfg(test)]
    {
        1.0
    }
}

/// Resolve the UI base size: an explicit user choice wins; an unset size
/// follows the macOS accessibility text setting over the 16px baseline.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "rounded and clamped into the 14..=24 u8 font-size range"
)]
fn scaled_ui_font_size(explicit: Option<u8>, system_scale: f32) -> u8 {
    match explicit {
        Some(size) => size.clamp(MIN_UI_FONT_SIZE, MAX_UI_FONT_SIZE),
        None => (f32::from(DEFAULT_UI_FONT_SIZE) * system_scale)
            .round()
            .clamp(f32::from(MIN_UI_FONT_SIZE), f32::from(MAX_UI_FONT_SIZE)) as u8,
    }
}
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

    pub(super) fn label(self) -> String {
        match self {
            Self::System => rust_i18n::t!("app.appearance.motion_system").to_string(),
            Self::On => rust_i18n::t!("app.appearance.motion_on").to_string(),
            Self::Off => rust_i18n::t!("app.appearance.motion_off").to_string(),
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

/// Where files open when selected in the sidebar, palettes, review views, or attached sources.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum FileOpenDestination {
    /// Built-in read-only preview overlay.
    #[default]
    Viewer,
    /// The system default application for the file type.
    System,
    /// Visual Studio Code.
    VsCode,
    /// Sublime Text.
    Sublime,
    /// Xcode.
    Xcode,
    /// Android Studio.
    AndroidStudio,
    /// Reveal the file in Finder.
    Finder,
    /// Open the containing directory in Terminal.
    Terminal,
    /// Open the containing directory in iTerm2.
    ITerm2,
    /// Open the containing directory in Ghostty.
    Ghostty,
}

impl FileOpenDestination {
    pub(super) const ALL: [Self; 10] = [
        Self::Viewer,
        Self::System,
        Self::VsCode,
        Self::Sublime,
        Self::Xcode,
        Self::AndroidStudio,
        Self::Finder,
        Self::Terminal,
        Self::ITerm2,
        Self::Ghostty,
    ];

    /// Destinations needing no install detection: the viewer plus macOS apps
    /// that ship on the sealed system volume and cannot be uninstalled.
    pub(super) const ALWAYS_AVAILABLE: [Self; 4] = [Self::Viewer, Self::System, Self::Finder, Self::Terminal];

    pub(super) const fn id(self) -> &'static str {
        match self {
            Self::Viewer => "viewer",
            Self::System => "system",
            Self::VsCode => "vscode",
            Self::Sublime => "sublime",
            Self::Xcode => "xcode",
            Self::AndroidStudio => "androidstudio",
            Self::Finder => "finder",
            Self::Terminal => "terminal",
            Self::ITerm2 => "iterm2",
            Self::Ghostty => "ghostty",
        }
    }

    /// The external application for this destination, if it leaves the viewer.
    pub(super) const fn external_editor(self) -> Option<ExternalEditor> {
        match self {
            Self::Viewer => None,
            Self::System => Some(ExternalEditor::System),
            Self::VsCode => Some(ExternalEditor::VsCode),
            Self::Sublime => Some(ExternalEditor::Sublime),
            Self::Xcode => Some(ExternalEditor::Xcode),
            Self::AndroidStudio => Some(ExternalEditor::AndroidStudio),
            Self::Finder => Some(ExternalEditor::Finder),
            Self::Terminal => Some(ExternalEditor::Terminal),
            Self::ITerm2 => Some(ExternalEditor::ITerm2),
            Self::Ghostty => Some(ExternalEditor::Ghostty),
        }
    }

    /// Dropdown options in canonical order: always-available destinations and
    /// detected editors, plus the saved choice even when undetected so the
    /// control never renders blank and reinstalls restore behavior.
    pub(super) fn offered(saved: Self, detected: &[ExternalEditor]) -> Vec<Self> {
        Self::ALL
            .into_iter()
            .filter(|destination| {
                Self::ALWAYS_AVAILABLE.contains(destination)
                    || *destination == saved
                    || destination.external_editor().is_some_and(|editor| detected.contains(&editor))
            })
            .collect()
    }

    /// Display label for the settings dropdown. Descriptive destinations are
    /// localized; editor brand names are proper nouns and stay as-is.
    pub(super) fn label(self) -> String {
        match self {
            Self::Viewer => rust_i18n::t!("app.settings.files.destination.viewer").to_string(),
            Self::System => rust_i18n::t!("app.settings.files.destination.system").to_string(),
            Self::VsCode => "VS Code".to_string(),
            Self::Sublime => "Sublime Text".to_string(),
            Self::Xcode => "Xcode".to_string(),
            Self::AndroidStudio => "Android Studio".to_string(),
            Self::Finder => "Finder".to_string(),
            Self::Terminal => "Terminal".to_string(),
            Self::ITerm2 => "iTerm2".to_string(),
            Self::Ghostty => "Ghostty".to_string(),
        }
    }

    /// Parse a saved id. Missing or unknown values fall back to the default
    /// so a corrupt preference never breaks file selection.
    pub(super) fn from_saved(value: Option<&str>) -> Self {
        let Some(id) = value.map(str::trim) else {
            return Self::default();
        };
        Self::ALL
            .into_iter()
            .find(|destination| destination.id().eq_ignore_ascii_case(id))
            .unwrap_or_default()
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct FileDestinationOption(FileOpenDestination);

impl FileDestinationOption {
    pub(super) fn for_destinations(destinations: &[FileOpenDestination]) -> SearchableVec<Self> {
        SearchableVec::new(destinations.iter().map(|destination| Self(*destination)).collect::<Vec<_>>())
    }

    pub(super) fn selected_index(
        destinations: &[FileOpenDestination],
        destination: FileOpenDestination,
    ) -> Option<IndexPath> {
        destinations
            .iter()
            .position(|option| *option == destination)
            .map(IndexPath::new)
    }
}

impl SearchableListItem for FileDestinationOption {
    type Value = FileOpenDestination;

    fn title(&self) -> SharedString {
        self.0.label().into()
    }

    fn render(&self, _: &mut gpui_kit::Window, _: &mut App) -> impl gpui_kit::IntoElement {
        super::accessible_select_option(format!("destination-option-{}", self.0.id()), self.title())
    }

    fn value(&self) -> &Self::Value {
        &self.0
    }

    fn matches(&self, query: &str) -> bool {
        let query = query.trim().to_lowercase();
        self.0.label().to_lowercase().contains(&query) || self.0.id().contains(&query)
    }
}

pub(super) type FileDestinationSelectState = SelectState<SearchableVec<FileDestinationOption>>;

/// Terminal used by the CLI handoff ("Continue in terminal").
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum CliTerminal {
    #[default]
    Terminal,
    ITerm2,
    Ghostty,
}

impl CliTerminal {
    pub(super) const ALL: [Self; 3] = [Self::Terminal, Self::ITerm2, Self::Ghostty];

    pub(super) const fn id(self) -> &'static str {
        match self {
            Self::Terminal => "terminal",
            Self::ITerm2 => "iterm2",
            Self::Ghostty => "ghostty",
        }
    }

    pub(super) const fn external_editor(self) -> ExternalEditor {
        match self {
            Self::Terminal => ExternalEditor::Terminal,
            Self::ITerm2 => ExternalEditor::ITerm2,
            Self::Ghostty => ExternalEditor::Ghostty,
        }
    }

    /// Dropdown options in canonical order: Terminal always (it ships on the
    /// sealed system volume and cannot be uninstalled), the rest when
    /// detected, plus the saved choice even when undetected so the control
    /// never renders blank and reinstalls restore behavior.
    pub(super) fn offered(saved: Self, detected: &[ExternalEditor]) -> Vec<Self> {
        Self::ALL
            .into_iter()
            .filter(|terminal| {
                matches!(terminal, Self::Terminal)
                    || *terminal == saved
                    || detected.contains(&terminal.external_editor())
            })
            .collect()
    }

    /// Display label. Terminal brand names are proper nouns and stay as-is.
    pub(super) fn label(self) -> String {
        match self {
            Self::Terminal => "Terminal".to_string(),
            Self::ITerm2 => "iTerm2".to_string(),
            Self::Ghostty => "Ghostty".to_string(),
        }
    }

    /// Parse a saved id. Missing or unknown values fall back to the default
    /// so a corrupt preference never breaks the handoff.
    pub(super) fn from_saved(value: Option<&str>) -> Self {
        let Some(id) = value.map(str::trim) else {
            return Self::default();
        };
        Self::ALL
            .into_iter()
            .find(|terminal| terminal.id().eq_ignore_ascii_case(id))
            .unwrap_or_default()
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct CliTerminalOption(CliTerminal);

impl CliTerminalOption {
    pub(super) fn for_terminals(terminals: &[CliTerminal]) -> SearchableVec<Self> {
        SearchableVec::new(terminals.iter().map(|terminal| Self(*terminal)).collect::<Vec<_>>())
    }

    pub(super) fn selected_index(terminals: &[CliTerminal], terminal: CliTerminal) -> Option<IndexPath> {
        terminals.iter().position(|option| *option == terminal).map(IndexPath::new)
    }
}

impl SearchableListItem for CliTerminalOption {
    type Value = CliTerminal;

    fn title(&self) -> SharedString {
        self.0.label().into()
    }

    fn render(&self, _: &mut gpui_kit::Window, _: &mut App) -> impl gpui_kit::IntoElement {
        super::accessible_select_option(format!("terminal-option-{}", self.0.id()), self.title())
    }

    fn value(&self) -> &Self::Value {
        &self.0
    }

    fn matches(&self, query: &str) -> bool {
        let query = query.trim().to_lowercase();
        self.0.label().to_lowercase().contains(&query) || self.0.id().contains(&query)
    }
}

pub(super) type CliTerminalSelectState = SelectState<SearchableVec<CliTerminalOption>>;

/// When turn-terminal notifications (completed, failed, blocked) post.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum TurnCompletionMode {
    /// Post even while the window is focused.
    Always,
    /// Post only while the window is in the background.
    #[default]
    Unfocused,
    /// Never post turn-terminal notifications.
    Never,
}

impl TurnCompletionMode {
    pub(super) const ALL: [Self; 3] = [Self::Always, Self::Unfocused, Self::Never];

    pub(super) const fn id(self) -> &'static str {
        match self {
            Self::Always => "always",
            Self::Unfocused => "unfocused",
            Self::Never => "never",
        }
    }

    /// Display label for the settings dropdown.
    pub(super) fn label(self) -> String {
        match self {
            Self::Always => rust_i18n::t!("app.settings.notifications.turn_completion.always"),
            Self::Unfocused => rust_i18n::t!("app.settings.notifications.turn_completion.unfocused"),
            Self::Never => rust_i18n::t!("app.settings.notifications.turn_completion.never"),
        }
        .to_string()
    }

    /// Parse a saved id. Missing or unknown values fall back to the default
    /// so a corrupt preference never breaks notification delivery.
    pub(super) fn from_saved(value: Option<&str>) -> Self {
        let Some(id) = value.map(str::trim) else {
            return Self::default();
        };
        Self::ALL
            .into_iter()
            .find(|mode| mode.id().eq_ignore_ascii_case(id))
            .unwrap_or_default()
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct TurnCompletionModeOption(TurnCompletionMode);

impl TurnCompletionModeOption {
    pub(super) fn for_modes() -> SearchableVec<Self> {
        SearchableVec::new(TurnCompletionMode::ALL.map(Self).to_vec())
    }

    pub(super) fn selected_index(mode: TurnCompletionMode) -> Option<IndexPath> {
        TurnCompletionMode::ALL
            .iter()
            .position(|option| *option == mode)
            .map(IndexPath::new)
    }
}

impl SearchableListItem for TurnCompletionModeOption {
    type Value = TurnCompletionMode;

    fn title(&self) -> SharedString {
        self.0.label().into()
    }

    fn render(&self, _: &mut gpui_kit::Window, _: &mut App) -> impl gpui_kit::IntoElement {
        super::accessible_select_option(format!("notification-mode-option-{}", self.0.id()), self.title())
    }

    fn value(&self) -> &Self::Value {
        &self.0
    }

    fn matches(&self, query: &str) -> bool {
        let query = query.trim().to_lowercase();
        self.0.label().to_lowercase().contains(&query) || self.0.id().contains(&query)
    }
}

pub(super) type TurnCompletionModeSelectState = SelectState<SearchableVec<TurnCompletionModeOption>>;

/// Client-local notification delivery preferences. Every toggle is optional
/// so a missing value keeps existing installs on the default behavior.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(super) struct NotificationPreferences {
    /// Master toggle for desktop and in-app notifications.
    pub enabled: Option<bool>,
    /// When turn-terminal notifications post (`always`/`unfocused`/`never`).
    pub turn_completion: Option<String>,
    /// Notify on approval and permission prompts.
    pub permission_prompts: Option<bool>,
    /// Notify on questions needing input to continue.
    pub questions: Option<bool>,
    /// Notify when a newer VT Code release is available.
    pub updates: Option<bool>,
    /// Latest release version already announced; suppresses repeat banners.
    pub last_notified_update: Option<String>,
    /// Running version whose release notes were already shown once in the
    /// changelog viewer (the post-update "what's new" moment).
    pub last_seen_changelog_version: Option<String>,
}

impl NotificationPreferences {
    pub(super) fn enabled(&self) -> bool {
        self.enabled.unwrap_or(true)
    }

    pub(super) fn turn_completion(&self) -> TurnCompletionMode {
        TurnCompletionMode::from_saved(self.turn_completion.as_deref())
    }

    pub(super) fn set_turn_completion(&mut self, mode: TurnCompletionMode) {
        self.turn_completion = Some(mode.id().to_owned());
    }

    pub(super) fn permission_prompts(&self) -> bool {
        self.permission_prompts.unwrap_or(true)
    }

    pub(super) fn questions(&self) -> bool {
        self.questions.unwrap_or(true)
    }

    pub(super) fn updates(&self) -> bool {
        self.updates.unwrap_or(true)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(super) struct Preferences {
    /// Selected desktop UI language. Unknown values fall back to English.
    pub ui_language: Option<String>,
    /// Default file open destination id. Unknown values fall back to the viewer.
    pub file_open_destination: Option<String>,
    /// Terminal for CLI handoff id. Unknown values fall back to Terminal.
    pub cli_terminal: Option<String>,
    /// Legacy diff viewer layout id (`unified`/`split`): only a read fallback
    /// for installs that never saved the new `diff` struct below.
    pub diff_mode: Option<String>,
    /// Diff viewer presentation choices; owns every diff option going forward.
    pub diff: diff_preferences::DiffPreferences,
    /// The optional diagnostics footer is hidden on a fresh installation.
    pub show_status_bar: bool,
    pub reduced_motion: ReducedMotionPreference,
    pub shortcuts: std::collections::BTreeMap<String, String>,
    /// Experimental vim-style navigation (`j`/`k`/`g`/`G`/`h`/`l`, `i`, `/`).
    /// Missing values keep existing installs on the default off behavior.
    pub vim_navigation: bool,
    pub ui_font_family: Option<String>,
    pub code_font_family: Option<String>,
    pub ui_font_size: Option<u8>,
    pub code_font_size: Option<u8>,
    /// Selected theme id.
    pub theme: Option<String>,
    /// Selected application-icon artwork id (`dynamic`/`light`/`dark`).
    /// Unknown values fall back to `Dynamic` on read.
    pub app_icon: Option<String>,
    /// Keep independent theme and typography choices for light and dark mode.
    pub separate_theme_profiles: bool,
    pub light_theme: Option<String>,
    pub dark_theme: Option<String>,
    pub light_typography: Option<TypographyProfile>,
    pub dark_typography: Option<TypographyProfile>,
    /// Desktop-only user-authored themes, in creation order. They never sync
    /// to the CLI, which owns only its own palette registry.
    #[serde(default, deserialize_with = "themes::deserialize_custom_themes")]
    pub custom_themes: Vec<themes::CustomTheme>,
    /// None preserves legacy explicit theme choices; new profiles follow the CLI.
    pub follow_cli_theme: Option<bool>,
    /// Recently opened workspaces, most recent first.
    pub recent_workspaces: Vec<String>,
    /// Explicit confirmation choices, retained alongside recent workspace paths.
    pub workspace_permissions: std::collections::BTreeMap<String, vtcode_desktop_protocol::PermissionMode>,
    /// Reasoning effort levels shown in model controls. Missing means every
    /// allowed level; unknown entries are ignored on read.
    pub available_efforts: Option<Vec<String>>,
    /// Last inspector state, retained for compatibility; launch always starts collapsed.
    pub inspector_open: Option<bool>,
    /// Last zen layout choice. Missing values keep fresh installs in zen.
    pub zen_mode: Option<bool>,
    /// Place the project and thread navigation on the right side of the workspace.
    /// Missing values keep existing installs on the new right-side default.
    pub navigation_on_right: Option<bool>,
    /// Show the project and thread navigation panel. Its actions remain in the header when hidden.
    pub navigation_visible: Option<bool>,
    /// Use the system's blurred backdrop behind the navigation panel.
    pub translucent_sidebar: bool,
    /// Keep the main window available from the macOS status bar when closed.
    pub show_in_menu_bar: bool,
    /// Additional foreground contrast, from 0 (theme default) to 100.
    pub contrast: u8,
    /// Sidebar width in logical pixels.
    pub sidebar_width: Option<f32>,
    /// Inspector width in logical pixels.
    pub inspector_width: Option<f32>,
    /// Open the main window maximized (zoomed). Missing values launch
    /// maximized so fresh installs start full-size.
    pub window_maximized: Option<bool>,
    /// Client-local notification delivery preferences.
    pub notifications: NotificationPreferences,
}

impl Preferences {
    pub(super) fn contrast(&self) -> u8 {
        self.contrast.min(100)
    }
    pub(super) fn ui_language(&self) -> UiLanguage {
        UiLanguage::from_saved(self.ui_language.as_deref())
    }

    pub(super) fn set_ui_language(&mut self, language: UiLanguage) {
        self.ui_language = Some(language.locale().to_owned());
    }

    pub(super) fn app_icon(&self) -> vtcode_commons::ui_protocol::AppIconPreference {
        vtcode_commons::ui_protocol::AppIconPreference::from_id(self.app_icon.as_deref())
    }

    pub(super) fn set_app_icon(&mut self, preference: vtcode_commons::ui_protocol::AppIconPreference) {
        self.app_icon = Some(preference.id().to_owned());
    }

    pub(super) fn file_open_destination(&self) -> FileOpenDestination {
        FileOpenDestination::from_saved(self.file_open_destination.as_deref())
    }

    pub(super) fn set_file_open_destination(&mut self, destination: FileOpenDestination) {
        self.file_open_destination = Some(destination.id().to_owned());
    }

    pub(super) fn cli_terminal(&self) -> CliTerminal {
        CliTerminal::from_saved(self.cli_terminal.as_deref())
    }

    pub(super) fn set_cli_terminal(&mut self, terminal: CliTerminal) {
        self.cli_terminal = Some(terminal.id().to_owned());
    }

    pub(super) fn diff_mode(&self) -> DiffMode {
        // New installs write `diff.mode`; the legacy top-level key still
        // applies for installs that never saved the new struct.
        if self.diff.mode.is_none() {
            return diff_preferences::mode_from_saved(self.diff_mode.as_deref());
        }
        self.diff.mode()
    }

    pub(super) fn set_diff_mode(&mut self, mode: DiffMode) {
        self.diff.set_mode(mode);
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

    pub(super) fn zen_mode(&self) -> bool {
        self.zen_mode.unwrap_or(true)
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
                ui_font_size: scaled_ui_font_size(profile.ui_font_size, system_text_scale()),
                code_font_size: profile
                    .code_font_size
                    .unwrap_or(14)
                    .clamp(MIN_CODE_FONT_SIZE, MAX_CODE_FONT_SIZE),
            },
            None => TypographySettings {
                ui_font_family: self.ui_font_family.as_deref(),
                code_font_family: self.code_font_family.as_deref(),
                ui_font_size: scaled_ui_font_size(self.ui_font_size, system_text_scale()),
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

    pub(super) fn window_maximized(&self) -> bool {
        self.window_maximized.unwrap_or(true)
    }

    pub(super) fn set_window_maximized(&mut self, maximized: bool) {
        self.window_maximized = Some(maximized);
    }

    /// Effort levels shown in model controls, in canonical order. Unknown
    /// saved entries are dropped; a missing preference shows every level.
    pub(super) fn available_efforts(&self) -> Vec<String> {
        let allowed = vtcode_commons::reasoning::ReasoningEffortLevel::allowed_values();
        match self.available_efforts.as_ref() {
            None => allowed.iter().map(ToString::to_string).collect(),
            Some(selected) => allowed
                .iter()
                .filter(|level| selected.iter().any(|entry| entry == *level))
                .map(ToString::to_string)
                .collect(),
        }
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
        if let Some(path) = TEST_PREFERENCES_PATH.with(|path| path.borrow().clone()) {
            return Some(path);
        }
        dirs::home_dir().map(|home| home.join(".vtcode").join("desktop").join("preferences.json"))
    }

    /// Load preferences, returning defaults for a missing file. A corrupt
    /// file is quarantined beside the original before falling back, so the
    /// next save cannot silently overwrite the evidence with defaults.
    pub(super) fn load() -> Self {
        let Some(path) = Self::preferences_path() else {
            return Self::default();
        };
        let content = match std::fs::read_to_string(&path) {
            Ok(content) => content,
            Err(_) => return Self::default(),
        };
        match serde_json::from_str(&content) {
            Ok(preferences) => preferences,
            Err(error) => {
                let quarantined = path.with_extension("json.corrupt");
                if let Err(rename_error) = std::fs::rename(&path, &quarantined) {
                    vtcode_commons::diagnostics::stderr(format_args!(
                        "failed to quarantine corrupt desktop preferences ({}): {rename_error}",
                        quarantined.display()
                    ));
                } else {
                    vtcode_commons::diagnostics::stderr(format_args!(
                        "desktop preferences corrupt ({error}); moved to {}",
                        quarantined.display()
                    ));
                }
                Self::default()
            }
        }
    }

    /// Best-effort saves for existing comfort controls. Failures are loud on
    /// stderr so a silently unpersisted setting stays diagnosable.
    pub(super) fn save(&self) {
        if let Err(error) = self.try_save() {
            vtcode_commons::diagnostics::stderr(format_args!("failed to save desktop preferences: {error}"));
        }
    }

    pub(super) fn try_save(&self) -> Result<(), String> {
        let path =
            Self::preferences_path().ok_or_else(|| rust_i18n::t!("app.errors.preferences_unavailable").to_string())?;
        let content = serde_json::to_vec_pretty(self).map_err(|error| error.to_string())?;
        vtcode_commons::VtCodePaths::write_private_file_atomic(path, &content)
            .map_err(|error| rust_i18n::t!("app.errors.preferences_save_failed", error = error.to_string()).to_string())
    }
}

fn clamp_width(width: f32) -> f32 {
    width.clamp(MIN_PANE_WIDTH, MAX_PANE_WIDTH)
}

#[cfg(test)]
std::thread_local! {
    static TEST_PREFERENCES_PATH: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
}

/// Drop any leftover file from a dead process so the new test starts from
/// defaults. Test paths embed pid+sequence, which collide after pid reuse.
#[cfg(test)]
fn remove_stale_test_file(path: &std::path::Path) {
    drop(std::fs::remove_file(path));
}

/// Redirect preference I/O for one GUI-test setup without touching the real
/// user home. Each call receives a fresh file, even when the test runner
/// reuses a worker thread for another test.
#[cfg(test)]
pub(super) fn install_test_preferences_path() -> PathBuf {
    static NEXT_TEST_PATH: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let sequence = NEXT_TEST_PATH.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!("vtcode-desktop-test-prefs-{}-{sequence}.json", std::process::id()));
    remove_stale_test_file(&path);
    set_test_preferences_path(path.clone());
    path
}

#[cfg(test)]
pub(super) fn set_test_preferences_path(path: PathBuf) {
    TEST_PREFERENCES_PATH.with(|slot| *slot.borrow_mut() = Some(path));
}

#[cfg(test)]
mod tests;
