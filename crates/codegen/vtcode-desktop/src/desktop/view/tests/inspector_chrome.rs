use super::*;

#[gpui_kit::test]
fn inspector_tabs_expose_selection_and_keyboard_navigation(cx: &mut TestAppContext) {
    let (window, view) = open_desktop_sized(cx, 600., 900.);
    // Narrow the rail so the scroll-reveal assertions below hold by
    // construction; the default width fits every tab (see the fit test).
    view.update(cx, |view, _| view.inspector_width = 240.);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.within("inspector-tabs").find(0usize).role(), Some(gpui_kit::Role::Tab));
        assert_eq!(window.within("inspector-tabs").find(0usize).selected(), Some(true));
        window.within("inspector-tabs").click(1usize, cx);
        window.render_frame(cx);
        assert_eq!(view.read(cx).inspector_tab, crate::desktop::inspector::InspectorTab::Files);
        assert_eq!(window.within("inspector-tabs").find(1usize).selected(), Some(true));
        for (key, expected) in [
            ("right", crate::desktop::inspector::InspectorTab::Activity),
            ("end", crate::desktop::inspector::InspectorTab::Utilities),
            ("home", crate::desktop::inspector::InspectorTab::Changes),
            ("left", crate::desktop::inspector::InspectorTab::Utilities),
            ("right", crate::desktop::inspector::InspectorTab::Changes),
        ] {
            window.press(key, cx);
            window.render_frame(cx);
            assert_eq!(view.read(cx).inspector_tab, expected, "{key}");
            if key == "end" || key == "left" {
                let scroll = &view.read(cx).inspector_tabs_scroll;
                let visible = horizontal_scroll_item_is_visible(scroll, 4);
                let offset = scroll.offset().x;
                assert!(visible, "keyboard selection reveals the final inspector tab; scroll offset={offset:?}");
                assert!(offset < px(0.), "narrow inspector tab row must scroll to its final tab");
            }
        }
        window.press("cmd-l", cx);
        window.input("composer text", cx);
        window.press("left", cx);
        assert_eq!(view.read(cx).inspector_tab, crate::desktop::inspector::InspectorTab::Changes);
        assert_eq!(view.read(cx).composer.read(cx).value(), "composer text");
    })
    .unwrap();
}

#[gpui_kit::test]
fn inspector_tabs_fit_without_scrolling_at_default_width(cx: &mut TestAppContext) {
    let (window, view) = open_desktop_sized(cx, 1280., 900.);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(
            (view.read(cx).inspector_width - 474.).abs() < f32::EPSILON,
            "baseline pins the default width this fit is measured against"
        );
        let bar = window.find("inspector-tabs-focus").bounds();
        // No approval pending here, so the rail holds the five persistent
        // tabs; Plan joins only while a plan waits (see the plan-tab tests).
        let tab_count = view.read(cx).visible_inspector_tabs().len();
        for index in 0..tab_count {
            let tab = window.within("inspector-tabs").find(index).bounds();
            assert!(
                tab.left() >= bar.left() && tab.right() <= bar.right(),
                "tab {index} overflows the rail at default width: {tab:?} vs {bar:?}"
            );
        }
    })
    .unwrap();
}

#[gpui_kit::test]
fn hiding_a_focused_inspector_restores_the_desktop_focus_path(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        view.update(cx, |view, cx| view.inspector_focus.focus(window, cx));
        window.render_frame(cx);
        assert!(view.read(cx).inspector_focus.is_focused(window));

        view.update(cx, |view, cx| view.toggle_inspector(cx));
        window.render_frame(cx);

        assert!(!view.read(cx).inspector_open);
        assert!(view.read(cx).focus_handle.is_focused(window));
        window.press("cmd-l", cx);
        window.render_frame(cx);
        assert!(view.read(cx).composer.read(cx).focus_handle(cx).is_focused(window));
    })
    .unwrap();
}

#[gpui_kit::test]
fn inspector_programmatic_selection_reveals_selected_tab(cx: &mut TestAppContext) {
    let (window, view) = open_desktop_sized(cx, 600., 900.);
    // Narrow the rail so the scroll-reveal assertions below hold by
    // construction; the default width fits every tab (see the fit test).
    view.update(cx, |view, _| view.inspector_width = 240.);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        view.update(cx, |view, cx| view.open_utilities(cx));
        window.render_frame(cx);
        assert_eq!(view.read(cx).inspector_tab, crate::desktop::inspector::InspectorTab::Utilities);
        assert!(
            horizontal_scroll_item_is_visible(&view.read(cx).inspector_tabs_scroll, 4),
            "workflow selection reveals the selected inspector tab"
        );
        assert!(
            view.read(cx).inspector_tabs_scroll.offset().x < px(0.),
            "workflow selection must scroll the narrow inspector tab row"
        );
    })
    .unwrap();
}
