//! regressions for the accessibility pane controls.

use super::super::settings::SettingsTab;
use super::*;

#[gpui_kit::test]
fn high_contrast_switch_drives_the_slider_and_restores_a_custom_value(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.set_contrast(60.0, window, cx);
            view.settings_tab = SettingsTab::Accessibility;
            view.open_settings(window, cx);
        });
        window.render_frame(cx);
        scroll_settings_by(window, px(-3000.), cx);
        window.render_frame(cx);
        assert_eq!(window.find("high-contrast").checked(), Some(false));
        window.click("high-contrast", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(view.read(cx).preferences.high_contrast);
        assert_eq!(view.read(cx).preferences.contrast(), 100);
        assert_eq!(view.read(cx).contrast_slider.read(cx).value().start(), 100.0);
        assert!(crate::desktop::preferences::Preferences::load().high_contrast);
        assert_eq!(window.find("high-contrast").checked(), Some(true));

        // Manual slider control ends the overlay and keeps the chosen value.
        // The real slider persists once on Release; do that explicitly here.
        view.update(cx, |view, cx| {
            view.set_contrast(80.0, window, cx);
            view.save_preferences();
        });
        assert!(!view.read(cx).preferences.high_contrast);
        assert!(!crate::desktop::preferences::Preferences::load().high_contrast);
    })
    .unwrap();
}

#[gpui_kit::test]
fn high_contrast_switch_restores_the_saved_contrast_when_disabled(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.set_contrast(45.0, window, cx);
            view.set_high_contrast(true, window, cx);
        });
        assert_eq!(view.read(cx).preferences.contrast(), 100);
        view.update(cx, |view, cx| view.set_high_contrast(false, window, cx));
        assert_eq!(view.read(cx).preferences.contrast(), 45);
        assert_eq!(crate::desktop::preferences::Preferences::load().contrast(), 45);
    })
    .unwrap();
}
