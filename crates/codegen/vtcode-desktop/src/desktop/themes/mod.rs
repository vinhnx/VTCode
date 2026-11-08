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

mod custom;
pub(super) use custom::{
    CustomTheme, CustomThemeError, custom_id_for, deserialize_custom_themes, name_taken,
    validate as validate_custom_theme,
};
mod glass;
pub(super) use glass::{
    glass_disabled_wash, glass_primary_rest, glass_primary_rest_on_popover, glass_primary_rest_on_secondary,
};

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

    pub(super) fn mode(self) -> ThemeMode {
        match self {
            Self::System(mode) => mode,
            Self::Palette(theme) => theme.mode,
        }
    }
}

/// The two system entries shown first in the picker.
pub(super) const SYSTEM_SWATCHES: [SwatchEntry; 2] = [
    SwatchEntry::System(ThemeMode::Light),
    SwatchEntry::System(ThemeMode::Dark),
];

const FEATURED_THEME_IDS: [&str; 3] = [DEFAULT_THEME_ID, "ciapre-dark", "ciapre-blue"];

/// Picker order within one mode group leads with the VT Code themes of
/// that mode, then the matching system base and the remaining shared
/// palette catalog in its existing order.
pub(super) fn picker_swatches_for(mode: ThemeMode) -> impl Iterator<Item = SwatchEntry> {
    let themes = all_themes();
    FEATURED_THEME_IDS
        .iter()
        .filter_map(|id| themes.iter().find(|theme| theme.id == *id))
        .copied()
        .filter(move |theme| theme.mode == mode)
        .map(SwatchEntry::Palette)
        .chain(SYSTEM_SWATCHES.into_iter().filter(move |entry| entry.mode() == mode))
        .chain(
            themes
                .iter()
                .copied()
                .filter(move |theme| theme.mode == mode && !FEATURED_THEME_IDS.contains(&theme.id))
                .map(SwatchEntry::Palette),
        )
}

/// Picker order grouped by mode: every light swatch first, then every dark
/// swatch. The settings picker renders the two groups as separate rows.
pub(super) fn picker_swatches() -> impl Iterator<Item = SwatchEntry> {
    picker_swatches_for(ThemeMode::Light).chain(picker_swatches_for(ThemeMode::Dark))
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

/// Custom-aware variant of [`apply_with_contrast`].
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
        colors.ring = accessible_focus_color(colors.primary, colors);
        colors.caret = accessible_focus_color(colors.primary, colors);
    });
    cx.refresh_windows();
    mode
}

fn apply_palette(seeds: PaletteSeeds, window: Option<&mut Window>, cx: &mut App) -> ThemeMode {
    Theme::change(seeds.mode, window, cx);
    Theme::update(cx, |kit_theme| {
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
        colors.sidebar_primary = seeds.accent;
        colors.sidebar_primary_foreground = filled_button_foreground(seeds.accent);
        colors.list = seeds.background;
        colors.list_active_border = seeds.accent;
        colors.success = seeds.success;
        colors.success_foreground = filled_button_foreground(seeds.success);
        colors.danger = seeds.danger;
        colors.danger_foreground = filled_button_foreground(seeds.danger);
        colors.warning = seeds.warning;
        colors.primary = seeds.accent;
        colors.primary_foreground = filled_button_foreground(seeds.accent);
        let contrast_end = filled_button_foreground(colors.primary_foreground);
        colors.primary_hover = mix(seeds.accent, contrast_end, 0.08);
        colors.primary_active = mix(seeds.accent, contrast_end, 0.16);
        colors.button_primary = colors.primary;
        colors.button_primary_foreground = colors.primary_foreground;
        colors.button_primary_hover = colors.primary_hover;
        colors.button_primary_active = colors.primary_active;
        apply_unified_hover_surface(colors, hover_surface(seeds.background, seeds.foreground));
        colors.ring = accessible_focus_color(seeds.accent, colors);
        // The text caret shares the focus-visibility rule: brand accent when
        // it holds 3:1 on interactive surfaces, else the foreground.
        colors.caret = accessible_focus_color(seeds.accent, colors);
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
        colors.ring = accessible_focus_color(colors.primary, colors);
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

/// Hard ceilings (percent of the background-to-foreground span) for the fill
/// search. Fills take the *weakest* step that still separates from the canvas,
/// so dark themes read as layered black; the ceiling only binds on themes
/// whose contrast curve needs larger steps (notably near-white canvases, where
/// equal ratios need bigger lightness moves). Hover keeps its own stronger cap:
/// a state change must stay visible, so it takes the strongest readable step.
/// Press goes one band deeper so the resting -> hover -> press ramp never runs
/// backwards; it also degrades to the hover step on palettes without headroom.
const RESTING_FILL_MAX_STEP: u8 = 10;
const RAISED_FILL_MAX_STEP: u8 = 24;
const HOVER_FILL_MAX_STEP: u8 = 10;
const ACTIVE_FILL_MAX_STEP: u8 = 16;

/// Give panels and controls a visible neutral fill while keeping their text at
/// WCAG AA. Popovers and input fields use a stronger layer than resting panels.
fn apply_filled_surfaces(colors: &mut ThemeColor) {
    let surface = filled_surface(colors.background, colors.foreground, RESTING_FILL_MAX_STEP, None);
    let raised_surface = filled_surface(colors.background, colors.foreground, RAISED_FILL_MAX_STEP, Some(surface));

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
    // Press states sit deeper than resting so Default, Secondary, and Ghost
    // buttons all show a visible press; Kit keeps border and text identical
    // across states, so the fill carries the whole signal.
    let press = active_surface(colors.background, colors.foreground);
    colors.button_active = press;
    colors.button_secondary = surface;
    colors.button_secondary_foreground = colors.foreground;
    colors.button_secondary_active = press;
    colors.secondary_active = surface;
    colors.popover = raised_surface;
    colors.popover_foreground = colors.foreground;
    // Kit's segmented TabBar paints its track from `tab_bar_segmented` and the
    // selected pill in the canvas color, so the track needs real separation
    // from the canvas: the resting panel fill only guarantees 1.04, which
    // renders selection nearly invisible. Rest labels dim toward the track so
    // the hover/selected shift to the full foreground reads as a highlight
    // instead of a rounding error.
    let track = segmented_track(colors.background, colors.foreground, surface);
    colors.tab_bar_segmented = track;
    colors.tab_foreground = inactive_tab_foreground(colors.foreground, track, &[track, colors.background, surface]);
    colors.tab_active_foreground = colors.foreground;
    // Folder TabBars paint their band from `tab_bar` and the selected tab
    // from `tab_active`: a transparent band so the bar never stripes a
    // non-canvas surface (the attachment picker sits on a popover), and a
    // canvas tab so the selection joins the content below it. Both reconcile
    // into the legacy `tokens` Kit reads at render time.
    colors.tab_bar = gpui_kit::transparent_black();
    colors.tab_active = colors.background;
}

/// Resting tab labels sit on canvas (thread strip, inspector) and directly on
/// popover (attachment picker folder bar); the segmented track survives only
/// as the dimming anchor. Dim the palette foreground toward the track as far
/// as every scan seat stays at WCAG AA (the folder test pins the popover seat
/// separately). The linear scan (not a binary search) stays valid because
/// per-seat contrast is not monotonic along the blend.
fn inactive_tab_foreground(foreground: Hsla, track: Hsla, seats: &[Hsla]) -> Hsla {
    const AA_RATIO: f32 = 4.5;
    let mut best = foreground;
    for step in 1_u8..=100 {
        let candidate = mix(foreground, track, f32::from(step) / 100.0);
        if seats.iter().all(|seat| contrast_ratio(candidate, *seat) >= AA_RATIO) {
            best = candidate;
        }
    }
    best
}

/// Segmented-tab track: the strongest foreground-leaning fill that keeps label
/// text readable with room to spare. Resting labels dim to the 4.5 floor, so
/// the track holds the full foreground about a point above it whenever the
/// palette allows — that gap is the visible hover/selected shift. Palettes
/// without the headroom fall back to plain AA, where selection still reads
/// through the pill but the highlight degrades gracefully.
fn segmented_track(background: Hsla, foreground: Hsla, panel: Hsla) -> Hsla {
    let headroom = contrast_ratio(foreground, background);
    let floor = 5.5_f32.min(headroom - 1.0).max(4.5);
    if let Some(track) = strongest_track_step(background, foreground, floor) {
        return track;
    }
    if let Some(track) = strongest_track_step(background, foreground, 4.5) {
        return track;
    }
    panel
}

fn strongest_track_step(background: Hsla, foreground: Hsla, minimum_text: f32) -> Option<Hsla> {
    (4_u8..=30)
        .rev()
        .map(|step| mix(background, foreground, f32::from(step) / 100.0))
        .find(|candidate| {
            contrast_ratio(foreground, *candidate) >= minimum_text && contrast_ratio(background, *candidate) >= 1.04
        })
}

/// Weakest fill that still separates from the canvas while keeping text at
/// WCAG AA. Raised layers additionally pass their lower seat so the stack
/// keeps a visible order instead of collapsing onto one fill.
fn filled_surface(background: Hsla, foreground: Hsla, maximum_step: u8, distinct_from: Option<Hsla>) -> Hsla {
    const SEPARATION_FLOOR: f32 = 1.04;
    const LAYER_GAP: f32 = 1.02;
    for step in 4_u8..=maximum_step {
        let candidate = mix(background, foreground, f32::from(step) / 100.0);
        let layered = distinct_from.is_none_or(|seat| contrast_ratio(seat, candidate) >= LAYER_GAP);
        if contrast_ratio(foreground, candidate) >= 4.5
            && contrast_ratio(background, candidate) >= SEPARATION_FLOOR
            && layered
        {
            return candidate;
        }
    }
    background
}

/// Strongest readable hover step: unlike resting fills, a state change must
/// stay visible, so hover maximizes within its cap instead of minimizing.
fn hover_surface(background: Hsla, foreground: Hsla) -> Hsla {
    for step in (4_u8..=HOVER_FILL_MAX_STEP).rev() {
        let candidate = mix(background, foreground, f32::from(step) / 100.0);
        if contrast_ratio(foreground, candidate) >= 4.5 {
            return candidate;
        }
    }
    background
}

/// Strongest readable press step: one band deeper than hover so a press reads
/// as a deeper state, never a repeat of resting. Shares hover's step range, so
/// on palettes without headroom it degrades to exactly the hover fill.
fn active_surface(background: Hsla, foreground: Hsla) -> Hsla {
    for step in (4_u8..=ACTIVE_FILL_MAX_STEP).rev() {
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
/// Quiet controls (the Zen exit) tint their icon with this instead of using
/// element opacity, so the keyboard focus ring keeps full strength.
pub(super) fn faded_chrome(background: Hsla, foreground: Hsla) -> Hsla {
    text_at_contrast(background, foreground, NON_TEXT_RATIO)
}

/// Resting tone for animated shimmer labels: `tone` blended toward the seat
/// until it just holds the quiet-text floor, so the moving sweep carries the
/// emphasis instead of the static glyphs. Returns `tone` untouched when it is
/// already at or below the floor, and for the static reduced-motion path
/// callers pass `tone` directly rather than this.
///
/// Anchored to [`ThemeColor::popover`], like [`muted_text`], because raised
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

/// Secondary copy (row descriptions, hints) one step brighter than
/// [`aa_muted`]: small muted text at the muted floor reads washed, so this
/// targets 7:1 on the hardest surface (the raised popover fill, which is
/// lighter than canvas, panel, and sidebar fills, so the ratio only grows
/// there). Falls back toward the palette foreground on low-headroom themes
/// instead of collapsing below AA.
pub(super) fn muted_text(colors: &ThemeColor) -> Hsla {
    const TARGET_RATIO: f32 = 7.0;
    let headroom = contrast_ratio(colors.foreground, colors.popover);
    text_at_contrast(colors.popover, colors.foreground, TARGET_RATIO.min(headroom))
}

/// A 1px separator tone for `fill`: the smallest blend toward the
/// foreground that stays visible. Contrast grows monotonically along the
/// lightness-only blend, so binary search lands just inside the threshold;
/// when even the full blend falls short (extreme hues), the foreground
/// itself keeps the divider visible.
pub(super) fn hairline(fill: Hsla, foreground: Hsla) -> Hsla {
    const MINIMUM: f32 = 1.6;
    if contrast_ratio(mix(fill, foreground, 1.0), fill) < MINIMUM {
        return foreground;
    }
    let mut low = 0.0_f32;
    let mut high = 1.0_f32;
    for _ in 0..24 {
        let middle = (low + high) / 2.0;
        if contrast_ratio(mix(fill, foreground, middle), fill) >= MINIMUM {
            high = middle;
        } else {
            low = middle;
        }
    }
    mix(fill, foreground, high)
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

/// Choose a translucent sidebar fill that remains readable over light and dark backdrops.
pub(super) fn translucent_sidebar_surface(colors: &ThemeColor) -> Hsla {
    let surface = navigation_surfaces(colors.background, colors.secondary, colors.foreground).panel;
    for step in 0..=20 {
        let alpha = 0.80 + step as f32 * 0.01;
        let readable = [0.0, 1.0].into_iter().all(|backdrop| {
            let backdrop = Hsla { h: 0.0, s: 0.0, l: backdrop, a: 1.0 };
            let composite = composite_status_surface(backdrop, surface, alpha);
            contrast_ratio(colors.foreground, composite) >= 4.5
                && contrast_ratio(colors.muted_foreground, composite) >= 4.5
                && contrast_ratio(label_foreground(colors), composite) >= 4.5
                && contrast_ratio(colors.primary, composite) >= 3.0
        });
        if readable {
            return surface.opacity(alpha);
        }
    }
    surface
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
mod contrast_tests;
#[cfg(test)]
mod tests;
