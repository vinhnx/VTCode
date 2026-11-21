use super::super::deeplinks::{DeepLink, NotificationTarget};
use super::super::licenses::LicenseDocument;
use super::super::settings::SettingsTab;
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

/// Status carrying the installed binary's own notes, already announced so it
/// does not arm the what's-new banner. The reader only ever shows these, never
/// the notes of a cached-but-uninstalled newer release.
fn installed_notes_payload(current: &str, highlights: &[&str], url: Option<&str>) -> serde_json::Value {
    let mut payload = with_current_notes(status_payload(current, None, false, &[], None), highlights, url);
    let _ = payload
        .as_object_mut()
        .expect("status payload is an object")
        .insert("currentReleaseSeen".into(), json!(true));
    payload
}

/// Deliver a launch-path status response through the drain loop (the only
/// path with a window, so the only one that can auto-open the dialog).
fn drain_status(
    window: AnyWindowHandle,
    view: &Entity<DesktopView>,
    payload: serde_json::Value,
    cx: &mut TestAppContext,
) {
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.event_tx
                .send(BackendEvent::Response {
                    method: vtcode_desktop_protocol::method::UPDATE_STATUS.into(),
                    result: Some(payload),
                    error: None,
                })
                .unwrap();
            let _batch_full = view.drain_events(window, cx);
        });
        window.render_frame(cx);
    })
    .unwrap();
}

#[gpui_kit::test]
fn about_release_notes_row_opens_viewer_with_highlights(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    seed_status(
        &view,
        installed_notes_payload(
            "0.181.0",
            &["Faster startup", "New model picker"],
            Some("https://github.com/vinhnx/vtcode/releases/tag/0.181.0"),
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
    seed_status(&view, installed_notes_payload("0.181.0", &["Faster startup"], None), cx);
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
    seed_status(&view, installed_notes_payload("0.181.0", &["Faster startup"], None), cx);
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
fn empty_highlights_keep_the_reader_closed(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    // The installed version's cached notes have no Highlights section: there
    // is nothing to read, so neither the row nor the verb opens a shell dialog.
    seed_status(
        &view,
        installed_notes_payload("0.181.0", &[], Some("https://github.com/vinhnx/vtcode/releases/tag/0.181.0")),
        cx,
    );
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.open_changelog(window, cx));
        window.render_frame(cx);
        assert!(window.try_find("changelog-dialog").is_none());
    })
    .unwrap();
}

#[gpui_kit::test]
fn invalid_release_url_hides_browser_action(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    for bad_url in ["not a url", "javascript:alert(1)", "ftp://example.test/release"] {
        seed_status(&view, installed_notes_payload("0.181.0", &["Faster startup"], Some(bad_url)), cx);
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
fn update_notification_click_lands_on_about_update_status(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    seed_status(&view, status_payload("0.181.0", Some("0.182.0"), true, &["Faster startup"], None), cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.open_notification_target(NotificationTarget::UpdateSettings, window, cx);
        });
        window.render_frame(cx);
        assert!(view.read(cx).current_route() == Route::Settings, "an offer lands on Settings");
        assert_eq!(view.read(cx).settings_tab, SettingsTab::About, "on the About page");
        assert!(!view.read(cx).changelog_open, "never opens notes for an uninstalled release");
        assert!(window.try_find("about-settings-check-for-updates").is_some(), "Update status actions are visible");
    })
    .unwrap();
}

#[gpui_kit::test]
fn refresh_clearing_availability_closes_viewer(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    seed_status(&view, installed_notes_payload("0.181.0", &["Faster startup"], None), cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.open_changelog(window, cx));
        window.render_frame(cx);
        assert!(view.read(cx).changelog_open);
        // Updated elsewhere: the refresh reports no installed notes.
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
    seed_status(&view, installed_notes_payload("0.181.0", &["Faster startup"], None), cx);
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
fn open_changelog_refuses_without_installed_notes(cx: &mut TestAppContext) {
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
fn release_notes_banner_shows_on_the_welcome_screen(cx: &mut TestAppContext) {
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
        assert!(window.try_find("welcome").is_some(), "the welcome screen is showing");
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

/// Launch-time shape after an upgrade the cache never saw: the running
/// version's notes are unseen, but no highlights are cached for them.
fn unseen_without_notes_payload() -> serde_json::Value {
    let mut payload = status_payload("0.182.0", None, false, &[], None);
    let _ = payload
        .as_object_mut()
        .unwrap()
        .insert("currentReleaseSeen".into(), json!(false));
    payload
}

/// A desktop-only install never warms the update cache, so the launch-time
/// status can carry unseen-but-empty notes. The client backfills with one live
/// check instead of staying quiet on its first launch after an upgrade.
#[gpui_kit::test]
fn launch_status_without_notes_triggers_one_live_backfill(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(8);
    view.update(cx, |view, _| {
        view.backend = Some(BackendHandle { commands });
    });
    seed_status(&view, unseen_without_notes_payload(), cx);
    let BackendCommand::Request { method, .. } = requests.try_recv().expect("backfill was requested") else {
        panic!("unseen-but-empty notes must trigger update/check");
    };
    assert_eq!(method, vtcode_desktop_protocol::method::UPDATE_CHECK);
    assert!(requests.try_recv().is_err(), "exactly one backfill check");
    // The live check lands with notes: the banner arms without any auto-opened
    // dialog (the backfill arrives after launch settles, so it stays passive).
    view.update(cx, |view, _| {
        view.handle_update_check_response(post_update_payload());
    });
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(view.read(cx).release_notes_banner_visible(), "backfilled notes arm the banner");
        assert!(!view.read(cx).changelog_open, "the backfill never auto-opens the dialog");
    })
    .unwrap();
    assert!(requests.try_recv().is_err(), "served notes never re-trigger");
}

/// The notes backfill is best-effort: an offline launch must not paint an
/// error for a quiet refresh, and the banner simply stays hidden.
#[gpui_kit::test]
fn notes_backfill_failure_stays_silent(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(8);
    view.update(cx, |view, _| {
        view.backend = Some(BackendHandle { commands });
    });
    seed_status(&view, unseen_without_notes_payload(), cx);
    let _ = requests.try_recv().expect("backfill was requested");
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.handle_backend_event(
                BackendEvent::Response {
                    method: vtcode_desktop_protocol::method::UPDATE_CHECK.into(),
                    result: None,
                    error: Some("network unreachable".into()),
                },
                cx,
            );
        });
        window.render_frame(cx);
        assert!(view.read(cx).error.is_none(), "a failed backfill stays silent");
        assert!(!view.read(cx).release_notes_banner_visible(), "and the banner stays hidden");
    })
    .unwrap();
}

/// Notes present at launch need no backfill: the status already arms the
/// banner (and the launch path auto-opens the dialog).
#[gpui_kit::test]
fn launch_status_with_notes_skips_the_backfill(cx: &mut TestAppContext) {
    let (_window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(8);
    view.update(cx, |view, _| {
        view.backend = Some(BackendHandle { commands });
    });
    seed_status(&view, post_update_payload(), cx);
    assert!(requests.try_recv().is_err(), "served notes need no live check");
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
        assert!(!view.read(cx).release_notes_banner_visible(), "viewing retires the banner");
        assert!(window.try_find("changelog-subtitle").is_none(), "installed notes carry no upgrade arrow");
        assert!(window.try_find("open-release-notes").is_some(), "current notes link out");
    })
    .unwrap();
}

/// First launch of an updated version announces itself: the launch-time
/// status carries unseen notes, so the what's-new dialog opens on its own
/// instead of waiting for the banner's View click.
#[gpui_kit::test]
fn launch_status_with_unseen_notes_auto_opens_current_changelog(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(8);
    view.update(cx, |view, _| {
        view.backend = Some(BackendHandle { commands });
    });
    drain_status(window, &view, post_update_payload(), cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(view.read(cx).changelog_open, "first sight opens the dialog");
        assert!(window.try_find("changelog-dialog").is_some());
        assert!(window.try_find("changelog-highlights").is_some(), "highlights render");
        assert!(!view.read(cx).release_notes_banner_visible(), "auto-open retires the banner");
        assert!(window.try_find("release-notes-banner").is_none());
    })
    .unwrap();
    let BackendCommand::Request { method, .. } = requests.try_recv().expect("auto-open acknowledges") else {
        panic!("auto-open must send update/acknowledge");
    };
    assert_eq!(method, vtcode_desktop_protocol::method::UPDATE_ACKNOWLEDGE);
}

/// Seen notes, notes-less launches, and failed responses all stay quiet: the
/// dialog opens only for genuinely unseen notes on a successful status.
#[gpui_kit::test]
fn launch_status_without_unseen_notes_never_auto_opens(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    // The CLI already announced this release: the shared marker is set.
    let mut seen = post_update_payload();
    let _ = seen.as_object_mut().unwrap().insert("currentReleaseSeen".into(), json!(true));
    drain_status(window, &view, seen, cx);
    cx.update(|cx| assert!(!view.read(cx).changelog_open, "seen notes stay quiet"));
    // Fresh-install shape: up to date with no notes cached at all.
    drain_status(window, &view, status_payload("0.182.0", None, false, &[], None), cx);
    cx.update(|cx| assert!(!view.read(cx).changelog_open, "notes-less launches stay quiet"));
    // A failing service must not pop the dialog either.
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.event_tx
                .send(BackendEvent::Response {
                    method: vtcode_desktop_protocol::method::UPDATE_STATUS.into(),
                    result: None,
                    error: Some("status unavailable".into()),
                })
                .unwrap();
            let _batch_full = view.drain_events(window, cx);
        });
        window.render_frame(cx);
        assert!(!view.read(cx).changelog_open, "failed responses stay quiet");
    })
    .unwrap();
}

/// One announcement per launch: after the dialog closes, a refresh still
/// echoing unseen (a failed acknowledgement) re-arms nothing.
#[gpui_kit::test]
fn release_notes_auto_open_fires_once_per_launch(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    drain_status(window, &view, post_update_payload(), cx);
    cx.update(|cx| assert!(view.read(cx).changelog_open, "first sight opens"));
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.close_changelog(window, cx));
        window.render_frame(cx);
    })
    .unwrap();
    drain_status(window, &view, post_update_payload(), cx);
    cx.update(|cx| {
        assert!(!view.read(cx).changelog_open, "a closed announcement never re-opens");
        assert!(!view.read(cx).release_notes_banner_visible(), "and the banner stays retired");
    });
}

/// An already-open viewer is left alone: notes arriving mid-read do not yank
/// what the user is reading, and the banner stays as the fallback.
#[gpui_kit::test]
fn release_notes_auto_open_yields_to_an_open_viewer(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    seed_status(&view, installed_notes_payload("0.182.0", &["Previously read"], None), cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.open_changelog(window, cx));
        window.render_frame(cx);
        assert!(view.read(cx).changelog_open);
    })
    .unwrap();
    let payload = with_current_notes(
        status_payload("0.182.0", Some("0.183.0"), true, &["Something newer"], None),
        &["Freshly installed"],
        None,
    );
    drain_status(window, &view, payload, cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(view.read(cx).changelog_open, "the open viewer stays intact");
        assert!(window.try_find("changelog-dialog").is_some());
        assert!(view.read(cx).release_notes_banner_visible(), "the banner stays as the fallback");
    })
    .unwrap();
}

#[gpui_kit::test]
fn changelog_verb_opens_installed_notes(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    // A newer release is offered, but only the installed binary's notes are
    // readable — the verb must ignore the cached offer.
    seed_status(
        &view,
        with_current_notes(
            status_payload("0.182.0", Some("0.183.0"), true, &["Something newer"], None),
            &["Freshly installed"],
            Some("https://github.com/vinhnx/vtcode/releases/tag/v0.182.0"),
        ),
        cx,
    );
    cx.update_window(window, |_, window, cx| {
        let reachable = view.update(cx, |view, cx| view.open_deeplink(DeepLink::Changelog, window, cx));
        window.render_frame(cx);
        assert!(reachable, "installed notes make the verb reachable");
        assert!(view.read(cx).changelog_open);
        assert!(window.try_find("changelog-subtitle").is_none(), "no upgrade arrow for installed notes");
        assert!(window.try_find("changelog-highlights").is_some());
    })
    .unwrap();
}

#[gpui_kit::test]
fn changelog_verb_with_only_installed_notes(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    seed_status(&view, post_update_payload(), cx);
    cx.update_window(window, |_, window, cx| {
        let reachable = view.update(cx, |view, cx| view.open_deeplink(DeepLink::Changelog, window, cx));
        window.render_frame(cx);
        assert!(reachable, "known installed notes make the verb reachable");
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
        installed_notes_payload(
            "0.181.0",
            &["Faster startup"],
            Some("https://github.com/vinhnx/vtcode/releases/tag/0.181.0"),
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

/// Up-to-date launch: the About row offers the installed binary's own notes
/// even though no update is pending. A cached newer release never becomes the
/// reader's subject, with or without an offer.
#[gpui_kit::test]
fn about_row_reads_installed_notes_without_an_offered_update(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    seed_status(
        &view,
        with_current_notes(
            status_payload("0.182.0", Some("0.183.0"), false, &["Something newer"], None),
            &["Freshly installed"],
            Some("https://github.com/vinhnx/vtcode/releases/tag/v0.182.0"),
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
    });
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("changelog-dialog").is_some());
        assert!(window.try_find("changelog-subtitle").is_none(), "installed notes never show an upgrade arrow");
        assert!(window.try_find("changelog-highlights").is_some());
    })
    .unwrap();
}

/// The exact stale-cache shape: the service knows a newer release (with its
/// notes) but the running binary is older and has no installed-release notes.
/// The reader must stay inert — advertising those notes would describe a
/// release the user has not installed yet. The update offer itself is separate
/// and still lives in the Update status row.
#[gpui_kit::test]
fn newer_release_notes_never_become_a_reader_entry_point(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    seed_status(
        &view,
        status_payload(
            "0.183.0",
            Some("0.183.1"),
            true,
            &["Support ultrafast on GPT-6.2 Astra and Sol"],
            Some("https://github.com/vinhnx/vtcode/releases/tag/v0.183.1"),
        ),
        cx,
    );
    open_about_settings(window, &view, cx);
    cx.update_window(window, |_, window, cx| {
        scroll_settings_into_view(window, "about-settings-changelog", cx);
        assert_eq!(window.find("about-settings-changelog").disabled(), Some(true), "uninstalled notes stay unreadable");
        assert!(window.try_find("about-settings-install-update").is_some(), "the offer stays actionable");
    })
    .unwrap();
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.open_changelog(window, cx));
        window.render_frame(cx);
        assert!(!view.read(cx).changelog_open, "the verb refuses too");
        assert!(window.try_find("changelog-dialog").is_none());
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

/// With no newer release cached, the running version's own notes are still a
/// readable changelog.
#[gpui_kit::test]
fn about_row_reads_current_notes_when_no_newer_release_is_cached(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    seed_status(&view, post_update_payload(), cx);
    open_about_settings(window, &view, cx);
    cx.update_window(window, |_, window, cx| {
        scroll_settings_into_view(window, "about-settings-changelog", cx);
        window.click("about-settings-changelog", cx);
        window.render_frame(cx);
    })
    .unwrap();
    cx.update(|cx| assert!(view.read(cx).changelog_open, "running notes are all there is"));
}

/// A cached release whose notes carry no Highlights section has nothing to
/// read: the About row stays inert rather than opening an empty viewer, even
/// when that release is the offered update.
#[gpui_kit::test]
fn about_row_stays_inert_when_installed_notes_have_no_highlights(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    seed_status(
        &view,
        installed_notes_payload("0.182.0", &[], Some("https://github.com/vinhnx/vtcode/releases/tag/0.182.0")),
        cx,
    );
    open_about_settings(window, &view, cx);
    cx.update_window(window, |_, window, cx| {
        scroll_settings_into_view(window, "about-settings-changelog", cx);
        assert_eq!(window.find("about-settings-changelog").disabled(), Some(true), "nothing to read");
        window.click("about-settings-changelog", cx);
        window.render_frame(cx);
        assert!(window.try_find("changelog-dialog").is_none(), "a disabled row opens nothing");
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

/// With only the running version's notes cached, the About row names *that*
/// version rather than inventing a newer "latest" release.
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
        assert_eq!(
            view.installed_changelog_version(),
            Some(view.update_status.as_ref().expect("status").current_version.as_str()),
            "the reader names the running version"
        );
    });
}
