/// The translucent toolbar must not paint a band: any tinted fill seams
/// against the unpainted rail and pure-blur gaps around it and reads as an
/// opaque slab, so the bar floats fully transparent on the window vibrancy
/// like the rail. Opaque mode keeps the solid rail fill.
#[test]
fn translucent_toolbar_leaves_no_band() {
    let rail = gpui_kit::Hsla { h: 0.1, s: 0.2, l: 0.3, a: 1.0 };
    let glass = crate::desktop::themes::toolbar_band_surface(true, rail);
    assert_eq!(glass.a, 0.0, "translucent toolbar must be fully transparent, got {glass:?}");
    let solid = crate::desktop::themes::toolbar_band_surface(false, rail);
    assert_eq!(solid.h, rail.h, "opaque toolbar keeps the rail fill");
    assert_eq!(solid.s, rail.s, "opaque toolbar keeps the rail fill");
    assert_eq!(solid.l, rail.l, "opaque toolbar keeps the rail fill");
    assert_eq!(solid.a, rail.a, "opaque toolbar keeps the rail fill");
}

/// The band fill and the quiet icon tone come from one derivation. Translucent
/// mode cannot be rendered headlessly, but its backdrop is modeled by the
/// rail, so the icon must keep the same tone in both modes.
#[test]
fn toolbar_chrome_shares_one_surface_model_across_modes() {
    let background = gpui_kit::Hsla { h: 0.6, s: 0.1, l: 0.95, a: 1.0 };
    let secondary = gpui_kit::Hsla { h: 0.6, s: 0.1, l: 0.88, a: 1.0 };
    let foreground = gpui_kit::Hsla { h: 0.6, s: 0.1, l: 0.12, a: 1.0 };

    let opaque = crate::desktop::themes::toolbar_chrome(false, background, secondary, foreground);
    let translucent = crate::desktop::themes::toolbar_chrome(true, background, secondary, foreground);

    assert_eq!(opaque.band.a, 1.0, "opaque band is painted");
    assert_eq!(translucent.band.a, 0.0, "translucent band is empty glass");
    assert_ne!(opaque.band, translucent.band, "the band itself changes with the mode");
    assert_eq!(opaque.icon_tone, translucent.icon_tone, "the icon tone models the same rail in both modes");
}
