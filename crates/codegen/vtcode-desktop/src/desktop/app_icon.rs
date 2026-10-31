//! Application-icon artwork: embedded PNGs, Dock application, and picker previews.
//!
//! The macOS bundle ships the `Dynamic` split tile as its static icon. At
//! runtime the picker selection resolves to light/dark art: explicit
//! variants pin their artwork, while `Dynamic` follows the active theme
//! mode (per the artwork README, automatic switching selects Light or Dark
//! art at runtime; the split tile only previews the `Dynamic` option).

use gpui_kit::component::theme::ThemeMode;
use vtcode_commons::ui_protocol::AppIconPreference;

const LIGHT_PNG: &[u8] = include_bytes!("../../../../../resources/icons/vtcode-app-light-512.png");
const DARK_PNG: &[u8] = include_bytes!("../../../../../resources/icons/vtcode-app-dark-512.png");
const DYNAMIC_PNG: &[u8] = include_bytes!("../../../../../resources/icons/vtcode-app-dynamic-512.png");

/// Artwork bytes for a preference under the given theme mode.
pub(super) fn png_bytes(preference: AppIconPreference, mode: ThemeMode) -> &'static [u8] {
    if preference.is_dark(mode == ThemeMode::Dark) {
        DARK_PNG
    } else {
        LIGHT_PNG
    }
}

/// Preview bytes for the Appearance picker: each option shows its own art,
/// with the split tile standing in for `Dynamic`.
pub(super) fn preview_bytes(preference: AppIconPreference) -> &'static [u8] {
    match preference {
        AppIconPreference::Light => LIGHT_PNG,
        AppIconPreference::Dark => DARK_PNG,
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

    use super::{DARK_PNG, DYNAMIC_PNG, LIGHT_PNG, apply_dock_icon, png_bytes, preview_bytes};

    #[test]
    fn embedded_artwork_is_valid_distinct_png() {
        for png in [LIGHT_PNG, DARK_PNG, DYNAMIC_PNG] {
            assert!(png.len() > 8, "artwork must be non-empty");
            assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n", "artwork must be a PNG");
        }
        assert_ne!(LIGHT_PNG, DARK_PNG);
        assert_ne!(LIGHT_PNG, DYNAMIC_PNG);
        assert_ne!(DARK_PNG, DYNAMIC_PNG);
    }

    #[test]
    fn png_bytes_pins_explicit_variants_and_follows_mode_for_dynamic() {
        for mode in [ThemeMode::Light, ThemeMode::Dark] {
            assert_eq!(png_bytes(AppIconPreference::Light, mode), LIGHT_PNG);
            assert_eq!(png_bytes(AppIconPreference::Dark, mode), DARK_PNG);
        }
        assert_eq!(png_bytes(AppIconPreference::Dynamic, ThemeMode::Light), LIGHT_PNG);
        assert_eq!(png_bytes(AppIconPreference::Dynamic, ThemeMode::Dark), DARK_PNG);
        assert_eq!(png_bytes(AppIconPreference::Unknown, ThemeMode::Dark), DARK_PNG);
    }

    #[test]
    fn preview_bytes_shows_split_tile_for_dynamic() {
        assert_eq!(preview_bytes(AppIconPreference::Light), LIGHT_PNG);
        assert_eq!(preview_bytes(AppIconPreference::Dark), DARK_PNG);
        assert_eq!(preview_bytes(AppIconPreference::Dynamic), DYNAMIC_PNG);
        assert_eq!(preview_bytes(AppIconPreference::Unknown), DYNAMIC_PNG);
    }

    #[test]
    fn dock_icon_apply_is_a_headless_no_op_in_tests() {
        assert!(!apply_dock_icon(LIGHT_PNG));
    }
}
