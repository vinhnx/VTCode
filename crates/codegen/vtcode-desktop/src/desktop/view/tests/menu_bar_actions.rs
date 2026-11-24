use super::super::menu_bar::{MenuBarAction, dispatch_status_action};
use super::*;

fn init_without_window(cx: &mut TestAppContext) {
    crate::desktop::preferences::set_test_preferences_path(isolate_preferences());
    crate::desktop::localization::initialize(crate::desktop::localization::UiLanguage::English);
    cx.update(gpui_kit::init);
    cx.update(crate::desktop::notifications::initialize);
    let preferences = crate::desktop::preferences::Preferences::load();
    cx.update(|cx| super::super::init_keybindings(cx, &preferences));
    cx.update(super::super::menu_bar::init_native_menus);
}

/// Native menu items as the platform retained them at the last `set_menus`.
/// Accept the English name as well so a test can detect a stale, untranslated
/// menu after a locale switch.
fn view_menu_items(cx: &TestAppContext) -> Vec<gpui_kit::OwnedMenuItem> {
    cx.update(|cx| {
        let menus = cx.get_menus().expect("test platform retains native menus");
        let localized = rust_i18n::t!("app.menus.view_menu").to_string();
        let english = rust_i18n::t!("app.menus.view_menu", locale = "en").to_string();
        let view = menus
            .iter()
            .find(|menu| menu.name == localized || menu.name == english)
            .expect("View menu");
        view.items.clone()
    })
}

fn full_screen_item(cx: &TestAppContext) -> String {
    let labels = [
        rust_i18n::t!("app.menus.enter_full_screen").to_string(),
        rust_i18n::t!("app.menus.exit_full_screen").to_string(),
        rust_i18n::t!("app.menus.enter_full_screen", locale = "en").to_string(),
        rust_i18n::t!("app.menus.exit_full_screen", locale = "en").to_string(),
    ];
    view_menu_items(cx)
        .iter()
        .find_map(|item| match item {
            gpui_kit::OwnedMenuItem::Action { name, .. } if labels.iter().any(|label| label == name) => {
                Some(name.clone())
            }
            _ => None,
        })
        .expect("full screen menu item")
}

/// `set_menus` snapshots the keymap into each native item's accelerator, so a
/// shortcut save has to rebuild the menus or they keep showing the old chord.
/// The View item's Enter/Exit label is an observable marker that the rebuild
/// ran with the caller's current full-screen state.
#[gpui_kit::test]
fn saving_shortcuts_rebuilds_the_native_menus(cx: &mut TestAppContext) {
    init_without_window(cx);
    assert_eq!(full_screen_item(cx), rust_i18n::t!("app.menus.enter_full_screen").to_string());

    cx.update(|cx| {
        super::super::bootstrap::reinstall_shortcuts(
            cx,
            &std::collections::BTreeMap::new(),
            &std::collections::BTreeMap::new(),
            true,
        );
    });
    assert_eq!(full_screen_item(cx), rust_i18n::t!("app.menus.exit_full_screen").to_string());
}

/// Locale changes must also go through the one menu rebuild entry point:
/// `set_menus` captures label text at build time, so a language switch would
/// otherwise leave the native menu bar in the previous language.
#[gpui_kit::test]
fn switching_language_rebuilds_the_native_menus(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    assert_eq!(full_screen_item(cx), rust_i18n::t!("app.menus.enter_full_screen").to_string());

    let view_for_language = view.clone();
    cx.update_window(window, |_, window, cx| {
        view_for_language.update(cx, |view, cx| {
            view.set_ui_language(crate::desktop::localization::UiLanguage::Vietnamese, window, cx);
        });
    })
    .unwrap();
    let vietnamese = rust_i18n::t!("app.menus.enter_full_screen", locale = "vi").to_string();
    assert_ne!(vietnamese, "Enter Full Screen", "precondition: the locale has a translation");
    assert_eq!(full_screen_item(cx), vietnamese);

    let view_for_language = view.clone();
    cx.update_window(window, |_, window, cx| {
        view_for_language.update(cx, |view, cx| {
            view.set_ui_language(crate::desktop::localization::UiLanguage::English, window, cx);
        });
    })
    .unwrap();
}

#[gpui_kit::test]
fn status_menu_show_opens_the_main_window(cx: &mut TestAppContext) {
    init_without_window(cx);
    cx.update(|cx| assert!(cx.windows().is_empty()));
    cx.update(|cx| dispatch_status_action(MenuBarAction::Show, cx));
    cx.update(|cx| assert_eq!(cx.windows().len(), 1));
    cx.update(|cx| dispatch_status_action(MenuBarAction::Show, cx));
    cx.update(|cx| assert_eq!(cx.windows().len(), 1, "showing again must reuse the window"));
}

#[gpui_kit::test]
fn status_menu_opens_settings(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| window.render_frame(cx)).unwrap();
    cx.update(|cx| dispatch_status_action(MenuBarAction::OpenSettings, cx));
    cx.update(|cx| assert_eq!(view.read(cx).current_route(), Route::Settings));
}

#[gpui_kit::test]
fn status_menu_opens_the_about_dialog(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| window.render_frame(cx)).unwrap();
    cx.update(|cx| dispatch_status_action(MenuBarAction::About, cx));
    cx.update(|cx| assert!(view.read(cx).about_open));
}

#[gpui_kit::test]
fn status_menu_new_session_returns_to_the_workspace(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    // The New Session handler only attaches while a session change is
    // allowed, which needs a connected backend like the main menu does.
    let (commands, _requests) = tokio::sync::mpsc::channel(8);
    view.update(cx, |view, _| view.backend = Some(BackendHandle { commands }));
    cx.update_window(window, |_, window, cx| window.render_frame(cx)).unwrap();
    cx.update(|cx| dispatch_status_action(MenuBarAction::OpenSettings, cx));
    cx.update(|cx| assert_eq!(view.read(cx).current_route(), Route::Settings));
    cx.update_window(window, |_, window, cx| window.render_frame(cx)).unwrap();
    cx.update(|cx| dispatch_status_action(MenuBarAction::NewSession, cx));
    cx.update(|cx| assert_ne!(view.read(cx).current_route(), Route::Settings));
}

#[gpui_kit::test]
fn status_menu_open_workspace_prompts_for_a_folder(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| window.render_frame(cx)).unwrap();
    cx.update(|cx| dispatch_status_action(MenuBarAction::OpenWorkspace, cx));
    cx.update(|cx| assert!(view.read(cx).workspace_picker_open));
}

#[gpui_kit::test]
fn status_menu_availability_matches_main_menu_gating(cx: &mut TestAppContext) {
    use MenuBarAction::{About, NewSession, OpenHelp, OpenSettings, OpenWorkspace, Quit, Show};
    init_without_window(cx);
    // Windowless: only window-free actions are enabled, like the main menu.
    cx.update(|cx| {
        for action in [Show, Quit, OpenHelp] {
            assert!(action.is_allowed(cx), "{action:?} works windowless");
        }
        for action in [NewSession, OpenWorkspace, OpenSettings, About] {
            assert!(!action.is_allowed(cx), "{action:?} needs a window");
        }
    });
    let (window, view) = open_desktop(cx);
    // Availability reads the rendered dispatch tree like main-menu validation,
    // so every state change below re-renders before asserting.
    let render = |cx: &mut TestAppContext| {
        cx.update_window(window, |_, window, cx| window.render_frame(cx)).unwrap();
    };
    // Fresh window without a backend: session changes need one, while the
    // workspace picker and the unconditional actions do not.
    render(cx);
    cx.update(|cx| {
        assert!(Show.is_allowed(cx) && Quit.is_allowed(cx) && OpenHelp.is_allowed(cx));
        assert!(OpenSettings.is_allowed(cx) && About.is_allowed(cx));
        assert!(!NewSession.is_allowed(cx));
        assert!(OpenWorkspace.is_allowed(cx));
    });
    // Connected and idle: everything is enabled.
    let (commands, _requests) = tokio::sync::mpsc::channel(8);
    view.update(cx, |view, _| view.backend = Some(BackendHandle { commands }));
    render(cx);
    cx.update(|cx| {
        for action in [Show, Quit, OpenHelp, OpenSettings, About, NewSession, OpenWorkspace] {
            assert!(action.is_allowed(cx), "{action:?} must be enabled when idle");
        }
    });
    // A running turn blocks workspace changes; New Session reroutes through
    // the stop-then-proceed confirm instead of going dark.
    view.update(cx, |view, _| view.running = true);
    render(cx);
    cx.update(|cx| {
        assert!(NewSession.is_allowed(cx));
        assert!(!OpenWorkspace.is_allowed(cx));
        assert!(OpenSettings.is_allowed(cx) && About.is_allowed(cx));
    });
    // A retained submission blocks both even once idle.
    view.update(cx, |view, _| {
        view.running = false;
        view.retained_submission = Some(super::super::SubmissionDraft {
            prompt: "held".into(),
            attachments: Vec::new(),
            echo_id: "echo-1".into(),
            delivery_uncertain: false,
        });
    });
    render(cx);
    cx.update(|cx| {
        assert!(!NewSession.is_allowed(cx));
        assert!(!OpenWorkspace.is_allowed(cx));
    });
    // An open picker blocks only the workspace item.
    view.update(cx, |view, _| {
        view.retained_submission = None;
        view.workspace_picker_open = true;
    });
    render(cx);
    cx.update(|cx| {
        assert!(NewSession.is_allowed(cx));
        assert!(!OpenWorkspace.is_allowed(cx));
    });
}
