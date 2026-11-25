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
fn contrast_slider_release_and_discrete_change_persist_but_drag_ticks_do_not(cx: &mut TestAppContext) {
    use gpui_kit::component::slider::{SliderEvent, SliderValue};

    let (window, view) = open_desktop(cx);
    // Stage an unsaved in-memory value, then simulate a press that began on
    // the wrapper: a `Change` while dragging applies without an fsync-backed
    // save, and the pointer `Release` persists once and clears the flag.
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            let staged: u8 = if view.preferences.contrast() == 60 { 61 } else { 60 };
            view.set_contrast(f32::from(staged), window, cx);
            view.contrast_drag_active = true;
            view.contrast_slider.update(cx, |_, cx| {
                cx.emit(SliderEvent::Change(SliderValue::Single(f32::from(staged))));
            });
        });
    })
    .unwrap();
    cx.update_window(window, |_, _, cx| {
        let staged = view.read(cx).preferences.contrast();
        assert!(view.read(cx).contrast_drag_active);
        assert_ne!(
            crate::desktop::preferences::Preferences::load().contrast(),
            staged,
            "a drag tick applies in memory without persisting"
        );
        let slider = view.read(cx).contrast_slider.clone();
        slider.update(cx, |_, cx| cx.emit(SliderEvent::Release(SliderValue::Single(f32::from(staged)))));
    })
    .unwrap();
    cx.update_window(window, |_, window, cx| {
        let staged = view.read(cx).preferences.contrast();
        assert!(!view.read(cx).contrast_drag_active);
        assert_eq!(
            crate::desktop::preferences::Preferences::load().contrast(),
            staged,
            "pointer release persists the dragged value"
        );
        // A discrete `Change` with no drag behind it (an assistive-technology
        // step) persists on its own; the `Release` Kit emits right after is an
        // idempotent second save.
        let stepped: u8 = if staged == 70 { 71 } else { 70 };
        view.update(cx, |view, cx| {
            view.set_contrast(f32::from(stepped), window, cx);
            view.contrast_slider.update(cx, |_, cx| {
                cx.emit(SliderEvent::Change(SliderValue::Single(f32::from(stepped))));
            });
        });
    })
    .unwrap();
    cx.update_window(window, |_, _, cx| {
        let stepped = view.read(cx).preferences.contrast();
        assert_eq!(
            crate::desktop::preferences::Preferences::load().contrast(),
            stepped,
            "a discrete change persists without waiting for release"
        );
        let slider = view.read(cx).contrast_slider.clone();
        slider.update(cx, |_, cx| cx.emit(SliderEvent::Release(SliderValue::Single(f32::from(stepped)))));
    })
    .unwrap();
    cx.update_window(window, |_, _, cx| {
        assert!(!view.read(cx).contrast_drag_active);
        assert_eq!(
            crate::desktop::preferences::Preferences::load().contrast(),
            view.read(cx).preferences.contrast(),
            "the paired release keeps the saved value"
        );
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
