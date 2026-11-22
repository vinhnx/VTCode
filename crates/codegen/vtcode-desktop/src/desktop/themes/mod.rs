//! VT Code desktop themes: the TUI theme registry rendered on the gpui-kit
//! tokens, with Ciapre as the app default.
//!
//! Palette rows come from the shared CLI theme registry. Desktop adjusts
//! foreground contrast for its raised surfaces while retaining palette hues.
//!
//! Each theme overrides the core kit colors after `Theme::change` installs
//! the matching light/dark base; derived tokens (border, panels, muted text)
//! are mixed from the palette so every theme stays legible by construction.

use fills::{
    PRIMARY_ACTIVE_MAX_STEP, PRIMARY_HOVER_MAX_STEP, RAISED_FILL_MAX_STEP, RESTING_FILL_MAX_STEP,
    apply_filled_surfaces, apply_unified_hover_surface, filled_surface, hover_surface, primary_state_surface,
};
use gpui_kit::component::theme::{Theme, ThemeColor, ThemeMode};
use gpui_kit::{App, Hsla, Window, px, rgb};
use std::collections::HashMap;
use vtcode_ui::theme::{self, UiThemeColors};

mod custom;
mod fills;
pub(super) use fills::{accented_button_shadows, button_inset_edges, elevated_card_shadows};
mod hairline;
mod muted_text;
pub(super) use muted_text::muted_text;
pub(super) mod layout;
pub(super) use custom::{
    CustomTheme, CustomThemeError, custom_id_for, deserialize_custom_themes, name_taken,
    validate as validate_custom_theme,
};
mod terminal;
pub(super) use terminal::{ciapre_ansi_slots, latte_ansi_slots, light_ansi_slots};
mod toolbar;
#[cfg(test)]
pub(super) use toolbar::toolbar_band_surface;
pub(super) use toolbar::toolbar_chrome;
mod swatches;
pub(super) use swatches::*;

/// The built-in theme identifiers referenced in source, grouped by the prefix
/// of their registry id. The CLI registry owns the full catalog at runtime;
/// this namespace gives the ids code names by one typed home instead of
/// scattered string literals.
#[allow(non_snake_case, reason = "ThemeId is the namespace for built-in theme identifiers")]
pub(super) mod ThemeId {
    /// Unprefixed id.
    pub(super) const NOIR: &str = "noir";

    /// Kit light/dark base sentinels — no palette override.
    pub(crate) mod system {
        pub(crate) const LIGHT: &str = "system-light";
        pub(crate) const DARK: &str = "system-dark";
    }

    pub(crate) mod ciapre {
        pub(crate) const BASE: &str = "ciapre";
        pub(crate) const DARK: &str = "ciapre-dark";
        pub(crate) const BLUE: &str = "ciapre-blue";
    }

    pub(crate) mod vitesse {
        pub(crate) const LIGHT: &str = "vitesse-light";
    }

    pub(crate) mod catppuccin {
        pub(crate) const LATTE: &str = "catppuccin-latte";
        #[cfg(test)]
        pub(crate) const MOCHA: &str = "catppuccin-mocha";
    }

    #[cfg(test)]
    pub(crate) mod gruvbox {
        pub(crate) const DARK: &str = "gruvbox-dark";
    }

    #[cfg(test)]
    pub(crate) mod github {
        pub(crate) const DARK: &str = "github-dark";
    }

    #[cfg(test)]
    pub(crate) mod solarized {
        pub(crate) const LIGHT: &str = "solarized-light";
    }
}

/// The theme applied on first launch. A neutral, high-headroom palette keeps
/// UI text crisp and lets the muted-text ramp read as a distinct tier; the
/// terminal-first palettes (Ciapre and variants) stay selectable in the picker.
pub(super) const DEFAULT_THEME_ID: &str = ThemeId::NOIR;

/// Sentinel ids that keep the kit's own light/dark base without overrides.
pub(super) const SYSTEM_LIGHT_ID: &str = ThemeId::system::LIGHT;
pub(super) const SYSTEM_DARK_ID: &str = ThemeId::system::DARK;

pub(super) fn mode_for_id(id: &str) -> Option<ThemeMode> {
    match id {
        ThemeId::system::LIGHT => Some(ThemeMode::Light),
        ThemeId::system::DARK => Some(ThemeMode::Dark),
        _ => find(id).map(|theme| theme.mode),
    }
}

/// Default palette for a mode: Noir in dark, Vitesse Light in light. Both are
/// neutral, high-headroom palettes so text reads crisply in either OS appearance.
pub(super) fn default_id(mode: ThemeMode) -> &'static str {
    match mode {
        ThemeMode::Light => ThemeId::vitesse::LIGHT,
        ThemeMode::Dark => DEFAULT_THEME_ID,
    }
}

pub(super) fn system_id(mode: ThemeMode) -> &'static str {
    match mode {
        ThemeMode::Light => SYSTEM_LIGHT_ID,
        ThemeMode::Dark => SYSTEM_DARK_ID,
    }
}

/// CLI registry id for a desktop selection, for theme sync.
///
/// Palette ids pass through; the kit-base sentinels map to the per-mode VT
/// Code default because the CLI has no kit base to mirror. Unknown ids stay
/// unknown so the service-side registry guard keeps rejecting them.
pub(super) fn cli_id_for(id: &str) -> Option<&'static str> {
    match id {
        SYSTEM_LIGHT_ID => Some(default_id(ThemeMode::Light)),
        SYSTEM_DARK_ID => Some(default_id(ThemeMode::Dark)),
        _ => find(id).map(|theme| theme.id),
    }
}

#[derive(Clone, Copy)]
pub(super) struct DesktopTheme {
    pub id: &'static str,
    pub label: &'static str,
    pub mode: ThemeMode,
    pub background: Hsla,
    pub foreground: Hsla,
    /// Brand accent used for highlights, primary buttons, and the text caret.
    pub accent: Hsla,
    /// Quieter companion accent for secondary panels and badges.
    pub secondary: Hsla,
    /// Semantic addition color used by diff views.
    pub success: Hsla,
    pub danger: Hsla,
    pub warning: Hsla,
}

/// The seed palette every theme resolves through [`apply_palette`]: seven
/// colors plus the mode. Built-in rows and custom themes share this shape so
/// the derived-token pipeline (and its WCAG floors) stays single-sourced.
#[derive(Clone, Copy, Debug)]
pub(super) struct PaletteSeeds {
    pub mode: ThemeMode,
    pub background: Hsla,
    pub foreground: Hsla,
    pub accent: Hsla,
    pub secondary: Hsla,
    pub success: Hsla,
    pub danger: Hsla,
    pub warning: Hsla,
}

impl DesktopTheme {
    pub(super) fn seeds(&self) -> PaletteSeeds {
        PaletteSeeds {
            mode: self.mode,
            background: self.background,
            foreground: self.foreground,
            accent: self.accent,
            secondary: self.secondary,
            success: self.success,
            danger: self.danger,
            warning: self.warning,
        }
    }
}

impl CustomTheme {
    /// Resolve a saved custom theme through the same derivation as built-ins.
    /// Accent and semantic colors are re-seated at apply time, so a saved
    /// value that later turns out unreadable still renders legibly.
    pub(super) fn seeds(&self) -> PaletteSeeds {
        PaletteSeeds {
            mode: self.mode,
            background: self.background,
            foreground: self.foreground,
            accent: readable_indicator_color(self.background, self.foreground, self.accent),
            secondary: self.secondary,
            success: readable_semantic_color(self.background, self.foreground, self.success),
            danger: readable_semantic_color(self.background, self.foreground, self.danger),
            warning: readable_semantic_color(self.background, self.foreground, self.warning),
        }
    }
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
        warning: readable_semantic_color(background, foreground, rgb(warning).into()),
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

/// Resolve a saved custom theme by id.
pub(super) fn find_custom<'a>(id: &str, customs: &'a [CustomTheme]) -> Option<&'a CustomTheme> {
    customs.iter().find(|theme| theme.id == id)
}

/// Mode of a selection: system sentinels, built-in palette rows, or a saved
/// custom theme. `None` means the id is unknown.
pub(super) fn mode_for_selection(id: &str, customs: &[CustomTheme]) -> Option<ThemeMode> {
    find_custom(id, customs).map(|theme| theme.mode).or_else(|| mode_for_id(id))
}

/// Seeds for a selection, used to seed the custom-theme editor from the
/// currently applied theme.
pub(super) fn selection_seeds(id: &str, customs: &[CustomTheme]) -> Option<PaletteSeeds> {
    find_custom(id, customs)
        .map(CustomTheme::seeds)
        .or_else(|| find(id).map(DesktopTheme::seeds))
}

/// Canonicalize a selection against the mode actually applied. An id that does
/// not resolve (a deleted custom theme, a stale preference) or that belongs to
/// the other mode is replaced by that mode's default, so the stored selection
/// never dangles. `apply_theme_state` and startup restore both route here.
pub(super) fn canonical_selection(id: &str, mode: ThemeMode, customs: &[CustomTheme]) -> String {
    if mode_for_selection(id, customs) == Some(mode) {
        id.to_string()
    } else {
        default_id(mode).to_string()
    }
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

/// Apply a theme by id: install the kit base for its mode, then override the
/// core tokens from the palette.
pub(super) fn apply_by_id(id: &str, window: Option<&mut Window>, cx: &mut App) -> ThemeMode {
    match id {
        SYSTEM_LIGHT_ID => {
            Theme::change(ThemeMode::Light, window, cx);
            normalize_system_interaction_colors(cx);
            cx.refresh_windows();
            ThemeMode::Light
        }
        SYSTEM_DARK_ID => {
            Theme::change(ThemeMode::Dark, window, cx);
            normalize_system_interaction_colors(cx);
            cx.refresh_windows();
            ThemeMode::Dark
        }
        _ => {
            let Some(theme) = find(id) else {
                return apply_by_id(DEFAULT_THEME_ID, window, cx);
            };
            apply_palette(theme.seeds(), window, cx)
        }
    }
}

/// Apply any selection: a kit base sentinel, a built-in palette row, or a saved
/// custom theme. Unknown ids keep the existing default fallback.
pub(super) fn apply_selection(
    id: &str,
    customs: &[CustomTheme],
    window: Option<&mut Window>,
    cx: &mut App,
) -> ThemeMode {
    match find_custom(id, customs) {
        Some(custom) => apply_palette(custom.seeds(), window, cx),
        None => apply_by_id(id, window, cx),
    }
}

/// Apply an additional foreground contrast while rebuilding its dependent surfaces.
#[cfg(test)]
pub(super) fn apply_with_contrast(id: &str, contrast: u8, window: Option<&mut Window>, cx: &mut App) -> ThemeMode {
    apply_selection_with_contrast(id, contrast, &[], window, cx)
}

/// Custom-aware variant of `apply_with_contrast`.
pub(super) fn apply_selection_with_contrast(
    id: &str,
    contrast: u8,
    customs: &[CustomTheme],
    window: Option<&mut Window>,
    cx: &mut App,
) -> ThemeMode {
    let mode = apply_selection(id, customs, window, cx);
    if contrast == 0 {
        return mode;
    }
    Theme::update(cx, |theme| {
        let colors = &mut theme.colors;
        let target = if relative_luminance(colors.foreground) >= relative_luminance(colors.background) {
            rgb(0xffffff).into()
        } else {
            rgb(0).into()
        };
        colors.foreground = mix(colors.foreground, target, f32::from(contrast.min(100)) / 100.0);
        apply_filled_surfaces(colors);
        apply_surface_borders(colors);
        apply_unified_hover_surface(colors, hover_surface(colors.background, colors.foreground));
        // Focus policy: the app draws no focus-ring border highlight anywhere,
        // so the ring token stays fully transparent (kit inputs, buttons, and
        // sliders all read it). Keyboard focus stays legible through the text
        // caret, which keeps its own accent/foreground treatment.
        colors.ring = colors.ring.opacity(0.);
        colors.caret = accessible_focus_color(colors.primary, colors);
    });
    // No explicit `cx.refresh_windows()`: both `apply_selection` (via
    // `Theme::change`) and `Theme::update` above already schedule one per
    // update cycle, and GPUI coalesces repeats. Contrast drags call this on
    // every `Change`, so keep the hot path free of redundant work.
    mode
}

/// Shared radius baseline (`md` 8px, `lg` 12px): Large buttons capsule at 16px, tiers stay concentric.
fn apply_desktop_radii(kit_theme: &mut Theme) {
    kit_theme.radius = px(8.);
    kit_theme.radius_lg = px(12.);
}

fn apply_palette(seeds: PaletteSeeds, window: Option<&mut Window>, cx: &mut App) -> ThemeMode {
    Theme::change(seeds.mode, window, cx);
    Theme::update(cx, |kit_theme| {
        apply_desktop_radii(kit_theme);
        let colors = &mut kit_theme.colors;
        colors.background = seeds.background;
        colors.foreground = seeds.foreground;
        apply_filled_surfaces(colors);
        apply_surface_borders(colors);
        colors.chart_2 = distinct_chart_secondary(
            colors.background,
            colors.foreground,
            [colors.chart_1, colors.chart_3, colors.chart_4],
            seeds.secondary,
        );
        // Filled labels come from `filled_button_foreground`, which rescues
        // mid-luminance seeds whose luminance falls between the softened
        // labels' AA floors; the accent seed itself stays untouched so the
        // registry palette remains the single brand source.
        let accent = seeds.accent;
        colors.sidebar_primary = accent;
        colors.sidebar_primary_foreground = filled_button_foreground(accent);
        colors.list = seeds.background;
        colors.list_active_border = accent;
        colors.success = seeds.success;
        colors.success_foreground = filled_button_foreground(seeds.success);
        colors.danger = seeds.danger;
        colors.danger_foreground = filled_button_foreground(seeds.danger);
        colors.warning = seeds.warning;
        colors.primary = accent;
        colors.primary_foreground = filled_button_foreground(accent);
        // State surfaces blend toward the pure lightness pole, not the label:
        // the softened label can share the accent's lightness, which would
        // collapse the rest -> hover ramp onto the resting fill.
        let contrast_end = filled_state_pole(colors.primary_foreground);
        colors.primary_hover =
            primary_state_surface(accent, contrast_end, colors.primary_foreground, PRIMARY_HOVER_MAX_STEP);
        colors.primary_active =
            primary_state_surface(accent, contrast_end, colors.primary_foreground, PRIMARY_ACTIVE_MAX_STEP);
        colors.button_primary = colors.primary;
        colors.button_primary_foreground = colors.primary_foreground;
        colors.button_primary_hover = colors.primary_hover;
        colors.button_primary_active = colors.primary_active;
        apply_unified_hover_surface(colors, hover_surface(seeds.background, seeds.foreground));
        // Focus policy (see `apply_selection_with_contrast`): no focus-ring
        // border highlight anywhere; the caret keeps its visibility rule.
        colors.ring = colors.ring.opacity(0.);
        // The text caret shares the focus-visibility rule: brand accent when
        // it holds 3:1 on interactive surfaces, else the foreground.
        colors.caret = accessible_focus_color(accent, colors);
    });
    seeds.mode
}

/// Keep keyboard focus and the text caret visible while retaining brand color
/// when it has enough contrast.
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
        apply_desktop_radii(kit_theme);
        let colors = &mut kit_theme.colors;
        apply_filled_surfaces(colors);
        apply_surface_borders(colors);
        let surface = hover_surface(colors.background, colors.foreground);
        apply_unified_hover_surface(colors, surface);
        colors.success = readable_semantic_color(colors.background, colors.foreground, colors.success);
        colors.danger = readable_semantic_color(colors.background, colors.foreground, colors.danger);
        colors.warning = readable_semantic_color(colors.background, colors.foreground, colors.warning);
        colors.success_foreground = filled_button_foreground(colors.success);
        colors.danger_foreground = filled_button_foreground(colors.danger);
        // Focus policy (see `apply_selection_with_contrast`): no focus-ring
        // border highlight anywhere; the caret keeps its visibility rule.
        colors.ring = colors.ring.opacity(0.);
        colors.caret = accessible_focus_color(colors.primary, colors);
    });
}

/// Soft 1px outlines for cards, inputs, pills, and dividers: the faintest blend
/// toward the foreground that stays visible on panel fills, derived the same
/// way in light and dark modes. Runs after the fills so the border tracks the
/// final panel tone; contrast tuning refreshes both together.
fn apply_surface_borders(colors: &mut ThemeColor) {
    colors.border = subtle_border(colors.secondary, colors.popover, colors.foreground);
    colors.sidebar_border = colors.border;
    colors.input = colors.border;
}

/// Border tone that holds the hairline floor on panels and separation from
/// raised fills: start from the panel-seat hairline, then step toward the
/// foreground until popovers clear the floor too. Stepping up only grows the
/// panel/canvas separation (contrast is monotonic along the blend), and the
/// foreground itself always clears popover text AA, so this terminates.
/// Requires the foreground to stand off the fills; every shipped theme keeps
/// text AA, and the border contract test fails loud otherwise.
fn subtle_border(secondary: Hsla, popover: Hsla, foreground: Hsla) -> Hsla {
    const RAISED_FLOOR: f32 = 1.04;
    let mut border = hairline(secondary, foreground);
    for _ in 0..100 {
        if contrast_ratio(border, popover) >= RAISED_FLOOR {
            return border;
        }
        let next = mix(border, foreground, 0.05);
        if next == border {
            return border;
        }
        border = next;
    }
    border
}

/// The single soft-edge tone: surfaces that used to paint full-strength
/// `border` (cards, dialogs, pills, dividers, code panels) use this one
/// translucent blend so boundaries read as quiet hairlines rather than rules.
/// This is the "soft edge thin" knob — tune here, not per surface. Contrast-
/// derived [`hairline()`] stays where a border must clear a floor on its own
/// fill; `border` itself stays opaque for Kit inputs and focus.
pub(super) fn soft_edge_color(colors: &ThemeColor) -> Hsla {
    const SOFT_EDGE_OPACITY: f32 = 0.55;
    colors.border.opacity(SOFT_EDGE_OPACITY)
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

/// Minimum contrast between chart data colors (and against the canvas).
/// Data colors carry redundant labels, so they hold the vanish floor rather
/// than the text floor. Shared by [`distinct_chart_secondary`] and its test
/// so the derivation and the guarantee cannot drift apart.
pub(super) const CHART_DISTINGUISH_FLOOR: f32 = 1.04;

/// Nudge the secondary seed until the chart's second series stands clear of
/// the canvas and its kit-default siblings, the same self-correction the
/// semantic seeds get. Pass the sibling slots it must clear.
pub(super) fn distinct_chart_secondary(canvas: Hsla, foreground: Hsla, siblings: [Hsla; 3], seed: Hsla) -> Hsla {
    adjust_color_until_readable(canvas, foreground, seed, |candidate| {
        contrast_ratio(candidate, canvas) >= CHART_DISTINGUISH_FLOOR
            && siblings
                .iter()
                .all(|sibling| contrast_ratio(candidate, *sibling) >= CHART_DISTINGUISH_FLOOR)
    })
}

/// Fallback seeds that keep Ask (success-family) and Auto-approve
/// (warning-family) permission states visually distinct on palettes where
/// `success == warning`. Raw palette values live only in this theme layer;
/// callers must pass the result through [`readable_indicator_color`] for the
/// target surface. Returns `(ask_seed, auto_approve_seed)`.
pub(super) fn distinct_permission_seeds() -> (Hsla, Hsla) {
    (rgb(0x237A45).into(), rgb(0xB45309).into())
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

/// Foreground for a filled control: the softened label pair — dark ink
/// `#171C26`, light ink `#CCCCCC` — chosen at the luminance crossover. A seed
/// whose luminance falls between the two labels' AA floors (the ~0.095–0.227
/// band) cannot reach 4.5:1 with either constant; there the label walks
/// toward its pure pole just until AA holds, so `#CCCCCC` stays the light
/// label everywhere it can possibly work.
pub(super) fn filled_button_foreground(color: Hsla) -> Hsla {
    const LIGHT: Hsla = Hsla { h: 0.0, s: 0.0, l: 0.8, a: 1.0 };
    let label = if relative_luminance(color) > 0.179 {
        rgb(0x171C26).into()
    } else {
        LIGHT
    };
    if contrast_ratio(label, color) >= 4.5 {
        return label;
    }
    // Dead-band rescue: the label moves toward the pole that grows contrast
    // against this fill — white under a dark-ish fill, black under a light one.
    let pole: Hsla = if relative_luminance(color) > 0.179 {
        rgb(0).into()
    } else {
        rgb(0xffffff).into()
    };
    for step in 1_u8..=50 {
        let candidate = mix(label, pole, f32::from(step) / 100.0);
        if contrast_ratio(candidate, color) >= 4.5 {
            return candidate;
        }
    }
    pole
}

/// Pure lightness pole a filled control's state ramp blends toward: white
/// under dark ink, black under light ink. The softened label can share the
/// accent's lightness (e.g. `#CCCCCC` over a very light accent), so the ramp
/// targets the pole itself to keep hover and press moving off the resting
/// fill.
pub(super) fn filled_state_pole(label: Hsla) -> Hsla {
    if relative_luminance(label) > 0.179 {
        rgb(0).into()
    } else {
        rgb(0xffffff).into()
    }
}

/// Label-grade text: section headings, group titles, and overlines render at
/// full palette foreground. Hierarchy comes from size, weight, and case —
/// not from dimming — so headings never read fainter than the body copy they
/// introduce, on any fill.
pub(super) fn label_foreground(colors: &ThemeColor) -> Hsla {
    colors.foreground
}

/// The softest text color that stays crisp against `background`: the palette
/// foreground blended toward the background just until a 5.0:1 floor holds
/// (one half-point above WCAG AA — small text at the 4.5 floor reads washed).
/// Returns the foreground untouched when even it cannot reach the floor (the
/// contrast test then flags the theme).
pub(super) fn aa_muted(background: Hsla, foreground: Hsla) -> Hsla {
    const MUTED_RATIO: f32 = 5.0;
    text_at_contrast(background, foreground, MUTED_RATIO)
}

/// The 3:1 floor for non-text marks and for text that is deliberately quiet
/// (WCAG 2.1 SC 1.4.11). Shared so quiet chrome and shimmer resting text cannot
/// drift apart.
const NON_TEXT_RATIO: f32 = 3.0;

/// Faintest chrome-icon tone on `background` that still meets the 3:1
/// non-text contrast floor (WCAG 2.1 SC 1.4.11) for meaningful icons.
/// Quiet controls (the Zen entry) tint their icon with this instead of using
/// element opacity, so the keyboard focus ring keeps full strength. Callers
/// must pass the surface the icon actually paints on (the toolbar rail for
/// the Zen entry), not the canvas background, or the ratio no longer holds.
pub(super) fn faded_chrome(background: Hsla, foreground: Hsla) -> Hsla {
    text_at_contrast(background, foreground, NON_TEXT_RATIO)
}

/// Resting tone for animated shimmer labels: `tone` blended toward the seat
/// until it just holds the quiet-text floor, so the moving sweep carries the
/// emphasis instead of the static glyphs. Returns `tone` untouched when it is
/// already at or below the floor, and for the static reduced-motion path
/// callers pass `tone` directly rather than this.
///
/// Anchored to [`ThemeColor::popover`], like [`muted_text()`], because raised
/// surfaces move toward the text color in both polarities, so the popover is
/// the hardest seat: holding the floor there holds it on the canvas, panel, and
/// group box too. Anchoring to the canvas instead (the obvious-looking choice)
/// lands at ~2.7:1 on the seats these labels actually use, below the floor.
///
/// The sweep's bright end is the palette foreground, not a pole picked here: it
/// is the maximum-contrast text token by construction (themes are AA-gated on
/// every surface while this is capped at the quiet floor), so the band provably
/// outshines the text it crosses. Pinning it also avoids two Kit behaviours: it
/// mixes the text toward a pole and flips when the text is within 0.1 lightness
/// of it (on Ciapre, muted and foreground are 0.097 apart, so the band came out
/// darker than the text), and on low-headroom palettes the "far pole" is the
/// background itself, a background-coloured sweep.
pub(super) fn shimmer_resting(colors: &ThemeColor, tone: Hsla) -> Hsla {
    text_at_contrast(colors.popover, tone, NON_TEXT_RATIO)
}

pub(super) use hairline::hairline;

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
        filled_surface(background, foreground, RESTING_FILL_MAX_STEP, None)
    } else {
        surfaces.rail
    };
    let panel = if contrast_ratio(rail, surfaces.panel) < 1.02 || contrast_ratio(background, surfaces.panel) < 1.02 {
        filled_surface(background, foreground, RAISED_FILL_MAX_STEP, Some(rail))
    } else {
        surfaces.panel
    };
    NavigationSurfaces { rail, panel }
}

/// Model of the backdrop a glass surface actually sits on: the system
/// vibrancy material tints the blurred wallpaper toward the appearance's own
/// base, and blur plus saturation pull extreme wallpapers toward mid-tone.
/// Checking contrast against pure black AND pure white instead would pin the
/// fill at alpha >= 0.80 — often fully opaque — and the "translucent sidebar"
/// toggle would render opaque. The macOS Liquid Glass answer is that the
/// material itself adapts behind the content, so glass readability is checked
/// against this modeled backdrop; the floor below keeps the scan honest when
/// a palette cannot reach AA over it.
const VIBRANCY_WALLPAPER_VARIANCE: f32 = 0.5;

/// Modeled luminance of the vibrancy backdrop per appearance: half-way toward
/// the mode's own pole from mid-gray (dark glass ≈ 0.25, light glass ≈ 0.75).
fn vibrancy_backdrop(dark: bool) -> Hsla {
    let l = if dark {
        VIBRANCY_WALLPAPER_VARIANCE * 0.5
    } else {
        1.0 - VIBRANCY_WALLPAPER_VARIANCE * 0.5
    };
    Hsla { h: 0.0, s: 0.0, l, a: 1.0 }
}

/// Translucent floor and step for the glass scan: visibly see-through, but
/// bounded so a palette that cannot reach AA falls back to its opaque tint.
const GLASS_ALPHA_FLOOR: f32 = 0.30;
const GLASS_ALPHA_STEPS: usize = 71;

/// Choose the lightest-alpha glass fill of `tint` over the modeled vibrancy
/// backdrop that keeps WCAG AA text (4.5:1) and icon (3:1) contrast. Falls
/// back to the opaque tint when no alpha in the glass band qualifies.
fn translucent_glass_surface(colors: &ThemeColor, tint: Hsla) -> Hsla {
    let dark = relative_luminance(colors.background) < 0.179;
    let backdrop = vibrancy_backdrop(dark);
    for step in 0..GLASS_ALPHA_STEPS {
        let alpha = GLASS_ALPHA_FLOOR + step as f32 * 0.01;
        let composite = composite_status_surface(backdrop, tint, alpha);
        let readable = contrast_ratio(colors.foreground, composite) >= 4.5
            && contrast_ratio(colors.muted_foreground, composite) >= 4.5
            && contrast_ratio(label_foreground(colors), composite) >= 4.5
            && contrast_ratio(colors.primary, composite) >= 3.0;
        if readable {
            return tint.opacity(alpha);
        }
    }
    tint
}

/// Choose a translucent sidebar fill whose readability is modeled against the
/// vibrancy backdrop (see [`vibrancy_backdrop`]), not worst-case wallpapers.
pub(super) fn translucent_sidebar_surface(colors: &ThemeColor) -> Hsla {
    let panel = navigation_surfaces(colors.background, colors.secondary, colors.foreground).panel;
    translucent_glass_surface(colors, panel)
}

/// WCAG contrast ratio between two colors.
pub(super) fn contrast_ratio(a: Hsla, b: Hsla) -> f32 {
    contrast_ratio_luminance(relative_luminance(a), relative_luminance(b))
}

/// Ratio from two already-measured luminances. Lets callers that compare one
/// color against a fixed reference (e.g. `hairline`'s binary search) measure
/// that reference once instead of once per iteration, bit-identically.
fn contrast_ratio_luminance(la: f32, lb: f32) -> f32 {
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
mod contrast_tests;
#[cfg(test)]
mod tests;
