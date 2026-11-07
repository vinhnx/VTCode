use super::super::deeplinks::{DeepLink, NotificationTarget};
use super::super::licenses::LicenseDocument;
use super::super::settings::SettingsTab;
use super::super::updates::ChangelogMode;
use super::*;

fn status_payload(
    current: &str,
    latest: Option<&str>,
    available: bool,
    highlights: &[&str],
    release_url: Option<&str>,
) -> serde_json::Value {
    json!({
        "currentVersion": current,
        "latestVersion": latest,
        "updateAvailable": available,
        "releaseUrl": release_url,
        "releaseHighlights": highlights,
        "channel": "stable",
        "checkDue": false,
    })
}

fn seed_status(view: &Entity<DesktopView>, payload: serde_json::Value, cx: &mut TestAppContext) {
    view.update(cx, |view, _| view.handle_update_status_response(payload));
}

/// Layer current-version notes onto a status payload (the post-update shape).
fn with_current_notes(mut payload: serde_json::Value, highlights: &[&str], url: Option<&str>) -> serde_json::Value {
    let object = payload.as_object_mut().expect("status payload is an object");
    let _ = object.insert("currentReleaseHighlights".into(), json!(highlights));
    let _ = object.insert("currentReleaseUrl".into(), json!(url));
    payload
}

fn consume_whats_new(window: AnyWindowHandle, view: &Entity<DesktopView>, cx: &mut TestAppContext) {
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.maybe_show_pending_whats_new(window, cx));
        window.render_frame(cx);
    })
    .unwrap();
    cx.run_until_parked();
}

fn open_notification_settings(window: AnyWindowHandle, view: &Entity<DesktopView>, cx: &mut TestAppContext) {
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.open_settings(window, cx);
            view.settings_tab = SettingsTab::Notifications;
        });
        window.render_frame(cx);
    })
    .unwrap();
    cx.run_until_parked();
}

#[gpui_kit::test]
fn settings_button_opens_viewer_with_highlights(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    seed_status(
        &view,
        status_payload(
            "0.181.0",
            Some("0.182.0"),
            true,
            &["Faster startup", "New model picker"],
            Some("https://github.com/vinhnx/vtcode/releases/tag/0.182.0"),
        ),
        cx,
    );
    open_notification_settings(window, &view, cx);
    cx.update_window(window, |_, window, cx| {
        assert_eq!(window.find("view-changelog").label(), Some("View changelog"));
        assert_eq!(window.find("view-changelog").role(), Some(gpui_kit::Role::Button));
        window.click("view-changelog", cx);
        window.render_frame(cx);
    })
    .unwrap();
    cx.update(|cx| {
        assert!(view.read(cx).changelog_open, "settings button opens the viewer");
    });
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("changelog-dialog").is_some(), "the viewer opens");
        assert_eq!(window.find("changelog-dialog").role(), Some(gpui_kit::Role::Dialog));
        assert!(window.try_find("changelog-highlights").is_some(), "highlights render");
        assert!(window.try_find("changelog-empty").is_none(), "empty state stays hidden");
        assert!(window.try_find("open-release-notes").is_some(), "browser action renders");
    })
    .unwrap();
}

#[gpui_kit::test]
fn settings_button_hides_without_an_available_update(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    seed_status(&view, status_payload("0.182.0", None, false, &[], None), cx);
    open_notification_settings(window, &view, cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("check-for-updates").is_some(), "check keeps its slot");
        assert!(window.try_find("view-changelog").is_none(), "no update means no viewer entry");
    })
    .unwrap();
}

#[gpui_kit::test]
fn done_button_closes_and_consumes_return_focus(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    seed_status(&view, status_payload("0.181.0", Some("0.182.0"), true, &["Faster startup"], None), cx);
    open_notification_settings(window, &view, cx);
    cx.update_window(window, |_, window, cx| {
        window.click("view-changelog", cx);
        window.render_frame(cx);
        assert!(view.read(cx).changelog_open);
        window.click("close-changelog", cx);
        window.render_frame(cx);
    })
    .unwrap();
    cx.update(|cx| {
        let view = view.read(cx);
        assert!(!view.changelog_open, "done closes the viewer");
        assert!(view.changelog_return_focus.is_none(), "return focus is consumed");
    });
}

#[gpui_kit::test]
fn escape_dismisses_viewer_before_settings(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    seed_status(&view, status_payload("0.181.0", Some("0.182.0"), true, &["Faster startup"], None), cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.open_settings(window, cx);
            view.open_changelog(window, cx);
        });
        window.render_frame(cx);
        assert!(view.read(cx).changelog_open);
        window.press("escape", cx);
        window.render_frame(cx);
        assert!(!view.read(cx).changelog_open, "escape closes the viewer first");
        assert!(view.read(cx).current_route() == Route::Settings, "settings stays open");
    })
    .unwrap();
}

#[gpui_kit::test]
fn empty_highlights_render_link_only_state(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    seed_status(
        &view,
        status_payload(
            "0.181.0",
            Some("0.182.0"),
            true,
            &[],
            Some("https://github.com/vinhnx/vtcode/releases/tag/0.182.0"),
        ),
        cx,
    );
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.open_changelog(window, cx));
        window.render_frame(cx);
        assert!(window.try_find("changelog-dialog").is_some());
        assert!(window.try_find("changelog-empty").is_some(), "empty state explains itself");
        assert!(window.try_find("changelog-highlights").is_none(), "no list without items");
        assert!(window.try_find("open-release-notes").is_some(), "browser action still offers recovery");
    })
    .unwrap();
}

#[gpui_kit::test]
fn invalid_release_url_hides_browser_action(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    for bad_url in ["not a url", "javascript:alert(1)", "ftp://example.test/release"] {
        seed_status(&view, status_payload("0.181.0", Some("0.182.0"), true, &["Faster startup"], Some(bad_url)), cx);
        cx.update_window(window, |_, window, cx| {
            view.update(cx, |view, cx| view.open_changelog(window, cx));
            window.render_frame(cx);
            assert!(window.try_find("open-release-notes").is_none(), "unopenable URL hides the action: {bad_url}");
            assert!(window.try_find("close-changelog").is_some(), "done always renders");
            view.update(cx, |view, cx| view.close_changelog(window, cx));
        })
        .unwrap();
    }
}

#[gpui_kit::test]
fn notification_click_opens_viewer_for_current_version(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    seed_status(&view, status_payload("0.181.0", Some("0.182.0"), true, &["Faster startup"], None), cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.open_notification_target(
                NotificationTarget::UpdateChangelog { version: "0.182.0".into() },
                window,
                cx,
            );
        });
        window.render_frame(cx);
        assert!(view.read(cx).changelog_open, "click opens the viewer");
        assert!(window.try_find("changelog-dialog").is_some());
    })
    .unwrap();
}

#[gpui_kit::test]
fn stale_notification_click_falls_back_to_settings(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    seed_status(&view, status_payload("0.181.0", Some("0.183.0"), true, &["Something newer"], None), cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            // The banner advertised 0.182.0, but a refresh moved on already.
            view.open_notification_target(
                NotificationTarget::UpdateChangelog { version: "0.182.0".into() },
                window,
                cx,
            );
        });
        window.render_frame(cx);
        assert!(!view.read(cx).changelog_open, "stale click never opens a mismatched viewer");
        assert!(view.read(cx).current_route() == Route::Settings, "stale click lands on settings");
        assert_eq!(view.read(cx).settings_tab, SettingsTab::Notifications);
    })
    .unwrap();
}

#[gpui_kit::test]
fn refresh_clearing_availability_closes_viewer(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    seed_status(&view, status_payload("0.181.0", Some("0.182.0"), true, &["Faster startup"], None), cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.open_changelog(window, cx));
        window.render_frame(cx);
        assert!(view.read(cx).changelog_open);
        // Updated elsewhere: the refresh reports nothing available.
        view.update(cx, |view, _| {
            view.handle_update_status_response(status_payload("0.182.0", None, false, &[], None));
        });
        window.render_frame(cx);
        assert!(!view.read(cx).changelog_open, "nothing viewable means nothing open");
    })
    .unwrap();
}

#[gpui_kit::test]
fn opening_viewer_discards_license_viewer_instead_of_stacking(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    seed_status(&view, status_payload("0.181.0", Some("0.182.0"), true, &["Faster startup"], None), cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.open_license_viewer(LicenseDocument::Mit, window, cx);
            view.open_changelog(window, cx);
        });
        window.render_frame(cx);
        assert!(view.read(cx).changelog_open);
        assert!(view.read(cx).license_viewer.is_none(), "one modal at a time: no stacked scrims");
        assert!(window.try_find("changelog-dialog").is_some());
        assert!(window.try_find("license-viewer").is_none());
    })
    .unwrap();
}

#[gpui_kit::test]
fn open_changelog_refuses_without_an_available_update(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.open_changelog(window, cx));
        window.render_frame(cx);
        assert!(!view.read(cx).changelog_open, "no status means no viewer");
        assert!(window.try_find("changelog-dialog").is_none());
    })
    .unwrap();
    seed_status(&view, status_payload("0.182.0", None, false, &[], None), cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.open_changelog(window, cx));
        window.render_frame(cx);
        assert!(!view.read(cx).changelog_open, "quiet status means no viewer");
    })
    .unwrap();
}

fn post_update_payload() -> serde_json::Value {
    with_current_notes(
        status_payload("0.182.0", None, false, &[], None),
        &["Freshly installed"],
        Some("https://github.com/vinhnx/vtcode/releases/tag/v0.182.0"),
    )
}

#[gpui_kit::test]
fn whats_new_shows_current_notes_once_and_records_seen(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    seed_status(&view, post_update_payload(), cx);
    consume_whats_new(window, &view, cx);
    cx.update(|cx| {
        let state = view.read(cx);
        assert!(state.changelog_open, "first sighting opens the viewer");
        assert_eq!(state.changelog_mode, ChangelogMode::Current);
        assert_eq!(
            state.preferences.notifications.last_seen_changelog_version.as_deref(),
            Some("0.182.0"),
            "shown versions record seen"
        );
    });
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("changelog-dialog").is_some());
        assert!(window.try_find("changelog-highlights").is_some());
        assert!(window.try_find("changelog-subtitle").is_none(), "current mode omits the upgrade path");
        assert!(window.try_find("open-release-notes").is_some(), "current notes link out");
        view.update(cx, |view, cx| view.close_changelog(window, cx));
    })
    .unwrap();
    // A later response for the same version stays silent.
    seed_status(&view, post_update_payload(), cx);
    consume_whats_new(window, &view, cx);
    cx.update(|cx| assert!(!view.read(cx).changelog_open, "seen versions never re-show"));
}

#[gpui_kit::test]
fn whats_new_skips_without_notes_and_stays_unrecorded(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    // Fresh install shape: up to date, no notes cached for the running version.
    seed_status(&view, status_payload("0.182.0", None, false, &[], None), cx);
    consume_whats_new(window, &view, cx);
    cx.update(|cx| {
        assert!(!view.read(cx).changelog_open, "nothing to show opens nothing");
        assert!(
            view.read(cx).preferences.notifications.last_seen_changelog_version.is_none(),
            "notes-less launches stay unrecorded so notes showing up later still display"
        );
    });
    // Notes landing later (a background check caught up) display on sighting.
    seed_status(&view, post_update_payload(), cx);
    consume_whats_new(window, &view, cx);
    cx.update(|cx| {
        assert!(view.read(cx).changelog_open, "late notes still get their moment");
        assert_eq!(view.read(cx).preferences.notifications.last_seen_changelog_version.as_deref(), Some("0.182.0"));
    });
}

#[gpui_kit::test]
fn whats_new_skips_while_modal_open_and_retries(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    seed_status(&view, post_update_payload(), cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.open_license_viewer(LicenseDocument::Mit, window, cx));
        window.render_frame(cx);
        assert!(view.read(cx).license_viewer.is_some());
    })
    .unwrap();
    consume_whats_new(window, &view, cx);
    cx.update(|cx| {
        assert!(!view.read(cx).changelog_open, "no stealing an open modal");
        assert!(view.read(cx).license_viewer.is_some(), "the open modal survives");
        assert!(
            view.read(cx).preferences.notifications.last_seen_changelog_version.is_none(),
            "skipped moments stay unrecorded for retry"
        );
    });
    // Once the modal closes, the next response retries.
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.close_license_viewer(window, cx));
        window.render_frame(cx);
    })
    .unwrap();
    seed_status(&view, post_update_payload(), cx);
    consume_whats_new(window, &view, cx);
    cx.update(|cx| assert!(view.read(cx).changelog_open, "retry after the modal closes"));
}

#[gpui_kit::test]
fn whats_new_shows_current_despite_an_available_update(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    // Skipped a version: 0.183 is out, but 0.182's own notes were never seen.
    // TUI parity prints both the current notes and the update banner.
    let payload = with_current_notes(
        status_payload(
            "0.182.0",
            Some("0.183.0"),
            true,
            &["Something newer"],
            Some("https://github.com/vinhnx/vtcode/releases/tag/v0.183.0"),
        ),
        &["Freshly installed"],
        Some("https://github.com/vinhnx/vtcode/releases/tag/v0.182.0"),
    );
    seed_status(&view, payload, cx);
    consume_whats_new(window, &view, cx);
    cx.update(|cx| {
        let state = view.read(cx);
        assert!(state.changelog_open);
        assert_eq!(state.changelog_mode, ChangelogMode::Current, "what's new shows current, not latest");
        assert_eq!(state.preferences.notifications.last_seen_changelog_version.as_deref(), Some("0.182.0"));
    });
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("changelog-subtitle").is_none(), "current mode, not the upgrade view");
    })
    .unwrap();
}

#[gpui_kit::test]
fn changelog_verb_prefers_the_available_update(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let payload = with_current_notes(
        status_payload(
            "0.182.0",
            Some("0.183.0"),
            true,
            &["Something newer"],
            Some("https://github.com/vinhnx/vtcode/releases/tag/v0.183.0"),
        ),
        &["Freshly installed"],
        Some("https://github.com/vinhnx/vtcode/releases/tag/v0.182.0"),
    );
    seed_status(&view, payload, cx);
    cx.update_window(window, |_, window, cx| {
        let reachable = view.update(cx, |view, cx| view.open_deeplink(DeepLink::Changelog, window, cx));
        window.render_frame(cx);
        assert!(reachable, "an update makes the verb reachable");
        assert!(view.read(cx).changelog_open);
        assert_eq!(view.read(cx).changelog_mode, ChangelogMode::Latest, "the verb prefers latest");
        assert!(window.try_find("changelog-subtitle").is_some(), "upgrade view renders");
    })
    .unwrap();
}

#[gpui_kit::test]
fn changelog_verb_falls_back_to_current_notes(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    seed_status(&view, post_update_payload(), cx);
    cx.update_window(window, |_, window, cx| {
        let reachable = view.update(cx, |view, cx| view.open_deeplink(DeepLink::Changelog, window, cx));
        window.render_frame(cx);
        assert!(reachable, "known current notes make the verb reachable");
        assert_eq!(view.read(cx).changelog_mode, ChangelogMode::Current);
        assert!(window.try_find("changelog-dialog").is_some());
    })
    .unwrap();
}

#[gpui_kit::test]
fn changelog_verb_refused_without_notes_leaves_state_untouched(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    seed_status(&view, status_payload("0.182.0", None, false, &[], None), cx);
    cx.update_window(window, |_, window, cx| {
        let reachable = view.update(cx, |view, cx| view.open_deeplink(DeepLink::Changelog, window, cx));
        window.render_frame(cx);
        assert!(!reachable, "nothing viewable refuses the verb so /go reports it");
        assert!(!view.read(cx).changelog_open, "refused links leave state untouched");
        assert!(window.try_find("changelog-dialog").is_none());
    })
    .unwrap();
}

#[gpui_kit::test]
fn whats_new_ignores_notification_toggles(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update(|cx| {
        view.update(cx, |view, _| {
            view.preferences.notifications.enabled = Some(false);
            view.preferences.notifications.updates = Some(false);
        });
    });
    seed_status(&view, post_update_payload(), cx);
    consume_whats_new(window, &view, cx);
    cx.update(|cx| {
        assert!(
            view.read(cx).changelog_open,
            "installed-version notes show once regardless of update toggles, like the TUI banner"
        );
    });
}
