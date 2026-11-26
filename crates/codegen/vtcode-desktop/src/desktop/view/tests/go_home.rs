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
        window.press("escape", cx);
        assert!(view.read(cx).last_escape_press.is_some());

        window.click("nav-home", cx);
        window.render_frame(cx);
        assert_eq!(view.read(cx).current_route(), Route::Workspace);
        assert!(view.read(cx).last_escape_press.is_none());
        window.press("escape", cx);
        assert_eq!(view.read(cx).composer.read(cx).value(), "keep this draft");
        assert!(view.read(cx).composer.read(cx).focus_handle(cx).is_focused(window));
    })
    .unwrap();
}

#[gpui_kit::test]
fn command_palette_suggests_home_threads_and_zen(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.workspace = Some(std::env::temp_dir());
            view.open_command_palette(window, cx);
        });
        window.render_frame(cx);
        view.update(cx, |view, cx| view.focus_palette_state(window, cx));
        window.render_frame(cx);

        let home = super::super::PaletteAction::OpenDeepLink(Box::new(super::super::deeplinks::DeepLink::Command(
            super::super::shortcuts::CommandId::GoHome,
        )));
        let entries: Vec<_> = view.read(cx).palette_groups.iter().flat_map(|(_, rows)| rows).collect();
        assert!(entries.iter().any(|row| row.action == home), "Home is suggested in ⌘K");
        assert!(
            entries
                .iter()
                .any(|row| row.action == super::super::PaletteAction::BrowseThreads),
            "Browse threads is suggested in ⌘K"
        );
        assert!(
            entries
                .iter()
                .any(|row| row.action == super::super::PaletteAction::ToggleZenMode && row.label.contains("Zen")),
            "Toggle Zen mode stays in the default ⌘K View group"
        );
        assert!(
            view.read(cx).palette_groups.iter().any(|(label, _)| label == "Workspace"),
            "workspace actions are suggested when a workspace is open"
        );

        window.within("command-palette").input("Home", cx);
        window.render_frame(cx);
        assert!(
            window.try_find(format!("{}-shortcut", palette_action_id(&home))).is_some(),
            "the Home row shows its cmd-shift-h badge"
        );

        view.update(cx, |view, cx| view.run_palette_action(home, window, cx));
        window.render_frame(cx);
        assert!(view.read(cx).palette.is_none(), "confirming the suggestion closes the palette");
        assert_eq!(view.read(cx).current_route(), Route::Workspace);
    })
    .unwrap();
}
