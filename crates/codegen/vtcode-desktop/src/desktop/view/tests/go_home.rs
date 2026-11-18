//! The Home button returns to the main workspace landing page from any route.

use super::*;

#[gpui_kit::test]
fn home_button_returns_from_settings_to_workspace(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.open_settings(window, cx));
        window.render_frame(cx);
        assert_eq!(view.read(cx).current_route(), Route::Settings);
        let sidebar = view.read(cx).sidebar_open;

        window.click("nav-home", cx);
        window.render_frame(cx);
        assert_eq!(view.read(cx).current_route(), Route::Workspace, "Home leaves Settings");
        assert!(window.try_find("settings-panel").is_none());
        assert_eq!(view.read(cx).sidebar_open, sidebar, "Home does not toggle the rail");
        assert!(view.read(cx).composer.read(cx).focus_handle(cx).is_focused(window));
    })
    .unwrap();
}

#[gpui_kit::test]
fn home_button_returns_from_browser_to_workspace(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.open_thread_browser(window, cx));
        window.render_frame(cx);
        assert_eq!(view.read(cx).current_route(), Route::Browser);

        window.click("nav-home", cx);
        window.render_frame(cx);
        assert_eq!(view.read(cx).current_route(), Route::Workspace, "Home leaves the browser");
        assert!(window.try_find("thread-browser").is_none());
        assert!(view.read(cx).composer.read(cx).focus_handle(cx).is_focused(window));
    })
    .unwrap();
}

#[gpui_kit::test]
fn home_button_clears_transient_overlays(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.open_command_palette(window, cx));
        window.render_frame(cx);
        assert!(view.read(cx).palette.is_some());

        // Dispatch the handler directly: an open palette layers above the rail.
        view.update(cx, |view, cx| view.go_home(window, cx));
        window.render_frame(cx);
        assert!(view.read(cx).palette.is_none(), "Home clears the command palette");
        assert_eq!(view.read(cx).current_route(), Route::Workspace);
        assert!(view.read(cx).composer.read(cx).focus_handle(cx).is_focused(window));
    })
    .unwrap();
}

#[gpui_kit::test]
fn home_button_on_workspace_keeps_the_composer_draft(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        window.press("cmd-l", cx);
        window.input("keep this draft", cx);

        window.click("nav-home", cx);
        window.render_frame(cx);
        assert_eq!(view.read(cx).current_route(), Route::Workspace);
        assert_eq!(view.read(cx).composer.read(cx).value(), "keep this draft");
        assert!(view.read(cx).composer.read(cx).focus_handle(cx).is_focused(window));
    })
    .unwrap();
}
