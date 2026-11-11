use super::*;

#[gpui_kit::test]
fn handoff_card_uses_the_settings_fork_command(cx: &mut TestAppContext) {
    let (_window, view) = open_desktop(cx);
    view.update(cx, |view, cx| {
        view.selected_session_id = Some("sess-1".to_string());
        view.open_cli_handoff(cx);
        let handoff = view.cli_handoff.as_ref().expect("card opens with a selection");
        assert_eq!(handoff.session_id, "sess-1");
        // Independent oracle: same literal the settings terminal row pins.
        assert_eq!(handoff.command, "vtcode --fork-session sess-1");

        // Reopening refreshes when the selection changed.
        view.selected_session_id = Some("sess-2".to_string());
        view.open_cli_handoff(cx);
        let handoff = view.cli_handoff.as_ref().expect("card stays open");
        assert_eq!(handoff.command, "vtcode --fork-session sess-2");
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
        assert_eq!(cx.read_from_clipboard().unwrap().text().as_deref(), Some("vtcode --fork-session sess-1"));
    })
    .unwrap();
}

#[gpui_kit::test]
fn handoff_terminal_button_copies_and_runs_the_handoff_script(cx: &mut TestAppContext) {
    let workspace = tempfile::TempDir::new().unwrap();
    // The fixture binary carries a space so the recorded script also proves
    // quoting flows through the view layer; it is never executed in tests.
    let binary = workspace.path().join("VT Code.app/Contents/Helpers/vtcode");
    let _binary_guard = crate::desktop::transport::set_test_handoff_binary(Ok(binary.clone()));
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.workspace = Some(workspace.path().to_path_buf());
            view.selected_session_id = Some("sess-1".to_string());
            view.test_launch_intercept = Some(TestLaunchIntercept::default());
            view.open_cli_handoff(cx);
        });
        window.render_frame(cx);
        assert_eq!(
            window.find("cli-handoff-hint").label(),
            Some("Continues this thread in Terminal as an independent copy; the command is also copied for pasting elsewhere.")
        );
        window.click("cli-handoff-terminal", cx);
        assert_eq!(cx.read_from_clipboard().unwrap().text().as_deref(), Some("vtcode --fork-session sess-1"));
    })
    .unwrap();
    view.update(cx, |view, _| {
        let intercept = view.test_launch_intercept.as_ref().unwrap();
        // Independent oracle: hardcoded template, only the two paths interpolated.
        let expected = format!(
            "#!/bin/sh\ncd '{}' && exec '{}' --fork-session 'sess-1'\necho \"vtcode handoff failed with exit $?\"\nexit 1\n",
            workspace.path().display(),
            binary.display()
        );
        assert_eq!(
            intercept.handoff_scripts,
            vec![HandoffScriptCall { content: expected, editor: ExternalEditor::Terminal }]
        );
        assert!(intercept.calls.is_empty(), "the script path must not also open a directory");
        assert!(view.error.is_none());
    });
}

#[gpui_kit::test]
fn handoff_terminal_falls_back_to_open_directory_without_a_binary(cx: &mut TestAppContext) {
    let workspace = tempfile::TempDir::new().unwrap();
    let _binary_guard = crate::desktop::transport::set_test_handoff_binary(Err("no binary".to_string()));
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
        assert_eq!(cx.read_from_clipboard().unwrap().text().as_deref(), Some("vtcode --fork-session sess-1"));
    })
    .unwrap();
    view.update(cx, |view, _| {
        let intercept = view.test_launch_intercept.as_ref().unwrap();
        assert!(intercept.handoff_scripts.is_empty(), "no binary means no script");
        assert_eq!(
            intercept.calls,
            vec![ExternalLaunchCall {
                rel_path: ".".into(),
                editor: ExternalEditor::Terminal
            }]
        );
        assert!(view.error.is_none());
    });
}

#[gpui_kit::test]
fn handoff_terminal_uses_the_chosen_terminal_and_paste_hint(cx: &mut TestAppContext) {
    let workspace = tempfile::TempDir::new().unwrap();
    let binary = workspace.path().join("vtcode");
    // A resolvable binary proves the fallback below is terminal-driven
    // (Ghostty cannot run scripts), not binary-driven.
    let _binary_guard = crate::desktop::transport::set_test_handoff_binary(Ok(binary));
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.workspace = Some(workspace.path().to_path_buf());
            view.selected_session_id = Some("sess-1".to_string());
            view.preferences.set_cli_terminal(CliTerminal::Ghostty);
            view.test_launch_intercept = Some(TestLaunchIntercept::default());
            view.open_cli_handoff(cx);
        });
        window.render_frame(cx);
        assert_eq!(
            window.find("cli-handoff-hint").label(),
            Some("Paste in any terminal to continue this thread as an independent copy. Opening a terminal copies it automatically.")
        );
        window.click("cli-handoff-terminal", cx);
    })
    .unwrap();
    view.update(cx, |view, _| {
        let intercept = view.test_launch_intercept.as_ref().unwrap();
        assert!(intercept.handoff_scripts.is_empty(), "Ghostty cannot run scripts");
        assert_eq!(
            intercept.calls,
            vec![ExternalLaunchCall {
                rel_path: ".".into(),
                editor: ExternalEditor::Ghostty
            }]
        );
        assert!(view.error.is_none());
    });
}

#[gpui_kit::test]
fn cli_terminal_dropdown_renders_persists_and_routes_selection(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.open_settings(window, cx));
        window.render_frame(cx);
        select_settings_category(window, 4, cx);
        window.render_frame(cx);
        assert_eq!(window.find_all("cli-terminal-select").len(), 1);
        assert_eq!(window.find("cli-terminal-select").value(), Some("Terminal"));
        assert_eq!(view.read(cx).preferences.cli_terminal(), CliTerminal::Terminal);
    })
    .unwrap();

    // Drive the production subscription path: a dropdown confirmation reaches the setter.
    cx.update_window(window, |_, _, cx| {
        let select = view.read(cx).cli_terminal_select.clone();
        select.update(cx, |_, cx| cx.emit(SelectEvent::Confirm(Some(CliTerminal::Ghostty))));
    })
    .unwrap();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(view.read(cx).preferences.cli_terminal(), CliTerminal::Ghostty);
    })
    .unwrap();
    assert_eq!(
        crate::desktop::preferences::Preferences::load().cli_terminal(),
        CliTerminal::Ghostty,
        "the choice persists for the next launch"
    );
}

#[gpui_kit::test]
fn cli_terminal_dropdown_offers_detected_and_retains_saved(cx: &mut TestAppContext) {
    let apps = tempfile::TempDir::new().unwrap();
    crate::desktop::files::write_test_bundle(apps.path(), "iTerm.app", "com.googlecode.iterm2");
    let pref_dir = tempfile::TempDir::new().unwrap();
    let prefs_path = pref_dir.path().join("prefs.json");
    std::fs::write(&prefs_path, r#"{"cliTerminal":"ghostty"}"#).unwrap();
    let _roots = crate::desktop::files::set_test_app_roots(vec![apps.path().to_path_buf()]);

    let (window, view) = open_desktop_with_preferences_path(cx, prefs_path);
    cx.update_window(window, |_, window, cx| {
        let select = view.read(cx).cli_terminal_select.clone();
        // The saved choice stays selected even though its app is missing.
        assert_eq!(select.read(cx).selected_value().copied(), Some(CliTerminal::Ghostty));
        // Probe the offered set through selection: Terminal always,
        // iTerm2 detected, Ghostty saved.
        for (terminal, offered) in [
            (CliTerminal::Terminal, true),
            (CliTerminal::ITerm2, true),
            (CliTerminal::Ghostty, true),
        ] {
            select.update(cx, |state, cx| state.set_selected_value(&terminal, window, cx));
            assert_eq!(
                select.read(cx).selected_value().copied().as_ref(),
                offered.then_some(&terminal),
                "{terminal:?} offered={offered}"
            );
        }
    })
    .unwrap();
    drop(_roots);

    // Asymmetric pair: no detections and a default preference offer Terminal alone.
    let _empty_roots = crate::desktop::files::set_test_app_roots(vec![]);
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        let select = view.read(cx).cli_terminal_select.clone();
        assert_eq!(select.read(cx).selected_value().copied(), Some(CliTerminal::Terminal));
        for (terminal, offered) in [
            (CliTerminal::Terminal, true),
            (CliTerminal::ITerm2, false),
            (CliTerminal::Ghostty, false),
        ] {
            select.update(cx, |state, cx| state.set_selected_value(&terminal, window, cx));
            assert_eq!(
                select.read(cx).selected_value().copied().as_ref(),
                offered.then_some(&terminal),
                "{terminal:?} offered={offered}"
            );
        }
    })
    .unwrap();
}

#[gpui_kit::test]
fn handoff_hint_discloses_staleness_while_a_turn_runs(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.selected_session_id = Some("sess-1".to_string());
            view.running = true;
            view.open_cli_handoff(cx);
        });
        window.render_frame(cx);
        assert_eq!(
            window.find("cli-handoff-hint").label(),
            Some("Continues this thread in Terminal as an independent copy; the command is also copied for pasting elsewhere. A turn is running; the copy includes only completed turns.")
        );
    })
    .unwrap();
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
        assert_eq!(handoff.command, "vtcode --fork-session sess-1");
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
            assert!(!view.palette_action_available(&super::super::PaletteAction::OpenInCli, cx));
            view.selected_session_id = Some("sess-1".to_string());
            assert!(view.palette_action_available(&super::super::PaletteAction::OpenInCli, cx));
            view.run_palette_action(super::super::PaletteAction::OpenInCli, window, cx);
            let handoff = view.cli_handoff.as_ref().expect("palette entry opens the card");
            assert_eq!(handoff.command, "vtcode --fork-session sess-1");
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
            assert_eq!(handoff.command, "vtcode --fork-session sess-1");
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
