use super::super::menu_bar::{MenuBarAction, dispatch_status_action};
use super::*;

fn init_without_window(cx: &mut TestAppContext) {
    crate::desktop::preferences::set_test_preferences_path(isolate_preferences());
    crate::desktop::localization::initialize(crate::desktop::localization::UiLanguage::English);
    cx.update(gpui_kit::init);
    cx.update(crate::desktop::notifications::initialize);
    let preferences = crate::desktop::preferences::Preferences::load();
    cx.update(|cx| super::super::init_keybindings(cx, &preferences));
    cx.update(super::super::init_native_menus);
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
    // A running turn blocks session and workspace changes only.
    view.update(cx, |view, _| view.running = true);
    render(cx);
    cx.update(|cx| {
        assert!(!NewSession.is_allowed(cx));
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
