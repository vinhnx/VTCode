use super::*;
use vtcode_desktop_protocol as protocol;

fn prepared_workspace(cx: &mut gpui_kit::App, view: &Entity<DesktopView>) {
    view.update(cx, |view, _| {
        view.workspace = Some(PathBuf::from("/work/first-project"));
        view.session_list_received = true;
        view.sessions = Vec::new();
        view.configuration = Some(runtime_options_config());
    });
}

#[gpui_kit::test]
fn onboarding_panel_shows_until_permission_choice_is_saved(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        prepared_workspace(cx, &view);
        window.render_frame(cx);
        assert!(window.try_find("onboarding-panel").is_some());
        assert!(window.try_find("onboarding-permissions-button").is_some());
        assert!(window.try_find("onboarding-agent-button").is_some());
        assert!(window.try_find("onboarding-auth").is_some());

        view.update(cx, |view, _| {
            view.save_workspace_permission(protocol::PermissionMode::AutoApproveCommands);
        });
        window.render_frame(cx);
        assert!(window.try_find("onboarding-panel").is_none());
    })
    .unwrap();
}

#[gpui_kit::test]
fn onboarding_panel_stays_hidden_until_session_list_loads(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.workspace = Some(PathBuf::from("/work/first-project"));
            view.session_list_received = false;
            view.sessions = Vec::new();
        });
        window.render_frame(cx);
        assert!(window.try_find("onboarding-panel").is_none());
    })
    .unwrap();
}

#[gpui_kit::test]
fn onboarding_auth_row_falls_back_to_setup_command(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        prepared_workspace(cx, &view);
        view.update(cx, |view, _| {
            let mut configuration = runtime_options_config();
            configuration.authenticated = false;
            configuration.credential_setup_command = "vtcode login openai".to_string();
            view.configuration = Some(configuration);
        });
        window.render_frame(cx);
        assert!(window.try_find("onboarding-auth").is_some());

        view.update(cx, |view, _| {
            view.configuration = None;
        });
        window.render_frame(cx);
        assert!(window.try_find("onboarding-auth").is_none());
        assert!(window.try_find("onboarding-panel").is_some());
    })
    .unwrap();
}
