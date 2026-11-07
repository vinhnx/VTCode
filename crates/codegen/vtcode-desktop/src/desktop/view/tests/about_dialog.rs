use super::*;

/// Dispatch the application-menu About action and settle the opened dialog.
fn open_about(window: AnyWindowHandle, view: &Entity<DesktopView>, cx: &mut TestAppContext) {
    cx.update_window(window, |_, window, cx| {
        window.dispatch_action(Box::new(super::super::AboutApplication), cx);
    })
    .unwrap();
    cx.run_until_parked();
    view.update(cx, |_, cx| cx.notify());
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
    })
    .unwrap();
}

#[gpui_kit::test]
fn about_action_opens_the_branded_dialog(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    open_about(window, &view, cx);
    cx.update_window(window, |_, window, _| {
        assert!(window.try_find("about-dialog").is_some(), "About action must open the dialog");
        assert!(window.try_find("about-version").is_some(), "the version chip must render");
        assert!(
            window.try_find("about-copyright").is_some(),
            "the copyright line must render (HIG about windows carry it)"
        );
    })
    .unwrap();
    cx.update(|cx| {
        assert!(view.read(cx).about_open, "dialog state must be open");
    });
}

#[gpui_kit::test]
fn escape_dismisses_the_about_dialog_and_restores_focus(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    open_about(window, &view, cx);
    cx.update_window(window, |_, window, cx| {
        window.dispatch_action(Box::new(super::super::DismissOverlay), cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(
            view.read(cx).composer.read(cx).focus_handle(cx).is_focused(window),
            "dismissal must restore composer focus"
        );
    })
    .unwrap();
    cx.update(|cx| {
        assert!(!view.read(cx).about_open, "Escape must close the topmost About layer");
    });
}

#[gpui_kit::test]
fn done_closes_the_about_dialog(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    open_about(window, &view, cx);
    cx.update_window(window, |_, window, cx| {
        window.click("about-done", cx);
        window.render_frame(cx);
    })
    .unwrap();
    cx.update(|cx| {
        assert!(!view.read(cx).about_open, "Done must close the dialog");
    });
}

#[gpui_kit::test]
fn acknowledgments_opens_the_viewer_at_the_third_party_notices(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    open_about(window, &view, cx);
    cx.update_window(window, |_, window, cx| {
        window.click("about-acknowledgments", cx);
        window.render_frame(cx);
    })
    .unwrap();
    cx.update(|cx| {
        let state = view.read(cx);
        assert!(!state.about_open, "acknowledgments must close the dialog");
        assert_eq!(
            state
                .license_viewer
                .as_ref()
                .map(super::super::licenses::LicenseViewerState::document),
            Some(super::super::licenses::LicenseDocument::Notices),
            "acknowledgments open the in-app third-party notices"
        );
    });
}

#[gpui_kit::test]
fn license_button_opens_the_viewer_at_the_mit_text(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    open_about(window, &view, cx);
    cx.update_window(window, |_, window, cx| {
        window.click("about-license", cx);
        window.render_frame(cx);
    })
    .unwrap();
    cx.update(|cx| {
        let state = view.read(cx);
        assert!(!state.about_open, "License must close the About dialog");
        assert_eq!(
            state
                .license_viewer
                .as_ref()
                .map(super::super::licenses::LicenseViewerState::document),
            Some(super::super::licenses::LicenseDocument::Mit),
            "License opens the in-app MIT text"
        );
    });
}

#[gpui_kit::test]
fn clicking_outside_closes_the_about_dialog(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update(|cx| cx.set_reduce_motion(true));
    open_about(window, &view, cx);
    cx.update_window(window, |_, window, cx| {
        window.click("message-composer-frame", cx);
        window.render_frame(cx);
        assert!(window.try_find("about-dialog").is_none(), "clicking away closes About");
        assert!(
            view.read(cx).composer.read(cx).focus_handle(cx).is_focused(window),
            "click-away restores composer focus"
        );
    })
    .unwrap();
    cx.update(|cx| {
        assert!(!view.read(cx).about_open, "dialog state must be closed");
    });
}

#[gpui_kit::test]
fn about_dialog_traps_tab_across_its_footer_actions(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    open_about(window, &view, cx);
    cx.update_window(window, |_, window, cx| {
        assert_tab_trapped_within_dialog(window, "about-license", "about-done", cx);
    })
    .unwrap();
}
