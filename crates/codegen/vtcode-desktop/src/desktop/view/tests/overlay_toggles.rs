use super::*;

#[gpui_kit::test]
fn second_trigger_closes_settings_browser_and_same_kind_palettes(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    // Action dispatch is deferred; park the loop before asserting each effect.
    cx.update_window(window, |_, window, cx| {
        focus_element_by_tab(window, "open-settings", cx);
        window.dispatch_action(Box::new(super::super::OpenSettings), cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(view.read(cx).route == Route::Settings);
        window.dispatch_action(Box::new(super::super::OpenSettings), cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(view.read(cx).route != Route::Settings, "second Settings action must close the page");
    })
    .unwrap();

    cx.update_window(window, |_, window, cx| {
        focus_element_by_tab(window, "nav-browse-threads", cx);
        window.dispatch_action(Box::new(super::super::OpenThreadBrowser), cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(view.read(cx).route == Route::Browser);
        window.dispatch_action(Box::new(super::super::OpenThreadBrowser), cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(view.read(cx).route != Route::Browser, "second browser action must close the browser");

        view.update(cx, |view, cx| view.open_command_palette(window, cx));
        window.render_frame(cx);
        assert!(view.read(cx).palette.is_some());
        view.update(cx, |view, cx| view.open_command_palette(window, cx));
        window.render_frame(cx);
        assert!(view.read(cx).palette.is_none(), "second Commands open must close the palette");

        view.update(cx, |view, _| {
            let (commands, _) = tokio::sync::mpsc::channel(2);
            view.backend = Some(BackendHandle { commands });
        });
        view.update(cx, |view, cx| view.open_model_palette(window, cx));
        window.render_frame(cx);
        assert!(view.read(cx).palette.is_some());
        view.update(cx, |view, cx| view.open_model_palette(window, cx));
        window.render_frame(cx);
        assert!(view.read(cx).palette.is_none(), "second Models open must close the palette");

        // A different palette kind replaces instead of closing.
        view.update(cx, |view, cx| view.open_command_palette(window, cx));
        view.update(cx, |view, cx| view.open_model_palette(window, cx));
        window.render_frame(cx);
        let state = view.read(cx);
        assert!(state.palette.is_some());
        assert!(state.palette_kind == super::super::PaletteKind::Models, "switching kinds must replace the palette");
    })
    .unwrap();
}

#[gpui_kit::test]
fn route_back_key_returns_to_workspace_and_focuses_composer(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        window.dispatch_action(Box::new(super::super::OpenSettings), cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(view.read(cx).route == Route::Settings);
        window.press("cmd-[", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(view.read(cx).route == Route::Workspace, "cmd-[ must go back to the workspace");
        assert!(
            view.read(cx).composer.read(cx).focus_handle(cx).is_focused(window),
            "route back lands focus in the composer"
        );
        // On the workspace route cmd-[ propagates instead of acting.
        window.press("cmd-[", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(view.read(cx).route == Route::Workspace);
    })
    .unwrap();
}
