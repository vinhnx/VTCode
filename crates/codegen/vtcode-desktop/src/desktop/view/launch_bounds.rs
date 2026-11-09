//! Launch-window geometry: base options plus the maximized visible-frame probe.
//!
//! A maximized launch fills the main screen's visible frame explicitly (see
//! `main_screen_visible_bounds`): AppKit `zoom:` targets the content-derived
//! standard frame, which leaves gaps and never satisfies `is_maximized`.
use super::{TRAFFIC_LIGHT_X, TRAFFIC_LIGHT_Y};
use gpui_kit::component::TitleBar;
use gpui_kit::{Bounds, Pixels, WindowOptions, point, px, size};
use objc2_foundation::NSRect;

pub(super) fn desktop_window_options() -> WindowOptions {
    let mut options = TitleBar::window_options();
    if let Some(titlebar) = options.titlebar.as_mut() {
        titlebar.traffic_light_position = Some(point(px(TRAFFIC_LIGHT_X), px(TRAFFIC_LIGHT_Y)));
    }
    // Smallest geometry that keeps every surface non-degenerate: 600x560 is
    // the smallest headless-tested size with all suites green, while 400px
    // tall provably starves flex content (the config header plus rail alone
    // exceed the row). The platform enforces this; headless tests may still
    // open smaller windows to stress overflow paths.
    options.window_min_size = Some(size(px(600.), px(560.)));
    options
}

/// Visible frame of the main screen in GPUI's top-left-origin coordinates,
/// or `None` when AppKit has no answer. `None` keeps the zoom fallback, so
/// every probe failure degrades to the previous launch behavior instead of
/// a broken window. The headless test platform has no screens, so tests
/// take the stub below.
#[cfg(not(test))]
pub(super) fn main_screen_visible_bounds() -> Option<Bounds<Pixels>> {
    use objc2::MainThreadMarker;
    use objc2_app_kit::NSScreen;

    let mtm = MainThreadMarker::new()?;
    let screen = NSScreen::mainScreen(mtm)?;
    visible_bounds_from_frames(screen.frame(), screen.visibleFrame())
}

#[cfg(test)]
pub(super) fn main_screen_visible_bounds() -> Option<Bounds<Pixels>> {
    None
}

/// Convert AppKit's bottom-left-origin screen frames to GPUI bounds. Pure
/// so headless tests pin the flip without AppKit; degenerate or escaped
/// visible frames fall back to zoom.
#[expect(
    clippy::cast_possible_truncation,
    reason = "screen coordinates are far below f32 precision limits"
)]
pub(super) fn visible_bounds_from_frames(frame: NSRect, visible: NSRect) -> Option<Bounds<Pixels>> {
    let width = visible.size.width;
    let height = visible.size.height;
    if !(width > 0.0 && height > 0.0) {
        return None;
    }
    let origin_x = visible.origin.x - frame.origin.x;
    let origin_y = (frame.origin.y + frame.size.height) - (visible.origin.y + visible.size.height);
    if !(origin_x >= 0.0 && origin_y >= 0.0) {
        return None;
    }
    Some(Bounds::new(
        point(px(origin_x as f32), px(origin_y as f32)),
        size(px(width as f32), px(height as f32)),
    ))
}
