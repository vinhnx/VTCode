//! Glass button fills: translucent primary rest fills, AA-pinned per theme.
//!
//! One recipe entry point per seat, each compositing over the surface the
//! button actually sits on:
//!
//! - [`glass_primary_rest`]: the canvas seat (`background`) for the floating
//!   composer cluster, which sits on the conversation card's unconditional
//!   canvas fill in every mode (opaque, translucent, zen, fullscreen).
//! - [`glass_primary_rest_on_popover`]: the popover seat for in-layer dialog
//!   commits (interaction approvals, About, changelog, rename, picker).
//! - [`glass_primary_rest_on_secondary`]: the secondary seat for the CLI
//!   config editor dialog and settings saves, which sit on the secondary /
//!   group-box fill (unified on every theme).
//!
//! A future seat needs its own recipe entry point plus a contrast-matrix row,
//! not a seat parameter: the entry point names the seat so a wrong-seat call
//! reads wrong at the call site. The entries share the private alpha scan.

use super::*;

/// Lowest rest alpha that still reads as a fill: below this the button is a
/// ghost with an accent ring, not glass.
const GLASS_REST_MIN_ALPHA: f32 = 0.60;
const GLASS_REST_ALPHA_STEP: f32 = 0.05;
const GLASS_REST_ALPHA_STEPS: u8 = 8;

/// Weakest accent alpha whose src-over composite on `seat` keeps `text` at
/// WCAG AA, else the opaque accent (today's look) when no translucent alpha
/// passes, so low-headroom seats degrade without a branch at the call site.
fn glass_rest_on(seat: Hsla, accent: Hsla, text: Hsla) -> Hsla {
    for step in 0..GLASS_REST_ALPHA_STEPS {
        let alpha = GLASS_REST_MIN_ALPHA + f32::from(step) * GLASS_REST_ALPHA_STEP;
        let composite = composite_status_surface(seat, accent, alpha);
        if contrast_ratio(text, composite) >= 4.5 {
            return accent.alpha(alpha);
        }
    }
    accent
}

/// Translucent primary rest fill over the canvas seat.
///
/// Hover and press keep the theme's opaque primary tokens: Kit owns the
/// hover/active style slots (an instance refinement would collide with its
/// render registration), so interaction solidifies the fill and the state
/// change reads unambiguously. Border, text, focus, and disabled text stay
/// Kit-owned primary semantics; only the resting fill turns to glass.
/// Pinned with Kit =0.9.0 (slot ownership re-verified: glass tests green); recheck the slot ownership on upgrade.
pub(crate) fn glass_primary_rest(colors: &ThemeColor) -> Hsla {
    glass_rest_on(colors.background, colors.button_primary, colors.button_primary_foreground)
}

/// Translucent primary rest fill over the popover seat: same contract as
/// [`glass_primary_rest`], composited over `popover` for dialog commits.
pub(crate) fn glass_primary_rest_on_popover(colors: &ThemeColor) -> Hsla {
    glass_rest_on(colors.popover, colors.button_primary, colors.button_primary_foreground)
}

/// Translucent primary rest fill over the secondary seat: same contract as
/// [`glass_primary_rest`], composited over `secondary` for settings-surface
/// commits. Group-box fills equal `secondary` on every theme, so settings
/// saves inside group boxes share this seat.
pub(crate) fn glass_primary_rest_on_secondary(colors: &ThemeColor) -> Hsla {
    glass_rest_on(colors.secondary, colors.button_primary, colors.button_primary_foreground)
}

/// Disabled wash for glass primaries: mirrors Kit's own disabled-primary fill
/// (`button_primary` at 0.15, `button.rs` `disabled` arm), so a disabled
/// glass button reads exactly like a disabled opaque one. Seat-independent:
/// every glass builder shares it. Pinned with Kit
/// =0.9.0 (disabled arm still primary at 0.15); recheck the formula on upgrade.
pub(crate) fn glass_disabled_wash(colors: &ThemeColor) -> Hsla {
    colors.button_primary.alpha(0.15)
}
