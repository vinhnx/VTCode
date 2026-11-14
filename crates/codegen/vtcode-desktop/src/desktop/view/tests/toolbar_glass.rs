/// The translucent toolbar must not paint a band: any tinted fill seams
/// against the unpainted rail and pure-blur gaps around it and reads as an
/// opaque slab, so the bar floats fully transparent on the window vibrancy
/// like the rail. Opaque mode keeps the solid rail fill.
#[test]
fn translucent_toolbar_leaves_no_band() {
    let rail = gpui_kit::Hsla { h: 0.1, s: 0.2, l: 0.3, a: 1.0 };
    let glass = super::super::toolbar_band_surface(true, rail);
    assert_eq!(glass.a, 0.0, "translucent toolbar must be fully transparent, got {glass:?}");
    let solid = super::super::toolbar_band_surface(false, rail);
    assert_eq!(solid.h, rail.h, "opaque toolbar keeps the rail fill");
    assert_eq!(solid.s, rail.s, "opaque toolbar keeps the rail fill");
    assert_eq!(solid.l, rail.l, "opaque toolbar keeps the rail fill");
    assert_eq!(solid.a, rail.a, "opaque toolbar keeps the rail fill");
}
