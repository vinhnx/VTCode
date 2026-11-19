use super::*;

#[gpui_kit::test]
fn settings_navigation_selects_tabs_and_config_with_wrapping(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update(|cx| cx.set_reduce_motion(true));
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.open_settings(window, cx));
        window.render_frame(cx);
        // Wide viewports host the Kit container: one sidebar row per page.
        for ix in 0..9 {
            assert!(
                window.try_find(format!("0-{ix}")).is_some(),
                "Kit sidebar row 0-{ix} missing from settings navigation"
            );
        }
        // Kit sidebar rows carry no accessible label in 0.9.0 (upstream
        // sidebar-a11y ask, re-verified 2026-11-09), so the config row pins by position for now.
        assert!(window.try_find("0-8").is_some());

        // Pin the cycle start: Home selects the first tab (VT Code).
        window.press("home", cx);
        window.render_frame(cx);
        assert!(view.read(cx).current_route() == Route::Settings);
        assert!(!view.read(cx).config_page_open);
        assert!(window.try_find("vtcode-settings").is_some());

        // Backward wrap: VT Code (first) -> About (last).
        window.press("left", cx);
        window.render_frame(cx);
        assert!(!view.read(cx).config_page_open);
        assert!(window.try_find("about-settings").is_some());
        assert!(window.try_find("vtcode-settings").is_none());

        // Forward wrap: About (last) -> VT Code (first).
        window.press("right", cx);
        window.render_frame(cx);
        assert!(!view.read(cx).config_page_open);
        assert!(window.try_find("vtcode-settings").is_some());

        // Up and Down alias the movement and wrap at both ends.
        window.press("up", cx);
        window.render_frame(cx);
        assert!(window.try_find("about-settings").is_some());
        window.press("down", cx);
        window.render_frame(cx);
        assert!(window.try_find("vtcode-settings").is_some());

        // Cycling to storage renders its pane.
        for _ in 0..6 {
            window.press("down", cx);
            window.render_frame(cx);
        }
        assert!(!view.read(cx).config_page_open);
        assert!(window.try_find("storage-settings").is_some());

        // Kit-internal clicks navigate Kit only (pinned gap until upstream
        // on-select lands): config renders while the view still selects storage.
        window.click("0-8", cx);
        window.render_frame(cx);
        assert!(window.try_find("cli-config-page").is_some());
        assert!(!view.read(cx).config_page_open);
        // The next view-driven push remounts Kit and wins: storage → computer-use → config → About.
        window.press("right", cx);
        window.press("right", cx);
        window.render_frame(cx);
        assert!(view.read(cx).config_page_open);
        assert!(window.try_find("cli-config-page").is_some());
        window.press("right", cx);
        window.render_frame(cx);
        assert!(!view.read(cx).config_page_open);
        assert!(window.try_find("about-settings").is_some());
    })
    .unwrap();
}

#[gpui_kit::test]
fn settings_toggle_reaches_the_open_page_from_the_keyboard(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update(|cx| cx.set_reduce_motion(true));
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.toggle_settings(window, cx));
        window.render_frame(cx);
        assert!(view.read(cx).current_route() == Route::Settings);
        // The fullscreen page covers the toolbar, so the toggle path while
        // open is the keyboard binding (reachable now that no sheet layer
        // sits between the page and the body actions).
        window.press("cmd-,", cx);
        window.render_frame(cx);
        assert!(view.read(cx).current_route() != Route::Settings);
        window.press("cmd-,", cx);
        window.render_frame(cx);
        assert!(view.read(cx).current_route() == Route::Settings);
    })
    .unwrap();
}
