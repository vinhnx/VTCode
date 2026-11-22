//! Toolbar chrome: the band fill and the quiet icon tone painted on it.
//!
//! Both are derived from the same navigation rail. The opaque band paints the
//! rail; the translucent band is fully transparent over window vibrancy whose
//! backdrop the codebase models as that same unpainted rail. Keeping the two
//! values in one helper prevents the icon from being calibrated against a
//! different surface than the band it sits on.

use gpui_kit::Hsla;

pub(in crate::desktop) struct ToolbarChrome {
    /// Fill behind the toolbar band.
    pub band: Hsla,
    /// Tone for quiet icons painted on that band.
    pub icon_tone: Hsla,
}

/// Toolbar chrome for the current translucency mode.
pub(in crate::desktop) fn toolbar_chrome(
    translucent: bool,
    background: Hsla,
    secondary: Hsla,
    foreground: Hsla,
) -> ToolbarChrome {
    let rail = super::navigation_surfaces(background, secondary, foreground).rail;
    ToolbarChrome {
        band: toolbar_band_surface(translucent, rail),
        icon_tone: super::faded_chrome(rail, foreground),
    }
}

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
