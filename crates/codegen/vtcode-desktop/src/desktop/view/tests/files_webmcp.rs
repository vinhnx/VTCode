use super::*;

#[gpui_kit::test]
fn transcript_content_column_is_centered_and_uses_scroller_inset(cx: &mut TestAppContext) {
    let (window, view) = open_desktop_sized(cx, 1600., 900.);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.sidebar_open = false;
            view.inspector_open = false;
            view.transcript.push(TranscriptItem::new(
                "assistant-center".into(),
                "assistant".into(),
                "VT Code".into(),
                "An assistant message.".into(),
            ));
            view.transcript.push(TranscriptItem::new(
                "user-center".into(),
                "user".into(),
                "You".into(),
                "A user message.".into(),
            ));
            view.transcript.push(TranscriptItem::new(
                "tool-center".into(),
                "tool".into(),
                "cargo check".into(),
                "Finished successfully".into(),
            ));
        });
        window.render_frame(cx);

        let viewport_center = window.find("transcript-scroll").bounds().center().x;
        for id in ["assistant-center", "user-center", "tool-center"] {
            let column = window.find(format!("transcript-content-{id}")).bounds();
            assert!(
                (column.center().x - viewport_center).abs() <= px(1.),
                "transcript column {id} should be centered in the conversation viewport: column={column:?}, viewport_center={viewport_center:?}"
            );
        }
        let assistant_column = window.find("transcript-content-assistant-center").bounds();
        let assistant_body = window.find("message-body-assistant-center").bounds();
        let user_body = window.find("message-body-user-center").bounds();
        let user_column = window.find("transcript-content-user-center").bounds();
        assert!(
            (assistant_body.top() - assistant_column.top()).abs() <= px(1.),
            "agent prose should start directly in its row without a speaker header"
        );
        // Later user turns open with a divider; the bubble still starts
        // directly after it with no speaker header.
        let divider = window.find("turn-divider-user-center").bounds();
        assert!(
            user_body.top() - divider.bottom() <= px(16.),
            "the user bubble should not reserve space for a speaker header"
        );
        assert!(user_body.left() > assistant_body.left(), "user bubbles align to the trailing edge");
        assert!(user_body.right() <= user_column.right(), "bubble text remains inside the conversation column");
        assert_eq!(window.find("message-body-user-center").label(), Some("You: A user message."));
        assert!(window.try_find("copy-message-user-center").is_none());
        assert!(window.try_find("copy-message-assistant-center").is_none());
        window.hover("transcript-row-assistant-center", cx);
        window.render_frame(cx);
        assert!(window.try_find("copy-message-user-center").is_none());
        assert!(window.try_find("copy-message-assistant-center").is_some());
        assert!(
            (assistant_body.origin.x - assistant_column.origin.x).abs() <= px(1.),
            "assistant text should begin at the centered transcript column inset: body={assistant_body:?}, column={assistant_column:?}"
        );
    })
    .unwrap();

    cx.simulate_window_resize(window, size(px(640.), px(900.)));
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let row = window.find("transcript-row-assistant-center").bounds();
        let column = window.find("transcript-content-assistant-center").bounds();
        let viewport = window.find("transcript-scroll").bounds();
        assert!(
            (column.origin.x - row.origin.x).abs() <= px(1.)
                && (column.right() - row.right()).abs() <= px(1.),
            "the content column should use the scroller's inset without adding another row inset: row={row:?}, column={column:?}"
        );
        assert!(column.origin.x >= viewport.origin.x && column.right() <= viewport.right());
    })
    .unwrap();
}

#[gpui_kit::test]
fn user_bubble_grows_with_multiline_content_without_clipping(cx: &mut TestAppContext) {
    let (window, view) = open_desktop_sized(cx, 1600., 900.);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.sidebar_open = false;
            view.inspector_open = false;
            view.transcript.push(TranscriptItem::new(
                "user-single".into(),
                "user".into(),
                "You".into(),
                "A user message.".into(),
            ));
            view.transcript.push(TranscriptItem::new(
                "user-paragraphs".into(),
                "user".into(),
                "You".into(),
                "First paragraph with enough text to fill the bubble.\n\nSecond paragraph with enough text to fill the bubble.\n\nThird paragraph with enough text to fill the bubble.".into(),
            ));
            view.transcript.push(TranscriptItem::new(
                "user-list".into(),
                "user".into(),
                "You".into(),
                "1. First item with enough text to read clearly\n2. Second item with enough text to read clearly\n3. Third item with enough text to read clearly".into(),
            ));
            view.transcript.push(TranscriptItem::new(
                "assistant-paragraphs".into(),
                "assistant".into(),
                "VT Code".into(),
                "First paragraph with enough text to fill the bubble.\n\nSecond paragraph with enough text to fill the bubble.\n\nThird paragraph with enough text to fill the bubble.".into(),
            ));
        });
        window.render_frame(cx);

        let single = window.find("message-body-user-single").bounds();
        let paragraphs = window.find("message-body-user-paragraphs").bounds();
        let list = window.find("message-body-user-list").bounds();
        let assistant = window.find("message-body-assistant-paragraphs").bounds();
        let column = window.find("transcript-content-user-paragraphs").bounds();

        // Vertical growth: three paragraphs carry two extra body lines plus two
        // 1rem paragraph gaps over a single line (~77px at default metrics);
        // half of that proves the bubble was not clipped to its first line.
        assert!(
            paragraphs.size.height - single.size.height > px(40.),
            "multi-paragraph user bubble must grow vertically: single={single:?} paragraphs={paragraphs:?}"
        );
        // Three list items carry two extra lines (~45px); same half-margin rule.
        assert!(
            list.size.height - single.size.height > px(30.),
            "multi-line list user bubble must grow vertically: single={single:?} list={list:?}"
        );
        // Asymmetric control: the narrower user bubble wraps at least as much
        // as the full-width assistant rendering of identical text, never less.
        assert!(
            paragraphs.size.height >= assistant.size.height - px(1.),
            "user bubble must show at least the assistant's full height: user={paragraphs:?} assistant={assistant:?}"
        );
        // No min-content collapse: a healthy bubble far exceeds the ~80px wide
        // "1." pill from the clipped screenshot.
        assert!(
            paragraphs.size.width > px(120.),
            "multi-paragraph user bubble must not collapse horizontally: {paragraphs:?}"
        );
        assert!(
            list.size.width > px(120.),
            "multi-line list user bubble must not collapse horizontally: {list:?}"
        );
        // Existing invariants preserved: trailing alignment plus containment.
        assert!(
            paragraphs.right() <= column.right() + px(1.),
            "bubble text remains inside the conversation column: body={paragraphs:?} column={column:?}"
        );
        assert!(
            paragraphs.left() > assistant.left(),
            "user bubbles align to the trailing edge: user={paragraphs:?} assistant={assistant:?}"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn file_tasks_discard_obsolete_workspace_and_preview_results(cx: &mut TestAppContext) {
    let first = tempfile::TempDir::new().unwrap();
    let second = tempfile::TempDir::new().unwrap();
    std::fs::write(first.path().join("old.txt"), "old workspace").unwrap();
    std::fs::write(second.path().join("new.txt"), "new workspace").unwrap();
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.workspace = Some(first.path().to_path_buf());
            view.refresh_files(cx);
            view.open_file_preview("old.txt".to_string(), window, cx);
            view.workspace = Some(second.path().to_path_buf());
        });
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            assert!(view.files.is_empty());
            assert!(view.file_preview.is_none());
            assert!(view.error.is_none());
            view.refresh_files(cx);
            view.open_file_preview("missing.txt".to_string(), window, cx);
            view.open_file_preview("new.txt".to_string(), window, cx);
        });
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, _, cx| {
        let view = view.read(cx);
        assert_eq!(view.files.len(), 1);
        assert_eq!(view.files[0].rel_path, "new.txt");
        assert_eq!(view.file_preview.as_ref().unwrap().text, "new workspace");
        assert!(view.error.is_none(), "superseded read cannot publish an error");
    })
    .unwrap();
}

#[gpui_kit::test]
fn file_selection_with_external_destination_launches_without_preview_overlay(cx: &mut TestAppContext) {
    let workspace = tempfile::TempDir::new().unwrap();
    std::fs::write(workspace.path().join("notes.txt"), "notes").unwrap();
    std::fs::write(workspace.path().join("other.txt"), "other").unwrap();
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.workspace = Some(workspace.path().to_path_buf());
            view.preferences.set_file_open_destination(FileOpenDestination::VsCode);
            view.test_launch_intercept = Some(TestLaunchIntercept::default());
            view.open_file_preview("notes.txt".to_string(), window, cx);
            assert!(view.file_preview.is_none(), "external mode never opens the overlay");
            assert!(view.error.is_none());
            assert_eq!(
                view.test_launch_intercept.as_ref().unwrap().calls,
                vec![ExternalLaunchCall {
                    rel_path: "notes.txt".into(),
                    editor: ExternalEditor::VsCode
                }]
            );

            // A different destination and file route to the newly selected editor.
            view.preferences.set_file_open_destination(FileOpenDestination::Sublime);
            view.open_file_preview("other.txt".to_string(), window, cx);
            assert!(view.file_preview.is_none());
            assert_eq!(
                view.test_launch_intercept.as_ref().unwrap().calls,
                vec![
                    ExternalLaunchCall {
                        rel_path: "notes.txt".into(),
                        editor: ExternalEditor::VsCode
                    },
                    ExternalLaunchCall {
                        rel_path: "other.txt".into(),
                        editor: ExternalEditor::Sublime
                    },
                ]
            );

            // Switching back to the viewer restores the overlay path.
            view.preferences.set_file_open_destination(FileOpenDestination::Viewer);
            view.open_file_preview("notes.txt".to_string(), window, cx);
        });
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, _, cx| {
        let view = view.read(cx);
        assert_eq!(view.file_preview.as_ref().unwrap().rel_path, "notes.txt");
        assert_eq!(view.test_launch_intercept.as_ref().unwrap().calls.len(), 2, "viewer mode launches nothing");
    })
    .unwrap();
}

#[gpui_kit::test]
fn external_launch_failure_surfaces_error_and_success_clears_it(cx: &mut TestAppContext) {
    let workspace = tempfile::TempDir::new().unwrap();
    std::fs::write(workspace.path().join("notes.txt"), "notes").unwrap();
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.workspace = Some(workspace.path().to_path_buf());
            view.preferences.set_file_open_destination(FileOpenDestination::Xcode);
            view.test_launch_intercept = Some(TestLaunchIntercept {
                fail_with: Some("missing app".into()),
                ..TestLaunchIntercept::default()
            });
            view.open_file_preview("notes.txt".to_string(), window, cx);
            assert!(view.file_preview.is_none(), "a failed launch opens no overlay");
            assert!(view.preview_editor_error.is_none(), "overlay-scoped errors stay unset");
            let error = view.error.as_ref().unwrap();
            assert!(error.contains("missing app"), "unexpected error: {error}");

            view.test_launch_intercept.as_mut().unwrap().fail_with = None;
            view.open_file_preview("notes.txt".to_string(), window, cx);
            assert!(view.error.is_none(), "a successful launch clears the notice");
            assert_eq!(view.test_launch_intercept.as_ref().unwrap().calls.len(), 2);
        });
    })
    .unwrap();
}

#[gpui_kit::test]
fn external_launch_ignores_results_after_workspace_change(cx: &mut TestAppContext) {
    let first = tempfile::TempDir::new().unwrap();
    let second = tempfile::TempDir::new().unwrap();
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, _, cx| {
        view.update(cx, |view, cx| {
            view.workspace = Some(first.path().to_path_buf());
            view.apply_external_launch_result(second.path(), Err("stale".into()), cx);
            assert!(view.error.is_none(), "a superseded launch cannot publish an error");
            view.apply_external_launch_result(first.path(), Err("current".into()), cx);
            assert!(view.error.as_ref().unwrap().contains("current"));
            view.apply_external_launch_result(second.path(), Ok(()), cx);
            assert!(view.error.is_some(), "a stale success cannot clear a current error");
            view.apply_external_launch_result(first.path(), Ok(()), cx);
            assert!(view.error.is_none());
        });
    })
    .unwrap();
}

#[gpui_kit::test]
fn external_destination_cancels_in_flight_and_open_previews(cx: &mut TestAppContext) {
    let workspace = tempfile::TempDir::new().unwrap();
    std::fs::write(workspace.path().join("notes.txt"), "notes").unwrap();
    std::fs::write(workspace.path().join("other.txt"), "other").unwrap();
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.workspace = Some(workspace.path().to_path_buf());
            view.preferences.set_file_open_destination(FileOpenDestination::Viewer);
            view.open_file_preview("notes.txt".to_string(), window, cx);
            assert!(view.preview_task.is_some(), "viewer load starts in the background");

            // Switching destinations mid-load cancels the viewer read: the late
            // result must never pop the overlay open after an external launch.
            view.preferences.set_file_open_destination(FileOpenDestination::VsCode);
            view.test_launch_intercept = Some(TestLaunchIntercept::default());
            view.open_file_preview("other.txt".to_string(), window, cx);
            assert!(view.preview_task.is_none());
            assert!(view.file_preview.is_none());
            assert_eq!(view.test_launch_intercept.as_ref().unwrap().calls.len(), 1);
        });
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            assert!(view.file_preview.is_none(), "a cancelled load publishes no overlay");

            // An already-open overlay is dropped when selection goes external.
            view.preferences.set_file_open_destination(FileOpenDestination::Viewer);
            view.open_file_preview("notes.txt".to_string(), window, cx);
        });
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            assert!(view.file_preview.is_some());
            view.preferences.set_file_open_destination(FileOpenDestination::Sublime);
            view.open_file_preview("other.txt".to_string(), window, cx);
            assert!(view.file_preview.is_none(), "external selection clears the open overlay");
            assert!(view.preview_editor_error.is_none());
            let calls = &view.test_launch_intercept.as_ref().unwrap().calls;
            assert_eq!(calls.len(), 2);
            assert_eq!(calls[1].editor, ExternalEditor::Sublime);
        });
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, _, cx| {
        assert!(view.read(cx).file_preview.is_none(), "no late overlay appears");
    })
    .unwrap();
}

#[gpui_kit::test]
fn file_destination_setting_renders_persists_and_routes_selection(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.open_settings(window, cx));
        window.render_frame(cx);
        window.scroll("settings-content", gpui_kit::ScrollDelta::Pixels(point(px(0.), px(-1000.))), cx);
        window.render_frame(cx);
        assert_eq!(window.find_all("file-open-destination-select").len(), 1);
        assert_eq!(window.find("file-open-destination-select").value(), Some("File viewer"));
        assert_eq!(view.read(cx).preferences.file_open_destination(), FileOpenDestination::Viewer);
    })
    .unwrap();

    // Drive the production subscription path: a dropdown confirmation reaches the setter.
    cx.update_window(window, |_, _, cx| {
        let select = view.read(cx).file_destination_select.clone();
        select.update(cx, |_, cx| cx.emit(SelectEvent::Confirm(Some(FileOpenDestination::Xcode))));
    })
    .unwrap();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(view.read(cx).preferences.file_open_destination(), FileOpenDestination::Xcode);
    })
    .unwrap();
    assert_eq!(
        crate::desktop::preferences::Preferences::load().file_open_destination(),
        FileOpenDestination::Xcode,
        "the dropdown choice persists across restarts"
    );

    // The persisted choice routes a real selection to the chosen editor.
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.test_launch_intercept = Some(TestLaunchIntercept::default());
            view.open_file_preview("notes.txt".to_string(), window, cx);
            assert!(view.file_preview.is_none());
            assert_eq!(
                view.test_launch_intercept.as_ref().unwrap().calls,
                vec![ExternalLaunchCall {
                    rel_path: "notes.txt".into(),
                    editor: ExternalEditor::Xcode
                }]
            );
        });
    })
    .unwrap();

    // A repeat confirmation re-saves the same choice.
    cx.update_window(window, |_, _, cx| {
        let select = view.read(cx).file_destination_select.clone();
        select.update(cx, |_, cx| cx.emit(SelectEvent::Confirm(Some(FileOpenDestination::Xcode))));
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(
        crate::desktop::preferences::Preferences::load().file_open_destination(),
        FileOpenDestination::Xcode,
        "a repeat confirmation keeps the saved choice"
    );

    cx.update_window(window, |_, _, cx| {
        view.update(cx, |view, cx| {
            view.set_file_open_destination(FileOpenDestination::Viewer, cx);
            assert_eq!(view.preferences.file_open_destination(), FileOpenDestination::Viewer);
        });
    })
    .unwrap();
    assert_eq!(
        crate::desktop::preferences::Preferences::load().file_open_destination(),
        FileOpenDestination::Viewer
    );
}

#[gpui_kit::test]
fn destination_dropdown_offers_detected_saved_and_system_choices(cx: &mut TestAppContext) {
    let apps = tempfile::TempDir::new().unwrap();
    crate::desktop::files::write_test_bundle(apps.path(), "Ghostty.app", "com.mitchellh.ghostty");
    crate::desktop::files::write_test_bundle(apps.path(), "Xcode.app", "com.apple.dt.Xcode");
    let pref_dir = tempfile::TempDir::new().unwrap();
    let prefs_path = pref_dir.path().join("prefs.json");
    std::fs::write(&prefs_path, r#"{"fileOpenDestination":"vscode"}"#).unwrap();
    let _roots = crate::desktop::files::set_test_app_roots(vec![apps.path().to_path_buf()]);

    let (window, view) = open_desktop_with_preferences_path(cx, prefs_path);
    cx.update_window(window, |_, window, cx| {
        let select = view.read(cx).file_destination_select.clone();
        // The saved choice stays selected even though its app is missing.
        assert_eq!(select.read(cx).selected_value().copied(), Some(FileOpenDestination::VsCode));
        // Probe the offered set through selection: detected and system
        // destinations select, undetected ones resolve to no selection.
        for (destination, offered) in [
            (FileOpenDestination::Ghostty, true),
            (FileOpenDestination::Xcode, true),
            (FileOpenDestination::Finder, true),
            (FileOpenDestination::Terminal, true),
            (FileOpenDestination::Sublime, false),
            (FileOpenDestination::AndroidStudio, false),
            (FileOpenDestination::ITerm2, false),
        ] {
            select.update(cx, |state, cx| state.set_selected_value(&destination, window, cx));
            assert_eq!(
                select.read(cx).selected_value().copied().as_ref(),
                offered.then_some(&destination),
                "{destination:?} offered={offered}"
            );
        }
    })
    .unwrap();
}

#[gpui_kit::test]
fn session_transitions_clear_runtime_projections_and_model_switch_preserves_the_thread(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let session = json!({
        "sessionId": "next-session", "title": "Next", "workspace": "/workspace",
        "completed": false, "resumable": true,
    });
    for (method, result) in [
        (vtcode_desktop_protocol::method::SESSION_CREATE, session.clone()),
        (vtcode_desktop_protocol::method::SESSION_CLOSE, json!({})),
        (
            vtcode_desktop_protocol::method::SESSION_LOAD,
            json!({
                "session": session,
                "messages": [{ "role": "assistant", "text": "Restored answer" }],
            }),
        ),
        (
            vtcode_desktop_protocol::method::CONFIG_SELECT,
            json!({
                "provider": "ollama", "model": "local", "authenticated": true,
                "credentialSetupCommand": "vtcode auth",
            }),
        ),
    ] {
        cx.update_window(window, |_, _, cx| {
            view.update(cx, |view, cx| {
                view.transcript.push(TranscriptItem::new(
                    "old".into(),
                    "assistant".into(),
                    "Old".into(),
                    "Old answer".into(),
                ));
                view.upsert_diff_card(
                    "old-change".into(),
                    &vtcode_exec_events::FileChangeItem {
                        diff_incomplete: Some(false),
                        changes: vec![vtcode_exec_events::FileUpdateChange {
                            path: "old.rs".into(),
                            kind: vtcode_exec_events::PatchChangeKind::Update,
                            terminators_normalized: None,
                        }],
                        status: vtcode_exec_events::PatchApplyStatus::Completed,
                        unified_diff: Some("--- a/old.rs\n+++ b/old.rs\n@@ -1 +1 @@\n-before\n+after\n".into()),
                        additions: Some(1),
                        deletions: Some(1),
                    },
                );
                assert_eq!(view.changes.entries().len(), 1);
                view.select_change("old.rs".into(), cx);
                assert!(view.change_viewer.is_some());
                view.last_usage = Some(vtcode_exec_events::Usage {
                    input_tokens: 100,
                    output_tokens: 20,
                    cached_input_tokens: 0,
                    cache_creation_tokens: 0,
                });
                view.selected_change = Some("old.rs".into());
                view.attachments
                    .push(vtcode_desktop_protocol::TurnAttachment { path: "old.rs".into(), text: "old code".into() });
                view.interaction_choice_ids.push(Some("old-choice".into()));
                view.interaction_answer_texts.push(Some("old answer".into()));
                view.interaction_step_index = 2;
                view.attachments_picker_open = true;
                if method == vtcode_desktop_protocol::method::SESSION_LOAD {
                    view.session_request = Some(vtcode_desktop_protocol::method::SESSION_LOAD);
                }
                view.handle_backend_event(
                    BackendEvent::Response {
                        method: method.into(),
                        result: Some(result),
                        error: None,
                    },
                    cx,
                );
            });
        })
        .unwrap();
        if method == vtcode_desktop_protocol::method::SESSION_LOAD {
            cx.run_until_parked();
        }
        cx.update_window(window, |_, _, cx| {
            let view = view.read(cx);
            if method == vtcode_desktop_protocol::method::CONFIG_SELECT {
                // A model switch restarts the runtime under the same session
                // id: every thread projection stays intact.
                let usage = view.last_usage.as_ref().expect("usage survives a model switch");
                assert_eq!(usage.input_tokens, 100);
                assert_eq!(usage.output_tokens, 20);
                assert_eq!(view.changes.entries().len(), 1);
                assert_eq!(view.selected_change.as_deref(), Some("old.rs"));
                assert!(view.change_viewer.is_some());
                assert_eq!(view.attachments.len(), 1);
                assert_eq!(view.attachments[0].path, "old.rs");
                assert_eq!(view.interaction_choice_ids, vec![Some("old-choice".to_string())]);
                assert_eq!(view.interaction_answer_texts, vec![Some("old answer".to_string())]);
                assert_eq!(view.interaction_step_index, 2);
                assert!(view.attachments_picker_open);
                // The loop shares one view: the LOAD leg's restored row plus
                // this leg's seeded rows (answer + diff card mirror) all
                // survive the model switch.
                assert_eq!(view.transcript.get(0).unwrap().text, "Restored answer");
                assert_eq!(view.transcript.get(1).unwrap().text, "Old answer");
                assert_eq!(
                    view.transcript.get(2).unwrap().text,
                    "--- a/old.rs\n+++ b/old.rs\n@@ -1 +1 @@\n-before\n+after\n"
                );
                assert!(view.transcript.get(3).is_none());
                return;
            }
            assert!(view.last_usage.is_none(), "{method}");
            assert!(view.changes.entries().is_empty(), "{method}");
            assert!(view.selected_change.is_none(), "{method}");
            assert!(view.change_viewer.is_none(), "{method}");
            assert!(view.attachments.is_empty(), "{method}");
            assert!(view.interaction_choice_ids.is_empty(), "{method}");
            assert!(view.interaction_answer_texts.is_empty(), "{method}");
            assert_eq!(view.interaction_step_index, 0, "{method}");
            assert!(!view.attachments_picker_open, "{method}");
            if method == vtcode_desktop_protocol::method::SESSION_LOAD {
                assert_eq!(view.transcript.get(0).unwrap().text, "Restored answer");
                assert!(view.session_request.is_none());
            } else {
                assert!(view.transcript.is_empty(), "{method}");
            }
        })
        .unwrap();
    }
}

#[gpui_kit::test]
fn pending_attachments_prevent_submitting_an_incomplete_prompt(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(1);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.selected_session_id = Some("session".into());
            view.attachments_picker_open = true;
            view.composer
                .update(cx, |state, cx| state.set_value("Review attached file", window, cx));
            view.submit_prompt(window, cx);
            assert!(!view.running);
            assert_eq!(view.composer.read(cx).value(), "Review attached file");
            assert!(view.transcript.is_empty());
        });
        window.render_frame(cx);
        assert_eq!(window.find("send-prompt").disabled(), Some(true));
    })
    .unwrap();
    assert!(requests.try_recv().is_err());
}

#[gpui_kit::test]
fn webmcp_connect_command_issues_a_pair_request_without_starting_an_agent_turn(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(2);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.composer.update(cx, |state, cx| {
                state.set_value("/webmcp connect https://vtcode.vinhnx.chatgpt.site", window, cx)
            });
            view.submit_prompt(window, cx);
            assert!(view.current_route() == Route::Settings);
            assert!(!view.running);
            assert!(view.composer.read(cx).value().is_empty());
        });
        window.render_frame(cx);
        assert!(window.try_find("connections-settings").is_some());
    })
    .unwrap();

    let Some(BackendCommand::Request { method, params }) = requests.try_recv().ok() else {
        panic!("connect command should request a browser pairing");
    };
    assert_eq!(method, vtcode_desktop_protocol::method::WEBMCP_PAIR);
    assert_eq!(params["origin"], "https://vtcode.vinhnx.chatgpt.site");
    assert_eq!(params["replace"], false);
    assert!(requests.try_recv().is_err());
}

#[gpui_kit::test]
fn unavailable_native_slash_commands_stay_in_the_composer(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(2);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.selected_session_id = Some("session".into());
            view.composer.update(cx, |state, cx| state.set_value("/mode", window, cx));
            view.submit_prompt(window, cx);
            assert_eq!(view.composer.read(cx).value(), "/mode");
            assert!(view.error.as_deref().is_some_and(|error| error.contains("Runtime options")));

            let mut configuration = runtime_options_config();
            configuration.session_options.as_mut().unwrap().fast_mode_supported = false;
            view.configuration = Some(configuration);
            view.composer.update(cx, |state, cx| state.set_value("/fast", window, cx));
            view.submit_prompt(window, cx);
            assert_eq!(view.composer.read(cx).value(), "/fast");
            assert_eq!(view.error.as_deref(), Some("Fast processing is unavailable for the selected model."));
            assert!(!view.running);
        });
    })
    .unwrap();

    assert!(requests.try_recv().is_err(), "unavailable native actions must not become agent prompts");
}

#[gpui_kit::test]
fn unavailable_fast_mode_alert_uses_the_selected_language(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(1);
    let language = crate::desktop::localization::UiLanguage::Spanish;
    crate::desktop::localization::set_language(language);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.preferences.set_ui_language(language);
            view.backend = Some(BackendHandle { commands });
            view.selected_session_id = Some("session".into());
            let mut configuration = runtime_options_config();
            configuration.session_options.as_mut().unwrap().fast_mode_supported = false;
            view.configuration = Some(configuration);
            view.composer.update(cx, |state, cx| state.set_value("/fast", window, cx));
            view.submit_prompt(window, cx);
        });
        window.render_frame(cx);
        let expected = rust_i18n::t!("app.errors.fast_processing_unavailable", locale = "es").to_string();
        assert_eq!(view.read(cx).error.as_deref(), Some(expected.as_str()));
        assert_eq!(window.find("composer-error").label(), Some(expected.as_str()));
    })
    .unwrap();
    assert!(requests.try_recv().is_err(), "an unavailable action must not become a prompt");
    crate::desktop::localization::set_language(crate::desktop::localization::UiLanguage::English);
}

#[gpui_kit::test]
fn connections_settings_tab_opens_and_refreshes_webmcp_status(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(1);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| view.backend = Some(BackendHandle { commands }));
        window.render_frame(cx);
        window.click("open-settings", cx);
        window.render_frame(cx);
        select_settings_category(window, 4, cx);
        window.render_frame(cx);
        assert!(window.try_find("connections-settings").is_some());
    })
    .unwrap();

    let Some(BackendCommand::Request { method, .. }) = requests.try_recv().ok() else {
        panic!("Connections tab should refresh connection status");
    };
    assert_eq!(method, vtcode_desktop_protocol::method::WEBMCP_STATUS);
    assert!(requests.try_recv().is_err());
}

#[gpui_kit::test]
fn settings_search_palette_to_connections_refreshes_webmcp_status(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(1);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.open_settings(window, cx);
        });
        window.render_frame(cx);
        window.press("cmd-f", cx);
        window.render_frame(cx);
        assert!(matches!(view.read(cx).palette_kind, super::super::PaletteKind::SettingsSearch));
        view.update(cx, |view, cx| view.focus_palette_state(window, cx));
        window.render_frame(cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.within("command-palette").input("Connections", cx);
        window.render_frame(cx);
        let action = super::super::PaletteAction::SelectSettingsTab(super::super::settings::SettingsTab::Connections);
        window.within("command-palette").click(palette_action_id(&action), cx);
        window.render_frame(cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(view.read(cx).settings_tab, super::super::settings::SettingsTab::Connections);
        assert!(window.try_find("connections-settings").is_some());
    })
    .unwrap();

    let Some(BackendCommand::Request { method, .. }) = requests.try_recv().ok() else {
        panic!("palette Connections entry should refresh connection status");
    };
    assert_eq!(method, vtcode_desktop_protocol::method::WEBMCP_STATUS);
    assert!(requests.try_recv().is_err());
}

#[gpui_kit::test]
fn splash_command_opens_connection_status_and_refreshes_webmcp(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(1);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.composer.update(cx, |state, cx| state.set_value("/splash", window, cx));
            view.submit_prompt(window, cx);
            assert!(view.current_route() == Route::Settings);
            assert_eq!(view.settings_tab, super::super::settings::SettingsTab::Connections);
            assert!(!view.running, "a diagnostics command must not start an agent turn");
        });
        window.render_frame(cx);
        assert!(window.try_find("connections-settings").is_some());
        assert!(window.try_find("local-service-status").is_some());
        assert!(window.try_find("local-service-unavailable").is_some());
    })
    .unwrap();

    let Some(BackendCommand::Request { method, .. }) = requests.try_recv().ok() else {
        panic!("/splash should refresh WebMCP status");
    };
    assert_eq!(method, vtcode_desktop_protocol::method::WEBMCP_STATUS);
    assert!(requests.try_recv().is_err());
}

#[gpui_kit::test]
fn local_service_settings_expose_the_negotiated_connection(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, _requests) = tokio::sync::mpsc::channel(1);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.settings_tab = super::super::settings::SettingsTab::Connections;
            view.backend = Some(BackendHandle { commands });
            view.service_info = Some(vtcode_desktop_protocol::InitializeResult {
                protocol: vtcode_desktop_protocol::PROTOCOL_VERSION,
                workspace: "/workspace/vtcode".into(),
                server_version: "0.2.0".into(),
                slash_commands: Vec::new(),
            });
            view.open_settings(window, cx);
        });
        window.render_frame(cx);
        assert_eq!(window.find("local-service-status").label(), Some("CLI service connection is active"));
        assert!(window.try_find("local-service-unavailable").is_none());
    })
    .unwrap();
}

#[gpui_kit::test]
fn webmcp_tools_and_roots_commands_show_shared_guidance_without_backend_requests(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(2);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.composer
                .update(cx, |state, cx| state.set_value("/webmcp tools", window, cx));
            view.submit_prompt(window, cx);
            assert!(view.current_route() == Route::Settings);
            assert!(!view.running);
            assert_eq!(
                view.webmcp_notice.as_ref().unwrap().text(),
                vtcode_desktop_protocol::slash_command::WEBMCP_TOOLS_HELP
            );

            view.composer
                .update(cx, |state, cx| state.set_value("/webmcp roots", window, cx));
            view.submit_prompt(window, cx);
            assert_eq!(
                view.webmcp_notice.as_ref().unwrap().text(),
                vtcode_desktop_protocol::slash_command::WEBMCP_ROOTS_HELP
            );
        });
    })
    .unwrap();

    assert!(requests.try_recv().is_err());
}

#[gpui_kit::test]
fn retained_submission_restores_exact_content_without_overwriting_a_new_draft(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.selected_session_id = Some("session".into());
            view.composer
                .update(cx, |state, cx| state.set_value("Original request", window, cx));
            view.attachments.push(vtcode_desktop_protocol::TurnAttachment {
                path: "source.rs".into(),
                text: "original file".into(),
            });
            view.submit_prompt(window, cx);
            assert!(view.pending_submission.is_some());
            view.composer.update(cx, |state, cx| state.set_value("New draft", window, cx));
            view.handle_backend_event(
                BackendEvent::Response {
                    method: vtcode_desktop_protocol::method::TURN_SUBMIT.into(),
                    result: None,
                    error: Some("selected session runtime has stopped".into()),
                },
                cx,
            );
            assert!(view.pending_submission.is_none());
            assert!(view.retained_submission.is_some());
            assert!(view.transcript.is_empty(), "rejected content was never sent to the agent");
            assert_eq!(view.composer.read(cx).value(), "New draft");
        });
        window.render_frame(cx);
        assert_eq!(window.find("restore-submission").disabled(), Some(true));
        assert_eq!(window.find("send-prompt").disabled(), Some(true));
        window.click("copy-retained-prompt", cx);
        assert_eq!(cx.read_from_clipboard().unwrap().text().as_deref(), Some("Original request"));
        view.update(cx, |view, cx| {
            view.restore_submission(window, cx);
            assert_eq!(view.composer.read(cx).value(), "New draft", "handler also guards against overwrite");
            view.composer.update(cx, |state, cx| state.set_value("", window, cx));
        });
        window.render_frame(cx);
        window.click("restore-submission", cx);
        {
            let view = view.read(cx);
            assert!(view.retained_submission.is_none());
            assert_eq!(view.composer.read(cx).value(), "Original request");
            assert_eq!(view.attachments[0].path, "source.rs");
            assert_eq!(view.attachments[0].text, "original file");
        }
        window.render_frame(cx);
        window.click("send-prompt", cx);
    })
    .unwrap();
    let BackendCommand::Request { params: first, .. } = requests.try_recv().unwrap() else {
        panic!("first request")
    };
    let BackendCommand::Request { params: retry, .. } = requests.try_recv().unwrap() else {
        panic!("retry request")
    };
    assert_eq!(first, retry, "explicit retry preserves prompt and attachment bytes");
    assert!(requests.try_recv().is_err(), "no automatic replay");
}

#[gpui_kit::test]
fn oversized_encoded_submission_keeps_composer_attachments_and_connection(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(2);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.selected_session_id = Some("session".into());
            view.composer
                .update(cx, |state, cx| state.set_value("Review files", window, cx));
            view.attachments = (0..5)
                .map(|index| vtcode_desktop_protocol::TurnAttachment {
                    path: format!("file-{index}.txt"),
                    text: "\u{1}".repeat(64 * 1024),
                })
                .collect();
            view.submit_prompt(window, cx);
            assert!(!view.running);
            assert!(view.backend.is_some());
            assert!(view.pending_submission.is_none());
            assert!(view.transcript.is_empty());
            assert_eq!(view.composer.read(cx).value(), "Review files");
            assert_eq!(view.attachments.len(), 5);
            assert!(view.error.as_deref().unwrap().contains("1 MiB"));
        });
    })
    .unwrap();
    assert!(requests.try_recv().is_err(), "oversized frame never reaches the transport");
}

#[gpui_kit::test]
fn zen_no_workspace_composer_keeps_the_workspace_popover(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    view.update(cx, |view, cx| {
        view.zen_mode = true;
        view.workspace = None;
        view.preferences.recent_workspaces = vec!["/project/alpha".into(), "/project/beta".into()];
        cx.notify();
    });

    let alpha_id = opaque_path_element_id("recent-workspace-popover", "/project/alpha");
    let beta_id = opaque_path_element_id("recent-workspace-popover", "/project/beta");
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("composer-context").is_some(), "zen without a workspace keeps the picker row");
        window.click("composer-workspace", cx);
        window.render_frame(cx);

        assert_eq!(window.find("open-workspace-folder").label(), Some("Choose folder…"));
        assert_eq!(window.find(alpha_id.clone()).label(), Some("alpha"));
        assert_eq!(window.find(beta_id.clone()).label(), Some("beta"));
    })
    .unwrap();
}

#[gpui_kit::test]
fn zen_no_workspace_webmcp_button_opens_connections_settings(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(1);
    view.update(cx, |view, _| {
        view.zen_mode = true;
        view.workspace = None;
        view.backend = Some(BackendHandle { commands });
    });
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("composer-cli-webmcp", cx);
        window.render_frame(cx);
        assert!(view.read(cx).current_route() == Route::Settings);
        assert!(window.try_find("connections-settings").is_some());
    })
    .unwrap();

    let Some(BackendCommand::Request { method, .. }) = requests.try_recv().ok() else {
        panic!("zen WebMCP button should refresh connection status");
    };
    assert_eq!(method, vtcode_desktop_protocol::method::WEBMCP_STATUS);
    assert!(requests.try_recv().is_err());
}
