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
        "currentReleaseSeen": true,
        "channel": "stable",
        "checkDue": false,
    })
}

fn seed_status(view: &Entity<DesktopView>, payload: serde_json::Value, cx: &mut TestAppContext) {
    view.update(cx, |view, _| view.handle_update_status_response(payload));
}

/// Layer current-version notes onto a status payload (the post-update shape).
/// Notes start unseen, which is what arms the banner.
fn with_current_notes(mut payload: serde_json::Value, highlights: &[&str], url: Option<&str>) -> serde_json::Value {
    let object = payload.as_object_mut().expect("status payload is an object");
    let _ = object.insert("currentReleaseHighlights".into(), json!(highlights));
    let _ = object.insert("currentReleaseUrl".into(), json!(url));
    let _ = object.insert("currentReleaseSeen".into(), json!(false));
    payload
}

#[gpui_kit::test]
fn about_release_notes_row_opens_viewer_with_highlights(cx: &mut TestAppContext) {
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
    open_about_settings(window, &view, cx);
    cx.update_window(window, |_, window, cx| {
        scroll_settings_into_view(window, "about-settings-changelog", cx);
        assert_eq!(window.find("about-settings-changelog").label(), Some("Release notes"));
        assert_eq!(window.find("about-settings-changelog").role(), Some(gpui_kit::Role::Button));
        assert!(window.try_find("view-changelog").is_none(), "one changelog entry point on About");
        window.click("about-settings-changelog", cx);
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
fn about_pane_offers_a_single_changelog_entry(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    seed_status(&view, status_payload("0.181.0", Some("0.182.0"), true, &["Faster startup"], None), cx);
    open_about_settings(window, &view, cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("about-settings-check-for-updates").is_some(), "check keeps its slot");
        assert!(window.try_find("view-changelog").is_none(), "the update row has no duplicate changelog action");
        assert!(window.try_find("about-settings-changelog").is_some(), "release notes stay the one entry point");
    })
    .unwrap();
}

#[gpui_kit::test]
fn done_button_closes_and_consumes_return_focus(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    seed_status(&view, status_payload("0.181.0", Some("0.182.0"), true, &["Faster startup"], None), cx);
    open_about_settings(window, &view, cx);
    cx.update_window(window, |_, window, cx| {
        scroll_settings_into_view(window, "about-settings-changelog", cx);
        window.click("about-settings-changelog", cx);
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
        assert_eq!(view.read(cx).settings_tab, SettingsTab::About);
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
fn release_notes_banner_shows_once_and_dismiss_records_seen(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    seed_status(&view, post_update_payload(), cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(view.read(cx).release_notes_banner_visible());
        assert!(window.try_find("release-notes-banner").is_some(), "the banner renders inline");
        assert!(!view.read(cx).changelog_open, "the banner never steals the workspace with a modal");
        window.click("release-notes-dismiss", cx);
        window.render_frame(cx);
        assert!(!view.read(cx).release_notes_banner_visible(), "dismiss hides it");
        assert!(window.try_find("release-notes-banner").is_none());
    })
    .unwrap();
    // The service echoes the shared seen marker on the next status response.
    let mut seen = post_update_payload();
    let _ = seen.as_object_mut().unwrap().insert("currentReleaseSeen".into(), json!(true));
    seed_status(&view, seen, cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(!view.read(cx).release_notes_banner_visible(), "seen releases never re-show");
    })
    .unwrap();
}

#[gpui_kit::test]
fn release_notes_banner_hides_when_the_cli_already_showed_it(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    // A CLI launch printed the notes and recorded the shared marker, so the
    // desktop must not repeat them on its first status response.
    let mut payload = post_update_payload();
    let _ = payload
        .as_object_mut()
        .unwrap()
        .insert("currentReleaseSeen".into(), json!(true));
    seed_status(&view, payload, cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(!view.read(cx).release_notes_banner_visible(), "one release announces once across surfaces");
    })
    .unwrap();
}

#[gpui_kit::test]
fn release_notes_acknowledge_failure_stays_silent(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(8);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _cx| {
            view.backend = Some(BackendHandle { commands });
            view.handle_update_status_response(post_update_payload());
        });
        window.render_frame(cx);
        assert!(view.read(cx).release_notes_banner_visible());
        window.click("release-notes-dismiss", cx);
        window.render_frame(cx);
    })
    .unwrap();
    // The dismissal queued one acknowledgement carrying no payload.
    let BackendCommand::Request { method, .. } = requests.try_recv().expect("acknowledge was requested") else {
        panic!("dismissal must send update/acknowledge");
    };
    assert_eq!(method, vtcode_desktop_protocol::method::UPDATE_ACKNOWLEDGE);

    // An older service rejects it. Bookkeeping must never paint an error
    // banner: the banner already retired itself locally.
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.handle_backend_event(
                BackendEvent::Response {
                    method: vtcode_desktop_protocol::method::UPDATE_ACKNOWLEDGE.into(),
                    result: None,
                    error: Some("unknown desktop method".into()),
                },
                cx,
            );
        });
        window.render_frame(cx);
        assert!(view.read(cx).error.is_none(), "a failed acknowledgement stays silent");
        assert!(!view.read(cx).release_notes_banner_visible(), "the banner stays retired");
    })
    .unwrap();
    // It must not have cleared an in-flight status request either.
    while requests.try_recv().is_ok() {}
}

#[gpui_kit::test]
fn release_notes_banner_shows_on_the_launch_screen(cx: &mut TestAppContext) {
    // Fresh launch: no workspace, no session, the welcome screen owns the
    // column. The banner must still reach the user there — that is the whole
    // point of an inline banner instead of a workspace-only dialog.
    let (window, view) = open_desktop_sized_with_preferences_path(cx, 1280., 900., isolate_preferences(), false);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _cx| {
            view.zen_mode = false;
            view.workspace = None;
            view.selected_session_id = None;
            view.handle_update_status_response(post_update_payload());
        });
        window.render_frame(cx);
        assert!(window.try_find("welcome").is_some(), "the launch screen is showing");
        assert!(window.try_find("release-notes-banner").is_some(), "the banner does not need a workspace");
        assert!(window.try_find("release-notes-dismiss").is_some(), "and it is dismissible there");
    })
    .unwrap();
}

#[gpui_kit::test]
fn release_notes_banner_shows_in_zen_mode(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _cx| {
            view.zen_mode = true;
            view.handle_update_status_response(post_update_payload());
        });
        window.render_frame(cx);
        assert!(window.try_find("release-notes-banner").is_some(), "zen mode keeps the banner inline");
    })
    .unwrap();
}

#[gpui_kit::test]
fn release_notes_banner_waits_for_notes_and_stays_unrecorded(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    // Fresh install shape: up to date, no notes cached for the running version.
    seed_status(&view, status_payload("0.182.0", None, false, &[], None), cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(!view.read(cx).release_notes_banner_visible(), "nothing to show renders nothing");
    })
    .unwrap();
    // Notes landing later (a background check caught up) arm the banner.
    seed_status(&view, post_update_payload(), cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(view.read(cx).release_notes_banner_visible(), "late notes still get their moment");
    })
    .unwrap();
}

#[gpui_kit::test]
fn release_notes_banner_view_opens_current_changelog(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    // Skipped a version: 0.183 is out, but 0.182's own notes were never seen.
    // The banner advertises the running version, not the pending update.
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
        window.render_frame(cx);
        assert!(view.read(cx).release_notes_banner_visible());
        window.click("release-notes-open", cx);
        window.render_frame(cx);
        assert!(view.read(cx).changelog_open, "View opens the viewer on demand");
        assert_eq!(view.read(cx).changelog_mode, ChangelogMode::Current, "what's new shows current, not latest");
        assert!(!view.read(cx).release_notes_banner_visible(), "viewing retires the banner");
        assert!(window.try_find("changelog-subtitle").is_none(), "current mode, not the upgrade view");
        assert!(window.try_find("open-release-notes").is_some(), "current notes link out");
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
fn release_notes_banner_ignores_notification_toggles(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update(|cx| {
        view.update(cx, |view, _| {
            view.preferences.notifications.enabled = Some(false);
            view.preferences.notifications.updates = Some(false);
        });
    });
    seed_status(&view, post_update_payload(), cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(
            view.read(cx).release_notes_banner_visible(),
            "installed-version notes show once regardless of update toggles, like the TUI banner"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn changelog_viewer_traps_tab_across_its_footer_actions(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    seed_status(
        &view,
        status_payload(
            "0.181.0",
            Some("0.182.0"),
            true,
            &["Faster startup"],
            Some("https://github.com/vinhnx/vtcode/releases/tag/0.182.0"),
        ),
        cx,
    );
    open_about_settings(window, &view, cx);
    cx.update_window(window, |_, window, cx| {
        scroll_settings_into_view(window, "about-settings-changelog", cx);
        window.click("about-settings-changelog", cx);
        window.render_frame(cx);
        assert!(window.try_find("changelog-dialog").is_some(), "the viewer opens");
        assert!(window.try_find("open-release-notes").is_some(), "two-stop footer renders");
        assert_tab_trapped_within_dialog(window, "open-release-notes", "close-changelog", cx);
    })
    .unwrap();
}

fn open_about_settings(window: AnyWindowHandle, view: &Entity<DesktopView>, cx: &mut TestAppContext) {
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.open_settings(window, cx);
            view.settings_tab = SettingsTab::About;
        });
        window.render_frame(cx);
    })
    .unwrap();
    cx.run_until_parked();
}

/// Up-to-date launch that still knows the newest release from the cache: the
/// About row must offer the notes even though no update is pending, while the
/// update-specific changelog action stays hidden.
#[gpui_kit::test]
fn about_row_reads_latest_notes_without_an_offered_update(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    seed_status(
        &view,
        status_payload(
            "0.182.0",
            Some("0.183.0"),
            false,
            &["Something newer"],
            Some("https://github.com/vinhnx/vtcode/releases/tag/0.183.0"),
        ),
        cx,
    );
    open_about_settings(window, &view, cx);
    cx.update_window(window, |_, window, cx| {
        scroll_settings_into_view(window, "about-settings-changelog", cx);
        assert_eq!(
            window.find("about-settings-changelog").label(),
            Some("Release notes"),
            "the row carries its title as the accessible name, like the license rows"
        );
        assert_ne!(window.find("about-settings-changelog").disabled(), Some(true), "notes are readable");
    })
    .unwrap();
    cx.update_window(window, |_, window, cx| {
        window.click("about-settings-changelog", cx);
        window.render_frame(cx);
    })
    .unwrap();
    cx.update(|cx| {
        let view = view.read(cx);
        assert!(view.changelog_open, "the About row opens the viewer");
        assert_eq!(view.changelog_mode, ChangelogMode::Latest);
    });
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("changelog-dialog").is_some());
        assert!(
            window.try_find("changelog-subtitle").is_none(),
            "reading notes without an offered update omits the upgrade arrow"
        );
        assert!(window.try_find("changelog-highlights").is_some());
    })
    .unwrap();
}

/// No cached notes anywhere: the row stays present but inert, explaining what
/// fills it, instead of offering an empty viewer.
#[gpui_kit::test]
fn about_row_is_disabled_without_cached_notes(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    seed_status(&view, status_payload("0.182.0", None, false, &[], None), cx);
    open_about_settings(window, &view, cx);
    cx.update_window(window, |_, window, cx| {
        scroll_settings_into_view(window, "about-settings-changelog", cx);
        assert_eq!(window.find("about-settings-changelog").disabled(), Some(true), "nothing to read");
        window.click("about-settings-changelog", cx);
        window.render_frame(cx);
    })
    .unwrap();
    cx.update(|cx| assert!(!view.read(cx).changelog_open, "a disabled row cannot open an empty viewer"));
}

/// The running version's own cached notes are the best available changelog,
/// so the row falls back to `Current` mode when no newer release is cached.
#[gpui_kit::test]
fn about_row_falls_back_to_current_notes(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    seed_status(&view, post_update_payload(), cx);
    open_about_settings(window, &view, cx);
    cx.update_window(window, |_, window, cx| {
        scroll_settings_into_view(window, "about-settings-changelog", cx);
        window.click("about-settings-changelog", cx);
        window.render_frame(cx);
    })
    .unwrap();
    cx.update(|cx| {
        let view = view.read(cx);
        assert!(view.changelog_open);
        assert_eq!(view.changelog_mode, ChangelogMode::Current, "running notes are all there is");
    });
}

/// A cached release whose notes carry no Highlights section is still readable:
/// the row stays enabled and the viewer explains the gap while keeping the
/// full-release-notes link.
#[gpui_kit::test]
fn about_row_opens_a_link_only_viewer_without_highlights(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    seed_status(
        &view,
        status_payload(
            "0.182.0",
            Some("0.183.0"),
            false,
            &[],
            Some("https://github.com/vinhnx/vtcode/releases/tag/0.183.0"),
        ),
        cx,
    );
    open_about_settings(window, &view, cx);
    cx.update_window(window, |_, window, cx| {
        scroll_settings_into_view(window, "about-settings-changelog", cx);
        assert_ne!(
            window.find("about-settings-changelog").disabled(),
            Some(true),
            "notes without Highlights still describe a readable release"
        );
        window.click("about-settings-changelog", cx);
        window.render_frame(cx);
        assert!(window.try_find("changelog-empty").is_some(), "the gap explains itself");
        assert!(window.try_find("open-release-notes").is_some(), "the release page stays reachable");
    })
    .unwrap();
}

/// Rendering the About pane must never issue a request. The pane reads a
/// cached snapshot and has no retry affordance, so a render-time fetch would
/// repeat on every frame for a service that keeps failing (the Storage summary
/// can fetch on display because its errored state renders a Retry button).
/// Selection and launch are the only request paths.
#[gpui_kit::test]
fn about_pane_never_requests_a_status_while_rendering(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(16);
    cx.update(|cx| cx.set_reduce_motion(true));
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.open_settings(window, cx);
            view.settings_tab = SettingsTab::About;
        });
        window.render_frame(cx);
    })
    .unwrap();
    cx.run_until_parked();
    while requests.try_recv().is_ok() {}
    cx.update_window(window, |_, window, cx| {
        for _ in 0..3 {
            window.render_frame(cx);
        }
    })
    .unwrap();
    assert!(requests.try_recv().is_err(), "rendering the pane must not request a status");
}

/// The service never sends an older release as `latest_version`, but the
/// client's chooser must still compose correctly when it does not: with only
/// the running version's notes cached, the About row names *that* version and
/// opens Current mode rather than inventing a "latest" release.
#[gpui_kit::test]
fn about_row_names_the_running_version_when_no_newer_release_is_known(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    seed_status(&view, post_update_payload(), cx);
    open_about_settings(window, &view, cx);
    cx.update_window(window, |_, window, cx| {
        scroll_settings_into_view(window, "about-settings-changelog", cx);
        window.click("about-settings-changelog", cx);
        window.render_frame(cx);
    })
    .unwrap();
    cx.update(|cx| {
        let view = view.read(cx);
        assert!(view.changelog_open);
        assert_eq!(view.changelog_mode, ChangelogMode::Current);
        assert_eq!(
            view.best_changelog_release(),
            Some((view.update_status.as_ref().expect("status").current_version.as_str(), ChangelogMode::Current)),
            "the chooser pairs the running version with Current mode"
        );
    });
}
