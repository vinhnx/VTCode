//! Derived surface fills: resting panels, raised layers, and the hover/press
//! state ramps, scanned for WCAG AA text contrast by construction.
//!
//! Resting fills take the weakest separating step; state fills take the
//! strongest readable step within their caps, so interaction feedback stays
//! visible on every palette. Tab tracks and dimmed tab labels derive here
//! too: they are fills under the same readability contract.

use super::{aa_muted, contrast_ratio, filled_button_foreground, mix};
use gpui_kit::component::theme::ThemeColor;
use gpui_kit::{BoxShadow, Hsla, black, px, white};

/// Hard ceilings (percent of the background-to-foreground span) for the fill
/// search. Fills take the *weakest* step that still separates from the canvas,
/// so dark themes read as layered black; the ceiling only binds on themes
/// whose contrast curve needs larger steps (notably near-white canvases, where
/// equal ratios need bigger lightness moves). Hover keeps its own stronger cap:
/// a state change must stay visible, so it takes the strongest readable step.
/// Press goes one band deeper so the resting -> hover -> press ramp never runs
/// backwards; it also degrades to the hover step on palettes without headroom.
/// Primary states scan the accent toward its contrast pole under the same
/// strongest-readable rule, with a small hover step and a clearer press.
pub(super) const RESTING_FILL_MAX_STEP: u8 = 10;
pub(super) const RAISED_FILL_MAX_STEP: u8 = 24;
const HOVER_FILL_MAX_STEP: u8 = 14;
const ACTIVE_FILL_MAX_STEP: u8 = 24;
pub(super) const PRIMARY_HOVER_MAX_STEP: u8 = 8;
pub(super) const PRIMARY_ACTIVE_MAX_STEP: u8 = 30;

/// Give panels and controls a visible neutral fill while keeping their text at
/// WCAG AA. Popovers and input fields use a stronger layer than resting panels.
pub(super) fn apply_filled_surfaces(colors: &mut ThemeColor) {
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
    // Buttons must read as controls on every surface they sit on. Panel fills
    // share this resting tone, so a button body painted with it vanishes on
    // settings pages and group boxes and the pair reads as two shapes; seat
    // the button fill one step above the panel tone so quiet buttons keep a
    // visible body next to filled siblings everywhere. The button hover and
    // press derive from this same seat (see `apply_unified_hover_surface` and
    // the secondary active below) so each state stays a visible step above
    // the resting fill instead of collapsing onto it.
    let button_surface = filled_surface(surface, colors.foreground, RESTING_FILL_MAX_STEP, None);
    colors.button = button_surface;
    colors.button_foreground = colors.foreground;
    // Press states sit deeper than resting so Default, Secondary, and Ghost
    // buttons all show a visible press; Kit keeps border and text identical
    // across states, so the fill carries the whole signal. `button_active`
    // doubles as the Ghost pressed/selected seat (transparent rest), so it
    // measures from the canvas; the Secondary press owns the button seat.
    let press = active_surface(colors.background, colors.foreground);
    colors.button_active = press;
    colors.button_secondary = button_surface;
    colors.button_secondary_foreground = colors.foreground;
    colors.button_secondary_active = active_surface(button_surface, colors.foreground);
    // Ciapre's red is reserved for destructive buttons. Status text and
    // indicators still use each palette's contrast-adjusted danger color.
    // The fill sits dark enough that the softened `#CCCCCC` filled-button
    // label holds WCAG AA 4.5:1 (pure `#a63333` measured 4.16:1).
    let danger = gpui_kit::rgb(0x9B2C2C).into();
    let danger_text = filled_button_foreground(danger);
    let toward = super::filled_state_pole(danger_text);
    colors.button_danger = danger;
    colors.button_danger_foreground = danger_text;
    colors.button_danger_hover = primary_state_surface(danger, toward, danger_text, PRIMARY_HOVER_MAX_STEP);
    colors.button_danger_active = primary_state_surface(danger, toward, danger_text, PRIMARY_ACTIVE_MAX_STEP);

    // Ghost selected states and every other Kit `secondary_active` reader
    // (stepper, title bar, table, and dock active fills) share the pressed
    // depth: the resting fill read as no state change at all. Dropdown
    // triggers no longer ride this token while open — the vendored Kit
    // keeps `open` paint-free, so the popup alone marks the state.
    colors.secondary_active = press;
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

/// Paint the face highlight first, then mask its outer edge with a recessed
/// frame. Both layers follow the button's rounded bounds.
pub(in crate::desktop) fn button_inset_edges() -> Vec<BoxShadow> {
    vec![
        BoxShadow::new(px(0.), px(1.), white().opacity(0.10))
            .spread_radius(px(2.))
            .inset(),
        BoxShadow::new(px(0.), px(-0.5), black().opacity(0.66))
            .spread_radius(px(2.))
            .inset(),
    ]
}

/// A soft drop shadow and narrow, neutral face edges keep opaque accented buttons
/// dimensional on both light and dark surfaces.
pub(in crate::desktop) fn accented_button_shadows() -> Vec<BoxShadow> {
    vec![
        BoxShadow::new(px(0.), px(1.), black().opacity(0.24)).blur_radius(px(3.)),
        BoxShadow::new(px(0.), px(1.), white().opacity(0.22)).inset(),
        BoxShadow::new(px(0.), px(-1.), black().opacity(0.14)).inset(),
    ]
}

/// Soft elevation for a raised content card (the welcome action cluster): one
/// neutral drop shadow that reads on both light and dark canvases without a
/// colored glow. Pair with the soft-edge hairline and one-step-above-canvas
/// fill so elevation comes from the surface, not from a heavy shadow.
pub(in crate::desktop) fn elevated_card_shadows() -> Vec<BoxShadow> {
    vec![BoxShadow::new(px(0.), px(2.), black().opacity(0.14)).blur_radius(px(10.))]
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
pub(super) fn filled_surface(
    background: Hsla,
    foreground: Hsla,
    maximum_step: u8,
    distinct_from: Option<Hsla>,
) -> Hsla {
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
pub(super) fn hover_surface(background: Hsla, foreground: Hsla) -> Hsla {
    for step in (4_u8..=HOVER_FILL_MAX_STEP).rev() {
        let candidate = mix(background, foreground, f32::from(step) / 100.0);
        if contrast_ratio(foreground, candidate) >= 4.5 {
            return candidate;
        }
    }
    background
}

/// Strongest accent step toward `toward` that keeps `text` at WCAG AA, so a
/// primary hover/press takes the most visible fill the palette allows. Kit
/// keeps button text identical across states, so the AA guard is the contract.
/// Without headroom the state degrades to the accent itself. A wider cap only
/// adds candidates to the scan, so it can never pick a shallower step than a
/// narrower one: the rest -> hover -> press ramp never runs backwards.
pub(super) fn primary_state_surface(accent: Hsla, toward: Hsla, text: Hsla, maximum_step: u8) -> Hsla {
    for step in (1_u8..=maximum_step).rev() {
        let candidate = mix(accent, toward, f32::from(step) / 100.0);
        if contrast_ratio(text, candidate) >= 4.5 {
            return candidate;
        }
    }
    accent
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

pub(super) fn apply_unified_hover_surface(colors: &mut ThemeColor, surface: Hsla) {
    // Keep the same soft neutral hover across buttons, list rows, and selected
    // command items; the brand accent remains reserved for primary actions.
    colors.accent = surface;
    colors.accent_foreground = colors.foreground;
    // Button hovers measure from the resting button fill, not the canvas: the
    // fill seats one step above the panel surface, so the canvas hover step
    // would sit beneath it and hover would not read on settings pages. The
    // callers run `apply_filled_surfaces` first, so `colors.button` is the
    // current resting seat. Default and Secondary share the seat.
    let button_hover = hover_surface(colors.button, colors.foreground);
    colors.button_hover = button_hover;
    colors.button_secondary_hover = button_hover;
    colors.secondary_hover = surface;
    colors.list_hover = surface;
    colors.sidebar_accent = surface;
    colors.sidebar_accent_foreground = colors.foreground;
}

#[cfg(test)]
mod tests;
