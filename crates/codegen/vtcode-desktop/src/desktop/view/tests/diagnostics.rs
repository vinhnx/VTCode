use super::super::settings::SettingsTab;
use super::*;
use serde_json::Value;

fn open_vtcode_settings(view: &Entity<DesktopView>, window: AnyWindowHandle, cx: &mut TestAppContext) {
    cx.update(|cx| cx.set_reduce_motion(true));
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.settings_tab = SettingsTab::VtCode;
            view.open_settings(window, cx);
        });
        window.render_frame(cx);
    })
    .unwrap();
    cx.run_until_parked();
}

fn test_repair(id: &str, kind: vtcode_desktop_protocol::DiagnosticsRepairKind, destructive: bool) -> Value {
    json!({
        "id": id,
        "title": format!("Repair {id}"),
        "subtitle": "Test repair",
        "kind": match kind {
            vtcode_desktop_protocol::DiagnosticsRepairKind::Apply => "apply",
            vtcode_desktop_protocol::DiagnosticsRepairKind::ModelPicker => "model_picker",
            vtcode_desktop_protocol::DiagnosticsRepairKind::SecretStore => "secret_store",
        },
        "destructive": destructive,
        "preview": {
            "targetDisplay": "vtcode.toml (workspace)",
            "changes": [{ "path": "skills.bundled.enabled", "old": "(unset)", "new": "true" }],
            "creates": [],
            "note": null,
        },
    })
}

fn send_response(
    view: &Entity<DesktopView>,
    window: AnyWindowHandle,
    method: &str,
    result: Value,
    cx: &mut TestAppContext,
) {
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.event_tx
                .send(BackendEvent::Response {
                    method: method.into(),
                    result: Some(result),
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

fn connect(view: &Entity<DesktopView>, window: AnyWindowHandle, backend: BackendHandle, cx: &mut TestAppContext) {
    cx.update_window(window, |_, _, cx| {
        view.update(cx, |view, _| {
            view.backend = Some(backend);
            view.workspace = Some(PathBuf::from("/tmp/vtcode-diagnostics"));
        });
    })
    .unwrap();
}

#[gpui_kit::test]
fn diagnose_button_requests_checkup_and_renders_repairs(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (backend, mut requests) = connected_backend();
    connect(&view, window, backend, cx);
    open_vtcode_settings(&view, window, cx);

    cx.update_window(window, |_, window, cx| {
        assert!(window.try_find("diagnostics-settings-group").is_some());
        window.click("diagnostics-run", cx);
        window.render_frame(cx);
        assert!(view.read(cx).diagnostics_pending);
    })
    .unwrap();
    cx.run_until_parked();

    let BackendCommand::Request { method, .. } = requests.try_recv().expect("checkup request") else {
        panic!("request expected")
    };
    assert_eq!(method, vtcode_desktop_protocol::method::DIAGNOSTICS_CHECKUP);

    send_response(
        &view,
        window,
        vtcode_desktop_protocol::method::DIAGNOSTICS_CHECKUP,
        json!({
            "repairs": [
                test_repair("allow_bundled_skills", vtcode_desktop_protocol::DiagnosticsRepairKind::Apply, false),
                test_repair("reset_invalid_config", vtcode_desktop_protocol::DiagnosticsRepairKind::Apply, true),
                test_repair("pick_provider_model", vtcode_desktop_protocol::DiagnosticsRepairKind::ModelPicker, false),
                test_repair("add_secret", vtcode_desktop_protocol::DiagnosticsRepairKind::SecretStore, false),
            ]
        }),
        cx,
    );
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(!view.read(cx).diagnostics_pending);
        assert_eq!(view.read(cx).diagnostics_repairs.as_ref().map(Vec::len), Some(4));
        assert!(window.try_find("diagnostics-review-allow_bundled_skills").is_some());
        assert!(window.try_find("diagnostics-review-reset_invalid_config").is_some());
        assert!(window.try_find("diagnostics-fix-pick_provider_model").is_some());
        assert!(window.try_find("diagnostics-fix-add_secret").is_none(), "secret repairs show guidance, no button");
        assert!(
            window.try_find("diagnostics-apply-allow_bundled_skills").is_none(),
            "apply appears only after expanding the preview"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn review_expands_preview_and_apply_sends_and_refreshes(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (backend, mut requests) = connected_backend();
    connect(&view, window, backend, cx);
    open_vtcode_settings(&view, window, cx);
    send_response(
        &view,
        window,
        vtcode_desktop_protocol::method::DIAGNOSTICS_CHECKUP,
        json!({
            "repairs": [
                test_repair("allow_bundled_skills", vtcode_desktop_protocol::DiagnosticsRepairKind::Apply, false),
            ]
        }),
        cx,
    );

    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("diagnostics-review-allow_bundled_skills", cx);
        window.render_frame(cx);
        assert_eq!(view.read(cx).diagnostics_expanded.as_deref(), Some("allow_bundled_skills"));
        assert!(window.try_find("diagnostics-apply-allow_bundled_skills").is_some());
        assert!(window.try_find("diagnostics-cancel-allow_bundled_skills").is_some());
        window.click("diagnostics-apply-allow_bundled_skills", cx);
        window.render_frame(cx);
    })
    .unwrap();
    cx.run_until_parked();

    let BackendCommand::Request { method, params } = requests.try_recv().expect("apply request") else {
        panic!("request expected")
    };
    assert_eq!(method, vtcode_desktop_protocol::method::DIAGNOSTICS_APPLY);
    assert_eq!(params["repairId"], "allow_bundled_skills");

    send_response(
        &view,
        window,
        vtcode_desktop_protocol::method::DIAGNOSTICS_APPLY,
        json!({ "applied": true, "message": "Applied 'Allow bundled skills'." }),
        cx,
    );
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(
            view.read(cx)
                .diagnostics_notice
                .as_deref()
                .unwrap_or_default()
                .contains("Applied")
        );
        assert!(view.read(cx).diagnostics_expanded.is_none(), "apply collapses the preview");
    })
    .unwrap();
    cx.run_until_parked();
    let BackendCommand::Request { method, .. } = requests.try_recv().expect("refresh checkup") else {
        panic!("request expected")
    };
    assert_eq!(method, vtcode_desktop_protocol::method::DIAGNOSTICS_CHECKUP);
}

#[gpui_kit::test]
fn cancel_collapses_without_sending(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (backend, mut requests) = connected_backend();
    connect(&view, window, backend, cx);
    open_vtcode_settings(&view, window, cx);
    send_response(
        &view,
        window,
        vtcode_desktop_protocol::method::DIAGNOSTICS_CHECKUP,
        json!({
            "repairs": [
                test_repair("reset_invalid_config", vtcode_desktop_protocol::DiagnosticsRepairKind::Apply, true),
            ]
        }),
        cx,
    );

    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("diagnostics-review-reset_invalid_config", cx);
        window.render_frame(cx);
        assert_eq!(view.read(cx).diagnostics_expanded.as_deref(), Some("reset_invalid_config"));
        assert!(window.try_find("diagnostics-apply-reset_invalid_config").is_some());
        window.click("diagnostics-cancel-reset_invalid_config", cx);
        window.render_frame(cx);
        assert!(view.read(cx).diagnostics_expanded.is_none());
        assert!(window.try_find("diagnostics-apply-reset_invalid_config").is_none());
    })
    .unwrap();
    cx.run_until_parked();
    assert!(requests.try_recv().is_err(), "preview and cancel must not send");
}

#[gpui_kit::test]
fn failed_checkup_clears_pending_and_surfaces_notice(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (backend, _requests) = connected_backend();
    connect(&view, window, backend, cx);
    open_vtcode_settings(&view, window, cx);

    cx.update_window(window, |_, window, cx| {
        window.click("diagnostics-run", cx);
        window.render_frame(cx);
        assert!(view.read(cx).diagnostics_pending);
    })
    .unwrap();
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.event_tx
                .send(BackendEvent::Response {
                    method: vtcode_desktop_protocol::method::DIAGNOSTICS_CHECKUP.into(),
                    result: None,
                    error: Some("boom".into()),
                })
                .unwrap();
            let _batch_full = view.drain_events(window, cx);
        });
        window.render_frame(cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(!view.read(cx).diagnostics_pending, "error must re-enable the buttons");
        assert_eq!(view.read(cx).diagnostics_notice.as_deref(), Some("boom"));
    })
    .unwrap();
}

#[gpui_kit::test]
fn picker_repair_opens_the_model_palette(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (backend, mut requests) = connected_backend();
    connect(&view, window, backend, cx);
    open_vtcode_settings(&view, window, cx);
    send_response(
        &view,
        window,
        vtcode_desktop_protocol::method::DIAGNOSTICS_CHECKUP,
        json!({
            "repairs": [
                test_repair("pick_provider_model", vtcode_desktop_protocol::DiagnosticsRepairKind::ModelPicker, false),
            ]
        }),
        cx,
    );

    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("diagnostics-fix-pick_provider_model", cx);
        window.render_frame(cx);
        assert!(window.try_find("command-palette").is_some(), "model palette must open");
    })
    .unwrap();
    cx.run_until_parked();
    assert!(requests.try_recv().is_err(), "picker routing stays client-side");
}
