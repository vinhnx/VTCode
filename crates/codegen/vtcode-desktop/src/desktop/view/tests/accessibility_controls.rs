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

        // Keyboard control is the path assistive technology uses. Driving it
        // through the real handler must end the overlay and persist the value
        // without a slider Release event.
        let slider_focus = view.read(cx).contrast_focus.clone();
        window.focus(&slider_focus, cx);
        window.press("left", cx);
        window.press("left", cx);
        window.render_frame(cx);
        assert_eq!(view.read(cx).preferences.contrast(), 98);
        assert_eq!(view.read(cx).contrast_slider.read(cx).value().start(), 98.0);
        assert!(!view.read(cx).preferences.high_contrast);
        assert_eq!(crate::desktop::preferences::Preferences::load().contrast(), 98);
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

#[gpui_kit::test]
fn contrast_slider_is_a_single_keyboard_stop(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.settings_tab = SettingsTab::Accessibility;
            view.open_settings(window, cx);
        });
        window.render_frame(cx);
        scroll_settings_by(window, px(-3000.), cx);
        window.render_frame(cx);
        focus_element_by_tab(window, "contrast-slider-control", cx);
        let slider_id = ("slider", view.read(cx).contrast_slider.entity_id());
        assert_eq!(window.find("contrast-slider-control").focused(), Some(true));
        assert_eq!(window.find("contrast-slider-control").role(), Some(gpui_kit::Role::Group));
        assert_eq!(window.find("contrast-slider-control").label(), Some("Contrast"));
        assert_eq!(window.find(slider_id).role(), Some(gpui_kit::Role::Slider));
        assert_eq!(window.find(slider_id).focused(), Some(false));
        assert_ne!(window.find(slider_id).focused(), Some(true), "the inner Kit slider must not add a second Tab stop");
        window.press("tab", cx);
        window.render_frame(cx);
        assert_ne!(window.find(slider_id).focused(), Some(true), "one Tab leaves the contrast control entirely");
        assert_eq!(window.find("contrast-slider-control").focused(), Some(false));
    })
    .unwrap();
}
