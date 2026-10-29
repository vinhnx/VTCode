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

fn update_status_event(current: &str, latest: Option<&str>, available: bool) -> BackendEvent {
    BackendEvent::Response {
        method: vtcode_desktop_protocol::method::UPDATE_STATUS.into(),
        result: Some(json!({
            "currentVersion": current,
            "latestVersion": latest,
            "updateAvailable": available,
            "releaseUrl": "https://github.com/vinhnx/vtcode/releases/tag/v0.181.0",
            "channel": "stable",
            "checkDue": false,
        })),
        error: None,
    }
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
        assert!(window.try_find("check-for-updates").is_some());

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
fn check_for_updates_button_requests_status(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, requests) = tokio::sync::mpsc::channel(8);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.open_settings(window, cx);
        });
        window.render_frame(cx);
        select_settings_category(window, 5, cx);
        window.render_frame(cx);
        window.click("check-for-updates", cx);
        window.render_frame(cx);
        assert!(view.read(cx).update_request_pending);
        let mut requests = requests;
        let BackendCommand::Request { method, .. } = requests.try_recv().unwrap() else {
            panic!("update request")
        };
        assert_eq!(method, vtcode_desktop_protocol::method::UPDATE_STATUS);
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
