//! VT Code desktop themes: the TUI theme registry rendered on the gpui-kit
//! tokens, with Ciapre as the app default.
//!
//! Palette rows come from `vtcode_ui::theme` — the longbridge/gpui-kit ports
//! are shared with the CLI registry — so a theme id looks the same in the
//! terminal and in the desktop client.
//!
//! Each theme overrides the core kit colors after `Theme::change` installs
//! the matching light/dark base; derived tokens (border, panels, muted text)
//! are mixed from the palette so every theme stays legible by construction.

use gpui_kit::component::theme::{Theme, ThemeMode};
use gpui_kit::{App, Hsla, Window, rgb};
use vtcode_ui::theme::{self, UiThemeColors};

/// The theme applied on first launch.
pub(super) const DEFAULT_THEME_ID: &str = "ciapre";

/// Sentinel ids that keep the kit's own light/dark base without overrides.
pub(super) const SYSTEM_LIGHT_ID: &str = "system-light";
pub(super) const SYSTEM_DARK_ID: &str = "system-dark";

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

/// Build one desktop palette row. The kit's color constructors are not
/// `const`, so rows are assembled at first use.
fn theme(
    id: &'static str,
    label: &'static str,
    mode: ThemeMode,
    [background, foreground, accent, secondary, danger, warning]: [u32; 6],
) -> DesktopTheme {
    DesktopTheme {
        id,
        label,
        mode,
        background: rgb(background).into(),
        foreground: rgb(foreground).into(),
        accent: rgb(accent).into(),
        secondary: rgb(secondary).into(),
        danger: rgb(danger).into(),
        warning: rgb(warning).into(),
    }
}

/// Convert a shared UI palette (already contrast-checked by the CLI registry)
/// into a desktop row.
fn ported(spec: (&'static str, bool, UiThemeColors)) -> DesktopTheme {
    let (id, dark, colors) = spec;
    theme(
        id,
        theme::gpui_kit_theme_label(id).unwrap_or(id),
        if dark { ThemeMode::Dark } else { ThemeMode::Light },
        [
            theme::rgb_u32(colors.background),
            theme::rgb_u32(colors.foreground),
            theme::rgb_u32(colors.accent),
            theme::rgb_u32(colors.secondary),
            theme::rgb_u32(colors.danger),
            theme::rgb_u32(colors.warning),
        ],
    )
}

/// Resolve a theme id; the sentinel system ids map to the kit defaults and
/// unknown ids fall back to the VT Code default.
pub(super) fn find(id: &str) -> Option<&'static DesktopTheme> {
    all_themes().iter().find(|theme| theme.id == id)
}

/// The full palette table: every ported gpui-kit theme plus the desktop-only
/// curated rows, built once because the kit's color constructors are not
/// `const`.
pub(super) fn all_themes() -> &'static [DesktopTheme] {
    static THEMES: std::sync::LazyLock<Vec<DesktopTheme>> = std::sync::LazyLock::new(build_themes);
    &THEMES
}

fn build_themes() -> Vec<DesktopTheme> {
    // Ported rows come first: they carry the shared upstream colors plus the
    // contrast-checked semantic tokens. Curated rows fill in the desktop-only
    // themes the CLI registry does not define.
    let mut themes: Vec<DesktopTheme> = theme::gpui_kit_theme_colors_all().map(ported).collect();
    for theme in curated_themes() {
        if !themes.iter().any(|ported| ported.id == theme.id) {
            themes.push(theme);
        }
    }
    themes
}

/// Desktop-only palettes, kept for ids the shared theme registry omits.
fn curated_themes() -> Vec<DesktopTheme> {
    vec![
        theme("ciapre", "Ciapre", ThemeMode::Dark, [0x181818, 0xAEA47F, 0xCC8A3E, 0xAEA47F, 0xC16A68, 0xCC8A3E]),
        theme(
            "ciapre-dark",
            "Ciapre Dark",
            ThemeMode::Dark,
            [0x262626, 0xBFB38F, 0xD99A4E, 0xBFB38F, 0xFF8A8A, 0xD99A4E],
        ),
        theme(
            "ciapre-blue",
            "Ciapre Blue",
            ThemeMode::Dark,
            [0x171C26, 0xBFB38F, 0xD99A4E, 0xBFB38F, 0xFF8A8A, 0xD99A4E],
        ),
        theme(
            "solarized-dark",
            "Solarized Dark",
            ThemeMode::Dark,
            [0x002B36, 0x839496, 0x3A95D6, 0x839496, 0xE56765, 0xB58900],
        ),
        theme(
            "solarized-light",
            "Solarized Light",
            ThemeMode::Light,
            [0xFDF6E3, 0x586E75, 0x2073AE, 0x586E75, 0xD3302D, 0xB58900],
        ),
        theme(
            "solarized-dark-hc",
            "Solarized Dark HC",
            ThemeMode::Dark,
            [0x002831, 0xE9E3CC, 0x4A90D2, 0xE9E3CC, 0xE0656A, 0xB58900],
        ),
        theme(
            "gruvbox-dark",
            "Gruvbox Dark",
            ThemeMode::Dark,
            [0x282828, 0xA89984, 0xD79921, 0xA89984, 0xDE716C, 0xD79921],
        ),
        theme(
            "gruvbox-light",
            "Gruvbox Light",
            ThemeMode::Light,
            [0xFBF4E8, 0x786C61, 0x986311, 0x786C61, 0x9D0006, 0x986311],
        ),
        theme("dracula", "Dracula", ThemeMode::Dark, [0x282A36, 0xF8F8F2, 0xFFB86C, 0x8BE9FD, 0xFF5858, 0xFFB86C]),
        theme(
            "github-dark",
            "GitHub Dark",
            ThemeMode::Dark,
            [0x0D1117, 0xC9D1D9, 0x58A6FF, 0xD29922, 0xF85149, 0xD29922],
        ),
        theme(
            "github",
            "GitHub Light",
            ThemeMode::Light,
            [0xFFFFFF, 0x24292E, 0x0366D6, 0x24292E, 0xD53948, 0x907000],
        ),
        theme(
            "atom-one-dark",
            "Atom One Dark",
            ThemeMode::Dark,
            [0x282C34, 0xABB2BF, 0x61AFEF, 0xE5C07B, 0xE2737C, 0xE5C07B],
        ),
        theme(
            "atom-one-light",
            "Atom One Light",
            ThemeMode::Light,
            [0xFAFAFA, 0x383A42, 0x3E70C3, 0x383A42, 0xC2493E, 0x986200],
        ),
        theme(
            "tomorrow-night",
            "Tomorrow Night",
            ThemeMode::Dark,
            [0x1D1F21, 0xC5C8C6, 0xDE935F, 0x81A2BE, 0xCE6B6B, 0xDE935F],
        ),
        theme(
            "catppuccin-latte",
            "Catppuccin Latte",
            ThemeMode::Light,
            [0xEFF1F5, 0x4C4F69, 0x1E66F5, 0x8839EF, 0xD20F39, 0xFE640B],
        ),
        theme(
            "catppuccin-mocha",
            "Catppuccin Mocha",
            ThemeMode::Dark,
            [0x1E1E2E, 0xCDD6F4, 0x89B4FA, 0xCBA6F7, 0xF38BA8, 0xFAB387],
        ),
    ]
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

/// Keep the navigation shell visually distinct from the inset workspace.
/// Dark palettes recess the shell and sidebar; light palettes use the shared
/// secondary surface without changing component colors.
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
                let edge = mix(surfaces.panel, colors.border, 0.85);
                let workspace_edge = mix(colors.background, colors.border, 0.85);

                assert!(
                    contrast_ratio(colors.background, surfaces.rail) >= 1.02
                        || contrast_ratio(colors.background, workspace_edge) >= 1.08,
                    "{id}: shell must contrast with the workspace top and left edges"
                );
                assert!(
                    contrast_ratio(colors.background, surfaces.panel) >= 1.02
                        || contrast_ratio(colors.background, edge) >= 1.08,
                    "{id}: sidebar must contrast with its surrounding workspace"
                );
                assert!(
                    contrast_ratio(surfaces.panel, surfaces.rail) >= 1.02
                        || contrast_ratio(surfaces.panel, edge) >= 1.08,
                    "{id}: sidebar must remain distinct from the navigation rail"
                );
                assert!(
                    contrast_ratio(colors.foreground, surfaces.panel) >= 4.5,
                    "{id}: sidebar text must meet WCAG AA"
                );
                assert!(contrast_ratio(colors.foreground, surfaces.rail) >= 4.5, "{id}: shell text must meet WCAG AA");
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
    fn every_cli_gpui_kit_theme_is_offered_in_the_desktop_picker() {
        let desktop_ids: Vec<&str> = all_themes().iter().map(|theme| theme.id).collect();
        for (id, dark, _colors) in theme::gpui_kit_theme_colors_all() {
            let found = find(id).unwrap_or_else(|| panic!("{id} missing from the desktop picker"));
            assert_eq!(matches!(found.mode, ThemeMode::Dark), dark, "{id} must keep the upstream light/dark mode");
        }
        // Desktop-only curated rows stay available alongside the ported set.
        assert!(desktop_ids.contains(&"dracula"));
    }

    #[test]
    fn ported_rows_use_the_shared_palette_colors() {
        for (id, _dark, colors) in theme::gpui_kit_theme_colors_all() {
            let row = find(id).unwrap_or_else(|| panic!("{id} missing"));
            for (field, expected, actual) in [
                ("background", colors.background, row.background),
                ("foreground", colors.foreground, row.foreground),
                ("accent", colors.accent, row.accent),
                ("secondary", colors.secondary, row.secondary),
                ("danger", colors.danger, row.danger),
                ("warning", colors.warning, row.warning),
            ] {
                assert_eq!(rgb_hex(actual), theme::rgb_u32(expected), "{id}: {field} must match the shared palette");
            }
            assert_eq!(row.label, theme::gpui_kit_theme_label(id).unwrap_or(id), "{id} label");
        }
    }

    /// Convert a GPUI color back to `0xRRGGBB` for palette comparisons.
    ///
    /// Rows are stored from 8-bit channels, so each component round-trips
    /// through the same 0..=255 scale; the float clamp keeps the cast exact.
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "clamped to 0..=255"
    )]
    fn rgb_hex(color: Hsla) -> u32 {
        let rgba = gpui_kit::Rgba::from(color);
        let channel = |value: f32| u32::from((value.clamp(0.0, 1.0) * 255.0).round() as u8);
        (channel(rgba.r) << 16) | (channel(rgba.g) << 8) | channel(rgba.b)
    }

    #[test]
    fn contrast_ratio_matches_known_values() {
        let black = Hsla { h: 0.0, s: 0.0, l: 0.0, a: 1.0 };
        let white = Hsla { h: 0.0, s: 0.0, l: 1.0, a: 1.0 };
        assert!((contrast_ratio(black, white) - 21.0).abs() < 0.1);
        assert!((contrast_ratio(black, black) - 1.0).abs() < 0.01);
    }
}
