use super::super::login_items;
use super::super::settings::SettingsTab;
use super::*;

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
        window.scroll("settings-content", gpui_kit::ScrollDelta::Pixels(point(px(0.), px(-3000.))), cx);
        window.render_frame(cx);
        assert_eq!(window.find("launch-at-login").checked(), Some(false));
        assert!(view.read(cx).preferences.launch_at_login.is_none());
        // On: the stub applies and the intent persists.
        window.click("launch-at-login", cx);
        window.render_frame(cx);
        assert_eq!(window.find("launch-at-login").checked(), Some(true));
        assert_eq!(view.read(cx).preferences.launch_at_login, Some(true));
        assert_eq!(login_items::status(cx), LoginItemStatus::Enabled);
        // Off: the reverse.
        window.click("launch-at-login", cx);
        window.render_frame(cx);
        assert_eq!(window.find("launch-at-login").checked(), Some(false));
        assert_eq!(view.read(cx).preferences.launch_at_login, Some(false));
        assert_eq!(login_items::status(cx), LoginItemStatus::Disabled);
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
        login_items::reconcile_launch_preference(&mut view.preferences, cx);
        assert_eq!(view.preferences.launch_at_login, Some(true));
    });
    // OS disabled, stale on: adopts off.
    cx.update(|cx| login_items::test_override(Some(LoginItemStatus::Disabled), cx));
    view.update(cx, |view, cx| {
        login_items::reconcile_launch_preference(&mut view.preferences, cx);
        assert_eq!(view.preferences.launch_at_login, Some(false));
    });
    // Approval pending: keeps the on intent for the settings hint.
    cx.update(|cx| login_items::test_override(Some(LoginItemStatus::RequiresApproval), cx));
    view.update(cx, |view, cx| {
        view.preferences.launch_at_login = Some(true);
        login_items::reconcile_launch_preference(&mut view.preferences, cx);
        assert_eq!(view.preferences.launch_at_login, Some(true));
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
        window.scroll("settings-content", gpui_kit::ScrollDelta::Pixels(point(px(0.), px(-3000.))), cx);
        window.render_frame(cx);
        assert!(window.try_find("open-login-items").is_some(), "approval shows the shortcut");
        window.click("open-login-items", cx);
        window.render_frame(cx);
        assert!(login_items::test_settings_opened(cx), "shortcut opens Login Items");
    })
    .unwrap();
    // A decided status hides the shortcut again.
    cx.update(|cx| login_items::test_override(Some(LoginItemStatus::Enabled), cx));
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("open-login-items").is_none());
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
