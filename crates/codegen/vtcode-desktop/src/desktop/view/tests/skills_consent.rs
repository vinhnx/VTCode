use super::*;

fn discover_with_consent(
    view: &Entity<DesktopView>,
    window: AnyWindowHandle,
    consent: Option<&str>,
    cx: &mut TestAppContext,
) {
    let mut snapshot = serde_json::to_value(runtime_options_config()).expect("snapshot json");
    if let Some(state) = consent {
        snapshot["bundledSkillsConsent"] = serde_json::Value::String(state.to_string());
    }
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.event_tx
                .send(BackendEvent::Response {
                    method: vtcode_desktop_protocol::method::CONFIG_DISCOVER.into(),
                    result: Some(snapshot),
                    error: None,
                })
                .unwrap();
            let _batch_full = view.drain_events(window, cx);
        });
        window.render_frame(cx);
    })
    .unwrap();
    cx.run_until_parked();
}

fn connected_backend() -> (BackendHandle, tokio::sync::mpsc::Receiver<BackendCommand>) {
    let (commands, requests) = tokio::sync::mpsc::channel(8);
    (BackendHandle { commands }, requests)
}

#[gpui_kit::test]
fn unasked_snapshot_opens_consent_and_allow_writes_user_grant(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (backend, mut requests) = connected_backend();
    cx.update_window(window, |_, _, cx| {
        view.update(cx, |view, _| {
            view.backend = Some(backend);
            view.workspace = Some(PathBuf::from("/tmp/vtcode-consent"));
        });
    })
    .unwrap();

    discover_with_consent(&view, window, Some("unasked"), cx);
    cx.update_window(window, |_, window, cx| {
        assert!(view.read(cx).skills_consent_open);
        assert!(window.try_find("skills-consent-dialog").is_some());
        window.click("skills-consent-allow", cx);
        window.render_frame(cx);
        assert!(!view.read(cx).skills_consent_open, "answer closes the card");
    })
    .unwrap();
    cx.run_until_parked();

    let BackendCommand::Request { method, params } = requests.try_recv().expect("consent write") else {
        panic!("request expected")
    };
    assert_eq!(method, vtcode_desktop_protocol::method::CONFIG_SETTINGS_WRITE);
    assert_eq!(params["target"], "user");
    assert_eq!(params["path"], "skills.bundled.enabled");
    assert_eq!(params["value"], true);
    assert!(requests.try_recv().is_err());

    // Seen once per launch: a second snapshot does not reopen.
    discover_with_consent(&view, window, Some("unasked"), cx);
    cx.update_window(window, |_, window, cx| {
        assert!(!view.read(cx).skills_consent_open);
        assert!(window.try_find("skills-consent-dialog").is_none());
    })
    .unwrap();
}

#[gpui_kit::test]
fn decline_writes_user_false(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (backend, mut requests) = connected_backend();
    cx.update_window(window, |_, _, cx| {
        view.update(cx, |view, _| {
            view.backend = Some(backend);
            view.workspace = Some(PathBuf::from("/tmp/vtcode-consent"));
        });
    })
    .unwrap();

    discover_with_consent(&view, window, Some("unasked"), cx);
    cx.update_window(window, |_, window, cx| {
        window.click("skills-consent-decline", cx);
        window.render_frame(cx);
        assert!(!view.read(cx).skills_consent_open);
    })
    .unwrap();
    cx.run_until_parked();

    let BackendCommand::Request { method, params } = requests.try_recv().expect("consent write") else {
        panic!("request expected")
    };
    assert_eq!(method, vtcode_desktop_protocol::method::CONFIG_SETTINGS_WRITE);
    assert_eq!(params["target"], "user");
    assert_eq!(params["path"], "skills.bundled.enabled");
    assert_eq!(params["value"], false);
    assert!(requests.try_recv().is_err());
}

#[gpui_kit::test]
fn escape_dismisses_consent_without_sending(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (backend, mut requests) = connected_backend();
    cx.update_window(window, |_, _, cx| {
        view.update(cx, |view, _| {
            view.backend = Some(backend);
            view.workspace = Some(PathBuf::from("/tmp/vtcode-consent"));
        });
    })
    .unwrap();

    discover_with_consent(&view, window, Some("unasked"), cx);
    cx.update_window(window, |_, window, cx| {
        assert!(window.try_find("skills-consent-dialog").is_some());
        window.press("escape", cx);
        window.render_frame(cx);
        assert!(!view.read(cx).skills_consent_open, "Escape dismisses the card");
        assert!(
            view.read(cx).composer.read(cx).focus_handle(cx).is_focused(window),
            "dismissal restores composer focus"
        );
    })
    .unwrap();
    cx.run_until_parked();
    assert!(requests.try_recv().is_err(), "dismissal must not send");
}

#[gpui_kit::test]
fn answered_states_never_open_consent(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (backend, mut requests) = connected_backend();
    cx.update_window(window, |_, _, cx| {
        view.update(cx, |view, _| {
            view.backend = Some(backend);
            view.workspace = Some(PathBuf::from("/tmp/vtcode-consent"));
        });
    })
    .unwrap();

    for state in [Some("granted"), Some("declined"), Some("grandfathered"), None] {
        discover_with_consent(&view, window, state, cx);
        cx.update_window(window, |_, window, cx| {
            assert!(!view.read(cx).skills_consent_open, "state {state:?} must not open the card");
            assert!(window.try_find("skills-consent-dialog").is_none());
        })
        .unwrap();
    }
    cx.run_until_parked();
    assert!(requests.try_recv().is_err(), "no state may send");
}
