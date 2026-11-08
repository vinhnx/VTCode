//! Glass button fills: a translucent primary rest fill, AA-pinned per theme.
//!
//! The pilot seat is the composer cluster, which sits on the conversation
//! card's unconditional canvas fill in every mode (opaque, translucent,
//! zen, fullscreen), so the recipe owns its seat instead of trusting a
//! caller to pass the right one. A future seat needs its own recipe entry
//! point plus a contrast-matrix row, not a seat parameter.

use super::*;

/// Lowest rest alpha that still reads as a fill: below this the button is a
/// ghost with an accent ring, not glass.
const GLASS_REST_MIN_ALPHA: f32 = 0.60;
const GLASS_REST_ALPHA_STEP: f32 = 0.05;
const GLASS_REST_ALPHA_STEPS: u8 = 8;

/// Translucent primary rest fill: the weakest accent alpha whose src-over
/// composite on the canvas keeps the theme's primary text at WCAG AA. Falls
/// back to the opaque primary fill (today's look) when no translucent alpha
/// passes, so low-headroom themes degrade without a branch at the call site.
///
/// Hover and press keep the theme's opaque primary tokens: Kit owns the
/// hover/active style slots (an instance refinement would collide with its
/// render registration), so interaction solidifies the fill and the state
/// change reads unambiguously. Border, text, focus, and disabled text stay
/// Kit-owned primary semantics; only the resting fill turns to glass.
/// Pinned with Kit =0.8.1; recheck the slot ownership on upgrade.
pub(crate) fn glass_primary_rest(colors: &ThemeColor) -> Hsla {
    for step in 0..GLASS_REST_ALPHA_STEPS {
        let alpha = GLASS_REST_MIN_ALPHA + f32::from(step) * GLASS_REST_ALPHA_STEP;
        let composite = composite_status_surface(colors.background, colors.button_primary, alpha);
        if contrast_ratio(colors.button_primary_foreground, composite) >= 4.5 {
            return colors.button_primary.alpha(alpha);
        }
    }
    colors.button_primary
}

/// Disabled wash for glass primaries: mirrors Kit's own disabled-primary fill
/// (`button_primary` at 0.15, `button.rs` `disabled` arm), so a disabled
/// glass button reads exactly like a disabled opaque one. Pinned with Kit
/// =0.8.1; recheck the formula on upgrade.
pub(crate) fn glass_disabled_wash(colors: &ThemeColor) -> Hsla {
    colors.button_primary.alpha(0.15)
}
