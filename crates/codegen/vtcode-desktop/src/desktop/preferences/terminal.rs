//! Embedded-terminal preferences: panel height bounds and the ANSI palette
//! selection. Split from `preferences.rs` to hold the 1000-line ceiling.

use gpui_kit::SharedString;
use gpui_kit::component::IndexPath;
use gpui_kit::component::searchable_list::SearchableListItem;
use gpui_kit::component::select::{SearchableVec, SelectState};
use gpui_kit::{App, Context, Window};

use super::Preferences;

/// Smallest terminal panel height.
pub(crate) const MIN_TERMINAL_PANEL_HEIGHT: f32 = 72.0;
/// Largest terminal panel height, so the panel cannot swallow the workspace.
pub(crate) const MAX_TERMINAL_PANEL_HEIGHT: f32 = 600.0;
const DEFAULT_TERMINAL_PANEL_HEIGHT: f32 = 240.0;
/// Narrowest side-docked panel that still fits a useful grid.
pub(crate) const MIN_TERMINAL_PANEL_WIDTH: f32 = 240.0;
/// Widest side-docked panel, so it cannot swallow the workspace.
pub(crate) const MAX_TERMINAL_PANEL_WIDTH: f32 = 640.0;
const DEFAULT_TERMINAL_PANEL_WIDTH: f32 = 360.0;

/// Which ANSI palette the embedded terminal renders.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum TerminalAnsiTheme {
    /// Follow the desktop theme mode.
    #[default]
    Auto,
    /// Always the dark ANSI table.
    Dark,
    /// Always the light ANSI table.
    Light,
}

impl TerminalAnsiTheme {
    pub(crate) const ALL: [Self; 3] = [Self::Auto, Self::Dark, Self::Light];

    pub(crate) const fn id(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Dark => "dark",
            Self::Light => "light",
        }
    }

    /// Display label for the settings dropdown. Dark/Light reuse the
    /// appearance keys; only Auto needs terminal-specific copy.
    pub(crate) fn label(self) -> String {
        match self {
            Self::Auto => rust_i18n::t!("app.settings.appearance.terminal_colors_auto"),
            Self::Dark => rust_i18n::t!("app.settings.appearance.dark"),
            Self::Light => rust_i18n::t!("app.settings.appearance.light"),
        }
        .to_string()
    }

    /// Parse a saved id. Missing or unknown values fall back to Auto so a
    /// corrupt preference never strands the terminal on one palette.
    pub(crate) fn from_saved(value: Option<&str>) -> Self {
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
pub(crate) struct TerminalAnsiThemeOption(TerminalAnsiTheme);

impl TerminalAnsiThemeOption {
    pub(crate) fn for_themes() -> SearchableVec<Self> {
        SearchableVec::new(TerminalAnsiTheme::ALL.map(Self).to_vec())
    }

    pub(crate) fn selected_index(theme: TerminalAnsiTheme) -> Option<IndexPath> {
        TerminalAnsiTheme::ALL
            .iter()
            .position(|option| *option == theme)
            .map(IndexPath::new)
    }

    /// Ready-to-mount dropdown state. Three fixed options need no search.
    pub(crate) fn select_state(
        selected: TerminalAnsiTheme,
        window: &mut Window,
        cx: &mut Context<TerminalAnsiThemeSelectState>,
    ) -> TerminalAnsiThemeSelectState {
        SelectState::new(Self::for_themes(), Self::selected_index(selected), window, cx).searchable(false)
    }
}

impl SearchableListItem for TerminalAnsiThemeOption {
    type Value = TerminalAnsiTheme;

    fn title(&self) -> SharedString {
        self.0.label().into()
    }

    fn render(&self, _: &mut Window, _: &mut App) -> impl gpui_kit::IntoElement {
        super::super::accessible_select_option(format!("terminal-ansi-option-{}", self.0.id()), self.title())
    }

    fn value(&self) -> &Self::Value {
        &self.0
    }

    fn matches(&self, query: &str) -> bool {
        let query = query.trim().to_lowercase();
        self.0.label().to_lowercase().contains(&query) || self.0.id().contains(&query)
    }
}

pub(crate) type TerminalAnsiThemeSelectState = SelectState<SearchableVec<TerminalAnsiThemeOption>>;

/// Where the embedded terminal panel docks.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum TerminalPosition {
    /// Below the content (the classic bottom panel).
    #[default]
    Bottom,
    /// Full-height strip left of the content.
    Left,
    /// Full-height strip right of the content.
    Right,
}

impl TerminalPosition {
    pub(crate) const ALL: [Self; 3] = [Self::Bottom, Self::Left, Self::Right];

    pub(crate) const fn id(self) -> &'static str {
        match self {
            Self::Bottom => "bottom",
            Self::Left => "left",
            Self::Right => "right",
        }
    }

    pub(crate) fn label(self) -> String {
        match self {
            Self::Bottom => rust_i18n::t!("app.settings.appearance.terminal_position_bottom"),
            Self::Left => rust_i18n::t!("app.settings.appearance.terminal_position_left"),
            Self::Right => rust_i18n::t!("app.settings.appearance.terminal_position_right"),
        }
        .to_string()
    }

    /// Parse a saved id. Missing or unknown values fall back to Bottom so a
    /// corrupt preference never strands the panel off-screen.
    pub(crate) fn from_saved(value: Option<&str>) -> Self {
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
pub(crate) struct TerminalPositionOption(TerminalPosition);

impl TerminalPositionOption {
    pub(crate) fn for_positions() -> SearchableVec<Self> {
        SearchableVec::new(TerminalPosition::ALL.map(Self).to_vec())
    }

    pub(crate) fn selected_index(position: TerminalPosition) -> Option<IndexPath> {
        TerminalPosition::ALL
            .iter()
            .position(|option| *option == position)
            .map(IndexPath::new)
    }

    /// Ready-to-mount dropdown state. Three fixed options need no search.
    pub(crate) fn select_state(
        selected: TerminalPosition,
        window: &mut Window,
        cx: &mut Context<TerminalPositionSelectState>,
    ) -> TerminalPositionSelectState {
        SelectState::new(Self::for_positions(), Self::selected_index(selected), window, cx).searchable(false)
    }
}

impl SearchableListItem for TerminalPositionOption {
    type Value = TerminalPosition;

    fn title(&self) -> SharedString {
        self.0.label().into()
    }

    fn render(&self, _: &mut Window, _: &mut App) -> impl gpui_kit::IntoElement {
        super::super::accessible_select_option(format!("terminal-position-option-{}", self.0.id()), self.title())
    }

    fn value(&self) -> &Self::Value {
        &self.0
    }

    fn matches(&self, query: &str) -> bool {
        let query = query.trim().to_lowercase();
        self.0.label().to_lowercase().contains(&query) || self.0.id().contains(&query)
    }
}

pub(crate) type TerminalPositionSelectState = SelectState<SearchableVec<TerminalPositionOption>>;

fn clamp_terminal_height(height: f32) -> f32 {
    height.clamp(MIN_TERMINAL_PANEL_HEIGHT, MAX_TERMINAL_PANEL_HEIGHT)
}

fn clamp_terminal_width(width: f32) -> f32 {
    width.clamp(MIN_TERMINAL_PANEL_WIDTH, MAX_TERMINAL_PANEL_WIDTH)
}

impl Preferences {
    pub(crate) fn terminal_panel_height(&self) -> f32 {
        clamp_terminal_height(self.terminal_panel_height.unwrap_or(DEFAULT_TERMINAL_PANEL_HEIGHT))
    }

    pub(crate) fn set_terminal_panel_height(&mut self, height: f32) {
        self.terminal_panel_height = Some(clamp_terminal_height(height));
    }

    pub(crate) fn terminal_ansi_theme(&self) -> TerminalAnsiTheme {
        TerminalAnsiTheme::from_saved(self.terminal_ansi_theme.as_deref())
    }

    pub(crate) fn set_terminal_ansi_theme(&mut self, theme: TerminalAnsiTheme) {
        self.terminal_ansi_theme = Some(theme.id().to_owned());
    }

    pub(crate) fn terminal_position(&self) -> TerminalPosition {
        TerminalPosition::from_saved(self.terminal_position.as_deref())
    }

    pub(crate) fn set_terminal_position(&mut self, position: TerminalPosition) {
        self.terminal_position = Some(position.id().to_owned());
    }

    pub(crate) fn terminal_panel_width(&self) -> f32 {
        clamp_terminal_width(self.terminal_panel_width.unwrap_or(DEFAULT_TERMINAL_PANEL_WIDTH))
    }

    pub(crate) fn set_terminal_panel_width(&mut self, width: f32) {
        self.terminal_panel_width = Some(clamp_terminal_width(width));
    }
}
