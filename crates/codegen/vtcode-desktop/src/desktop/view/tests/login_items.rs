use super::super::login_items;
use super::super::settings::SettingsTab;
use super::*;
use std::cell::Cell;
use std::rc::Rc;

#[gpui_kit::test]
fn launch_at_login_toggle_applies_and_persists_intent(cx: &mut TestAppContext) {
    let _prefs_path = crate::desktop::preferences::install_test_preferences_path();
    cx.update(|cx| cx.set_reduce_motion(true));
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.settings_tab = SettingsTab::VtCode;
            view.open_settings(window, cx);
        });
        window.render_frame(cx);
        scroll_settings_by(window, px(-3000.), cx);
        window.render_frame(cx);
        assert_eq!(window.find("launch-at-login").checked(), Some(false));
        assert!(view.read(cx).preferences.launch_at_login.is_none());
        // On: the stub applies and the intent persists.
        window.click("launch-at-login", cx);
        window.render_frame(cx);
        assert_eq!(window.find("launch-at-login").checked(), Some(true));
        assert_eq!(view.read(cx).preferences.launch_at_login, Some(true));
        assert_eq!(login_items::status(cx), LoginItemStatus::Enabled);
        assert_eq!(view.read(cx).login_item_status, Some(LoginItemStatus::Enabled));
        // Off: the reverse.
        window.click("launch-at-login", cx);
        window.render_frame(cx);
        assert_eq!(window.find("launch-at-login").checked(), Some(false));
        assert_eq!(view.read(cx).preferences.launch_at_login, Some(false));
        assert_eq!(login_items::status(cx), LoginItemStatus::Disabled);
        assert_eq!(view.read(cx).login_item_status, Some(LoginItemStatus::Disabled));
    })
    .unwrap();
    // The file round-trips the new key.
    assert_eq!(crate::desktop::preferences::Preferences::load().launch_at_login, Some(false));
}

#[gpui_kit::test]
fn reconcile_adopts_os_truth_but_keeps_approval_pending_intent(cx: &mut TestAppContext) {
    let _prefs_path = crate::desktop::preferences::install_test_preferences_path();
    let (_window, view) = open_desktop(cx);
    // OS enabled, no intent: adopts on.
    cx.update(|cx| login_items::test_override(Some(LoginItemStatus::Enabled), cx));
    view.update(cx, |view, cx| {
        assert!(view.preferences.launch_at_login.is_none());
        view.start_login_item_reconciliation(cx);
    });
    cx.run_until_parked();
    cx.update(|cx| assert_eq!(view.read(cx).preferences.launch_at_login, Some(true)));
    // OS disabled, stale on: adopts off.
    cx.update(|cx| login_items::test_override(Some(LoginItemStatus::Disabled), cx));
    view.update(cx, |view, cx| {
        view.refresh_login_item_status(cx);
        assert_eq!(view.preferences.launch_at_login, Some(false));
    });
    // Approval pending: keeps the on intent for the settings hint.
    cx.update(|cx| login_items::test_override(Some(LoginItemStatus::RequiresApproval), cx));
    view.update(cx, |view, cx| {
        view.preferences.launch_at_login = Some(true);
        view.refresh_login_item_status(cx);
        assert_eq!(view.preferences.launch_at_login, Some(true));
    });
}

#[gpui_kit::test]
fn late_launch_login_check_preserves_newer_settings_status(cx: &mut TestAppContext) {
    let (_window, view) = open_desktop(cx);
    view.update(cx, |view, cx| {
        login_items::test_override(Some(LoginItemStatus::Enabled), cx);
        view.start_login_item_reconciliation(cx);
        login_items::test_override(Some(LoginItemStatus::Disabled), cx);
        view.refresh_login_item_status(cx);
    });
    cx.run_until_parked();
    cx.update(|cx| {
        assert_eq!(view.read(cx).login_item_status, Some(LoginItemStatus::Disabled));
        assert!(view.read(cx).preferences.launch_at_login.is_none());
    });
}

#[gpui_kit::test]
fn approval_pending_shows_the_settings_shortcut(cx: &mut TestAppContext) {
    let _prefs_path = crate::desktop::preferences::install_test_preferences_path();
    cx.update(|cx| {
        cx.set_reduce_motion(true);
        login_items::test_override(Some(LoginItemStatus::RequiresApproval), cx);
    });
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.settings_tab = SettingsTab::VtCode;
            view.open_settings(window, cx);
        });
        window.render_frame(cx);
        scroll_settings_by(window, px(-3000.), cx);
        window.render_frame(cx);
        assert!(window.try_find("open-login-items").is_some(), "approval shows the shortcut");
        window.click("open-login-items", cx);
        window.render_frame(cx);
        assert!(login_items::test_settings_opened(cx), "shortcut opens Login Items");
    })
    .unwrap();
    // A decided status hides the shortcut again after a refresh.
    cx.update(|cx| login_items::test_override(Some(LoginItemStatus::Enabled), cx));
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.refresh_login_item_status(cx));
        window.render_frame(cx);
        assert!(window.try_find("open-login-items").is_none());
    })
    .unwrap();
}

#[gpui_kit::test]
fn vtcode_page_renders_cached_login_status_not_live_status(cx: &mut TestAppContext) {
    let _prefs_path = crate::desktop::preferences::install_test_preferences_path();
    cx.update(|cx| cx.set_reduce_motion(true));
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.settings_tab = SettingsTab::VtCode;
            view.open_settings(window, cx);
        });
        window.render_frame(cx);
        scroll_settings_by(window, px(-3000.), cx);
        window.render_frame(cx);
    })
    .unwrap();
    // The live stub flips behind the cache's back: render must not follow
    // it (pre-fix, render consulted `status(cx)` every frame, so the
    // shortcut appeared here — and every scroll frame paid for XPC).
    cx.update(|cx| login_items::test_override(Some(LoginItemStatus::RequiresApproval), cx));
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(
            window.try_find("open-login-items").is_none(),
            "render follows the cached status, not the live OS status"
        );
        view.update(cx, |view, cx| view.refresh_login_item_status(cx));
        window.render_frame(cx);
        assert!(window.try_find("open-login-items").is_some(), "refresh picks up the new OS status");
    })
    .unwrap();
}

#[gpui_kit::test]
fn palette_jump_to_vtcode_refreshes_login_status(cx: &mut TestAppContext) {
    let _prefs_path = crate::desktop::preferences::install_test_preferences_path();
    cx.update(|cx| cx.set_reduce_motion(true));
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.settings_tab = SettingsTab::Appearance;
            view.open_settings(window, cx);
        });
        window.render_frame(cx);
    })
    .unwrap();
    // The OS status flips while settings sits open on another tab; jumping
    // to VT Code through the palette must pick it up without a manual
    // refresh (open_settings early-returns on this path).
    cx.update(|cx| login_items::test_override(Some(LoginItemStatus::RequiresApproval), cx));
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.run_palette_action(super::super::PaletteAction::SelectSettingsTab(SettingsTab::VtCode), window, cx);
        });
        window.render_frame(cx);
        scroll_settings_by(window, px(-3000.), cx);
        window.render_frame(cx);
        assert!(window.try_find("open-login-items").is_some(), "palette entry refreshes the cached login status");
    })
    .unwrap();
}

#[gpui_kit::test]
fn settings_open_refreshes_login_status(cx: &mut TestAppContext) {
    let _prefs_path = crate::desktop::preferences::install_test_preferences_path();
    cx.update(|cx| cx.set_reduce_motion(true));
    let (window, view) = open_desktop(cx);
    // The OS status flips after construction but before settings opens;
    // opening must pick it up (construction alone cannot cover this).
    cx.update(|cx| login_items::test_override(Some(LoginItemStatus::RequiresApproval), cx));
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.settings_tab = SettingsTab::VtCode;
            view.open_settings(window, cx);
        });
        window.render_frame(cx);
        scroll_settings_by(window, px(-3000.), cx);
        window.render_frame(cx);
        assert!(window.try_find("open-login-items").is_some(), "open refreshes the cached login status");
    })
    .unwrap();
}

#[gpui_kit::test]
fn sidebar_select_to_vtcode_refreshes_login_status(cx: &mut TestAppContext) {
    let _prefs_path = crate::desktop::preferences::install_test_preferences_path();
    // Narrow viewport for the legacy navigation with clickable tab buttons.
    let (window, view) = open_desktop_sized(cx, 600., 700.);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.settings_tab = SettingsTab::Appearance;
            view.open_settings(window, cx);
        });
        window.render_frame(cx);
    })
    .unwrap();
    cx.update(|cx| login_items::test_override(Some(LoginItemStatus::RequiresApproval), cx));
    cx.update_window(window, |_, window, cx| {
        window.click("settings-nav-vtcode", cx);
        window.render_frame(cx);
        scroll_settings_by(window, px(-3000.), cx);
        window.render_frame(cx);
        assert!(window.try_find("open-login-items").is_some(), "sidebar select refreshes the cached login status");
    })
    .unwrap();
}

#[gpui_kit::test]
fn slash_settings_to_vtcode_refreshes_login_status(cx: &mut TestAppContext) {
    let _prefs_path = crate::desktop::preferences::install_test_preferences_path();
    cx.update(|cx| cx.set_reduce_motion(true));
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.settings_tab = SettingsTab::Appearance;
            view.open_settings(window, cx);
        });
        window.render_frame(cx);
    })
    .unwrap();
    cx.update(|cx| login_items::test_override(Some(LoginItemStatus::RequiresApproval), cx));
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            let action = super::super::composer_controls::desktop_slash_action("/settings vtcode")
                .expect("native /settings route");
            assert!(view.handle_desktop_slash_action(action, window, cx));
        });
        window.render_frame(cx);
        scroll_settings_by(window, px(-3000.), cx);
        window.render_frame(cx);
        assert!(window.try_find("open-login-items").is_some(), "slash entry refreshes the cached login status");
    })
    .unwrap();
}

#[test]
fn login_settings_stay_searchable_by_startup_terms() {
    let keywords = SettingsTab::VtCode.search_keywords();
    for term in ["startup", "login", "launch at login", "khởi động"] {
        assert!(keywords.contains(&term), "missing settings keyword: {term}");
    }
}

#[gpui_kit::test]
fn background_reconciliation_notifies_only_on_change(cx: &mut TestAppContext) {
    let (_window, view) = open_desktop(cx);
    cx.update(|cx| login_items::test_override(Some(LoginItemStatus::RequiresApproval), cx));
    // Settle launch work, then flush its notify effects before arming the
    // probe: the bootstrap reconciliation must not count towards this test.
    cx.run_until_parked();
    cx.update(|_| {});
    let notified = Rc::new(Cell::new(0_usize));
    let probe = notified.clone();
    let _subscription = view.update(cx, |_, cx| {
        let handle = cx.entity();
        cx.observe(&handle, move |_, _, _| probe.set(probe.get() + 1))
    });
    // Change path: an empty cache adopts the OS status and notifies, so
    // an open VT Code page re-renders without a manual refresh.
    view.update(cx, |view, cx| {
        view.login_item_status = None;
        view.start_login_item_reconciliation(cx);
    });
    cx.run_until_parked();
    cx.update(|_| {});
    cx.update(|cx| {
        assert_eq!(view.read(cx).login_item_status, Some(LoginItemStatus::RequiresApproval));
    });
    // At-least-once: fire counts multiply once the view has rendered (live
    // window invalidators), so only the delta is meaningful — and without
    // the completion notify the count stays 0.
    assert!(notified.get() >= 1, "adopting the OS status must notify");
    let after_change = notified.get();
    // No-change path: a populated cache is left alone, silently.
    view.update(cx, |view, cx| {
        view.login_item_status = Some(LoginItemStatus::Enabled);
        view.start_login_item_reconciliation(cx);
    });
    cx.run_until_parked();
    cx.update(|_| {});
    assert_eq!(notified.get(), after_change, "a no-op completion must not notify");
}

#[gpui_kit::test]
fn settings_slash_action_notifies_when_settings_already_open(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    // Settings already open on another tab: open_settings early-returns,
    // so the arm itself must notify for the tab switch and refresh.
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.settings_tab = SettingsTab::Appearance;
            view.open_settings(window, cx);
        });
        window.render_frame(cx);
    })
    .unwrap();
    cx.update(|cx| login_items::test_override(Some(LoginItemStatus::RequiresApproval), cx));
    cx.run_until_parked();
    cx.update(|_| {});
    let notified = Rc::new(Cell::new(0_usize));
    let probe = notified.clone();
    let _subscription = view.update(cx, |_, cx| {
        let handle = cx.entity();
        cx.observe(&handle, move |_, _, _| probe.set(probe.get() + 1))
    });
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            // Commands kind + closed palette: the composer-clear cascade
            // cannot incidentally notify for the arm.
            view.palette_kind = super::super::PaletteKind::Commands;
            view.palette = None;
            view.login_item_status = None;
            let action = super::super::composer_controls::desktop_slash_action("/settings vtcode")
                .expect("native /settings route");
            assert!(view.handle_desktop_slash_action(action, window, cx));
        });
    })
    .unwrap();
    cx.update(|_| {});
    cx.update(|cx| {
        assert_eq!(view.read(cx).settings_tab, SettingsTab::VtCode);
        assert_eq!(view.read(cx).login_item_status, Some(LoginItemStatus::RequiresApproval));
    });
    // At-least-once: fire counts multiply once the view has rendered (live
    // window invalidators), so only the delta is meaningful — and without
    // the arm notify the count stays 0.
    assert!(notified.get() >= 1, "the arm must notify for its own mutations");
}
