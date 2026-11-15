//! Toolbar band surface: the fill behind the window's toolbar.

use gpui_kit::Hsla;

/// Toolbar band fill: fully transparent in translucent mode so the bar reads
/// as window glass continuous with the unpainted rail — any tinted fill
/// seams against the pure-blur chrome around it and reads as an opaque
/// slab. Transparency must be explicit: Kit TitleBar paints an opaque
/// default gradient that a missing fill would leave standing. Opaque mode
/// keeps the solid rail fill.
pub(in crate::desktop) fn toolbar_band_surface(translucent: bool, rail: Hsla) -> Hsla {
    if translucent {
        gpui_kit::transparent_black()
    } else {
        rail
    }
}
