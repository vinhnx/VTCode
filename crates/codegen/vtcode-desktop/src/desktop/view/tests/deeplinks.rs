use super::*;

#[gpui_kit::test]
fn goto_palette_lists_destinations_and_lands_on_confirm(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update(|cx| cx.set_reduce_motion(true));
    cx.update_window(window, |_, window, cx| {
        // The ⌘K View group carries the Go to entry.
        view.update(cx, |view, cx| view.open_command_palette(window, cx));
        window.render_frame(cx);
        let goto = view
            .read(cx)
            .palette_groups
            .iter()
            .flat_map(|(_, entries)| entries)
            .find(|entry| matches!(entry.action, super::super::PaletteAction::OpenGoTo))
            .expect("command palette links to the Go to palette")
            .action
            .clone();
        view.update(cx, |view, cx| view.run_palette_action(goto, window, cx));
        window.render_frame(cx);
        assert!(matches!(view.read(cx).palette_kind, super::super::PaletteKind::GoTo));
        assert_eq!(window.find("command-palette").label(), Some("Go to"));
        // Three groups: destinations (4 routes + 12 palettes), settings
        // (8 tabs + config page), and every shortcut command.
        let groups = view.read(cx).palette_groups.clone();
        assert_eq!(groups.len(), 3);
        assert_eq!(groups[0].1.len(), 16);
        assert_eq!(groups[1].1.len(), 9);
        assert_eq!(groups[2].1.len(), 34);
        // Command rows reuse the shared badge mapping: filter to one and
        // its saved chord badge renders.
        view.update(cx, |view, cx| view.focus_palette_state(window, cx));
        window.render_frame(cx);
        window.within("command-palette").input("zen mode", cx);
        window.render_frame(cx);
        let zen = super::super::PaletteAction::OpenDeepLink(Box::new(super::super::deeplinks::DeepLink::Command(
            super::super::shortcuts::CommandId::ZenMode,
        )));
        assert!(
            window.try_find(format!("{}-shortcut", palette_action_id(&zen))).is_some(),
            "goto command rows show shortcut badges"
        );
        // Confirming a destination replaces the palette with its target.
        let browser = super::super::PaletteAction::OpenDeepLink(Box::new(super::super::deeplinks::DeepLink::Route(
            Route::Browser,
        )));
        view.update(cx, |view, cx| view.run_palette_action(browser, window, cx));
        window.render_frame(cx);
        assert!(view.read(cx).palette.is_none());
        assert!(view.read(cx).current_route() == Route::Browser);
    })
    .unwrap();
}

#[gpui_kit::test]
fn go_slash_opens_destinations_config_and_reports_unknown(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update(|cx| cx.set_reduce_motion(true));
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            let opened = view.handle_desktop_slash_action(
                super::super::composer_controls::DesktopSlashAction::Go("settings/shortcuts".into()),
                window,
                cx,
            );
            assert!(opened);
        });
        window.render_frame(cx);
        assert!(view.read(cx).current_route() == Route::Settings);
        assert_eq!(view.read(cx).settings_tab, super::super::settings::SettingsTab::Shortcuts);
    })
    .unwrap();
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            let opened = view.handle_desktop_slash_action(
                super::super::composer_controls::DesktopSlashAction::Go("config".into()),
                window,
                cx,
            );
            assert!(opened);
        });
        window.render_frame(cx);
        assert!(view.read(cx).current_route() == Route::Settings);
        assert!(view.read(cx).config_page_open, "/go config opens the config page");
    })
    .unwrap();
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            let opened = view.handle_desktop_slash_action(
                super::super::composer_controls::DesktopSlashAction::Go("nope".into()),
                window,
                cx,
            );
            assert!(opened, "unknown links are handled with a status error");
            assert!(
                view.error.as_deref().is_some_and(|error| error.contains("Unknown destination")),
                "unknown links name the problem, got {:?}",
                view.error
            );
        });
        window.render_frame(cx);
        view.update(cx, |view, cx| {
            let opened = view.handle_desktop_slash_action(
                super::super::composer_controls::DesktopSlashAction::Go(String::new()),
                window,
                cx,
            );
            assert!(opened);
        });
        window.render_frame(cx);
        assert!(matches!(view.read(cx).palette_kind, super::super::PaletteKind::GoTo));
    })
    .unwrap();
}

#[gpui_kit::test]
fn go_refuses_unavailable_action_links_without_navigating(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update(|cx| cx.set_reduce_motion(true));
    cx.update_window(window, |_, window, cx| {
        // No session selected: the conversation-search action is valid but
        // unreachable, so /go reports failure and leaves state untouched
        // (the submit path turns `false` into the unavailable status error).
        // The open command palette is the canary: running the link would
        // dismiss it before no-op-ing.
        view.update(cx, |view, cx| view.open_command_palette(window, cx));
        window.render_frame(cx);
        assert!(view.read(cx).palette.is_some(), "canary palette is open");
        view.update(cx, |view, cx| {
            let opened = view.handle_desktop_slash_action(
                super::super::composer_controls::DesktopSlashAction::Go("action/conversation-search/abc".into()),
                window,
                cx,
            );
            assert!(!opened, "unavailable action links must not run");
        });
        window.render_frame(cx);
        assert!(view.read(cx).palette.is_some(), "refused links leave the palette open");
        assert!(view.read(cx).current_route() == Route::Workspace, "no navigation happens");
    })
    .unwrap();
}

#[gpui_kit::test]
fn go_refuses_contentless_conversation_search_like_the_goto_row(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update(|cx| cx.set_reduce_motion(true));
    cx.update_window(window, |_, window, cx| {
        // A selected session with no searchable content hides the GoTo row;
        // /go must refuse identically instead of opening an empty search.
        view.update(cx, |view, cx| {
            view.selected_session_id = Some("session-1".to_string());
            let opened = view.handle_desktop_slash_action(
                super::super::composer_controls::DesktopSlashAction::Go("palette/conversation-search".into()),
                window,
                cx,
            );
            assert!(!opened, "contentless search must not open");
        });
        window.render_frame(cx);
        assert!(view.read(cx).palette.is_none(), "no palette opens");
    })
    .unwrap();
}

#[gpui_kit::test]
fn go_command_dispatches_the_shortcut_action(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update(|cx| cx.set_reduce_motion(true));
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.focus_composer(window, cx));
        view.update(cx, |view, cx| {
            let opened = view.handle_desktop_slash_action(
                super::super::composer_controls::DesktopSlashAction::Go("command/settings".into()),
                window,
                cx,
            );
            assert!(opened);
        });
    })
    .unwrap();
    // Action dispatch is deferred; park the loop before asserting the effect.
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(view.read(cx).current_route() == Route::Settings, "command links dispatch the shortcut action");
    })
    .unwrap();
}
