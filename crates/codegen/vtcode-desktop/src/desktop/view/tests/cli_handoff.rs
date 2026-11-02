use super::*;

#[gpui_kit::test]
fn handoff_card_uses_the_settings_resume_command(cx: &mut TestAppContext) {
    let (_window, view) = open_desktop(cx);
    view.update(cx, |view, cx| {
        view.selected_session_id = Some("sess-1".to_string());
        view.open_cli_handoff(cx);
        let handoff = view.cli_handoff.as_ref().expect("card opens with a selection");
        assert_eq!(handoff.session_id, "sess-1");
        // Independent oracle: same literal the settings terminal row pins.
        assert_eq!(handoff.command, "vtcode --resume sess-1 --all");

        // Reopening refreshes when the selection changed.
        view.selected_session_id = Some("sess-2".to_string());
        view.open_cli_handoff(cx);
        let handoff = view.cli_handoff.as_ref().expect("card stays open");
        assert_eq!(handoff.command, "vtcode --resume sess-2 --all");
    });
}

#[gpui_kit::test]
fn handoff_without_selection_reports_an_error(cx: &mut TestAppContext) {
    let (_window, view) = open_desktop(cx);
    view.update(cx, |view, cx| {
        view.selected_session_id = None;
        view.open_cli_handoff(cx);
        assert!(view.cli_handoff.is_none());
        assert_eq!(view.error.as_deref(), Some("Select a thread first to continue it in the CLI."));
    });
}

#[gpui_kit::test]
fn handoff_copy_button_copies_the_command(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.selected_session_id = Some("sess-1".to_string());
            view.open_cli_handoff(cx);
        });
        window.render_frame(cx);
        window.click("cli-handoff-copy", cx);
        assert_eq!(cx.read_from_clipboard().unwrap().text().as_deref(), Some("vtcode --resume sess-1 --all"));
    })
    .unwrap();
}

#[gpui_kit::test]
fn handoff_terminal_button_copies_and_records_a_terminal_launch(cx: &mut TestAppContext) {
    let workspace = tempfile::TempDir::new().unwrap();
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.workspace = Some(workspace.path().to_path_buf());
            view.selected_session_id = Some("sess-1".to_string());
            view.test_launch_intercept = Some(TestLaunchIntercept::default());
            view.open_cli_handoff(cx);
        });
        window.render_frame(cx);
        window.click("cli-handoff-terminal", cx);
        assert_eq!(cx.read_from_clipboard().unwrap().text().as_deref(), Some("vtcode --resume sess-1 --all"));
    })
    .unwrap();
    view.update(cx, |view, _| {
        assert_eq!(
            view.test_launch_intercept.as_ref().unwrap().calls,
            vec![ExternalLaunchCall {
                rel_path: ".".into(),
                editor: ExternalEditor::Terminal
            }]
        );
        assert!(view.error.is_none());
    });
}

#[gpui_kit::test]
fn handoff_dismiss_clears_the_card(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.selected_session_id = Some("sess-1".to_string());
            view.open_cli_handoff(cx);
            assert!(view.cli_handoff.is_some());
        });
        window.render_frame(cx);
        window.click("cli-handoff-dismiss", cx);
    })
    .unwrap();
    view.update(cx, |view, _| {
        assert!(view.cli_handoff.is_none());
    });
}

#[gpui_kit::test]
fn thread_header_button_opens_the_handoff_card(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.selected_session_id = Some("sess-1".to_string());
        });
        window.render_frame(cx);
        assert_ne!(window.find("thread-header-cli").disabled(), Some(true));
        window.click("thread-header-cli", cx);
    })
    .unwrap();
    view.update(cx, |view, _| {
        let handoff = view.cli_handoff.as_ref().expect("header button opens the card");
        assert_eq!(handoff.command, "vtcode --resume sess-1 --all");
    });
}

#[gpui_kit::test]
fn thread_header_button_is_disabled_without_a_selection(cx: &mut TestAppContext) {
    use crate::desktop::transcript::TranscriptRole;

    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            // Transcript content without a selection still renders the header.
            view.selected_session_id = None;
            view.transcript.push(TranscriptItem::new(
                "echo-1".into(),
                TranscriptRole::User.as_str().into(),
                "You".into(),
                "hello".into(),
            ));
        });
        window.render_frame(cx);
        assert_eq!(window.find("thread-header-cli").disabled(), Some(true));
    })
    .unwrap();
}

#[gpui_kit::test]
fn cli_palette_entry_opens_the_handoff_card(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            assert!(!view.palette_action_available(&super::super::PaletteAction::OpenInCli));
            view.selected_session_id = Some("sess-1".to_string());
            assert!(view.palette_action_available(&super::super::PaletteAction::OpenInCli));
            view.run_palette_action(super::super::PaletteAction::OpenInCli, window, cx);
            let handoff = view.cli_handoff.as_ref().expect("palette entry opens the card");
            assert_eq!(handoff.command, "vtcode --resume sess-1 --all");
        });
    })
    .unwrap();
}

#[gpui_kit::test]
fn cli_slash_action_opens_the_handoff_card(cx: &mut TestAppContext) {
    use super::super::composer_controls::{DesktopSlashAction, desktop_slash_action};

    assert_eq!(desktop_slash_action("/cli"), Some(DesktopSlashAction::Cli));
    assert_eq!(desktop_slash_action("/CLI extra"), Some(DesktopSlashAction::Cli));
    assert_eq!(desktop_slash_action("/desktop"), Some(DesktopSlashAction::Desktop));
    assert_eq!(desktop_slash_action("/DESKTOP extra"), Some(DesktopSlashAction::Desktop));

    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.selected_session_id = Some("sess-1".to_string());
            assert!(view.handle_desktop_slash_action(DesktopSlashAction::Cli, window, cx));
            let handoff = view.cli_handoff.as_ref().expect("slash opens the card");
            assert_eq!(handoff.command, "vtcode --resume sess-1 --all");
        });
    })
    .unwrap();
}

#[gpui_kit::test]
fn desktop_slash_action_reports_already_here(cx: &mut TestAppContext) {
    use super::super::composer_controls::DesktopSlashAction;

    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            assert!(view.handle_desktop_slash_action(DesktopSlashAction::Desktop, window, cx));
            assert_eq!(view.status, "You are already using VT Code Desktop.");
        });
    })
    .unwrap();
}

#[gpui_kit::test]
fn slash_catalog_gains_cli_and_loses_desktop(cx: &mut TestAppContext) {
    let (_window, view) = open_desktop(cx);
    view.update(cx, |view, _| {
        view.slash_commands = vec![
            vtcode_desktop_protocol::SlashCommandInfo {
                name: "desktop".to_string(),
                description: "Open the VT Code desktop app".to_string(),
            },
            vtcode_desktop_protocol::SlashCommandInfo {
                name: "model".to_string(),
                description: "Pick a model".to_string(),
            },
        ];
        view.localize_slash_catalog();
        let names: Vec<&str> = view.slash_commands.iter().map(|entry| entry.name.as_str()).collect();
        assert_eq!(names, vec!["model", "cli"]);
        let cli = view.slash_commands.iter().find(|entry| entry.name == "cli").expect("cli entry");
        assert_eq!(cli.description, "Continue the selected thread in the CLI");

        view.localize_slash_catalog();
        assert_eq!(view.slash_commands.iter().filter(|entry| entry.name == "cli").count(), 1);
    });
}
