//! Application-icon artwork: embedded PNGs, Dock application, and picker previews.
//!
//! The macOS bundle ships the `Dynamic` split tile as its static icon. At
//! runtime the picker selection resolves to concrete art: explicit variants
//! (`Light`/`Dark`/`Clear`/`Tinted`) pin their artwork, while `Dynamic`
//! follows the active theme mode (per the artwork README, automatic
//! switching selects Light or Dark art at runtime; the split tile only
//! previews the `Dynamic` option).
//!
//! Shape contract: macOS clips neither `.icns` art nor
//! `setApplicationIconImage` images to the system icon shape, so the tiled
//! embedded PNGs must carry the rounded squircle with transparency baked in
//! (see `scripts/make-macos-app-icons.py`). Full-bleed opaque squares render
//! as squares in the Dock; the `embedded_artwork_carries_macos_rounded_shape`
//! test pins both sides of that boundary. `Clear` is the exception: it
//! carries no tile, only the glass V on transparency, and is pinned by its
//! own see-through assertions in the same test.

use gpui_kit::component::theme::ThemeMode;
use vtcode_commons::ui_protocol::AppIconPreference;

const LIGHT_PNG: &[u8] = include_bytes!("../../../../../resources/icons/vtcode-app-light-512.png");
const DARK_PNG: &[u8] = include_bytes!("../../../../../resources/icons/vtcode-app-dark-512.png");
const DYNAMIC_PNG: &[u8] = include_bytes!("../../../../../resources/icons/vtcode-app-dynamic-512.png");
const CLEAR_PNG: &[u8] = include_bytes!("../../../../../resources/icons/vtcode-app-clear-512.png");
const TINTED_PNG: &[u8] = include_bytes!("../../../../../resources/icons/vtcode-app-tinted-512.png");

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

/// Preview bytes for the Appearance picker: each option shows its own art,
/// with the split tile standing in for `Dynamic`.
pub(super) fn preview_bytes(preference: AppIconPreference) -> &'static [u8] {
    match preference {
        AppIconPreference::Light => LIGHT_PNG,
        AppIconPreference::Dark => DARK_PNG,
        AppIconPreference::Clear => CLEAR_PNG,
        AppIconPreference::Tinted => TINTED_PNG,
        AppIconPreference::Dynamic | AppIconPreference::Unknown => DYNAMIC_PNG,
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

#[cfg(test)]
mod tests {
    use gpui_kit::component::theme::ThemeMode;
    use vtcode_commons::ui_protocol::AppIconPreference;

    use super::{CLEAR_PNG, DARK_PNG, DYNAMIC_PNG, LIGHT_PNG, TINTED_PNG, apply_dock_icon, png_bytes, preview_bytes};

    #[test]
    fn embedded_artwork_is_valid_distinct_png() {
        let variants = [LIGHT_PNG, DARK_PNG, DYNAMIC_PNG, CLEAR_PNG, TINTED_PNG];
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
            let last = 511;
            for (x, y) in [
                (0, 0),
                (last, 0),
                (0, last),
                (last, last),
                (256, 0),
                (0, 256),
                (last, 256),
                (256, last),
            ] {
                assert!(art.get_pixel(x, y)[3] < 16, "margin ({x}, {y}) must be transparent");
            }
            for (x, y) in [(256, 64), (64, 256), (447, 256), (256, 447), (100, 100), (256, 256)] {
                assert!(art.get_pixel(x, y)[3] > 240, "interior ({x}, {y}) must stay opaque");
            }
        }
    }

    #[test]
    fn embedded_clear_artwork_is_a_see_through_glass_v() {
        // Clear carries no tile: margins AND the V counter stay transparent
        // (fails on opaque tiles mislabeled as Clear), while the upper V
        // strokes stay opaque (fails on empty/missing glyphs), and the
        // opaque fraction sits in the narrow glass-V band the regen script
        // enforces at 1024.
        let art = image::load_from_memory(CLEAR_PNG).expect("artwork must decode").to_rgba8();
        assert_eq!((art.width(), art.height()), (512, 512));
        let last = 511;
        for (x, y) in [
            (0, 0),
            (last, 0),
            (0, last),
            (last, last),
            (256, 0),
            (0, 256),
            (last, 256),
            (256, last),
            (256, 64),
            (64, 256),
            (256, 256),
        ] {
            assert!(art.get_pixel(x, y)[3] < 16, "clear ({x}, {y}) must be transparent");
        }
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
        assert_eq!(preview_bytes(AppIconPreference::Light), LIGHT_PNG);
        assert_eq!(preview_bytes(AppIconPreference::Dark), DARK_PNG);
        assert_eq!(preview_bytes(AppIconPreference::Clear), CLEAR_PNG);
        assert_eq!(preview_bytes(AppIconPreference::Tinted), TINTED_PNG);
        assert_eq!(preview_bytes(AppIconPreference::Dynamic), DYNAMIC_PNG);
        assert_eq!(preview_bytes(AppIconPreference::Unknown), DYNAMIC_PNG);
    }

    #[test]
    fn dock_icon_apply_is_a_headless_no_op_in_tests() {
        assert!(!apply_dock_icon(LIGHT_PNG));
    }
}
