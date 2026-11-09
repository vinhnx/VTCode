use super::*;

#[test]
fn activity_delivery_follows_modes_toggles_and_focus() {
    use crate::desktop::notifications::{BackgroundAgentActivity, activity_allowed};
    use crate::desktop::preferences::{NotificationPreferences, TurnCompletionMode};

    fn allowed(
        activity: BackgroundAgentActivity,
        notifications: &NotificationPreferences,
        window_active: bool,
    ) -> bool {
        activity_allowed(activity, notifications, window_active)
    }

    let defaults = NotificationPreferences::default();
    // Turn-terminal outcomes follow the mode: unfocused posts only in the
    // background, always posts either way, never stays silent.
    for activity in [
        BackgroundAgentActivity::Completed,
        BackgroundAgentActivity::NeedsAttention,
    ] {
        assert!(allowed(activity, &defaults, false));
        assert!(!allowed(activity, &defaults, true));
    }
    let mut always = NotificationPreferences::default();
    always.set_turn_completion(TurnCompletionMode::Always);
    assert!(allowed(BackgroundAgentActivity::Completed, &always, true));
    assert!(allowed(BackgroundAgentActivity::Completed, &always, false));
    let mut never = NotificationPreferences::default();
    never.set_turn_completion(TurnCompletionMode::Never);
    assert!(!allowed(BackgroundAgentActivity::Completed, &never, false));
    assert!(!allowed(BackgroundAgentActivity::NeedsAttention, &never, false));

    // Permission and question toggles gate their activity in either focus state.
    assert!(allowed(BackgroundAgentActivity::PermissionRequired, &defaults, true));
    assert!(allowed(BackgroundAgentActivity::QuestionWaiting, &defaults, true));
    let quiet_permission = NotificationPreferences {
        permission_prompts: Some(false),
        ..Default::default()
    };
    assert!(!allowed(BackgroundAgentActivity::PermissionRequired, &quiet_permission, false));
    assert!(allowed(BackgroundAgentActivity::QuestionWaiting, &quiet_permission, false));
    let quiet_questions = NotificationPreferences { questions: Some(false), ..Default::default() };
    assert!(!allowed(BackgroundAgentActivity::QuestionWaiting, &quiet_questions, false));
    assert!(allowed(BackgroundAgentActivity::PermissionRequired, &quiet_questions, false));

    // Disconnects follow the master toggle only, regardless of focus.
    assert!(allowed(BackgroundAgentActivity::ServiceDisconnected, &defaults, true));
    assert!(allowed(BackgroundAgentActivity::ServiceDisconnected, &defaults, false));

    // The master toggle silences every activity.
    let disabled = NotificationPreferences { enabled: Some(false), ..Default::default() };
    for activity in [
        BackgroundAgentActivity::Completed,
        BackgroundAgentActivity::NeedsAttention,
        BackgroundAgentActivity::PermissionRequired,
        BackgroundAgentActivity::QuestionWaiting,
        BackgroundAgentActivity::ServiceDisconnected,
    ] {
        assert!(!allowed(activity, &disabled, false), "{activity:?} must stay silent");
    }
}

#[test]
fn interaction_kinds_map_to_permission_or_question_activities() {
    use crate::desktop::notifications::{BackgroundAgentActivity, activity_for_interaction_kind};

    for kind in [
        vtcode_desktop_protocol::InteractionKind::Approval,
        vtcode_desktop_protocol::InteractionKind::PlanApproval,
    ] {
        assert_eq!(
            activity_for_interaction_kind(kind),
            BackgroundAgentActivity::PermissionRequired,
            "{kind:?} gates on the permission toggle"
        );
    }
    for kind in [
        vtcode_desktop_protocol::InteractionKind::Question,
        vtcode_desktop_protocol::InteractionKind::Choice,
        vtcode_desktop_protocol::InteractionKind::Other,
    ] {
        assert_eq!(
            activity_for_interaction_kind(kind),
            BackgroundAgentActivity::QuestionWaiting,
            "{kind:?} gates on the question toggle"
        );
    }
}

#[test]
fn activity_severity_and_messages_cover_every_activity() {
    use crate::desktop::notifications::BackgroundAgentActivity;
    use gpui_kit::component::notification::NotificationType;

    let activities = [
        BackgroundAgentActivity::Completed,
        BackgroundAgentActivity::NeedsAttention,
        BackgroundAgentActivity::PermissionRequired,
        BackgroundAgentActivity::QuestionWaiting,
        BackgroundAgentActivity::ServiceDisconnected,
    ];
    // Kit severity has no PartialEq, so match pairs instead of comparing. A
    // new activity variant misses every arm and fails until the table claims
    // it; Info stays unclaimed by design.
    for activity in activities {
        assert!(
            matches!(
                (activity, activity.severity()),
                (BackgroundAgentActivity::Completed, NotificationType::Success)
                    | (
                        BackgroundAgentActivity::NeedsAttention
                            | BackgroundAgentActivity::PermissionRequired
                            | BackgroundAgentActivity::QuestionWaiting,
                        NotificationType::Warning,
                    )
                    | (BackgroundAgentActivity::ServiceDisconnected, NotificationType::Error)
            ),
            "{activity:?} severity"
        );
    }
    // Asymmetric oracle: five activities, five distinct non-empty messages,
    // so no two banners can be confused.
    let mut messages: Vec<String> = activities.iter().map(|activity| activity.message()).collect();
    assert!(messages.iter().all(|message| !message.is_empty()));
    messages.sort();
    messages.dedup();
    assert_eq!(messages.len(), activities.len());
}

fn update_status_event(current: &str, latest: Option<&str>, available: bool) -> BackendEvent {
    BackendEvent::Response {
        method: vtcode_desktop_protocol::method::UPDATE_STATUS.into(),
        result: Some(update_status_json(current, latest, available)),
        error: None,
    }
}

/// The `update/status` / `update/check` snapshot body. Shared by the
/// notification fixture and the About-row tests, which apply it directly.
fn update_status_json(current: &str, latest: Option<&str>, available: bool) -> serde_json::Value {
    json!({
        "currentVersion": current,
        "latestVersion": latest,
        "updateAvailable": available,
        "releaseUrl": latest.map(|version| format!("https://github.com/vinhnx/vtcode/releases/tag/v{version}")),
        "channel": "stable",
        "checkDue": false,
    })
}

#[gpui_kit::test]
fn notification_toggles_persist_to_preferences(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.open_settings(window, cx));
        window.render_frame(cx);
        select_settings_category(window, 5, cx);
        window.render_frame(cx);
        assert!(window.try_find("notification-settings").is_some());
        assert!(window.try_find("about-settings-check-for-updates").is_none(), "the update check lives on About now");

        for control in [
            "notifications-enabled",
            "turn-completion-select",
            "notifications-permission",
            "notifications-questions",
            "notifications-updates",
        ] {
            assert!(window.try_find(control).is_some(), "{control} renders");
        }

        window.click("notifications-enabled", cx);
        window.render_frame(cx);
        assert!(!view.read(cx).preferences.notifications.enabled());
        assert!(!crate::desktop::preferences::Preferences::load().notifications.enabled());
        window.click("notifications-enabled", cx);
        window.render_frame(cx);
        assert!(view.read(cx).preferences.notifications.enabled());

        window.click("notifications-permission", cx);
        window.render_frame(cx);
        assert!(!view.read(cx).preferences.notifications.permission_prompts());
        assert!(
            !crate::desktop::preferences::Preferences::load()
                .notifications
                .permission_prompts()
        );

        window.click("notifications-questions", cx);
        window.render_frame(cx);
        assert!(!view.read(cx).preferences.notifications.questions());
        assert!(!crate::desktop::preferences::Preferences::load().notifications.questions());

        window.click("notifications-updates", cx);
        window.render_frame(cx);
        assert!(!view.read(cx).preferences.notifications.updates());
        assert!(!crate::desktop::preferences::Preferences::load().notifications.updates());
    })
    .unwrap();
}

#[gpui_kit::test]
fn notification_switches_share_one_trailing_spine(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.open_settings(window, cx));
        window.render_frame(cx);
        select_settings_category(window, 5, cx);
        window.render_frame(cx);

        // The four labels differ in length ("Enable notifications" vs
        // "Enable permission notifications"), so equal toggle geometry proves
        // the trailing lane absorbs the difference instead of ragged toggles.
        let ids = [
            ("notifications-enabled", "Enable notifications"),
            ("notifications-permission", "Enable permission notifications"),
            ("notifications-questions", "Enable question notifications"),
            ("notifications-updates", "Enable update notifications"),
        ];
        let mut bounds = Vec::with_capacity(ids.len());
        for (id, label) in ids {
            let snapshot = window.try_find(id);
            assert!(snapshot.is_some(), "{id} renders");
            let snapshot = snapshot.unwrap();
            assert_eq!(snapshot.label(), Some(label), "{id} keeps its accessible name");
            let found = snapshot.bounds();
            assert!(found.size.width > px(0.) && found.size.height > px(0.), "{id} renders with size: {found:?}");
            bounds.push(found);
        }
        let first = &bounds[0];
        for ((id, _), found) in ids.iter().zip(bounds.iter()) {
            assert_eq!(found.size.width, first.size.width, "{id} toggle width matches");
            assert_eq!(found.right(), first.right(), "{id} toggle trailing edge aligns");
        }
    })
    .unwrap();
}

#[gpui_kit::test]
fn turn_completion_dropdown_persists_the_selected_mode(cx: &mut TestAppContext) {
    use crate::desktop::preferences::TurnCompletionMode;

    let (window, view) = open_desktop(cx);
    // Drive the production subscription path: a dropdown confirmation reaches the setter.
    cx.update_window(window, |_, _, cx| {
        let select = view.read(cx).turn_completion_select.clone();
        assert_eq!(select.read(cx).selected_value().copied(), Some(TurnCompletionMode::Unfocused));
        select.update(cx, |_, cx| cx.emit(SelectEvent::Confirm(Some(TurnCompletionMode::Never))));
    })
    .unwrap();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(view.read(cx).preferences.notifications.turn_completion(), TurnCompletionMode::Never);
        assert_eq!(
            crate::desktop::preferences::Preferences::load().notifications.turn_completion(),
            TurnCompletionMode::Never
        );
    })
    .unwrap();

    // A repeat confirmation is a no-op that keeps the saved choice.
    cx.update_window(window, |_, _, cx| {
        let select = view.read(cx).turn_completion_select.clone();
        select.update(cx, |_, cx| cx.emit(SelectEvent::Confirm(Some(TurnCompletionMode::Never))));
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(
        crate::desktop::preferences::Preferences::load().notifications.turn_completion(),
        TurnCompletionMode::Never,
        "a repeat confirmation keeps the saved choice"
    );
}

#[gpui_kit::test]
fn update_status_response_stores_snapshot_and_reports_invalid_payloads(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, _window, cx| {
        view.update(cx, |view, cx| {
            view.update_request_pending = true;
            view.handle_backend_event(update_status_event("0.180.5", Some("0.181.0"), true), cx);
            assert!(!view.update_request_pending);
            let status = view.update_status.as_ref().expect("snapshot stored");
            assert_eq!(status.current_version, "0.180.5");
            assert_eq!(status.latest_version.as_deref(), Some("0.181.0"));
            assert!(status.update_available);
            assert!(view.error.is_none());

            view.update_request_pending = true;
            view.handle_backend_event(
                BackendEvent::Response {
                    method: vtcode_desktop_protocol::method::UPDATE_STATUS.into(),
                    result: Some(json!({ "currentVersion": 42 })),
                    error: None,
                },
                cx,
            );
            assert!(!view.update_request_pending);
            assert!(
                view.error
                    .as_deref()
                    .is_some_and(|error| error.contains("invalid update status"))
            );
            assert_eq!(
                view.update_status.as_ref().and_then(|status| status.latest_version.as_deref()),
                Some("0.181.0")
            );
        });
    })
    .unwrap();
}

#[gpui_kit::test]
fn update_status_error_clears_the_pending_request(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, _window, cx| {
        view.update(cx, |view, cx| {
            view.update_request_pending = true;
            view.handle_backend_event(
                BackendEvent::Response {
                    method: vtcode_desktop_protocol::method::UPDATE_STATUS.into(),
                    result: None,
                    error: Some("update service unavailable".into()),
                },
                cx,
            );
            assert!(!view.update_request_pending);
            assert!(
                view.error
                    .as_deref()
                    .is_some_and(|error| error.contains("update service unavailable"))
            );
        });
    })
    .unwrap();
}

#[gpui_kit::test]
fn check_for_updates_button_requests_a_live_check(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, requests) = tokio::sync::mpsc::channel(8);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.open_settings(window, cx);
            view.settings_tab = super::super::settings::SettingsTab::About;
        });
        window.render_frame(cx);
        assert!(window.try_find("about-settings-check-for-updates").is_some(), "the update check lives on About");
        window.click("about-settings-check-for-updates", cx);
        window.render_frame(cx);
        assert!(view.read(cx).update_request_pending);
        let mut requests = requests;
        let BackendCommand::Request { method, .. } = requests.try_recv().unwrap() else {
            panic!("update request")
        };
        // `update/status` is read-only, so a button that only re-read the cache
        // could never observe a release published since the last check.
        assert_eq!(method, vtcode_desktop_protocol::method::UPDATE_CHECK);
    })
    .unwrap();
}

/// An offered update gains an install action; the check stays disabled while
/// one runs, so a second concurrent install can never be requested.
#[gpui_kit::test]
fn offered_update_gains_an_install_action_that_requests_install(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, requests) = tokio::sync::mpsc::channel(8);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.apply_update_status(update_status_json("0.181.7", Some("0.181.8"), true));
            view.open_settings(window, cx);
            view.settings_tab = super::super::settings::SettingsTab::About;
        });
        window.render_frame(cx);

        assert!(
            window.try_find("about-settings-install-update").is_some(),
            "an offered update must offer the install action"
        );
        window.click("about-settings-install-update", cx);
        window.render_frame(cx);

        let mut requests = requests;
        let BackendCommand::Request { method, .. } = requests.try_recv().unwrap() else {
            panic!("install request")
        };
        assert_eq!(method, vtcode_desktop_protocol::method::UPDATE_INSTALL);
    })
    .unwrap();
}

/// No offer means no install action: the row must not advertise installing a
/// release the service would refuse.
#[gpui_kit::test]
fn an_up_to_date_build_offers_no_install_action(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.apply_update_status(update_status_json("0.181.8", None, false));
            view.open_settings(window, cx);
            view.settings_tab = super::super::settings::SettingsTab::About;
        });
        window.render_frame(cx);

        assert!(
            window.try_find("about-settings-install-update").is_none(),
            "up-to-date builds must not offer an install"
        );
    })
    .unwrap();
}

/// Progress notifications drive the row's description, and a finished install
/// retires the action instead of offering to install twice.
#[gpui_kit::test]
fn install_progress_drives_the_row_description_and_retires_the_action(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.apply_update_status(update_status_json("0.181.7", Some("0.181.8"), true));
            view.open_settings(window, cx);
            view.settings_tab = super::super::settings::SettingsTab::About;
        });
        window.render_frame(cx);
        assert!(window.try_find("about-settings-install-update").is_some());

        view.update(cx, |view, _cx| {
            view.handle_update_install_progress(vtcode_desktop_protocol::UpdateInstallState::Downloading {
                downloaded: 1024,
                total: Some(2048),
            });
        });
        window.render_frame(cx);
        // Mid-install the action disappears: the service refuses a concurrent
        // install, so offering one would only produce a rejection.
        assert!(
            window.try_find("about-settings-install-update").is_none(),
            "an in-flight install must not offer a second one"
        );

        view.update(cx, |view, _cx| {
            view.handle_update_install_progress(vtcode_desktop_protocol::UpdateInstallState::Completed {
                version: "0.181.8".into(),
                needs_gatekeeper_approval: false,
            });
        });
        window.render_frame(cx);
        assert!(
            window.try_find("about-settings-install-update").is_none(),
            "a finished install must not offer another"
        );
        assert!(!view.read(cx).update_install_state.is_in_flight());
    })
    .unwrap();
}

/// A failed install reports why and re-arms the action for a retry.
#[gpui_kit::test]
fn a_failed_install_re_arms_the_action(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.apply_update_status(update_status_json("0.181.7", Some("0.181.8"), true));
            view.open_settings(window, cx);
            view.settings_tab = super::super::settings::SettingsTab::About;
        });
        window.render_frame(cx);

        view.update(cx, |view, _cx| {
            view.handle_update_install_progress(vtcode_desktop_protocol::UpdateInstallState::Failed {
                message: "network unreachable".into(),
            });
        });
        window.render_frame(cx);

        assert!(window.try_find("about-settings-install-update").is_some(), "a failed install must allow a retry");
        // The failure renders in the row description, not the cached status
        // line: capture the props the row actually renders from.
        let props = super::super::about::AboutSettingsProps::capture(view.read(cx));
        let description = props.update_row_description();
        assert!(description.contains("network unreachable"), "{description}");
    })
    .unwrap();
}

#[gpui_kit::test]
fn update_available_notification_records_each_version_once(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            // First sighting records the version (and posts once).
            view.maybe_notify_update_available(&update_status_event("0.180.5", Some("0.181.0"), true), window, cx);
            assert_eq!(view.preferences.notifications.last_notified_update.as_deref(), Some("0.181.0"));
            // A repeat for the same version is a no-op.
            view.preferences.notifications.last_notified_update = Some("0.181.0".to_string());
            view.maybe_notify_update_available(&update_status_event("0.180.5", Some("0.181.0"), true), window, cx);
            assert_eq!(view.preferences.notifications.last_notified_update.as_deref(), Some("0.181.0"));
            // A newer version records again.
            view.maybe_notify_update_available(&update_status_event("0.180.5", Some("0.182.0"), true), window, cx);
            assert_eq!(view.preferences.notifications.last_notified_update.as_deref(), Some("0.182.0"));
        });
    })
    .unwrap();
}

#[gpui_kit::test]
fn update_available_notification_respects_toggles_and_quiet_status(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            // Master toggle off suppresses without recording.
            view.preferences.notifications.enabled = Some(false);
            view.maybe_notify_update_available(&update_status_event("0.180.5", Some("0.181.0"), true), window, cx);
            assert_eq!(view.preferences.notifications.last_notified_update, None);

            // Category toggle off suppresses without recording.
            view.preferences.notifications.enabled = Some(true);
            view.preferences.notifications.updates = Some(false);
            view.maybe_notify_update_available(&update_status_event("0.180.5", Some("0.181.0"), true), window, cx);
            assert_eq!(view.preferences.notifications.last_notified_update, None);

            // A quiet status (up to date, or available-but-unknown version)
            // never records.
            view.preferences.notifications.updates = Some(true);
            view.maybe_notify_update_available(&update_status_event("0.181.0", None, false), window, cx);
            assert_eq!(view.preferences.notifications.last_notified_update, None);
            view.maybe_notify_update_available(&update_status_event("0.180.5", None, true), window, cx);
            assert_eq!(view.preferences.notifications.last_notified_update, None);
        });
    })
    .unwrap();
}

fn deeplink_thread_fixture(id: &str) -> vtcode_desktop_protocol::SessionSummary {
    vtcode_desktop_protocol::SessionSummary {
        session_id: id.into(),
        title: Some(format!("Thread {id}")),
        workspace: std::env::temp_dir().display().to_string(),
        completed: true,
        resumable: true,
        size_bytes: 0,
        updated_at: None,
    }
}

fn question_interaction_event(session_id: &str, turn_id: &str, interaction_id: &str) -> BackendEvent {
    BackendEvent::Interaction(vtcode_desktop_protocol::InteractionRequest {
        session_id: session_id.into(),
        turn_id: turn_id.into(),
        interaction_id: interaction_id.into(),
        kind: vtcode_desktop_protocol::InteractionKind::Question,
        wizard: false,
        title: "private prompt detail".into(),
        details: Vec::new(),
        choices: Vec::new(),
        steps: Vec::new(),
        selected_choice_id: None,
        selected_choice_ids: Vec::new(),
        plan_markdown: None,
    })
}

fn click_delivered_system_banner(cx: &mut TestAppContext) {
    let delivered = cx.delivered_system_notifications();
    assert_eq!(delivered.len(), 1, "one banner under test");
    cx.simulate_system_notification_response(gpui_kit::SystemNotificationResponse {
        tag: delivered[0].tag.clone(),
        action_id: None,
    });
    cx.run_until_parked();
}

#[gpui_kit::test]
fn system_notification_click_reselects_the_notified_thread(cx: &mut TestAppContext) {
    use vtcode_exec_events::{ThreadEvent, TurnCompletedEvent, Usage, VersionedThreadEvent};

    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    cx.update_window(window, |_, window, cx| {
        assert!(!window.is_window_active());
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.sessions = vec![deeplink_thread_fixture("thread-a"), deeplink_thread_fixture("thread-b")];
            view.selected_session_id = Some("thread-a".into());
            view.active_turn_id = Some("turn-1".into());
            view.running = true;
            view.event_tx
                .send(BackendEvent::ThreadEvent(vtcode_desktop_protocol::ThreadEventNotification {
                    session_id: "thread-a".into(),
                    turn_id: Some("turn-1".into()),
                    event: VersionedThreadEvent::new(ThreadEvent::TurnCompleted(TurnCompletedEvent {
                        final_response_item_id: None,
                        completed_at: None,
                        usage: Usage {
                            input_tokens: 0,
                            cached_input_tokens: 0,
                            cache_creation_tokens: 0,
                            output_tokens: 0,
                        },
                        in_progress_exec_sessions: Vec::new(),
                    })),
                }))
                .unwrap();
            let _batch_full = view.drain_events(window, cx);
            assert!(!view.running, "completion settles the turn");
        });
    })
    .unwrap();
    assert_eq!(cx.shown_system_notifications().len(), 1, "background completion posts one banner");
    while requests.try_recv().is_ok() {}

    // The user moves on before clicking: another thread, another surface.
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.selected_session_id = Some("thread-b".into());
            view.open_settings(window, cx);
        });
    })
    .unwrap();
    click_delivered_system_banner(cx);

    let BackendCommand::Request { method, params } = requests.try_recv().expect("click reloads the notified thread")
    else {
        panic!("click must send session/load, not stop");
    };
    assert_eq!(method, vtcode_desktop_protocol::method::SESSION_LOAD);
    assert_eq!(params, json!({ "sessionId": "thread-a" }));
    cx.update_window(window, |_, _, cx| {
        assert!(view.read(cx).current_route() == Route::Workspace, "click lands on the thread surface");
    })
    .unwrap();
}

#[gpui_kit::test]
fn system_notification_click_focuses_the_waiting_interaction(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.selected_session_id = Some("session".into());
            view.active_turn_id = Some("turn-3".into());
            view.running = true;
            view.event_tx
                .send(question_interaction_event("session", "turn-3", "interaction-1"))
                .unwrap();
            let _batch_full = view.drain_events(window, cx);
            assert!(view.interaction.is_some());
        });
        window.render_frame(cx);
        // Move to another surface without resolving the prompt.
        view.update(cx, |view, cx| view.open_settings(window, cx));
        window.render_frame(cx);
    })
    .unwrap();
    click_delivered_system_banner(cx);

    cx.update_window(window, |_, window, cx| {
        let view = view.read(cx);
        assert!(view.current_route() == Route::Workspace, "click returns to the thread");
        assert!(view.interaction_focus.contains_focused(window, cx), "click focuses the waiting prompt");
    })
    .unwrap();
}

#[gpui_kit::test]
fn system_notification_click_tolerates_a_resolved_interaction(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.selected_session_id = Some("session".into());
            view.active_turn_id = Some("turn-3".into());
            view.running = true;
            view.event_tx
                .send(question_interaction_event("session", "turn-3", "interaction-1"))
                .unwrap();
            let _batch_full = view.drain_events(window, cx);
            // Resolve before clicking: the banner now names a dead prompt.
            view.interaction = None;
            view.open_settings(window, cx);
        });
        window.render_frame(cx);
    })
    .unwrap();
    click_delivered_system_banner(cx);

    cx.update_window(window, |_, window, cx| {
        let view = view.read(cx);
        assert!(view.current_route() == Route::Workspace, "a stale click still lands on the thread");
        assert!(!view.interaction_focus.contains_focused(window, cx), "a stale click must not steal focus");
    })
    .unwrap();
}

#[gpui_kit::test]
fn system_notification_click_opens_update_settings(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        assert!(!window.is_window_active(), "a backgrounded window gets the system banner");
        view.update(cx, |view, cx| {
            view.maybe_notify_update_available(&update_status_event("0.180.5", Some("0.181.0"), true), window, cx);
        });
    })
    .unwrap();
    click_delivered_system_banner(cx);

    cx.update_window(window, |_, _, cx| {
        let view = view.read(cx);
        assert!(view.current_route() == Route::Settings, "click opens settings");
        assert_eq!(view.settings_tab, super::super::settings::SettingsTab::About);
    })
    .unwrap();
}

#[gpui_kit::test]
fn focused_waiting_prompt_posts_an_in_app_toast_instead_of_a_system_banner(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.selected_session_id = Some("session".into());
            view.active_turn_id = Some("turn-3".into());
            view.running = true;
            view.event_tx
                .send(question_interaction_event("session", "turn-3", "interaction-1"))
                .unwrap();
            let _batch_full = view.drain_events(window, cx);
            assert!(view.interaction.is_some());
        });
        window.render_frame(cx);
    })
    .unwrap();
    // The banner click activates the window headless, so the next waiting
    // prompt arrives while focused.
    click_delivered_system_banner(cx);
    cx.update_window(window, |_, window, _| {
        assert!(window.is_window_active());
    })
    .unwrap();

    let shown_before = cx.shown_system_notifications().len();
    cx.update_window(window, |_, window, cx| {
        assert!(window.notifications(cx).is_empty(), "backgrounded prompts stay system-only");
        view.update(cx, |view, cx| {
            view.event_tx
                .send(question_interaction_event("session", "turn-3", "interaction-2"))
                .unwrap();
            let _batch_full = view.drain_events(window, cx);
        });
        window.render_frame(cx);
        // Toast bodies stay private to Kit; the delivery split (no new system
        // notice, one new in-app toast) is the behavioral oracle.
        assert_eq!(
            window.notifications(cx).len(),
            1,
            "a focused waiting prompt toasts in-app where its click target lives"
        );
    })
    .unwrap();
    assert_eq!(
        cx.shown_system_notifications().len(),
        shown_before,
        "a focused prompt must not also escape to the OS center"
    );
}

#[gpui_kit::test]
fn notification_click_tolerates_an_unknown_session(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.open_settings(window, cx);
            // A thread deleted after its banner posted: the click still lands
            // somewhere sane and the unknown id fails through the normal
            // session-load response path, never a panic.
            view.open_notification_target(
                super::super::deeplinks::NotificationTarget::Thread { session_id: "vanished-thread".into() },
                window,
                cx,
            );
        });
    })
    .unwrap();

    let BackendCommand::Request { method, params } = requests.try_recv().expect("click reloads the thread") else {
        panic!("click must send session/load, not stop");
    };
    assert_eq!(method, vtcode_desktop_protocol::method::SESSION_LOAD);
    assert_eq!(params, json!({ "sessionId": "vanished-thread" }));
    cx.update_window(window, |_, _, cx| {
        assert!(view.read(cx).current_route() == Route::Workspace, "stale click lands on the workspace");
    })
    .unwrap();
}

#[gpui_kit::test]
fn turn_undo_success_toast_returns_to_the_undone_thread(cx: &mut TestAppContext) {
    use super::super::turn_undo::TurnUndoState;

    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.sessions = vec![deeplink_thread_fixture("thread-a"), deeplink_thread_fixture("thread-b")];
            view.selected_session_id = Some("thread-a".into());
            let _previous = view.turn_undos.insert("turn-9".into(), TurnUndoState::Pending);
            let result = serde_json::to_value(vtcode_desktop_protocol::TurnUndoResult {
                session_id: "thread-a".into(),
                turn_id: "turn-9".into(),
                undone: true,
                files: Vec::new(),
                reason: String::new(),
            })
            .expect("undo result serializes");
            view.event_tx
                .send(BackendEvent::Response {
                    method: vtcode_desktop_protocol::method::TURN_UNDO.into(),
                    result: Some(result),
                    error: None,
                })
                .unwrap();
            let _batch_full = view.drain_events(window, cx);
        });
        window.render_frame(cx);
        assert_eq!(window.notifications(cx).len(), 1, "a completed undo toasts in-app");
    })
    .unwrap();
    while requests.try_recv().is_ok() {}

    // The user moves on before clicking: another thread, another surface.
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.selected_session_id = Some("thread-b".into());
            view.open_settings(window, cx);
        });
    })
    .unwrap();

    // In-app toast clicks have no headless simulator (Kit exposes no stable
    // toast id), so fire the same handler the toast's click closure calls.
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.open_notification_target(
                super::super::deeplinks::NotificationTarget::Thread { session_id: "thread-a".into() },
                window,
                cx,
            );
        });
    })
    .unwrap();

    let BackendCommand::Request { method, params } = requests.try_recv().expect("click reloads the undone thread")
    else {
        panic!("click must send session/load, not stop");
    };
    assert_eq!(method, vtcode_desktop_protocol::method::SESSION_LOAD);
    assert_eq!(params, json!({ "sessionId": "thread-a" }));
    cx.update_window(window, |_, _, cx| {
        assert!(view.read(cx).current_route() == Route::Workspace, "click lands on the undone thread");
    })
    .unwrap();
}

#[gpui_kit::test]
fn system_notification_click_returns_a_disconnect_to_the_workspace(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, _requests) = tokio::sync::mpsc::channel(4);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.event_tx
                .send(BackendEvent::Exited { status: Some(1), authentication_required: false })
                .unwrap();
            let _batch_full = view.drain_events(window, cx);
            view.open_settings(window, cx);
        });
        window.render_frame(cx);
    })
    .unwrap();
    click_delivered_system_banner(cx);

    cx.update_window(window, |_, _, cx| {
        assert!(view.read(cx).current_route() == Route::Workspace, "click lands on the service status");
    })
    .unwrap();
}

/// A finished ad hoc-signed install names the Gatekeeper approval step: the
/// restart alone lands on a macOS block with no guidance otherwise.
#[gpui_kit::test]
fn completed_install_with_gatekeeper_names_the_approval_step(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.apply_update_status(update_status_json("0.181.7", Some("0.181.8"), true));
            view.open_settings(window, cx);
            view.settings_tab = super::super::settings::SettingsTab::About;
            view.handle_update_install_progress(vtcode_desktop_protocol::UpdateInstallState::Completed {
                version: "0.181.8".into(),
                needs_gatekeeper_approval: true,
            });
        });
        window.render_frame(cx);

        let props = super::super::about::AboutSettingsProps::capture(view.read(cx));
        let description = props.update_row_description();
        assert!(description.contains("0.181.8"), "{description}");
        assert!(description.contains("Open Anyway"), "{description}");
        assert!(
            window.try_find("about-settings-install-update").is_none(),
            "a gated install still needs a relaunch, not a reinstall"
        );
    })
    .unwrap();
}
