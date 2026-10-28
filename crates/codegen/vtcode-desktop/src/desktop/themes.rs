//! VT Code desktop themes: the TUI theme registry rendered on the gpui-kit
//! tokens, with Ciapre as the app default.
//!
//! Palette rows come from the shared CLI theme registry. Desktop adjusts
//! foreground contrast for its raised surfaces while retaining palette hues.
//!
//! Each theme overrides the core kit colors after `Theme::change` installs
//! the matching light/dark base; derived tokens (border, panels, muted text)
//! are mixed from the palette so every theme stays legible by construction.

use gpui_kit::component::theme::{Theme, ThemeColor, ThemeMode};
use gpui_kit::{App, Hsla, Window, rgb};
use vtcode_ui::theme::{self, UiThemeColors};

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
    /// Semantic addition color used by diff views.
    pub success: Hsla,
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
    let background = rgb(background).into();
    let foreground = rgb(foreground).into();
    let success_seed = rgb(match mode {
        ThemeMode::Dark => 0x63C779,
        ThemeMode::Light => 0x237A45,
    })
    .into();
    DesktopTheme {
        id,
        label,
        mode,
        background,
        foreground,
        accent: rgb(accent).into(),
        secondary: rgb(secondary).into(),
        success: readable_semantic_color(background, foreground, success_seed),
        danger: readable_semantic_color(background, foreground, rgb(danger).into()),
        warning: rgb(warning).into(),
    }
}

/// Convert a shared UI palette (already contrast-checked by the CLI registry)
/// into a desktop row.
fn ported(spec: (&'static str, bool, UiThemeColors)) -> DesktopTheme {
    let (id, dark, colors) = spec;
    let mut theme = theme(
        id,
        theme::theme_label(id).unwrap_or(id),
        if dark { ThemeMode::Dark } else { ThemeMode::Light },
        [
            theme::rgb_u32(colors.background),
            theme::rgb_u32(colors.foreground),
            theme::rgb_u32(colors.accent),
            theme::rgb_u32(colors.secondary),
            theme::rgb_u32(colors.danger),
            theme::rgb_u32(colors.warning),
        ],
    );
    theme.success =
        readable_semantic_color(theme.background, theme.foreground, rgb(theme::rgb_u32(colors.success)).into());
    theme
}

/// Resolve a theme id; the sentinel system ids map to the kit defaults and
/// unknown ids fall back to the VT Code default.
pub(super) fn find(id: &str) -> Option<&'static DesktopTheme> {
    all_themes().iter().find(|theme| theme.id == id)
}

/// The full CLI palette table, built once because the kit's color constructors
/// are not `const`.
pub(super) fn all_themes() -> &'static [DesktopTheme] {
    static THEMES: std::sync::LazyLock<Vec<DesktopTheme>> = std::sync::LazyLock::new(build_themes);
    &THEMES
}

fn build_themes() -> Vec<DesktopTheme> {
    theme::available_themes()
        .into_iter()
        .filter_map(|id| theme::builtin_theme_colors(id).map(|colors| ported((id, !theme::is_light_theme(id), colors))))
        .collect()
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

const FEATURED_THEME_IDS: [&str; 3] = ["ciapre", "ciapre-dark", "ciapre-blue"];

/// Picker order leads with the VT Code themes, then system bases and the
/// remaining shared palette catalog in its existing order.
pub(super) fn picker_swatches() -> impl Iterator<Item = SwatchEntry> {
    let themes = all_themes();
    FEATURED_THEME_IDS
        .iter()
        .filter_map(|id| themes.iter().find(|theme| theme.id == *id))
        .copied()
        .map(SwatchEntry::Palette)
        .chain(SYSTEM_SWATCHES)
        .chain(
            themes
                .iter()
                .copied()
                .filter(|theme| !FEATURED_THEME_IDS.contains(&theme.id))
                .map(SwatchEntry::Palette),
        )
}

/// Apply a theme by id: install the kit base for its mode, then override the
/// core tokens from the palette.
pub(super) fn apply_by_id(id: &str, window: Option<&mut Window>, cx: &mut App) -> ThemeMode {
    match id {
        SYSTEM_LIGHT_ID => {
            Theme::change(ThemeMode::Light, window, cx);
            apply_system_surface_borders(cx);
            normalize_system_interaction_colors(cx);
            cx.refresh_windows();
            ThemeMode::Light
        }
        SYSTEM_DARK_ID => {
            Theme::change(ThemeMode::Dark, window, cx);
            apply_system_surface_borders(cx);
            normalize_system_interaction_colors(cx);
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
        apply_surface_borders(colors);
        apply_filled_surfaces(colors);
        colors.chart_2 = theme.secondary;
        colors.sidebar_primary = theme.accent;
        colors.sidebar_primary_foreground = filled_button_foreground(theme.accent);
        colors.list = theme.background;
        colors.list_active_border = theme.accent;
        colors.success = theme.success;
        colors.success_foreground = filled_button_foreground(theme.success);
        colors.danger = theme.danger;
        colors.danger_foreground = filled_button_foreground(theme.danger);
        colors.warning = theme.warning;
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
        apply_unified_hover_surface(colors, hover_surface(theme.background, theme.foreground));
        colors.ring = accessible_focus_color(theme.accent, colors);
    });
    theme.mode
}

/// Keep keyboard focus visible while retaining brand color when it has enough contrast.
fn accessible_focus_color(accent: Hsla, colors: &ThemeColor) -> Hsla {
    let surfaces = [colors.background, colors.secondary, colors.popover];
    if surfaces.iter().all(|surface| contrast_ratio(accent, *surface) >= 3.0) {
        accent
    } else {
        colors.foreground
    }
}

fn normalize_system_interaction_colors(cx: &mut App) {
    Theme::update(cx, |kit_theme| {
        let colors = &mut kit_theme.colors;
        apply_filled_surfaces(colors);
        let surface = hover_surface(colors.background, colors.foreground);
        apply_unified_hover_surface(colors, surface);
        colors.success = readable_semantic_color(colors.background, colors.foreground, colors.success);
        colors.danger = readable_semantic_color(colors.background, colors.foreground, colors.danger);
        colors.success_foreground = filled_button_foreground(colors.success);
        colors.danger_foreground = filled_button_foreground(colors.danger);
        colors.ring = accessible_focus_color(colors.primary, colors);
    });
}

fn apply_system_surface_borders(cx: &mut App) {
    Theme::update(cx, |kit_theme| apply_surface_borders(&mut kit_theme.colors));
}

/// Keep decorative Kit outlines invisible; focus and validation use semantic colors.
fn apply_surface_borders(colors: &mut ThemeColor) {
    colors.border = colors.background.opacity(0.);
    colors.sidebar_border = colors.border;
    colors.input = colors.background.opacity(0.);
}

/// Give panels and controls a visible neutral fill while keeping their text at
/// WCAG AA. Popovers and input fields use a stronger layer than resting panels.
fn apply_filled_surfaces(colors: &mut ThemeColor) {
    let surface = filled_surface(colors.background, colors.foreground, 10);
    let raised_surface = filled_surface(colors.background, colors.foreground, 24);

    colors.secondary = surface;
    colors.secondary_foreground = colors.foreground;
    colors.muted = surface;
    colors.group_box = surface;
    colors.group_box_foreground = colors.foreground;
    colors.sidebar = surface;
    colors.sidebar_foreground = colors.foreground;
    colors.muted_foreground = aa_muted(raised_surface, colors.foreground);
    colors.list_active = surface;
    colors.button = surface;
    colors.button_foreground = colors.foreground;
    colors.button_active = surface;
    colors.button_secondary = surface;
    colors.button_secondary_foreground = colors.foreground;
    colors.button_secondary_active = surface;
    colors.secondary_active = surface;
    colors.popover = raised_surface;
    colors.popover_foreground = colors.foreground;
    // Kit's segmented TabBar otherwise retains its stock track when VT Code
    // replaces the palette, making selected tabs blend into other themes.
    colors.tab_bar_segmented = colors.secondary;
    colors.tab_foreground = text_at_contrast(raised_surface, colors.foreground, LABEL_CONTRAST);
    colors.tab_active_foreground = colors.foreground;
}

fn filled_surface(background: Hsla, foreground: Hsla, maximum_step: u8) -> Hsla {
    for step in (4_u8..=maximum_step).rev() {
        let candidate = mix(background, foreground, f32::from(step) / 100.0);
        if contrast_ratio(foreground, candidate) >= 4.5 && contrast_ratio(background, candidate) >= 1.04 {
            return candidate;
        }
    }
    background
}

fn hover_surface(background: Hsla, foreground: Hsla) -> Hsla {
    for step in (4_u8..=18).rev() {
        let candidate = mix(background, foreground, f32::from(step) / 100.0);
        if contrast_ratio(foreground, candidate) >= 4.5 {
            return candidate;
        }
    }
    background
}

fn apply_unified_hover_surface(colors: &mut ThemeColor, surface: Hsla) {
    // Keep the same soft neutral hover across buttons, list rows, and selected
    // command items; the brand accent remains reserved for primary actions.
    colors.accent = surface;
    colors.accent_foreground = colors.foreground;
    colors.button_hover = surface;
    colors.button_secondary_hover = surface;
    colors.secondary_hover = surface;
    colors.list_hover = surface;
    colors.sidebar_accent = surface;
    colors.sidebar_accent_foreground = colors.foreground;
}

pub(super) fn readable_semantic_color(background: Hsla, foreground: Hsla, color: Hsla) -> Hsla {
    const AA_RATIO: f32 = 4.5;
    adjust_color_until_readable(background, foreground, color, |candidate| {
        diff_marker_is_readable(background, candidate, AA_RATIO)
    })
}

pub(super) fn readable_indicator_color(background: Hsla, foreground: Hsla, color: Hsla) -> Hsla {
    const AA_RATIO: f32 = 4.5;
    adjust_color_until_readable(background, foreground, color, |candidate| {
        contrast_ratio(candidate, background) >= AA_RATIO
    })
}

fn adjust_color_until_readable(
    background: Hsla,
    foreground: Hsla,
    color: Hsla,
    is_readable: impl Fn(Hsla) -> bool,
) -> Hsla {
    if is_readable(color) {
        return color;
    }
    let lighten = relative_luminance(foreground) > relative_luminance(background);
    for step in 1_u8..=100 {
        let amount = f32::from(step) / 100.0;
        let lightness = if lighten {
            color.l + (1.0 - color.l) * amount
        } else {
            color.l * (1.0 - amount)
        };
        let candidate = Hsla { l: lightness, ..color };
        if is_readable(candidate) {
            return candidate;
        }
    }
    foreground
}

fn diff_marker_is_readable(background: Hsla, marker: Hsla, minimum_contrast: f32) -> bool {
    contrast_ratio(marker, background) >= minimum_contrast
        && contrast_ratio(marker, composite_status_surface(background, marker, 0.12)) >= minimum_contrast
}

/// Opaque, hue-tinted line surface with as much color as the foreground can
/// retain while meeting WCAG AA. The strong marker color carries the semantic
/// distinction when a palette has little contrast headroom for a larger tint.
pub(super) fn diff_line_background(background: Hsla, foreground: Hsla, status: Hsla) -> Hsla {
    for step in (8_u8..=22).rev() {
        let opacity = f32::from(step) / 100.0;
        let candidate = composite_status_surface(background, status, opacity);
        if contrast_ratio(foreground, candidate) >= 4.5 && contrast_ratio(status, candidate) >= 4.5 {
            return candidate;
        }
    }
    background
}

pub(super) fn diff_text_foreground(background: Hsla, foreground: Hsla, addition: Hsla, deletion: Hsla) -> Hsla {
    const AA_RATIO: f32 = 4.5;
    let statuses = [addition, deletion];
    let readable = |candidate: Hsla| {
        contrast_ratio(candidate, background) >= AA_RATIO
            && statuses.iter().all(|status| {
                contrast_ratio(candidate, composite_status_surface(background, *status, 0.22)) >= AA_RATIO
            })
    };
    if readable(foreground) {
        return foreground;
    }

    let lighten = relative_luminance(foreground) > relative_luminance(background);
    for step in 1_u8..=100 {
        let amount = f32::from(step) / 100.0;
        let lightness = if lighten {
            foreground.l + (1.0 - foreground.l) * amount
        } else {
            foreground.l * (1.0 - amount)
        };
        let candidate = Hsla { l: lightness, ..foreground };
        if readable(candidate) {
            return candidate;
        }
    }
    foreground
}

fn composite_status_surface(background: Hsla, status: Hsla, opacity: f32) -> Hsla {
    let background = gpui_kit::Rgba::from(background);
    let status = gpui_kit::Rgba::from(status);
    gpui_kit::Rgba {
        r: background.r * (1.0 - opacity) + status.r * opacity,
        g: background.g * (1.0 - opacity) + status.g * opacity,
        b: background.b * (1.0 - opacity) + status.b * opacity,
        a: 1.0,
    }
    .into()
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

/// Minimum contrast for label-grade text: headings, tab labels, and section
/// overlines sit between primary foreground and muted metadata.
const LABEL_CONTRAST: f32 = 7.0;

/// Label-grade text readable on every filled surface: the palette foreground
/// blended toward the hardest surface (the raised popover fill) just until the
/// 7:1 threshold holds. Background, panel, sidebar, and hover fills are all
/// farther from the foreground, so the ratio only grows there. Returns the
/// foreground untouched when even it cannot reach the threshold.
pub(super) fn label_foreground(colors: &ThemeColor) -> Hsla {
    text_at_contrast(colors.popover, colors.foreground, LABEL_CONTRAST)
}

/// The softest text color that still meets WCAG AA against `background`:
/// the palette foreground blended toward the background just until the
/// 4.5:1 threshold holds. Returns the foreground untouched when even it
/// cannot reach AA (the contrast test then flags the theme).
pub(super) fn aa_muted(background: Hsla, foreground: Hsla) -> Hsla {
    const AA_RATIO: f32 = 4.5;
    text_at_contrast(background, foreground, AA_RATIO)
}

fn text_at_contrast(background: Hsla, foreground: Hsla, minimum: f32) -> Hsla {
    let mut low = 0.0_f32;
    let mut high = 1.0_f32;
    if contrast_ratio(foreground, background) < minimum {
        return foreground;
    }
    for _ in 0..24 {
        let middle = (low + high) / 2.0;
        if contrast_ratio(mix(foreground, background, middle), background) >= minimum {
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
    let surfaces = if relative_luminance(background) < 0.179 {
        let recessed = filled_button_foreground(foreground);
        NavigationSurfaces {
            rail: mix(background, recessed, 0.35),
            panel: mix(background, recessed, 0.55),
        }
    } else {
        NavigationSurfaces {
            rail: secondary,
            panel: mix(background, secondary, 0.55),
        }
    };
    let rail = if contrast_ratio(background, surfaces.rail) < 1.02 {
        filled_surface(background, foreground, 10)
    } else {
        surfaces.rail
    };
    let panel = if contrast_ratio(rail, surfaces.panel) < 1.02 || contrast_ratio(background, surfaces.panel) < 1.02 {
        filled_surface(background, foreground, 24)
    } else {
        surfaces.panel
    };
    NavigationSurfaces { rail, panel }
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
                assert_eq!(kit.colors.border.a, 0.);
                assert_eq!(kit.colors.sidebar_border, kit.colors.border, "{}", palette.id);
                assert_eq!(kit.colors.input.a, 0.);
                assert_eq!(kit.colors.tab_bar_segmented, kit.colors.secondary, "{}", palette.id);
                assert!(
                    contrast_ratio(kit.colors.background, kit.colors.tab_bar_segmented) >= 1.04,
                    "{}: selected segmented tabs must stand out from the track",
                    palette.id
                );
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

    #[gpui_kit::test]
    fn system_themes_hide_decorative_surface_borders(cx: &mut gpui_kit::TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            for id in [SYSTEM_LIGHT_ID, SYSTEM_DARK_ID] {
                let _mode = apply_by_id(id, None, cx);
                let colors = cx.theme().colors;
                assert_eq!(colors.border.a, 0., "{id}");
                assert_eq!(colors.sidebar_border, colors.border, "{id}");
                assert_eq!(colors.input.a, 0., "{id}");
                assert_eq!(colors.tab_bar_segmented, colors.secondary, "{id}");
                assert!(
                    contrast_ratio(colors.background, colors.tab_bar_segmented) >= 1.04,
                    "{id}: selected segmented tabs must stand out from the track"
                );
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
    fn focus_indicators_remain_visible_on_interactive_surfaces(cx: &mut gpui_kit::TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            let ids = [SYSTEM_LIGHT_ID, SYSTEM_DARK_ID]
                .into_iter()
                .chain(all_themes().iter().map(|theme| theme.id));
            for id in ids {
                let _mode = apply_by_id(id, None, cx);
                let colors = cx.theme().colors;
                for (name, surface) in [
                    ("canvas", colors.background),
                    ("panel", colors.secondary),
                    ("popover", colors.popover),
                ] {
                    let ratio = contrast_ratio(colors.ring, surface);
                    assert!(ratio >= 3.0, "{id}: focus indicator contrast on {name} is {ratio:.2}");
                }
            }
        });
    }

    #[gpui_kit::test]
    fn diff_colors_and_line_surfaces_are_readable_in_every_theme(cx: &mut gpui_kit::TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            let ids = [SYSTEM_LIGHT_ID, SYSTEM_DARK_ID]
                .into_iter()
                .chain(all_themes().iter().map(|theme| theme.id));
            for id in ids {
                let _mode = apply_by_id(id, None, cx);
                let colors = cx.theme().colors;
                for (kind, status, status_foreground) in [
                    ("addition", colors.success, colors.success_foreground),
                    ("deletion", colors.danger, colors.danger_foreground),
                ] {
                    let status_ratio = contrast_ratio(status, colors.background);
                    assert!(status_ratio >= 4.5, "{id}: {kind} marker contrast {status_ratio:.2}");
                    assert!(
                        contrast_ratio(status_foreground, status) >= 4.5,
                        "{id}: {kind} filled status contrast is below WCAG AA"
                    );
                    let diff_foreground =
                        diff_text_foreground(colors.background, colors.foreground, colors.success, colors.danger);
                    let kit_line_background = composite_status_surface(colors.background, status, 0.12);
                    let kit_text_ratio = contrast_ratio(diff_foreground, kit_line_background);
                    assert!(kit_text_ratio >= 4.5, "{id}: {kind} Kit diff text contrast {kit_text_ratio:.2}");
                    let kit_marker_ratio = contrast_ratio(status, kit_line_background);
                    assert!(kit_marker_ratio >= 4.5, "{id}: {kind} Kit diff marker contrast {kit_marker_ratio:.2}");
                    let line_background = diff_line_background(colors.background, diff_foreground, status);
                    let line_ratio = contrast_ratio(diff_foreground, line_background);
                    assert!(line_ratio >= 4.5, "{id}: {kind} diff text contrast {line_ratio:.2}");
                    let marker_ratio = contrast_ratio(status, line_background);
                    assert!(marker_ratio >= 4.5, "{id}: {kind} marker on diff surface contrast {marker_ratio:.2}");
                }
            }
        });
    }

    #[gpui_kit::test]
    fn hover_surfaces_are_unified_and_contrast_safe_in_every_theme(cx: &mut gpui_kit::TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            let ids = [SYSTEM_LIGHT_ID, SYSTEM_DARK_ID]
                .into_iter()
                .chain(all_themes().iter().map(|theme| theme.id));
            for id in ids {
                let _mode = apply_by_id(id, None, cx);
                let colors = cx.theme().colors;
                let hover = hover_surface(colors.background, colors.foreground);
                for (name, actual) in [
                    ("accent", colors.accent),
                    ("button", colors.button_hover),
                    ("secondary button", colors.button_secondary_hover),
                    ("secondary", colors.secondary_hover),
                    ("list", colors.list_hover),
                    ("sidebar", colors.sidebar_accent),
                ] {
                    assert_eq!(actual, hover, "{id}: {name} hover uses the shared surface");
                }
                let text_ratio = contrast_ratio(colors.foreground, hover);
                assert!(text_ratio >= 4.5, "{id}: hover text contrast {text_ratio:.2}");
                assert!(
                    contrast_ratio(colors.background, hover) >= 1.01,
                    "{id}: hover surface is too close to the surrounding background"
                );
                assert_eq!(
                    (hover.l - colors.background.l).signum(),
                    (colors.foreground.l - colors.background.l).signum(),
                    "{id}: hover should move gently toward the foreground"
                );
            }
        });
    }

    #[gpui_kit::test]
    fn filled_surfaces_are_distinct_and_readable_in_every_theme(cx: &mut gpui_kit::TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            let ids = [SYSTEM_LIGHT_ID, SYSTEM_DARK_ID]
                .into_iter()
                .chain(all_themes().iter().map(|theme| theme.id));
            for id in ids {
                let _mode = apply_by_id(id, None, cx);
                let colors = cx.theme().colors;
                for (name, surface, foreground) in [
                    ("panel", colors.secondary, colors.secondary_foreground),
                    ("group box", colors.group_box, colors.group_box_foreground),
                    ("sidebar", colors.sidebar, colors.sidebar_foreground),
                    ("popover and input", colors.popover, colors.popover_foreground),
                ] {
                    assert!(
                        contrast_ratio(colors.background, surface) >= 1.04,
                        "{id}: {name} fill is too close to the canvas"
                    );
                    assert!(contrast_ratio(foreground, surface) >= 4.5, "{id}: {name} text contrast is below WCAG AA");
                    assert!(
                        contrast_ratio(colors.muted_foreground, surface) >= 4.5,
                        "{id}: muted {name} text contrast is below WCAG AA"
                    );
                }
                assert!(
                    contrast_ratio(colors.muted_foreground, colors.background) >= 4.5,
                    "{id}: muted text contrast is below WCAG AA on the canvas"
                );
                assert!(
                    contrast_ratio(colors.secondary, colors.popover) >= 1.01,
                    "{id}: input/popover fill should be distinct from panels"
                );
            }
        });
    }

    #[gpui_kit::test]
    fn label_grade_text_stays_crisp_on_every_surface(cx: &mut gpui_kit::TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            let ids = [SYSTEM_LIGHT_ID, SYSTEM_DARK_ID]
                .into_iter()
                .chain(all_themes().iter().map(|theme| theme.id));
            for id in ids {
                let _mode = apply_by_id(id, None, cx);
                let colors = cx.theme().colors;
                let sidebar = navigation_surfaces(colors.background, colors.secondary, colors.foreground).panel;
                // Label-grade text targets 7:1, falling back to the palette
                // foreground itself when a theme has no headroom above it.
                for (name, surface) in [
                    ("canvas", colors.background),
                    ("panel", colors.secondary),
                    ("popover", colors.popover),
                    ("sidebar", sidebar),
                ] {
                    let foreground_ratio = contrast_ratio(colors.foreground, surface);
                    for (kind, text) in [
                        ("section label", label_foreground(&colors)),
                        ("inactive tab", colors.tab_foreground),
                    ] {
                        let ratio = contrast_ratio(text, surface);
                        assert!(
                            ratio >= LABEL_CONTRAST || ratio >= foreground_ratio - 0.01,
                            "{id}: {kind} contrast on {name} is {ratio:.2} (foreground {foreground_ratio:.2})"
                        );
                    }
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
                assert!(
                    contrast_ratio(colors.background, surfaces.rail) >= 1.02,
                    "{id}: shell must contrast with the workspace top and left edges"
                );
                assert!(
                    contrast_ratio(colors.background, surfaces.panel) >= 1.02,
                    "{id}: sidebar must contrast with its surrounding workspace"
                );
                assert!(
                    contrast_ratio(surfaces.panel, surfaces.rail) >= 1.02,
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
    fn every_cli_theme_is_offered_in_the_desktop_picker() {
        let desktop_ids: Vec<&str> = all_themes().iter().map(|theme| theme.id).collect();
        for id in theme::available_themes() {
            let found = find(id).unwrap_or_else(|| panic!("{id} missing from the desktop picker"));
            assert_eq!(matches!(found.mode, ThemeMode::Light), theme::is_light_theme(id), "{id} mode");
            assert!(desktop_ids.contains(&id));
        }
        assert_eq!(desktop_ids.len(), theme::available_themes().len());
    }

    #[test]
    fn ported_rows_use_the_shared_palette_colors() {
        for id in theme::available_themes() {
            let colors = theme::builtin_theme_colors(id).expect("registered theme palette");
            let row = find(id).unwrap_or_else(|| panic!("{id} missing"));
            let background = rgb(theme::rgb_u32(colors.background)).into();
            let foreground = rgb(theme::rgb_u32(colors.foreground)).into();
            for (field, expected, actual) in [
                ("background", colors.background, row.background),
                ("foreground", colors.foreground, row.foreground),
                ("accent", colors.accent, row.accent),
                ("secondary", colors.secondary, row.secondary),
                ("warning", colors.warning, row.warning),
            ] {
                assert_eq!(rgb_hex(actual), theme::rgb_u32(expected), "{id}: {field} must match the shared palette");
            }
            assert_eq!(
                row.success,
                readable_semantic_color(background, foreground, rgb(theme::rgb_u32(colors.success)).into()),
                "{id}: addition color must use the shared semantic color at readable contrast"
            );
            assert_eq!(
                row.danger,
                readable_semantic_color(background, foreground, rgb(theme::rgb_u32(colors.danger)).into()),
                "{id}: deletion color must use the shared semantic color at readable contrast"
            );
            assert_eq!(row.label, theme::theme_label(id).unwrap_or(id), "{id} label");
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
    fn contrast_blend_covers_fallback_and_blend_arms_in_both_directions() {
        let black = Hsla { h: 0.0, s: 0.0, l: 0.0, a: 1.0 };
        let white = Hsla { h: 0.0, s: 0.0, l: 1.0, a: 1.0 };
        // Below the threshold the foreground passes through untouched.
        let faint = Hsla { h: 0.0, s: 0.0, l: 0.08, a: 1.0 };
        assert!(contrast_ratio(faint, black) < 4.5);
        assert_eq!(aa_muted(black, faint), faint);
        // Above it both theme directions blend toward the background while
        // holding the threshold.
        for (background, foreground) in [(black, white), (white, black)] {
            let muted = aa_muted(background, foreground);
            assert_ne!(muted, foreground);
            assert!(contrast_ratio(muted, background) >= 4.5, "unreadable muted text");
            assert!(
                (muted.l - background.l).abs() < (foreground.l - background.l).abs(),
                "blend must move toward the background"
            );
        }
    }

    #[test]
    fn contrast_ratio_matches_known_values() {
        let black = Hsla { h: 0.0, s: 0.0, l: 0.0, a: 1.0 };
        let white = Hsla { h: 0.0, s: 0.0, l: 1.0, a: 1.0 };
        assert!((contrast_ratio(black, white) - 21.0).abs() < 0.1);
        assert!((contrast_ratio(black, black) - 1.0).abs() < 0.01);
    }
}
