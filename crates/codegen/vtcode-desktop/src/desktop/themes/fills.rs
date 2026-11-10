//! Derived surface fills: resting panels, raised layers, and the hover/press
//! state ramps, scanned for WCAG AA text contrast by construction.
//!
//! Resting fills take the weakest separating step; state fills take the
//! strongest readable step within their caps, so interaction feedback stays
//! visible on every palette. Tab tracks and dimmed tab labels derive here
//! too: they are fills under the same readability contract.

use super::{aa_muted, contrast_ratio, mix};
use gpui_kit::Hsla;
use gpui_kit::component::theme::ThemeColor;

/// Hard ceilings (percent of the background-to-foreground span) for the fill
/// search. Fills take the *weakest* step that still separates from the canvas,
/// so dark themes read as layered black; the ceiling only binds on themes
/// whose contrast curve needs larger steps (notably near-white canvases, where
/// equal ratios need bigger lightness moves). Hover keeps its own stronger cap:
/// a state change must stay visible, so it takes the strongest readable step.
/// Press goes one band deeper so the resting -> hover -> press ramp never runs
/// backwards; it also degrades to the hover step on palettes without headroom.
/// Primary states scan the accent toward its contrast pole under the same
/// strongest-readable rule; their caps sit one band above the neutral ones
/// because equal lightness steps read weaker on saturated fills.
pub(super) const RESTING_FILL_MAX_STEP: u8 = 10;
pub(super) const RAISED_FILL_MAX_STEP: u8 = 24;
const HOVER_FILL_MAX_STEP: u8 = 14;
const ACTIVE_FILL_MAX_STEP: u8 = 24;
pub(super) const PRIMARY_HOVER_MAX_STEP: u8 = 18;
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
    // Ghost selected/open states (palette triggers while their popup is up)
    // and every other Kit `secondary_active` reader (stepper, title bar,
    // table, and dock active fills) share the pressed depth: the resting
    // fill read as no state change at all.
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
    colors.button_hover = surface;
    colors.button_secondary_hover = surface;
    colors.secondary_hover = surface;
    colors.list_hover = surface;
    colors.sidebar_accent = surface;
    colors.sidebar_accent_foreground = colors.foreground;
}

#[cfg(test)]
mod tests;
