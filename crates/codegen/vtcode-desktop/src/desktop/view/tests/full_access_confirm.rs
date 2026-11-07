use super::*;

fn full_access_options() -> vtcode_desktop_protocol::SessionOptionsSelectParams {
    vtcode_desktop_protocol::SessionOptionsSelectParams {
        permission_mode: Some(vtcode_desktop_protocol::PermissionMode::FullAccess),
        ..Default::default()
    }
}

#[gpui_kit::test]
fn full_access_modal_requires_acknowledge_before_confirm(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(8);
    // Opening from the palette shows the modal without sending anything.
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.configuration = Some(runtime_options_config());
            view.run_palette_action(
                super::super::PaletteAction::SelectSessionOptions(full_access_options()),
                window,
                cx,
            );
        });
        window.render_frame(cx);
        assert!(view.read(cx).full_access_confirm.is_some());
        assert!(window.try_find("full-access-confirm-dialog").is_some());
    })
    .unwrap();
    cx.run_until_parked();
    assert!(requests.try_recv().is_err(), "modal must not send before confirm");

    // Confirm without acknowledge sends nothing and keeps the modal open.
    cx.update_window(window, |_, window, cx| {
        window.click("full-access-confirm", cx);
        window.render_frame(cx);
        assert!(view.read(cx).full_access_confirm.is_some());
    })
    .unwrap();
    cx.run_until_parked();
    assert!(requests.try_recv().is_err(), "unacknowledged confirm must not send");

    // Acknowledge, then confirm: one attested request, modal closed.
    cx.update_window(window, |_, window, cx| {
        window.click("full-access-acknowledge", cx);
        window.render_frame(cx);
        window.click("full-access-confirm", cx);
        window.render_frame(cx);
        assert!(view.read(cx).full_access_confirm.is_none());
    })
    .unwrap();
    cx.run_until_parked();
    let BackendCommand::Request { method, params } = requests.try_recv().expect("attested request") else {
        panic!("request expected")
    };
    assert_eq!(method, vtcode_desktop_protocol::method::CONFIG_OPTIONS);
    assert_eq!(params["permissionMode"], "fullAccess");
    assert_eq!(params["fullAccessConfirmed"], true);
    assert!(requests.try_recv().is_err());

    // Cancel path: reopen, cancel, nothing sent.
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.session_request = None;
            view.run_palette_action(
                super::super::PaletteAction::SelectSessionOptions(full_access_options()),
                window,
                cx,
            );
        });
        window.render_frame(cx);
        assert!(view.read(cx).full_access_confirm.is_some());
        window.click("full-access-cancel", cx);
        window.render_frame(cx);
        assert!(view.read(cx).full_access_confirm.is_none());
    })
    .unwrap();
    cx.run_until_parked();
    assert!(requests.try_recv().is_err(), "cancel must not send");
}

#[gpui_kit::test]
fn clicking_outside_cancels_full_access(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update(|cx| cx.set_reduce_motion(true));
    let (commands, mut requests) = tokio::sync::mpsc::channel(8);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.configuration = Some(runtime_options_config());
            view.run_palette_action(
                super::super::PaletteAction::SelectSessionOptions(full_access_options()),
                window,
                cx,
            );
        });
        window.render_frame(cx);
        assert!(view.read(cx).full_access_confirm.is_some());
        assert!(window.try_find("full-access-confirm-dialog").is_some());
        window.click("message-composer-frame", cx);
        window.render_frame(cx);
        assert!(view.read(cx).full_access_confirm.is_none(), "clicking away cancels");
        assert!(window.try_find("full-access-confirm-dialog").is_none());
    })
    .unwrap();
    cx.run_until_parked();
    assert!(requests.try_recv().is_err(), "clicking away must not send");
}

#[gpui_kit::test]
fn escape_cancels_full_access_without_sending(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(8);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.configuration = Some(runtime_options_config());
            view.run_palette_action(
                super::super::PaletteAction::SelectSessionOptions(full_access_options()),
                window,
                cx,
            );
        });
        window.render_frame(cx);
        assert!(window.try_find("full-access-confirm-dialog").is_some());
        window.press("escape", cx);
        window.render_frame(cx);
        assert!(view.read(cx).full_access_confirm.is_none(), "Escape cancels the modal");
        assert!(
            view.read(cx).composer.read(cx).focus_handle(cx).is_focused(window),
            "cancellation restores composer focus"
        );
    })
    .unwrap();
    cx.run_until_parked();
    assert!(requests.try_recv().is_err(), "cancellation must not send");
}

#[gpui_kit::test]
fn full_access_modal_traps_tab_across_acknowledge_and_actions(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, _requests) = tokio::sync::mpsc::channel(8);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.configuration = Some(runtime_options_config());
            view.run_palette_action(
                super::super::PaletteAction::SelectSessionOptions(full_access_options()),
                window,
                cx,
            );
        });
        window.render_frame(cx);
        // Acknowledging enables confirm, so all three stops participate.
        window.click("full-access-acknowledge", cx);
        window.render_frame(cx);
        assert_tab_trapped_within_dialog(window, "full-access-acknowledge", "full-access-confirm", cx);
    })
    .unwrap();
}
