//! VT Code desktop themes: curated ports of the TUI theme registry onto the
//! gpui-kit tokens, with Ciapre as the app default.
//!
//! Each theme overrides the core kit colors after `Theme::change` installs
//! the matching light/dark base; derived tokens (border, panels, muted text)
//! are mixed from the palette so every theme stays legible by construction.

use gpui_kit::component::theme::{Theme, ThemeMode};
use gpui_kit::{App, Hsla, Window, rgb};

/// The theme applied on first launch.
pub(super) const DEFAULT_THEME_ID: &str = "ciapre";

/// Sentinel ids that keep the kit's own light/dark base without overrides.
pub(super) const SYSTEM_LIGHT_ID: &str = "system-light";
pub(super) const SYSTEM_DARK_ID: &str = "system-dark";

pub(super) fn mode_for_id(id: &str) -> Option<ThemeMode> {
    match id {
        SYSTEM_LIGHT_ID => Some(ThemeMode::Light),
        SYSTEM_DARK_ID => Some(ThemeMode::Dark),
        _ => find(id).map(|theme| theme.mode),
    }
}

pub(super) fn default_id(mode: ThemeMode) -> &'static str {
    match mode {
        ThemeMode::Light => "catppuccin-latte",
        ThemeMode::Dark => DEFAULT_THEME_ID,
    }
}

pub(super) fn system_id(mode: ThemeMode) -> &'static str {
    match mode {
        ThemeMode::Light => SYSTEM_LIGHT_ID,
        ThemeMode::Dark => SYSTEM_DARK_ID,
    }
}

#[derive(Clone, Copy)]
pub(super) struct DesktopTheme {
    pub id: &'static str,
    pub label: &'static str,
    pub mode: ThemeMode,
    pub background: Hsla,
    pub foreground: Hsla,
    /// Brand accent used for highlights and primary buttons.
    pub accent: Hsla,
    /// Quieter companion accent for secondary panels and badges.
    pub secondary: Hsla,
    pub danger: Hsla,
    pub warning: Hsla,
}

macro_rules! theme {
    (
        $id:literal, $label:literal, $mode:expr,
        bg $bg:literal, fg $fg:literal,
        accent $accent:literal, secondary $secondary:literal,
        danger $danger:literal, warning $warning:literal,
    ) => {
        DesktopTheme {
            id: $id,
            label: $label,
            mode: $mode,
            background: rgb($bg).into(),
            foreground: rgb($fg).into(),
            accent: rgb($accent).into(),
            secondary: rgb($secondary).into(),
            danger: rgb($danger).into(),
            warning: rgb($warning).into(),
        }
    };
}

/// Resolve a theme id; the sentinel system ids map to the kit defaults and
/// unknown ids fall back to the VT Code default.
pub(super) fn find(id: &str) -> Option<&'static DesktopTheme> {
    all_themes().iter().find(|theme| theme.id == id)
}

/// The curated palette table, built once (gpui color constructors are not
/// `const`).
pub(super) fn all_themes() -> &'static [DesktopTheme] {
    static THEMES: std::sync::LazyLock<Vec<DesktopTheme>> = std::sync::LazyLock::new(|| {
        vec![
            theme!(
                "ciapre", "Ciapre", ThemeMode::Dark,
                bg 0x181818, fg 0xAEA47F,
                accent 0xCC8A3E, secondary 0xAEA47F,
                danger 0xC16A68, warning 0xCC8A3E,
            ),
            theme!(
                "ciapre-dark", "Ciapre Dark", ThemeMode::Dark,
                bg 0x262626, fg 0xBFB38F,
                accent 0xD99A4E, secondary 0xBFB38F,
                danger 0xFF8A8A, warning 0xD99A4E,
            ),
            theme!(
                "ciapre-blue", "Ciapre Blue", ThemeMode::Dark,
                bg 0x171C26, fg 0xBFB38F,
                accent 0xD99A4E, secondary 0xBFB38F,
                danger 0xFF8A8A, warning 0xD99A4E,
            ),
            theme!(
                "solarized-dark", "Solarized Dark", ThemeMode::Dark,
                bg 0x002B36, fg 0x839496,
                accent 0x3A95D6, secondary 0x839496,
                danger 0xE56765, warning 0xB58900,
            ),
            theme!(
                "solarized-light", "Solarized Light", ThemeMode::Light,
                bg 0xFDF6E3, fg 0x586E75,
                accent 0x2073AE, secondary 0x586E75,
                danger 0xD3302D, warning 0xB58900,
            ),
            theme!(
                "solarized-dark-hc", "Solarized Dark HC", ThemeMode::Dark,
                bg 0x002831, fg 0xE9E3CC,
                accent 0x4A90D2, secondary 0xE9E3CC,
                danger 0xE0656A, warning 0xB58900,
            ),
            theme!(
                "gruvbox-dark", "Gruvbox Dark", ThemeMode::Dark,
                bg 0x282828, fg 0xA89984,
                accent 0xD79921, secondary 0xA89984,
                danger 0xDE716C, warning 0xD79921,
            ),
            theme!(
                "gruvbox-light", "Gruvbox Light", ThemeMode::Light,
                bg 0xFBF4E8, fg 0x786C61,
                accent 0x986311, secondary 0x786C61,
                danger 0x9D0006, warning 0x986311,
            ),
            theme!(
                "dracula", "Dracula", ThemeMode::Dark,
                bg 0x282A36, fg 0xF8F8F2,
                accent 0xFFB86C, secondary 0x8BE9FD,
                danger 0xFF5858, warning 0xFFB86C,
            ),
            theme!(
                "github-dark", "GitHub Dark", ThemeMode::Dark,
                bg 0x0D1117, fg 0xC9D1D9,
                accent 0x58A6FF, secondary 0xD29922,
                danger 0xF85149, warning 0xD29922,
            ),
            theme!(
                "github", "GitHub Light", ThemeMode::Light,
                bg 0xFFFFFF, fg 0x24292E,
                accent 0x0366D6, secondary 0x24292E,
                danger 0xD53948, warning 0x907000,
            ),
            theme!(
                "atom-one-dark", "Atom One Dark", ThemeMode::Dark,
                bg 0x282C34, fg 0xABB2BF,
                accent 0x61AFEF, secondary 0xE5C07B,
                danger 0xE2737C, warning 0xE5C07B,
            ),
            theme!(
                "atom-one-light", "Atom One Light", ThemeMode::Light,
                bg 0xFAFAFA, fg 0x383A42,
                accent 0x3E70C3, secondary 0x383A42,
                danger 0xC2493E, warning 0x986200,
            ),
            theme!(
                "tomorrow-night", "Tomorrow Night", ThemeMode::Dark,
                bg 0x1D1F21, fg 0xC5C8C6,
                accent 0xDE935F, secondary 0x81A2BE,
                danger 0xCE6B6B, warning 0xDE935F,
            ),
            theme!(
                "catppuccin-latte", "Catppuccin Latte", ThemeMode::Light,
                bg 0xEFF1F5, fg 0x4C4F69,
                accent 0x1E66F5, secondary 0x8839EF,
                danger 0xD20F39, warning 0xFE640B,
            ),
            theme!(
                "catppuccin-mocha", "Catppuccin Mocha", ThemeMode::Dark,
                bg 0x1E1E2E, fg 0xCDD6F4,
                accent 0x89B4FA, secondary 0xCBA6F7,
                danger 0xF38BA8, warning 0xFAB387,
            ),
        ]
    });
    &THEMES
}

/// One selectable entry in the theme picker: a palette theme or one of the
/// kit's system light/dark bases.
#[derive(Clone, Copy)]
pub(super) enum SwatchEntry {
    System(ThemeMode),
    Palette(DesktopTheme),
}

impl SwatchEntry {
    pub(super) fn id(self) -> String {
        match self {
            Self::System(ThemeMode::Light) => SYSTEM_LIGHT_ID.to_string(),
            Self::System(ThemeMode::Dark) => SYSTEM_DARK_ID.to_string(),
            Self::Palette(theme) => theme.id.to_string(),
        }
    }

    pub(super) fn label(self) -> &'static str {
        match self {
            Self::System(ThemeMode::Light) => "System Light",
            Self::System(ThemeMode::Dark) => "System Dark",
            Self::Palette(theme) => theme.label,
        }
    }

    /// The two colors a picker swatch previews.
    pub(super) fn preview(self) -> (Hsla, Hsla) {
        match self {
            Self::System(ThemeMode::Light) => (rgb(0xFFFFFF).into(), rgb(0x1F2937).into()),
            Self::System(ThemeMode::Dark) => (rgb(0x0F172A).into(), rgb(0xE2E8F0).into()),
            Self::Palette(theme) => (theme.background, theme.foreground),
        }
    }

    pub(super) fn accent(self) -> Option<Hsla> {
        match self {
            Self::System(_) => None,
            Self::Palette(theme) => Some(theme.accent),
        }
    }
}

/// The two system entries shown first in the picker.
pub(super) const SYSTEM_SWATCHES: [SwatchEntry; 2] = [
    SwatchEntry::System(ThemeMode::Light),
    SwatchEntry::System(ThemeMode::Dark),
];

/// Apply a theme by id: install the kit base for its mode, then override the
/// core tokens from the palette.
pub(super) fn apply_by_id(id: &str, window: Option<&mut Window>, cx: &mut App) -> ThemeMode {
    match id {
        SYSTEM_LIGHT_ID => {
            Theme::change(ThemeMode::Light, window, cx);
            cx.refresh_windows();
            ThemeMode::Light
        }
        SYSTEM_DARK_ID => {
            Theme::change(ThemeMode::Dark, window, cx);
            cx.refresh_windows();
            ThemeMode::Dark
        }
        _ => {
            let Some(theme) = find(id) else {
                return apply_by_id(DEFAULT_THEME_ID, window, cx);
            };
            apply_palette(theme, window, cx)
        }
    }
}

fn apply_palette(theme: &DesktopTheme, window: Option<&mut Window>, cx: &mut App) -> ThemeMode {
    Theme::change(theme.mode, window, cx);
    Theme::update(cx, |kit_theme| {
        let colors = &mut kit_theme.colors;
        colors.background = theme.background;
        colors.foreground = theme.foreground;
        colors.border = mix(theme.background, theme.foreground, 0.20);
        colors.muted_foreground = aa_muted(theme.background, theme.foreground);
        // Recessed surfaces move away from the foreground, preserving text contrast.
        let surface = mix(theme.background, filled_button_foreground(theme.foreground), 0.10);
        colors.secondary = surface;
        colors.muted = surface;
        colors.chart_2 = theme.secondary;
        colors.group_box = surface;
        colors.group_box_foreground = theme.foreground;
        colors.sidebar = surface;
        colors.sidebar_foreground = theme.foreground;
        colors.sidebar_border = colors.border;
        colors.sidebar_accent = surface;
        colors.sidebar_accent_foreground = theme.foreground;
        colors.sidebar_primary = theme.accent;
        colors.sidebar_primary_foreground = filled_button_foreground(theme.accent);
        colors.list = theme.background;
        colors.list_hover = surface;
        colors.list_active = surface;
        colors.list_active_border = theme.accent;
        colors.input = colors.border;
        colors.button = surface;
        colors.button_foreground = theme.foreground;
        colors.button_hover = surface;
        colors.button_active = surface;
        colors.button_secondary = surface;
        colors.button_secondary_foreground = theme.foreground;
        colors.button_secondary_hover = surface;
        colors.button_secondary_active = surface;
        colors.secondary_hover = surface;
        colors.secondary_active = surface;
        colors.secondary_foreground = theme.foreground;
        // Kit accent is the neutral hover/selection surface; brand emphasis is primary.
        colors.accent = surface;
        colors.accent_foreground = theme.foreground;
        colors.danger = theme.danger;
        colors.danger_foreground = filled_button_foreground(theme.danger);
        colors.warning = theme.warning;
        colors.popover = surface;
        colors.popover_foreground = theme.foreground;
        colors.primary = theme.accent;
        colors.primary_foreground = filled_button_foreground(theme.accent);
        let contrast_end = filled_button_foreground(colors.primary_foreground);
        colors.primary_hover = mix(theme.accent, contrast_end, 0.08);
        colors.primary_active = mix(theme.accent, contrast_end, 0.16);
        colors.button_primary = colors.primary;
        colors.button_primary_foreground = colors.primary_foreground;
        colors.button_primary_hover = colors.primary_hover;
        colors.button_primary_active = colors.primary_active;
        colors.ring = theme.accent;
        colors.tab_foreground = colors.muted_foreground;
        colors.tab_active_foreground = theme.foreground;
    });
    theme.mode
}

/// Foreground for a filled control: the standard white/black choice at the
/// WCAG equal-contrast crossover (both give the same ratio at luminance
/// 0.179), which stays readable on every saturated accent.
pub(super) fn filled_button_foreground(color: Hsla) -> Hsla {
    const WHITE: Hsla = Hsla { h: 0.0, s: 0.0, l: 1.0, a: 1.0 };
    const BLACK: Hsla = Hsla { h: 0.0, s: 0.0, l: 0.0, a: 1.0 };
    if relative_luminance(color) > 0.179 {
        BLACK
    } else {
        WHITE
    }
}

/// The softest text color that still meets WCAG AA against `background`:
/// the palette foreground blended toward the background just until the
/// 4.5:1 threshold holds. Returns the foreground untouched when even it
/// cannot reach AA (the contrast test then flags the theme).
pub(super) fn aa_muted(background: Hsla, foreground: Hsla) -> Hsla {
    const AA_RATIO: f32 = 4.5;
    let mut low = 0.0_f32;
    let mut high = 1.0_f32;
    if contrast_ratio(foreground, background) < AA_RATIO {
        return foreground;
    }
    for _ in 0..24 {
        let middle = (low + high) / 2.0;
        if contrast_ratio(mix(foreground, background, middle), background) >= AA_RATIO {
            low = middle;
        } else {
            high = middle;
        }
    }
    // Sit just inside the passing range.
    mix(foreground, background, low)
}

/// Linear blend of two colors; `amount` is the share of `upper`.
pub(super) fn mix(lower: Hsla, upper: Hsla, amount: f32) -> Hsla {
    Hsla {
        h: lower.h,
        s: lower.s,
        l: lower.l + (upper.l - lower.l) * amount,
        a: lower.a + (upper.a - lower.a) * amount,
    }
}

#[derive(Clone, Copy)]
pub(super) struct NavigationSurfaces {
    pub(super) rail: Hsla,
    pub(super) panel: Hsla,
}

/// Give navigation a clear secondary layer without changing shared component tokens.
pub(super) fn navigation_surfaces(background: Hsla, secondary: Hsla, foreground: Hsla) -> NavigationSurfaces {
    if relative_luminance(background) < 0.179 {
        let recessed = filled_button_foreground(foreground);
        NavigationSurfaces {
            rail: mix(background, recessed, 0.35),
            panel: mix(background, recessed, 0.55),
        }
    } else {
        NavigationSurfaces {
            rail: secondary,
            panel: mix(background, secondary, 0.85),
        }
    }
}

/// WCAG contrast ratio between two colors.
pub(super) fn contrast_ratio(a: Hsla, b: Hsla) -> f32 {
    let la = relative_luminance(a);
    let lb = relative_luminance(b);
    let (lighter, darker) = if la >= lb { (la, lb) } else { (lb, la) };
    (lighter + 0.05) / (darker + 0.05)
}

fn relative_luminance(color: Hsla) -> f32 {
    let channel = |value: f32| -> f32 {
        if value <= 0.03928 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    };
    let rgba = gpui_kit::Rgba::from(color);
    0.2126 * channel(rgba.r) + 0.7152 * channel(rgba.g) + 0.0722 * channel(rgba.b)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::component::theme::ActiveTheme;

    #[gpui_kit::test]
    fn palettes_synchronize_component_and_base_tokens_with_readable_controls(cx: &mut gpui_kit::TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            for palette in all_themes() {
                let _mode = apply_by_id(palette.id, None, cx);
                let kit = cx.global::<Theme>();
                let base = gpui_kit::base::Theme::global(cx);
                assert_eq!(
                    kit.tokens.primary,
                    gpui_kit::component::theme::ThemeToken::from(palette.accent),
                    "{}",
                    palette.id
                );
                assert_eq!(base.tokens.colors.background, palette.background, "{}", palette.id);
                assert_eq!(base.tokens.colors.primary, palette.accent, "{}", palette.id);
                for foreground in [kit.colors.tab_foreground, kit.colors.tab_active_foreground] {
                    assert!(contrast_ratio(foreground, palette.background) >= 4.5, "{}: unreadable tab", palette.id);
                }
                for background in [kit.colors.primary, kit.colors.primary_hover, kit.colors.primary_active] {
                    assert!(
                        contrast_ratio(kit.colors.primary_foreground, background) >= 4.5,
                        "{}: unreadable primary control",
                        palette.id
                    );
                }
                for (background, foreground) in [
                    (kit.colors.secondary, kit.colors.secondary_foreground),
                    (kit.colors.accent, kit.colors.accent_foreground),
                    (kit.colors.group_box, kit.colors.group_box_foreground),
                    (kit.colors.sidebar, kit.colors.sidebar_foreground),
                    (kit.colors.popover, kit.colors.popover_foreground),
                ] {
                    assert!(
                        contrast_ratio(foreground, background) >= 4.5,
                        "{}: unreadable component surface",
                        palette.id
                    );
                }
                for background in [
                    kit.colors.background,
                    kit.colors.secondary,
                    kit.colors.popover,
                    mix(kit.colors.background, kit.colors.secondary, 0.08),
                ] {
                    assert!(
                        contrast_ratio(kit.colors.muted_foreground, background) >= 4.5,
                        "{}: muted text must stay readable on raised surfaces",
                        palette.id
                    );
                }
                assert_ne!(kit.colors.accent, kit.colors.primary, "brand and hover surfaces must differ");
                assert_eq!(kit.colors.button_primary, kit.colors.primary);
                assert_eq!(kit.colors.button_primary_foreground, kit.colors.primary_foreground);
                assert_eq!(kit.colors.button_primary_hover, kit.colors.primary_hover);
                assert_eq!(kit.colors.button_primary_active, kit.colors.primary_active);
            }
        });
    }

    #[test]
    fn every_theme_keeps_wcag_aa_foreground_contrast() {
        for theme in all_themes() {
            let ratio = contrast_ratio(theme.foreground, theme.background);
            assert!(ratio >= 4.5, "{}: foreground contrast {ratio:.2} is below WCAG AA", theme.id);
            let muted = aa_muted(theme.background, theme.foreground);
            let muted_ratio = contrast_ratio(muted, theme.background);
            assert!(muted_ratio >= 4.5, "{}: muted text contrast {muted_ratio:.2} is below WCAG AA", theme.id);
        }
    }

    #[gpui_kit::test]
    fn native_toolbar_keeps_text_contrast_across_themes(cx: &mut gpui_kit::TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            let ids = [SYSTEM_LIGHT_ID, SYSTEM_DARK_ID]
                .into_iter()
                .chain(all_themes().iter().map(|theme| theme.id));
            for id in ids {
                let _mode = apply_by_id(id, None, cx);
                let colors = cx.theme().colors;
                let toolbar = navigation_surfaces(colors.background, colors.secondary, colors.foreground).rail;
                for (label, foreground) in [
                    ("foreground", colors.foreground),
                    ("toolbar control foreground", colors.secondary_foreground),
                ] {
                    let ratio = contrast_ratio(foreground, toolbar);
                    assert!(ratio >= 4.5, "{id}: native toolbar {label} contrast {ratio:.2} is below WCAG AA");
                }
            }
        });
    }

    #[gpui_kit::test]
    fn navigation_surfaces_are_distinct_and_readable(cx: &mut gpui_kit::TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            let ids = [SYSTEM_LIGHT_ID, SYSTEM_DARK_ID]
                .into_iter()
                .chain(all_themes().iter().map(|theme| theme.id));
            for id in ids {
                let _mode = apply_by_id(id, None, cx);
                let colors = cx.theme().colors;
                let surfaces = navigation_surfaces(colors.background, colors.secondary, colors.foreground);
                let rail = surfaces.rail;
                let panel = surfaces.panel;
                let edge = mix(panel, colors.border, 0.85);
                let workspace_edge = mix(colors.background, colors.border, 0.85);
                let canvas_panel = contrast_ratio(colors.background, panel);
                let canvas_edge = contrast_ratio(colors.background, edge);
                let canvas_shell = contrast_ratio(colors.background, rail);
                let canvas_workspace_edge = contrast_ratio(colors.background, workspace_edge);
                let panel_rail = contrast_ratio(panel, rail);
                let panel_edge = contrast_ratio(panel, edge);

                assert!(
                    canvas_shell >= 1.02 || canvas_workspace_edge >= 1.08,
                    "{id}: workspace top/left edge must contrast with the surrounding shell"
                );
                assert!(
                    canvas_panel >= 1.02 || canvas_edge >= 1.08,
                    "{id}: panel/canvas contrast {canvas_panel:.3}, including border {canvas_edge:.3}"
                );
                assert!(
                    panel_rail >= 1.02 || panel_edge >= 1.08,
                    "{id}: panel/rail contrast {panel_rail:.3}, including border {panel_edge:.3}"
                );
                assert!(
                    contrast_ratio(colors.foreground, panel) >= 4.5,
                    "{id}: navigation text must meet WCAG AA on the panel"
                );
                assert!(
                    contrast_ratio(colors.foreground, rail) >= 4.5,
                    "{id}: navigation text must meet WCAG AA on the rail"
                );
            }
        });
    }

    #[test]
    fn accent_badges_stay_readable() {
        for theme in all_themes() {
            let text = filled_button_foreground(theme.accent);
            let ratio = contrast_ratio(text, theme.accent);
            assert!(ratio >= 3.0, "{}: accent badge contrast {ratio:.2} is too low", theme.id);
        }
    }

    #[test]
    fn theme_ids_are_unique_and_default_exists() {
        let mut ids: Vec<&str> = all_themes().iter().map(|theme| theme.id).collect();
        let count = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), count, "theme ids must be unique");
        assert!(find(DEFAULT_THEME_ID).is_some(), "the default theme must exist");
        assert!(find(SYSTEM_LIGHT_ID).is_none() && find(SYSTEM_DARK_ID).is_none());
    }

    #[test]
    fn theme_modes_have_valid_defaults_and_system_ids() {
        for mode in [ThemeMode::Light, ThemeMode::Dark] {
            assert_eq!(mode_for_id(default_id(mode)), Some(mode));
            assert_eq!(mode_for_id(system_id(mode)), Some(mode));
        }
        assert_eq!(mode_for_id("solarized-light"), Some(ThemeMode::Light));
        assert_eq!(mode_for_id("ciapre"), Some(ThemeMode::Dark));
        assert_eq!(mode_for_id("invalid-theme"), None);
    }

    #[test]
    fn contrast_ratio_matches_known_values() {
        let black = Hsla { h: 0.0, s: 0.0, l: 0.0, a: 1.0 };
        let white = Hsla { h: 0.0, s: 0.0, l: 1.0, a: 1.0 };
        assert!((contrast_ratio(black, white) - 21.0).abs() < 0.1);
        assert!((contrast_ratio(black, black) - 1.0).abs() < 0.01);
    }
}
