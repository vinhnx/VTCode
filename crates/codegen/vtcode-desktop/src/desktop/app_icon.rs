//! Application-icon artwork: embedded PNGs, Dock application, and picker previews.
//!
//! The macOS bundle ships the `Dynamic` split tile as its static icon. At
//! runtime the picker selection resolves to concrete art: explicit variants
//! (`Light`/`Dark`/`Clear`/`Tinted`) pin their artwork, while `Dynamic`
//! follows the active theme mode on pre-Tahoe systems (per the artwork
//! README, automatic switching selects Light or Dark art at runtime; the
//! split tile only previews the `Dynamic` option). On macOS 26+ the bundled
//! layered icon is system-composed in Liquid Glass, so `Dynamic` defers to
//! the bundle icon instead of overriding it with a flat raster; only
//! explicit pins override the Dock tile there.
//!
//! Two sizes are embedded: 512px for the Dock application and 128px for
//! small in-app renders (picker tiles, brand logo), where the 512px art
//! would minify 8x through a mipmap-less sampler and render stair-stepped
//! edges (see `preview_bytes`).
//!
//! Shape contract: macOS clips neither `.icns` art nor
//! `setApplicationIconImage` images to the system icon shape, so the tiled
//! embedded PNGs must carry the rounded squircle with transparency baked in
//! (see `scripts/make-macos-app-icons.py`). Full-bleed opaque squares render
//! as squares in the Dock; the `embedded_artwork_carries_macos_rounded_shape`
//! test pins both sides of that boundary. `Clear` carries the same squircle
//! as a subtle translucent glass tile (flat PNGs get no system glass
//! composition, so the regen script bakes the frosted fill, rim, and edge
//! hairline); without it the V floats borderless next to tiled neighbors.
//! The `embedded_clear_artwork_is_a_see_through_glass_v` test pins that tile.

use gpui_kit::component::theme::ThemeMode;
use vtcode_commons::ui_protocol::AppIconPreference;

/// First macOS major where the bundle icon is system-composed Liquid Glass.
/// At or above this version `Dynamic` defers to the bundle icon.
const LIQUID_GLASS_MACOS_MAJOR: isize = 26;

/// Whether the Dock tile should defer to the system-composed bundle icon.
///
/// Pure predicate (separate from the AppKit version query) so the contract
/// is testable without a Dock. `Dynamic`/`Unknown` defer at or above macOS
/// 26; explicit pins always override because the bundle icon is fixed at
/// packaging time and cannot follow a pin.
#[must_use]
pub(super) const fn prefers_system_dock_icon(preference: AppIconPreference, macos_major: isize) -> bool {
    matches!(preference, AppIconPreference::Dynamic | AppIconPreference::Unknown)
        && macos_major >= LIQUID_GLASS_MACOS_MAJOR
}

const LIGHT_PNG: &[u8] = include_bytes!("../../../../../resources/icons/vtcode-app-light-512.png");
const DARK_PNG: &[u8] = include_bytes!("../../../../../resources/icons/vtcode-app-dark-512.png");
/// The 512px split tile has no runtime consumer (the Dock resolves `Dynamic`
/// by theme mode; previews use the 128px art) — it is embedded only so the
/// shape tests pin the file the bundle packaging consumes.
#[cfg(test)]
const DYNAMIC_PNG: &[u8] = include_bytes!("../../../../../resources/icons/vtcode-app-dynamic-512.png");
const CLEAR_PNG: &[u8] = include_bytes!("../../../../../resources/icons/vtcode-app-clear-512.png");
const TINTED_PNG: &[u8] = include_bytes!("../../../../../resources/icons/vtcode-app-tinted-512.png");
const LIGHT_128_PNG: &[u8] = include_bytes!("../../../../../resources/icons/vtcode-app-light-128.png");
const DARK_128_PNG: &[u8] = include_bytes!("../../../../../resources/icons/vtcode-app-dark-128.png");
const DYNAMIC_128_PNG: &[u8] = include_bytes!("../../../../../resources/icons/vtcode-app-dynamic-128.png");
const CLEAR_128_PNG: &[u8] = include_bytes!("../../../../../resources/icons/vtcode-app-clear-128.png");
const TINTED_128_PNG: &[u8] = include_bytes!("../../../../../resources/icons/vtcode-app-tinted-128.png");

/// Artwork bytes for a preference under the given theme mode.
pub(super) fn png_bytes(preference: AppIconPreference, mode: ThemeMode) -> &'static [u8] {
    match preference {
        AppIconPreference::Light => LIGHT_PNG,
        AppIconPreference::Dark => DARK_PNG,
        AppIconPreference::Clear => CLEAR_PNG,
        AppIconPreference::Tinted => TINTED_PNG,
        AppIconPreference::Dynamic | AppIconPreference::Unknown => {
            if mode == ThemeMode::Dark {
                DARK_PNG
            } else {
                LIGHT_PNG
            }
        }
    }
}

/// Preview bytes for small in-app renders (the Appearance picker tiles, the
/// brand logo): each option shows its own art, with the split tile standing
/// in for `Dynamic`.
///
/// Display-sized 128px art, not the 512px Dock art: the GPU sampler minifies
/// without mipmaps, so an 8x-minified 512px texture renders hard
/// stair-stepped edges, while 128px is 1:1 on Retina and a gentle 2x
/// minification on 1x screens (and 16x less texture memory per variant).
pub(super) fn preview_bytes(preference: AppIconPreference) -> &'static [u8] {
    match preference {
        AppIconPreference::Light => LIGHT_128_PNG,
        AppIconPreference::Dark => DARK_128_PNG,
        AppIconPreference::Clear => CLEAR_128_PNG,
        AppIconPreference::Tinted => TINTED_128_PNG,
        AppIconPreference::Dynamic | AppIconPreference::Unknown => DYNAMIC_128_PNG,
    }
}

/// Apply PNG artwork as the running app's Dock icon.
///
/// Returns false off the main thread or when AppKit rejects the image;
/// callers treat failure as a no-op since the bundle icon remains. The
/// headless test platform has no Dock, so tests take the stub below.
#[cfg(not(test))]
#[expect(unsafe_code, reason = "AppKit dock icon setter is an unsafe Objective-C call")]
pub(super) fn apply_dock_icon(png: &'static [u8]) -> bool {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSApplication, NSImage};
    use objc2_foundation::NSData;

    let Some(mtm) = MainThreadMarker::new() else {
        return false;
    };
    let data = NSData::with_bytes(png);
    let Some(image) = NSImage::initWithData(mtm.alloc::<NSImage>(), &data) else {
        return false;
    };
    let app = NSApplication::sharedApplication(mtm);
    // SAFETY: called on the main thread with a valid NSImage; the setter retains the image.
    unsafe {
        app.setApplicationIconImage(Some(&image));
    }
    true
}

#[cfg(test)]
pub(super) fn apply_dock_icon(_png: &'static [u8]) -> bool {
    false
}

/// Running macOS major version for the Dock gate. Headless tests never
/// query the process; the pure gate takes the major explicitly.
#[cfg(not(test))]
fn macos_major_version() -> isize {
    use objc2_foundation::NSProcessInfo;

    NSProcessInfo::processInfo().operatingSystemVersion().majorVersion
}

#[cfg(test)]
fn macos_major_version() -> isize {
    LIQUID_GLASS_MACOS_MAJOR
}

/// Clear a prior Dock override so the system-composed bundle icon returns.
///
/// Returns false off the main thread. Headless tests take the stub below.
#[cfg(not(test))]
#[expect(unsafe_code, reason = "AppKit dock icon setter is an unsafe Objective-C call")]
fn clear_dock_icon() -> bool {
    use objc2::MainThreadMarker;
    use objc2_app_kit::NSApplication;

    let Some(mtm) = MainThreadMarker::new() else {
        return false;
    };
    let app = NSApplication::sharedApplication(mtm);
    // SAFETY: called on the main thread; nil restores the bundle icon.
    unsafe {
        app.setApplicationIconImage(None);
    }
    true
}

#[cfg(test)]
fn clear_dock_icon() -> bool {
    false
}

/// Apply the saved preference to the Dock, deferring to the bundle icon on
/// Tahoe+ `Dynamic`. The single entry point so startup, picker, theme-change,
/// and reset cannot drift.
pub(super) fn apply_dock_preference(preference: AppIconPreference, mode: ThemeMode) -> bool {
    if prefers_system_dock_icon(preference, macos_major_version()) {
        clear_dock_icon()
    } else {
        apply_dock_icon(png_bytes(preference, mode))
    }
}

#[cfg(test)]
mod tests {
    use gpui_kit::component::theme::ThemeMode;
    use vtcode_commons::ui_protocol::AppIconPreference;

    use super::{
        CLEAR_128_PNG, CLEAR_PNG, DARK_128_PNG, DARK_PNG, DYNAMIC_128_PNG, DYNAMIC_PNG, LIGHT_128_PNG, LIGHT_PNG,
        TINTED_128_PNG, TINTED_PNG, apply_dock_icon, clear_dock_icon, png_bytes, prefers_system_dock_icon,
        preview_bytes,
    };

    #[test]
    fn embedded_artwork_is_valid_distinct_png() {
        let variants = [
            LIGHT_PNG,
            DARK_PNG,
            DYNAMIC_PNG,
            CLEAR_PNG,
            TINTED_PNG,
            LIGHT_128_PNG,
            DARK_128_PNG,
            DYNAMIC_128_PNG,
            CLEAR_128_PNG,
            TINTED_128_PNG,
        ];
        for png in variants {
            assert!(png.len() > 8, "artwork must be non-empty");
            assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n", "artwork must be a PNG");
        }
        for (index, first) in variants.iter().enumerate() {
            for second in &variants[index + 1..] {
                assert_ne!(first, second, "each variant must ship its own art");
            }
        }
    }

    /// Shared inset-geometry predicate: every variant at every size carries
    /// transparent canvas margins (corners + edge midpoints), proving the
    /// stock content-box inset (fails on full-bleed squares, which also
    /// render oversized).
    fn assert_margins_transparent(art: &image::RgbaImage, label: &str) {
        let last = art.width() - 1;
        let mid = art.width() / 2;
        for (x, y) in [
            (0, 0),
            (last, 0),
            (0, last),
            (last, last),
            (mid, 0),
            (0, mid),
            (last, mid),
            (mid, last),
        ] {
            assert!(art.get_pixel(x, y)[3] < 16, "{label} margin ({x}, {y}) must be transparent");
        }
    }

    #[test]
    fn preview_artwork_is_display_sized_with_a_smooth_edge_ramp() {
        // In-app renders are small (the Appearance picker shows 64px tiles;
        // the brand logo shows at 40-64px) and the GPU sampler minifies
        // without mipmaps, so an 8x-minified 512px texture renders hard
        // stair-stepped edges. Preview bytes are the 128px art instead: 1:1
        // on Retina, a gentle 2x minification on 1x screens. Each tile must
        // also carry a smooth alpha ramp at asset scale, which is what the
        // bilinear sampler blends across (fails on hard-edged small art).
        // Probe geometry at 128: the content box is 103px at offset 12.5,
        // so the tile top edge crosses y≈12 (ramp scan 6..22, rim scan
        // 10..17) and the V counter is the center pixel.
        for preference in AppIconPreference::ALL {
            let art = image::load_from_memory(preview_bytes(preference))
                .expect("preview must decode")
                .to_rgba8();
            assert_eq!(
                (art.width(), art.height()),
                (128, 128),
                "{preference:?} preview must be display-sized, not Dock-sized"
            );
            let label = format!("{preference:?} 128 preview");
            assert_margins_transparent(&art, &label);
            if preference == AppIconPreference::Clear {
                let fill = art.get_pixel(64, 64)[3];
                assert!((8..=120).contains(&fill), "clear preview counter must show only the faint fill");
                let rim = (10..17).map(|y| art.get_pixel(64, y)[3]).max().unwrap_or(0);
                assert!(
                    rim > 50 && rim > fill.saturating_add(20),
                    "clear preview top edge must catch the rim, got alpha {rim} over fill {fill}"
                );
            } else {
                assert!(art.get_pixel(64, 64)[3] > 240, "{preference:?} preview center must stay opaque");
                assert!(
                    (6..22).any(|y| (16..240).contains(&art.get_pixel(64, y)[3])),
                    "{preference:?} preview top edge must carry a smooth ramp at asset scale"
                );
            }
        }
    }

    #[test]
    fn embedded_opaque_artwork_carries_liquid_glass_shading() {
        // Opaque tiles get no system composition when the Dock is overridden,
        // so the regen bakes Liquid Glass shading (top sheen + rim). Without
        // it the flat solid renders bias-free (fails on borderless art);
        // an overly-strong rim (fails on blown highlights) is also rejected.
        // Top interior probes the rim band, center probes flat background.
        // Expected sums are asymmetric: rim > center for every opaque variant.
        for png in [LIGHT_PNG, DARK_PNG, TINTED_PNG] {
            let art = image::load_from_memory(png).expect("artwork must decode").to_rgba8();
            let mid = 256;
            let top = art.get_pixel(mid, 52);
            let center = art.get_pixel(mid, 256);
            let top_sum = top.0[0] as u32 + top.0[1] as u32 + top.0[2] as u32;
            let center_sum = center.0[0] as u32 + center.0[1] as u32 + center.0[2] as u32;
            assert!(
                top_sum > center_sum && top_sum <= center_sum + 260,
                "512 top rim must be lighter than center (shading) without blowing out: top={:?} center={:?}",
                top,
                center
            );
            assert!(top[3] > 240 && center[3] > 240, "opaque shading must keep tile opaque");
        }
        // Preview scale mirrors the Dock shading at display size: same
        // top-vs-center asymmetry, smaller absolute delta.
        for png in [LIGHT_128_PNG, DARK_128_PNG, TINTED_128_PNG] {
            let art = image::load_from_memory(png).expect("preview must decode").to_rgba8();
            let mid = 64;
            let top = art.get_pixel(mid, 14);
            let center = art.get_pixel(mid, 64);
            let top_sum = top.0[0] as u32 + top.0[1] as u32 + top.0[2] as u32;
            let center_sum = center.0[0] as u32 + center.0[1] as u32 + center.0[2] as u32;
            assert!(
                top_sum > center_sum && top_sum <= center_sum + 260,
                "128 top rim must be lighter than center (shading): top={:?} center={:?}",
                top,
                center
            );
        }
    }

    #[test]
    fn embedded_artwork_carries_macos_rounded_shape() {
        // Stock inset geometry: the 824/1024 content box leaves 50px
        // transparent margins at 512, so corners AND edge midpoints are clear
        // (fails on full-bleed squares, which also render oversized), while
        // just-inside points, a near-corner interior point, and the center
        // stay opaque (fails on over-aggressive masks or mostly-transparent
        // art).
        for png in [LIGHT_PNG, DARK_PNG, DYNAMIC_PNG, TINTED_PNG] {
            let art = image::load_from_memory(png).expect("artwork must decode").to_rgba8();
            assert_eq!((art.width(), art.height()), (512, 512));
            assert_margins_transparent(&art, "512 tile");
            for (x, y) in [(256, 64), (64, 256), (447, 256), (256, 447), (100, 100), (256, 256)] {
                assert!(art.get_pixel(x, y)[3] > 240, "interior ({x}, {y}) must stay opaque");
            }
        }
    }

    #[test]
    fn embedded_clear_artwork_is_a_see_through_glass_v() {
        // Clear carries a subtle translucent tile under the glass V: margins
        // stay transparent (fails on full-bleed art), the V counter and tile
        // interior show only the faint fill (fails on missing tiles AND on
        // opaque tiles mislabeled as Clear), just inside the top edge the
        // bright rim reads well above the fill without blowing out (fails
        // on borderless art and on opaque rims), and the upper V strokes
        // stay opaque (fails on empty/missing glyphs) with the opaque
        // fraction in the narrow glass-V band the regen script enforces
        // at 1024.
        let art = image::load_from_memory(CLEAR_PNG).expect("artwork must decode").to_rgba8();
        assert_eq!((art.width(), art.height()), (512, 512));
        assert_margins_transparent(&art, "512 clear tile");
        for (x, y) in [(256, 64), (64, 256), (256, 256)] {
            let alpha = art.get_pixel(x, y)[3];
            assert!(
                (8..=120).contains(&alpha),
                "clear tile ({x}, {y}) must show only the faint fill, got alpha {alpha}"
            );
        }
        let fill = art.get_pixel(256, 256)[3];
        let rim = art.get_pixel(256, 51)[3];
        assert!(
            rim > 70 && rim <= 200 && rim > fill.saturating_add(40),
            "clear top edge must catch the bright-but-subtle rim, got alpha {rim} over fill {fill}"
        );
        for (x, y) in [(150, 150), (362, 150)] {
            assert!(art.get_pixel(x, y)[3] > 240, "clear stroke ({x}, {y}) must stay opaque");
        }
        let opaque = art.pixels().filter(|pixel| pixel[3] >= 248).count();
        let fraction = opaque as f64 / f64::from(art.width() * art.height());
        assert!(
            (0.005..=0.15).contains(&fraction),
            "clear opaque fraction {fraction:.3} outside the 0.005-0.15 glass-V band"
        );
    }

    #[test]
    fn png_bytes_pins_explicit_variants_and_follows_mode_for_dynamic() {
        for mode in [ThemeMode::Light, ThemeMode::Dark] {
            assert_eq!(png_bytes(AppIconPreference::Light, mode), LIGHT_PNG);
            assert_eq!(png_bytes(AppIconPreference::Dark, mode), DARK_PNG);
            assert_eq!(png_bytes(AppIconPreference::Clear, mode), CLEAR_PNG);
            assert_eq!(png_bytes(AppIconPreference::Tinted, mode), TINTED_PNG);
        }
        assert_eq!(png_bytes(AppIconPreference::Dynamic, ThemeMode::Light), LIGHT_PNG);
        assert_eq!(png_bytes(AppIconPreference::Dynamic, ThemeMode::Dark), DARK_PNG);
        assert_eq!(png_bytes(AppIconPreference::Unknown, ThemeMode::Dark), DARK_PNG);
    }

    #[test]
    fn preview_bytes_shows_split_tile_for_dynamic() {
        assert_eq!(preview_bytes(AppIconPreference::Light), LIGHT_128_PNG);
        assert_eq!(preview_bytes(AppIconPreference::Dark), DARK_128_PNG);
        assert_eq!(preview_bytes(AppIconPreference::Clear), CLEAR_128_PNG);
        assert_eq!(preview_bytes(AppIconPreference::Tinted), TINTED_128_PNG);
        assert_eq!(preview_bytes(AppIconPreference::Dynamic), DYNAMIC_128_PNG);
        assert_eq!(preview_bytes(AppIconPreference::Unknown), DYNAMIC_128_PNG);
    }

    #[test]
    fn dock_icon_apply_is_a_headless_no_op_in_tests() {
        assert!(!apply_dock_icon(LIGHT_PNG));
    }

    #[test]
    fn dock_clear_is_a_headless_no_op_in_tests() {
        assert!(!clear_dock_icon());
    }

    #[test]
    fn prefers_system_dock_icon_defers_only_dynamic_on_tahoe() {
        // Tahoe boundary: Dynamic defers at and above the Liquid Glass major;
        // below it each mode resolves to Light/Dark art (pre-Tahoe bundle
        // cannot compose appearances). Pins never defer at any version.
        for major in [26, 27, 28] {
            assert!(prefers_system_dock_icon(AppIconPreference::Dynamic, major), "Dynamic defers on {major}");
            assert!(prefers_system_dock_icon(AppIconPreference::Unknown, major), "Unknown defers on {major}");
            for pin in [
                AppIconPreference::Light,
                AppIconPreference::Dark,
                AppIconPreference::Clear,
                AppIconPreference::Tinted,
            ] {
                assert!(!prefers_system_dock_icon(pin, major), "{pin:?} must override on {major}");
            }
        }
        for major in [15, 21, 25] {
            for preference in [
                AppIconPreference::Dynamic,
                AppIconPreference::Unknown,
                AppIconPreference::Light,
                AppIconPreference::Dark,
                AppIconPreference::Clear,
                AppIconPreference::Tinted,
            ] {
                assert!(!prefers_system_dock_icon(preference, major), "{preference:?} must override on {major}");
            }
        }
    }
}
