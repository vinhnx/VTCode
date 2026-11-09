use super::*;

fn feed_initialize(view: &Entity<DesktopView>, cx: &mut gpui_kit::App) {
    view.update(cx, |view, cx| {
        view.handle_backend_event(
            BackendEvent::Response {
                method: vtcode_desktop_protocol::method::INITIALIZE.to_string(),
                result: Some(json!({
                    "protocol": vtcode_desktop_protocol::PROTOCOL_VERSION,
                    "workspace": std::env::temp_dir().display().to_string(),
                    "serverVersion": "0.180.6",
                })),
                error: None,
            },
            cx,
        );
    });
}

fn feed_discover(view: &Entity<DesktopView>, snapshot: serde_json::Value, cx: &mut gpui_kit::App) {
    view.update(cx, |view, cx| {
        view.handle_backend_event(
            BackendEvent::Response {
                method: vtcode_desktop_protocol::method::CONFIG_DISCOVER.to_string(),
                result: Some(snapshot),
                error: None,
            },
            cx,
        );
    });
}

fn open_provider_settings(window: &mut gpui_kit::Window, cx: &mut gpui_kit::App) {
    window.click("open-settings", cx);
    window.render_frame(cx);
    select_settings_category(window, 0, cx);
    window.render_frame(cx);
}

#[gpui_kit::test]
fn secret_form_saves_through_the_protocol_and_repairs_the_banner(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _cx| {
            view.backend = Some(BackendHandle { commands });
            view.selected_session_id = Some("stale-session".into());
        });
        feed_initialize(&view, cx);
        feed_discover(
            &view,
            json!({
                "provider": "meta",
                "model": "muse-spark-1.3",
                "authenticated": false,
                "credentialSetupCommand": "vtcode secret add meta",
                "secretStore": {"provider": "meta", "envVar": "META_API_KEY"},
            }),
            cx,
        );
        view.update(cx, |view, cx| {
            assert!(view.secret_input.read(cx).presentation().is_masked(), "the key input must mask its value");
            view.secret_input
                .update(cx, |input, cx| input.set_value("  pasted-key-123  ", window, cx));
        });
        open_provider_settings(window, cx);
        assert_eq!(window.find("secret-input").label(), Some("Provider API key"));
        // The initialize feed emits the discovery burst; drain it so the
        // save below observes an empty queue like a live service would.
        while requests.try_recv().is_ok() {}
        window.click("save-secret", cx);
        view.update(cx, |view, cx| {
            assert!(view.secret_save_pending);
            assert_eq!(view.secret_input.read(cx).value().to_string(), "", "sent keys are wiped from UI state");
            assert_eq!(view.status, "Saving API key…");
        });
    })
    .unwrap();

    let BackendCommand::Request { method, params } = requests.try_recv().expect("secret store request") else {
        panic!("expected secret store request");
    };
    assert_eq!(method, vtcode_desktop_protocol::method::CONFIG_SECRET_STORE);
    assert_eq!(params["provider"], "meta");
    assert_eq!(params["apiKey"], "pasted-key-123");

    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.handle_backend_event(
                BackendEvent::Response {
                    method: vtcode_desktop_protocol::method::CONFIG_SECRET_STORE.to_string(),
                    result: Some(json!({
                        "configuration": {
                            "provider": "meta",
                            "model": "muse-spark-1.3",
                            "authenticated": true,
                            "credentialSetupCommand": "vtcode secret add meta",
                            "secretStore": {"provider": "meta", "envVar": "META_API_KEY"},
                        },
                        "sessionReset": true,
                    })),
                    error: None,
                },
                cx,
            );
            assert!(!view.secret_save_pending);
            assert!(view.secret_editor_error.is_none());
            assert!(
                view.configuration
                    .as_ref()
                    .is_some_and(|configuration| configuration.authenticated)
            );
            assert!(view.selected_session_id.is_none(), "a repaired save resets the stale runtime");
            assert!(view.error.is_none(), "the credential banner clears once authenticated");
            assert_eq!(view.status, "API key saved. Send a message to start.");
        });
        window.render_frame(cx);
    })
    .unwrap();
    // The session reset refreshes cached bridge state, like a provider switch.
    let BackendCommand::Request { method, .. } = requests.try_recv().expect("bridge refresh") else {
        panic!("expected bridge refresh");
    };
    assert_eq!(method, vtcode_desktop_protocol::method::WEBMCP_STATUS);
    assert!(requests.try_recv().is_err());

    // An unrelated banner survives a successful save.
    cx.update_window(window, |_, _window, cx| {
        view.update(cx, |view, cx| {
            view.error = Some("Some unrelated failure.".into());
            view.handle_backend_event(
                BackendEvent::Response {
                    method: vtcode_desktop_protocol::method::CONFIG_SECRET_STORE.to_string(),
                    result: Some(json!({
                        "configuration": {
                            "provider": "meta",
                            "model": "muse-spark-1.3",
                            "authenticated": true,
                            "credentialSetupCommand": "vtcode secret add meta",
                            "secretStore": {"provider": "meta", "envVar": "META_API_KEY"},
                        },
                        "sessionReset": false,
                    })),
                    error: None,
                },
                cx,
            );
            assert_eq!(view.error.as_deref(), Some("Some unrelated failure."));
            assert_eq!(view.status, "API key saved. Send a message to start.");
        });
    })
    .unwrap();
}

#[gpui_kit::test]
fn empty_secret_input_sends_nothing(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _cx| {
            view.backend = Some(BackendHandle { commands });
        });
        feed_initialize(&view, cx);
        feed_discover(
            &view,
            json!({
                "provider": "meta",
                "model": "muse-spark-1.3",
                "authenticated": false,
                "credentialSetupCommand": "vtcode secret add meta",
                "secretStore": {"provider": "meta", "envVar": "META_API_KEY"},
            }),
            cx,
        );
        open_provider_settings(window, cx);
        while requests.try_recv().is_ok() {}
        window.click("save-secret", cx);
        view.update(cx, |view, cx| {
            // The button is disabled while the input is empty, so the click
            // must not send; the direct call below pins the empty guard.
            view.save_secret_store("meta".into(), "   ".into(), window, cx);
            assert_eq!(view.secret_editor_error.as_deref(), Some("API key must not be empty."));
            assert!(!view.secret_save_pending);
        });
    })
    .unwrap();
    assert!(requests.try_recv().is_err(), "empty input must not send");
}

#[gpui_kit::test]
fn secret_save_interrupted_by_service_exit_clears_pending(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, _window, cx| {
        view.update(cx, |view, cx| {
            view.secret_save_pending = true;
            view.handle_backend_event(BackendEvent::Exited { status: None, authentication_required: false }, cx);
            assert!(!view.secret_save_pending);
            assert!(
                view.secret_editor_error
                    .as_deref()
                    .is_some_and(|error| error.contains("disconnected while saving")),
                "inline error reports the interrupted save",
            );
        });
    })
    .unwrap();
}

#[gpui_kit::test]
fn secret_save_error_stays_inline_without_touching_the_banner(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _cx| {
            view.backend = Some(BackendHandle { commands });
            view.error = Some("Some unrelated failure.".into());
        });
        feed_initialize(&view, cx);
        feed_discover(
            &view,
            json!({
                "provider": "meta",
                "model": "muse-spark-1.3",
                "authenticated": true,
                "credentialSetupCommand": "vtcode secret add meta",
                "secretStore": {"provider": "meta", "envVar": "META_API_KEY"},
            }),
            cx,
        );
        view.update(cx, |view, cx| {
            view.secret_input.update(cx, |input, cx| input.set_value("bogus", window, cx));
        });
        open_provider_settings(window, cx);
        while requests.try_recv().is_ok() {}
        window.click("save-secret", cx);
    })
    .unwrap();

    let BackendCommand::Request { method, params } = requests.try_recv().expect("secret store request") else {
        panic!("expected secret store request");
    };
    assert_eq!(method, vtcode_desktop_protocol::method::CONFIG_SECRET_STORE);
    assert_eq!(params["provider"], "meta");
    assert_eq!(params["apiKey"], "bogus");
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.handle_backend_event(
                BackendEvent::Response {
                    method: vtcode_desktop_protocol::method::CONFIG_SECRET_STORE.to_string(),
                    result: None,
                    error: Some("Unknown provider: bogus.".into()),
                },
                cx,
            );
            assert!(!view.secret_save_pending);
            assert_eq!(view.secret_editor_error.as_deref(), Some("Unknown provider: bogus."));
            assert!(view.backend.is_some(), "a rejected save must not disconnect the service");
            assert_eq!(view.error.as_deref(), Some("Some unrelated failure."));
            assert_eq!(view.status, "API key was not saved.");
        });
        window.render_frame(cx);
        assert_eq!(window.find("secret-error").label(), Some("Unknown provider: bogus."));
    })
    .unwrap();
}

#[gpui_kit::test]
fn secret_response_for_a_stale_provider_skips_adoption_but_honors_reset(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, _window, cx| {
        view.update(cx, |view, cx| {
            view.secret_save_pending = true;
            view.selected_session_id = Some("live-session".into());
            view.configuration = serde_json::from_value(json!({
                "provider": "openai",
                "model": "gpt-5.6",
                "authenticated": false,
                "credentialSetupCommand": "vtcode login openai",
            }))
            .unwrap();
            // A save for meta lands after the user already switched to openai.
            view.handle_backend_event(
                BackendEvent::Response {
                    method: vtcode_desktop_protocol::method::CONFIG_SECRET_STORE.to_string(),
                    result: Some(json!({
                        "configuration": {
                            "provider": "meta",
                            "model": "muse-spark-1.3",
                            "authenticated": true,
                            "credentialSetupCommand": "vtcode secret add meta",
                            "secretStore": {"provider": "meta", "envVar": "META_API_KEY"},
                        },
                        "sessionReset": true,
                    })),
                    error: None,
                },
                cx,
            );
            assert!(!view.secret_save_pending);
            assert_eq!(
                view.configuration.as_ref().map(|configuration| configuration.provider.as_str()),
                Some("openai")
            );
            assert!(view.selected_session_id.is_none(), "a server-side reset always clears the selection");
            assert_eq!(view.status, "API key saved.");

            // Without a reset, a stale response leaves the live session alone too.
            view.secret_save_pending = true;
            view.selected_session_id = Some("live-session".into());
            view.handle_backend_event(
                BackendEvent::Response {
                    method: vtcode_desktop_protocol::method::CONFIG_SECRET_STORE.to_string(),
                    result: Some(json!({
                        "configuration": {
                            "provider": "meta",
                            "model": "muse-spark-1.3",
                            "authenticated": true,
                            "credentialSetupCommand": "vtcode secret add meta",
                            "secretStore": {"provider": "meta", "envVar": "META_API_KEY"},
                        },
                        "sessionReset": false,
                    })),
                    error: None,
                },
                cx,
            );
            assert!(!view.secret_save_pending);
            assert_eq!(
                view.configuration.as_ref().map(|configuration| configuration.provider.as_str()),
                Some("openai")
            );
            assert_eq!(view.selected_session_id.as_deref(), Some("live-session"));
        });
    })
    .unwrap();
}

#[gpui_kit::test]
fn login_route_providers_keep_the_terminal_copy_row(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        feed_initialize(&view, cx);
        feed_discover(
            &view,
            json!({
                "provider": "openai",
                "model": "gpt-5.6",
                "authenticated": false,
                "credentialSetupCommand": "vtcode login openai",
            }),
            cx,
        );
        open_provider_settings(window, cx);
        window.click("copy-credential-command", cx);
        assert_eq!(cx.read_from_clipboard().unwrap().text().as_deref(), Some("vtcode login openai"));
    })
    .unwrap();
}

#[gpui_kit::test]
fn auth_guidance_points_at_the_in_app_form_for_static_key_providers(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, _window, cx| {
        feed_discover(
            &view,
            json!({
                "provider": "meta",
                "model": "muse-spark-1.3",
                "authenticated": false,
                "credentialSetupCommand": "vtcode secret add meta",
                "secretStore": {"provider": "meta", "envVar": "META_API_KEY"},
            }),
            cx,
        );
        view.update(cx, |view, _cx| {
            let error = view.error.as_deref().expect("unauthenticated discover sets the banner");
            assert!(error.contains("Settings → VT Code → Provider"), "{error}");
            assert!(error.contains("vtcode secret add meta"), "{error}");
            assert!(error.contains("Reload CLI settings"), "{error}");
        });
        feed_discover(
            &view,
            json!({
                "provider": "copilot",
                "model": "claude-opus-4-6",
                "authenticated": false,
                "credentialSetupCommand": "vtcode login copilot",
            }),
            cx,
        );
        view.update(cx, |view, _cx| {
            let error = view.error.as_deref().expect("login-route banner");
            assert!(!error.contains("Settings → VT Code → Provider"), "{error}");
            assert!(error.contains("vtcode login copilot"), "{error}");
        });
    })
    .unwrap();
}
