use super::*;

#[gpui_kit::test]
fn zen_mode_hides_welcome_for_empty_threads(cx: &mut TestAppContext) {
    let workspace = tempfile::TempDir::new().expect("temporary workspace");
    let (window, view) = open_desktop(cx);
    view.update(cx, |view, _| {
        view.workspace = Some(workspace.path().to_path_buf());
    });

    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("welcome").is_some());
        assert!(window.try_find("send-prompt").is_some());

        window.press("ctrl-cmd-z", cx);
        window.render_frame(cx);
        assert!(window.try_find("welcome").is_none());
        assert!(window.try_find("transcript-scroll").is_some());
        assert!(window.try_find("send-prompt").is_some());

        window.press("ctrl-cmd-z", cx);
        window.render_frame(cx);
        assert!(window.try_find("welcome").is_some());
    })
    .unwrap();
}

#[gpui_kit::test]
fn zen_toggle_floats_above_content_with_reduced_motion(cx: &mut TestAppContext) {
    let workspace = tempfile::TempDir::new().expect("temporary workspace");
    let (window, view) = open_desktop(cx);
    view.update(cx, |view, cx| {
        view.workspace = Some(workspace.path().to_path_buf());
        view.preferences.navigation_visible = Some(false);
        view.inspector_open = false;
        view.set_reduced_motion(crate::desktop::preferences::ReducedMotionPreference::On, cx);
    });

    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("zen-mode-toggle").label(), Some("Enter Zen mode"));
        let entered = window.find("zen-mode-toggle").bounds();

        window.press("ctrl-cmd-z", cx);
        window.render_frame(cx);
        assert_eq!(window.find("zen-mode-toggle").label(), Some("Exit Zen mode"));
        assert!(window.try_find("welcome").is_none());
        assert!(window.try_find("send-prompt").is_some());
        assert!(window.try_find("title-bar").is_none(), "zen drops the toolbar");
        let zen = window.find("zen-mode-toggle").bounds();
        assert_eq!(zen.size, entered.size, "enter and exit share one frame");
        assert!(
            window.find("zen-drag-strip").bounds().contains(&zen.center()),
            "the exit floats above the drag strip"
        );

        window.press("ctrl-cmd-z", cx);
        window.render_frame(cx);
        assert_eq!(window.find("zen-mode-toggle").label(), Some("Enter Zen mode"));
        assert_eq!(window.find("zen-mode-toggle").bounds().origin, entered.origin);
        assert!(window.try_find("welcome").is_some());

        view.update(cx, |view, cx| {
            view.set_reduced_motion(crate::desktop::preferences::ReducedMotionPreference::System, cx);
        });
    })
    .unwrap();
}

#[gpui_kit::test]
fn zen_mode_hides_navigation_and_floats_the_toggle(cx: &mut TestAppContext) {
    let workspace = tempfile::TempDir::new().expect("temporary workspace");
    let (window, view) = open_desktop(cx);
    view.update(cx, |view, _| {
        view.workspace = Some(workspace.path().to_path_buf());
        view.sessions = vec![vtcode_desktop_protocol::SessionSummary {
            session_id: "zen-thread".into(),
            title: Some("Zen thread title".into()),
            workspace: workspace.path().display().to_string(),
            completed: false,
            resumable: true,
            size_bytes: 0,
            updated_at: None,
        }];
        view.selected_session_id = Some("zen-thread".into());
        view.transcript.push(TranscriptItem::new(
            "zen-message".into(),
            "assistant".into(),
            "VT Code".into(),
            "Conversation remains visible.".into(),
        ));
        view.sidebar_open = true;
        view.inspector_open = true;
        view.preferences.show_status_bar = true;
    });

    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("project-thread-sidebar").is_some());
        assert!(window.try_find("toggle-inspector").is_some());
        assert!(window.try_find("status-bar").is_some());
        assert_eq!(window.find("zen-mode-toggle").label(), Some("Enter Zen mode"));
        assert!(window.try_find("thread-header").is_some());
        assert!(window.try_find("composer-context").is_none(), "normal mode folds context into the button row");
        assert!(window.try_find("composer-mode").is_some(), "mode lives in the composer button row");
        let expanded_panels_slot = window.find("zen-mode-toggle").bounds();
        let expanded_panels_bar = window.find("title-bar").bounds();

        view.update(cx, |view, _| view.preferences.navigation_visible = Some(false));
        window.render_frame(cx);
        assert!(window.try_find("navigation-rail").is_none());
        assert!(window.try_find("top-new-thread").is_some());
        assert!(window.try_find("top-utilities").is_some());
        assert_eq!(window.find("zen-mode-toggle").label(), Some("Enter Zen mode"));
        let collapsed_nav_slot = window.find("zen-mode-toggle").bounds();
        let collapsed_nav_bar = window.find("title-bar").bounds();
        assert_eq!(
            collapsed_nav_bar.right() - collapsed_nav_slot.right(),
            expanded_panels_bar.right() - expanded_panels_slot.right(),
            "hiding navigation must keep the zen toggle in its trailing slot"
        );
        let normal_frame = window.find("message-composer-frame").bounds();
        let normal_send = window.find("send-prompt").bounds();
        assert_eq!(
            window.find("transcript-content-zen-message").bounds().center().x,
            normal_frame.center().x,
            "transcript and composer share one centered spine"
        );

        window.press("ctrl-cmd-z", cx);
        window.render_frame(cx);
        assert_eq!(window.find("zen-mode-toggle").label(), Some("Exit Zen mode"));
        assert!(window.try_find("navigation-rail").is_none());
        assert!(window.try_find("title-bar").is_none(), "zen drops the toolbar");
        let zen_slot = window.find("zen-mode-toggle").bounds();
        assert_eq!(zen_slot.size, collapsed_nav_slot.size, "enter and exit share one frame");
        assert!(
            window.find("zen-drag-strip").bounds().contains(&zen_slot.center()),
            "the exit floats above the drag strip while the workspace zooms"
        );
        assert!(
            zen_slot.size.width >= px(24.) && zen_slot.size.height >= px(24.),
            "zen toggle must keep a comfortable pointer target: {zen_slot:?}"
        );
        assert!(window.try_find("nav-home").is_none());
        assert!(window.try_find("nav-new-thread").is_none());
        assert!(window.try_find("nav-utilities").is_none());
        assert!(window.try_find("top-new-thread").is_none());
        assert!(window.try_find("top-utilities").is_none());
        assert!(window.try_find("toggle-sidebar").is_none());
        assert!(window.try_find("search-commands").is_none());
        assert!(window.try_find("toggle-inspector").is_none());
        assert!(window.try_find("open-settings").is_none());
        assert!(window.try_find("header-overflow").is_none());
        assert!(window.try_find("project-thread-sidebar").is_none());
        assert!(window.try_find("inspector-navigation").is_none());
        assert!(window.try_find("status-bar").is_none());
        assert!(window.try_find("thread-header").is_none());
        assert!(window.try_find("composer-context").is_none());
        assert!(window.try_find("attach-files").is_none());
        assert!(window.try_find("composer-policy").is_some());
        assert!(window.try_find("composer-mode").is_some());
        assert!(window.try_find("pick-model").is_some());
        assert!(window.try_find("send-prompt").is_some());
        assert!(window.try_find("workspace-surface").is_some());
        let zen_frame = window.find("message-composer-frame").bounds();
        assert!(
            zen_frame.size.width < normal_frame.size.width,
            "zen composer narrows: {zen_frame:?} vs {normal_frame:?}"
        );
        assert!(zen_frame.size.width <= px(600.), "zen column cap: {zen_frame:?}");
        assert_eq!(
            window.find("send-prompt").bounds().size.height,
            normal_send.size.height,
            "zen keeps the send action at the composer size"
        );
        assert_eq!(
            window.find("transcript-content-zen-message").bounds().center().x,
            zen_frame.center().x,
            "zen column keeps one shared spine"
        );

        window.click("zen-mode-toggle", cx);
        window.render_frame(cx);
        assert_eq!(window.find("zen-mode-toggle").label(), Some("Enter Zen mode"));
        assert_eq!(
            window.find("zen-mode-toggle").bounds().origin,
            collapsed_nav_slot.origin,
            "exiting zen restores the same slot"
        );
        assert!(window.try_find("thread-header").is_some());
        assert!(window.try_find("composer-context").is_none(), "exiting zen restores the single-row composer");
        assert!(window.try_find("navigation-rail").is_none());
        assert!(window.try_find("project-thread-sidebar").is_none());
        assert!(window.try_find("top-new-thread").is_some());
        assert!(window.try_find("top-utilities").is_some());
        assert!(window.try_find("toggle-inspector").is_some());
        assert!(window.try_find("status-bar").is_some());
    })
    .unwrap();
}

#[gpui_kit::test]
fn header_icon_band_shares_one_frame_and_center_line(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    view.update(cx, |view, _| {
        view.workspace = Some(std::env::temp_dir());
        view.preferences.navigation_visible = Some(false);
    });

    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        // Leading cluster and trailing toolbar are different owners; the band
        // they form must still share one frame height and center line.
        let slots = [
            "navigate-back",
            "navigate-forward",
            "toggle-sidebar",
            "top-new-thread",
            "top-utilities",
            "search-commands",
            "toggle-inspector",
            "open-settings",
            "header-overflow",
            "zen-mode-toggle",
        ];
        let first = window.find(slots[0]).bounds();
        assert_eq!(first.size.height, px(24.), "the band pins the compact tier");
        assert_eq!(first.size.width, px(24.), "icon slots stay square");
        for slot in slots {
            let bounds = window.find(slot).bounds();
            assert_eq!(bounds.size.height, first.size.height, "{slot} shares the band frame");
            assert_eq!(bounds.size.width, first.size.width, "{slot} shares the band frame");
            assert_eq!(bounds.center().y, first.center().y, "{slot} shares the band center line");
        }
    })
    .unwrap();
}

#[gpui_kit::test]
fn recent_threads_virtualize_rows_and_scroll_to_older_sessions(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let sessions = (0..120)
        .map(|index| vtcode_desktop_protocol::SessionSummary {
            session_id: format!("thread-{index}"),
            title: Some(format!("Thread {index}")),
            workspace: "/project/vtcode".into(),
            completed: false,
            resumable: true,
            size_bytes: 0,
            updated_at: None,
        })
        .collect();
    view.update(cx, |view, _| {
        view.sessions = sessions;
        view.selected_session_id = Some("thread-0".into());
    });

    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let list = window.find("session-list");
        assert_eq!(list.role(), Some(gpui_kit::Role::List));
        assert_eq!(list.label(), Some("Recent threads"));
        let first_thread = window.find("session-row-thread-0");
        assert_eq!(first_thread.role(), Some(gpui_kit::Role::ListItem));
        assert_eq!(first_thread.label(), Some("Thread 0"));
        assert_eq!(first_thread.selected(), Some(true));
        assert!(window.try_find("session-thread-0").is_some(), "the first thread is visible at the top");
        assert!(window.try_find("session-thread-119").is_none(), "offscreen threads are not built into the UI tree");

        view.read(cx).session_scroll.scroll_to_item(119, gpui_kit::ScrollStrategy::Top);
        window.render_frame(cx);

        assert!(window.try_find("session-thread-119").is_some(), "scrolling renders the requested thread");
        assert!(window.try_find("session-thread-0").is_none(), "the previous top rows leave the rendered range");
    })
    .unwrap();
}

#[test]
fn fixed_virtual_list_geometry_reuses_rows_until_count_or_font_size_changes() {
    let mut cache = FixedRowSizeCache::default();
    let default_session_height = scale_row_height_for_ui_font(36., px(16.));
    let large_session_height = scale_row_height_for_ui_font(36., px(24.));
    assert_eq!(default_session_height, 36.);
    assert_eq!(large_session_height, 54.);
    assert_eq!(scale_row_height_for_ui_font(72., px(24.)), 108.);

    let first = cache.get(120, default_session_height);
    let same_geometry = cache.get(120, default_session_height);
    assert!(std::rc::Rc::ptr_eq(&first, &same_geometry));
    assert_eq!(first.len(), 120);
    assert_eq!(first[0].height, px(36.));

    let changed_count = cache.get(121, default_session_height);
    assert!(!std::rc::Rc::ptr_eq(&same_geometry, &changed_count));
    assert_eq!(changed_count.len(), 121);

    let changed_font_size = cache.get(121, large_session_height);
    assert!(!std::rc::Rc::ptr_eq(&changed_count, &changed_font_size));
    assert_eq!(changed_font_size[0].height, px(54.));
}

#[gpui_kit::test]
fn recent_session_rows_scale_with_the_accessibility_font_size(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    view.update(cx, |view, cx| {
        view.preferences.ui_font_size = Some(24);
        view.sessions.push(vtcode_desktop_protocol::SessionSummary {
            session_id: "font-scale".into(),
            title: Some("Readable thread title".into()),
            workspace: std::env::temp_dir().display().to_string(),
            completed: false,
            resumable: true,
            size_bytes: 0,
            updated_at: None,
        });
        view.apply_typography(cx);
    });

    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("session-row-font-scale").bounds().size.height, px(54.));
    })
    .unwrap();
}

#[gpui_kit::test]
fn activity_rows_scale_with_the_accessibility_font_size(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    view.update(cx, |view, cx| {
        view.preferences.ui_font_size = Some(24);
        view.apply_typography(cx);
        view.inspector_open = true;
        view.inspector_tab = crate::desktop::inspector::InspectorTab::Activity;
        view.transcript.push(TranscriptItem::new(
            "tool-font-scale".into(),
            "tool".into(),
            "Read source".into(),
            "completed".into(),
        ));
    });

    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("activity-tool-font-scale").bounds().size.height, px(66.));
    })
    .unwrap();
}

#[gpui_kit::test]
fn sidebar_workspace_section_exposes_recent_projects_and_folder_picker(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    view.update(cx, |view, cx| {
        view.workspace = Some(PathBuf::from("/project/alpha"));
        view.preferences.recent_workspaces = vec!["/project/alpha".into(), "/project/beta".into()];
        cx.notify();
    });

    let alpha_id = opaque_path_element_id("workspace", "/project/alpha");
    let beta_id = opaque_path_element_id("workspace", "/project/beta");
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("choose-workspace").is_some(), "sidebar offers the folder picker");
        assert_eq!(window.find(alpha_id.clone()).label(), Some("alpha (current workspace)"));
        assert_eq!(window.find(beta_id.clone()).label(), Some("beta"));
    })
    .unwrap();
}

#[gpui_kit::test]
fn selecting_a_recent_workspace_uses_the_existing_shutdown_transition(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let target = tempfile::tempdir().unwrap();
    let target_path = target.path().to_path_buf();
    let canonical_target = vtcode_commons::paths::canonicalize(&target_path).unwrap();
    let target_display = target_path.display().to_string();
    let (commands, mut received) = tokio::sync::mpsc::channel(1);
    view.update(cx, |view, cx| {
        view.preferences.recent_workspaces = vec![target_display];
        view.backend = Some(BackendHandle { commands });
        cx.notify();
    });

    let target_id = opaque_path_element_id("workspace", &target_path.display().to_string());
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click(target_id, cx);
        window.render_frame(cx);
    })
    .unwrap();

    cx.update(|cx| {
        assert!(view.read(cx).workspace_selection_pending);
        assert_eq!(view.read(cx).workspace.as_deref(), Some(std::env::temp_dir().as_path()));
    });
    cx.run_until_parked();
    cx.update(|cx| {
        assert!(!view.read(cx).workspace_selection_pending);
        assert_eq!(view.read(cx).pending_workspace.as_deref(), Some(canonical_target.as_path()));
    });
    assert!(matches!(received.try_recv(), Ok(BackendCommand::Stop)));
}

#[gpui_kit::test]
fn workspace_path_validation_runs_before_switching_the_service(cx: &mut TestAppContext) {
    let (_window, view) = open_desktop(cx);
    let target = tempfile::tempdir().unwrap();
    let target_path = target.path().to_path_buf();
    let canonical_target = vtcode_commons::paths::canonicalize(&target_path).unwrap();
    let original_workspace = std::env::temp_dir();
    let (commands, mut requests) = tokio::sync::mpsc::channel(1);

    view.update(cx, |view, cx| {
        view.backend = Some(BackendHandle { commands });
        view.select_workspace(target_path, cx);
        assert!(view.workspace_selection_pending);
        assert_eq!(view.workspace.as_deref(), Some(original_workspace.as_path()));
        assert!(requests.try_recv().is_err(), "the worker has not switched the service during the UI callback");
    });

    cx.update(|cx| assert!(!view.read(cx).workspace_switch_idle()));
    cx.run_until_parked();
    cx.update(|cx| {
        let view = view.read(cx);
        assert!(!view.workspace_selection_pending);
        assert_eq!(view.workspace.as_deref(), Some(original_workspace.as_path()));
        assert_eq!(view.pending_workspace.as_deref(), Some(canonical_target.as_path()));
    });
    assert!(matches!(requests.try_recv(), Ok(BackendCommand::Stop)));
}

#[gpui_kit::test]
fn workspace_switch_revalidates_state_after_async_folder_check(cx: &mut TestAppContext) {
    let (_window, view) = open_desktop(cx);
    let target = tempfile::tempdir().unwrap();
    let original_workspace = std::env::temp_dir();
    let (commands, mut requests) = tokio::sync::mpsc::channel(1);

    view.update(cx, |view, cx| {
        view.backend = Some(BackendHandle { commands });
        view.select_workspace(target.path().to_path_buf(), cx);
        view.running = true;
    });

    cx.run_until_parked();
    cx.update(|cx| {
        let view = view.read(cx);
        assert!(!view.workspace_selection_pending);
        assert_eq!(view.workspace.as_deref(), Some(original_workspace.as_path()));
        assert!(view.pending_workspace.is_none());
        assert!(view.error.as_deref().unwrap().contains("Cancel the active turn"));
    });
    assert!(requests.try_recv().is_err(), "a stale selection must not stop the active service");
}

#[gpui_kit::test]
fn stale_startup_workspace_is_skipped_without_a_folder_error(cx: &mut TestAppContext) {
    let (_window, view) = open_desktop(cx);
    let missing_workspace = tempfile::tempdir().unwrap();
    let missing = missing_workspace.path().to_path_buf();
    drop(missing_workspace);

    view.update(cx, |view, cx| view.select_workspace_on_startup(missing, cx));
    cx.run_until_parked();

    cx.update(|cx| {
        let view = view.read(cx);
        assert!(!view.workspace_selection_pending);
        assert!(view.error.is_none());
        assert_eq!(view.workspace.as_deref(), Some(std::env::temp_dir().as_path()));
    });
}

#[gpui_kit::test]
fn status_bar_announces_agent_state_changes(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    view.update(cx, |view, cx| {
        view.preferences.show_status_bar = true;
        view.running = false;
        cx.notify();
    });

    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let status_bar = window.find("status-bar");
        assert_eq!(status_bar.role(), Some(gpui_kit::Role::Group));
        assert_eq!(status_bar.label(), Some("Status bar"));
        let status = window.find("agent-status");
        assert_eq!(status.role(), Some(gpui_kit::Role::Status));
        assert_eq!(status.label(), Some("Agent: Idle"));
    })
    .unwrap();

    view.update(cx, |view, cx| {
        view.running = true;
        cx.notify();
    });

    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let status = window.find("agent-status");
        assert_eq!(status.role(), Some(gpui_kit::Role::Status));
        assert_eq!(status.label(), Some("Agent: Running"));
    })
    .unwrap();
}

#[gpui_kit::test]
fn composer_errors_are_exposed_as_alerts(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    view.update(cx, |view, cx| {
        view.error = Some("The local service request queue is full. Try again.".into());
        cx.notify();
    });

    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let error = window.find("composer-error");
        assert_eq!(error.role(), Some(gpui_kit::Role::Alert));
        assert_eq!(error.label(), Some("The local service request queue is full. Try again."));
    })
    .unwrap();
}

#[gpui_kit::test]
fn restored_history_accessibility_ids_follow_message_content_across_reload(cx: &mut TestAppContext) {
    use vtcode_desktop_protocol::SessionHistoryMessage;

    let (window, view) = open_desktop(cx);
    let session_id = "archive-session";
    let prompt = SessionHistoryMessage {
        hidden: false,
        archive_index: None,
        role: "user".into(),
        text: "Question".into(),
    };
    let answer = SessionHistoryMessage {
        hidden: false,
        archive_index: None,
        role: "assistant".into(),
        text: "Answer".into(),
    };
    let answer_id = super::super::restored_history_element_id(session_id, &answer.role, &answer.text, 0);
    let repeated_answer_id = super::super::restored_history_element_id(session_id, &answer.role, &answer.text, 1);
    let history = super::super::prepare_session_history(session_id, vec![prompt.clone(), answer.clone()]);
    assert_ne!(answer_id, repeated_answer_id, "repeated archive messages need distinct row identities");
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| view.load_history(history));
        window.render_frame(cx);
        assert_eq!(window.find(format!("message-body-{answer_id}")).label(), Some("VT Code: Answer"));

        let inserted = SessionHistoryMessage {
            hidden: false,
            archive_index: None,
            role: "user".into(),
            text: "A newer question".into(),
        };
        let history = super::super::prepare_session_history(
            session_id,
            vec![
                inserted,
                prompt,
                answer,
                SessionHistoryMessage {
                    hidden: false,
                    archive_index: None,
                    role: "assistant".into(),
                    text: "Answer".into(),
                },
            ],
        );
        // The snapshot can advance while the original row remains in its projection.
        view.update(cx, |view, _| view.load_history(history));
        window.render_frame(cx);
        assert_eq!(window.find(format!("message-body-{answer_id}")).label(), Some("VT Code: Answer"));
        assert_eq!(window.find(format!("message-body-{repeated_answer_id}")).label(), Some("VT Code: Answer"));
    })
    .unwrap();
}

#[test]
fn restored_history_archive_ids_remain_stable_when_an_earlier_page_is_loaded() {
    use vtcode_desktop_protocol::SessionHistoryMessage;

    let session_id = "archive-session";
    let recent = super::super::prepare_session_history(
        session_id,
        vec![
            SessionHistoryMessage {
                hidden: false,
                archive_index: Some(160),
                role: "assistant".into(),
                text: "Repeated".into(),
            },
            SessionHistoryMessage {
                hidden: false,
                archive_index: Some(161),
                role: "assistant".into(),
                text: "Repeated".into(),
            },
        ],
    );
    let previous_id = recent[0].id.clone();
    assert_ne!(previous_id, recent[1].id, "identical archived messages have distinct stable identities");

    let page = super::super::prepare_session_history(
        session_id,
        vec![
            SessionHistoryMessage {
                hidden: false,
                archive_index: Some(158),
                role: "user".into(),
                text: "Earlier".into(),
            },
            SessionHistoryMessage {
                hidden: false,
                archive_index: Some(159),
                role: "assistant".into(),
                text: "Context".into(),
            },
            SessionHistoryMessage {
                hidden: false,
                archive_index: Some(160),
                role: "assistant".into(),
                text: "Repeated".into(),
            },
        ],
    );
    assert_eq!(page[2].id, previous_id);
}

#[gpui_kit::test]
fn earlier_history_page_uses_bounded_request_and_preserves_reader_anchor(cx: &mut TestAppContext) {
    use vtcode_desktop_protocol::{SessionHistoryMessage, SessionHistoryPage};

    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(2);
    let scroller = view.update(cx, |view, _| {
        view.backend = Some(BackendHandle { commands });
        view.selected_session_id = Some("archive-session".into());
        view.history_start_index = 160;
        view.history_has_earlier = true;
        let messages = (160..170)
            .map(|index| SessionHistoryMessage {
                hidden: false,
                archive_index: Some(index),
                role: "assistant".into(),
                text: format!("Message {index}\n\n{}", "A tall archived row.\n\n".repeat(32)),
            })
            .collect();
        view.transcript
            .replace_prepared(super::super::prepare_session_history("archive-session", messages));
        view.transcript.follow_latest();
        view.message_scroller.clone()
    });

    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("load-earlier-history").role(), Some(gpui_kit::Role::Button));
        scroller.update(cx, |state, cx| assert!(state.scroll_to_item(5, cx)));
        window.render_frame(cx);
        window.click("load-earlier-history", cx);
    })
    .unwrap();
    cx.run_until_parked();

    let BackendCommand::Request { method, params } = requests.try_recv().unwrap() else {
        panic!("earlier history request")
    };
    assert_eq!(method, vtcode_desktop_protocol::method::SESSION_HISTORY);
    assert_eq!(params["sessionId"], "archive-session");
    assert_eq!(params["beforeIndex"], 160);
    assert!(params["limit"].as_u64().is_some_and(|limit| limit > 0 && limit <= 160));
    let request_id = params["requestId"].as_str().unwrap().to_string();
    let before = cx.update(|cx| scroller.read(cx).logical_scroll_top());

    let page = SessionHistoryPage {
        session_id: "archive-session".into(),
        request_id,
        start_index: 157,
        messages: (157..160)
            .map(|index| SessionHistoryMessage {
                hidden: false,
                archive_index: Some(index),
                role: "assistant".into(),
                text: format!("Earlier {index}\n\n{}", "Earlier archived row.\n\n".repeat(24)),
            })
            .collect(),
        has_earlier: true,
    };
    view.update(cx, |view, cx| view.begin_session_history_page(json!(page), cx));
    cx.run_until_parked();

    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let after = scroller.read(cx).logical_scroll_top();
        assert_eq!(after.item_ix, before.item_ix + 3);
        assert!((after.offset_in_item - before.offset_in_item).abs() <= px(1.));
        view.update(cx, |view, _| {
            assert_eq!(view.history_start_index, 157);
            assert!(view.history_has_earlier);
            assert!(!view.history_page_loading);
            assert_eq!(view.transcript.len(), 13);
            assert_eq!(view.transcript.get(3).unwrap().text.as_ref().lines().next(), Some("Message 160"));
        });
    })
    .unwrap();
}

#[gpui_kit::test]
fn hidden_history_page_advances_without_showing_runtime_context(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    view.update(cx, |view, cx| {
        view.selected_session_id = Some("archive-session".into());
        view.history_start_index = 2;
        view.history_page_request_id = Some("page".into());
        view.history_page_limit = 2;
        view.history_page_loading = true;
        view.begin_session_history_page(
            json!({
                "sessionId": "archive-session", "requestId": "page", "startIndex": 1,
                "messages": [{ "archiveIndex": 1, "role": "system", "text": "private guidance", "hidden": true }],
                "hasEarlier": true
            }),
            cx,
        );
    });
    cx.run_until_parked();
    view.update(cx, |view, _| {
        assert_eq!(view.history_start_index, 1);
        assert!(view.history_has_earlier);
        assert!(!view.history_page_loading);
        assert!(view.history_page_error.is_none());
        assert!(view.transcript.is_empty());
    });
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("load-earlier-history").role(), Some(gpui_kit::Role::Button));
    })
    .unwrap();
}

#[test]
fn hidden_history_marker_does_not_suppress_user_messages() {
    let messages = vec![
        vtcode_desktop_protocol::SessionHistoryMessage {
            hidden: true,
            archive_index: Some(0),
            role: "system".into(),
            text: "model context".into(),
        },
        vtcode_desktop_protocol::SessionHistoryMessage {
            hidden: true,
            archive_index: Some(1),
            role: "user".into(),
            text: "keep my message".into(),
        },
    ];
    let rows = super::super::prepare_session_history("session", messages);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].text.as_ref(), "keep my message");
}

#[gpui_kit::test]
fn earlier_history_rejects_a_page_that_skips_messages(cx: &mut TestAppContext) {
    let (_window, view) = open_desktop(cx);
    view.update(cx, |view, cx| {
        view.selected_session_id = Some("archive-session".into());
        view.history_start_index = 160;
        view.history_page_request_id = Some("page".into());
        view.history_page_limit = 80;
        view.history_page_loading = true;
        view.begin_session_history_page(
            json!({
                "sessionId": "archive-session", "requestId": "page", "startIndex": 157,
                "messages": [{ "archiveIndex": 157, "role": "assistant", "text": "Earlier" }],
                "hasEarlier": true
            }),
            cx,
        );
    });
    cx.run_until_parked();
    view.update(cx, |view, _| {
        assert_eq!(view.history_start_index, 160);
        assert!(!view.history_page_loading);
        assert!(view.history_page_error.is_some());
        assert!(view.transcript.is_empty());
    });
}

#[gpui_kit::test]
fn stale_history_page_error_does_not_clear_the_current_request(cx: &mut TestAppContext) {
    let (_window, view) = open_desktop(cx);
    view.update(cx, |view, cx| {
        view.history_page_request_id = Some("current-page".into());
        view.history_page_limit = 80;
        view.history_page_loading = true;
        view.handle_backend_event(
            BackendEvent::SessionHistoryResponse {
                request_id: "stale-page".into(),
                result: None,
                error: Some("old session failed".into()),
            },
            cx,
        );
        assert_eq!(view.history_page_request_id.as_deref(), Some("current-page"));
        assert_eq!(view.history_page_limit, 80);
        assert!(view.history_page_loading);
        assert!(view.history_page_error.is_none());
    });
}

#[gpui_kit::test]
fn sidebar_service_status_announces_state_changes(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    view.update(cx, |view, cx| {
        view.status = "Starting local VT Code service…".into();
        cx.notify();
    });

    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let status = window.find("sidebar-status");
        assert_eq!(status.role(), Some(gpui_kit::Role::Status));
        assert_eq!(status.label(), Some("Starting local VT Code service…"));
    })
    .unwrap();

    view.update(cx, |view, cx| {
        view.status = "Connected to the local VT Code service".into();
        cx.notify();
    });

    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let status = window.find("sidebar-status");
        assert_eq!(status.role(), Some(gpui_kit::Role::Status));
        assert_eq!(status.label(), Some("Connected to the local VT Code service"));
    })
    .unwrap();
}

#[gpui_kit::test]
fn sidebar_shows_running_for_background_work_after_turn_completion(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    view.update(cx, |view, cx| {
        view.status = "Completed".into();
        view.running = false;
        view.ui_progress_active = true;
        cx.notify();
    });
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("sidebar-status").label(), Some("Running"));
        assert!(window.try_find("sidebar-status-loading").is_some());
        view.update(cx, |view, cx| {
            view.ui_progress_active = false;
            cx.notify();
        });
        window.render_frame(cx);
        assert_eq!(window.find("sidebar-status").label(), Some("Completed"));
        assert!(window.try_find("sidebar-status-loading").is_none());
    })
    .unwrap();
}

#[gpui_kit::test]
fn zen_empty_state_centers_brand_mark_with_greeting(cx: &mut TestAppContext) {
    let workspace = tempfile::TempDir::new().expect("temporary workspace");
    let (window, view) = open_desktop(cx);
    view.update(cx, |view, _| {
        view.workspace = Some(workspace.path().to_path_buf());
        view.zen_mode = true;
    });

    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("welcome").is_none());
        assert_eq!(window.find("zen-welcome").label(), Some("VT Code zen start"));
        assert!(window.try_find("send-prompt").is_some());
        let mark = window.find("welcome-brand-mark").bounds();
        let prompt = window.find("zen-welcome-prompt");
        assert_eq!(prompt.role(), Some(gpui_kit::Role::Heading));
        assert_eq!(mark.center().x, prompt.bounds().center().x, "mark and greeting share one centered spine");
        let name = workspace.path().file_name().unwrap().to_string_lossy().into_owned();
        assert!(
            prompt.label().unwrap().contains(&name),
            "zen names the workspace in plain text: {:?}",
            prompt.label()
        );
        assert!(window.try_find("welcome-workspace").is_none(), "zen keeps no inline workspace link");
        assert!(window.try_find("open-first-workspace").is_none());
    })
    .unwrap();
}

#[gpui_kit::test]
fn zen_empty_state_stays_greeting_only_with_picker_in_composer_context(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    view.update(cx, |view, _| {
        view.workspace = None;
        view.zen_mode = true;
    });

    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("zen-welcome").is_some());
        assert!(window.try_find("open-first-workspace").is_none());
        // The workspace picker stays reachable through the composer context row.
        assert!(window.try_find("composer-context").is_some());
        window.click("composer-workspace", cx);
        window.render_frame(cx);
        assert_eq!(window.find("open-workspace-folder").label(), Some("Choose folder…"));
    })
    .unwrap();
}

#[gpui_kit::test]
fn zen_composer_pairs_option_controls_with_send_action(cx: &mut TestAppContext) {
    let workspace = tempfile::TempDir::new().expect("temporary workspace");
    let (window, view) = open_desktop(cx);
    view.update(cx, |view, _| {
        view.workspace = Some(workspace.path().to_path_buf());
        view.zen_mode = true;
        view.configuration = Some({
            let mut configuration = runtime_options_config();
            configuration.provider = "anthropic".into();
            configuration.model = "claude-sonnet-5".into();
            configuration
        });
    });

    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("attach-files").is_none());
        assert_eq!(window.find("pick-model").label(), Some("claude-sonnet-5 · Medium · Ultrafast"));
        assert_eq!(window.find("composer-mode").label(), Some("Agent mode: Build"));
        assert_eq!(window.find("composer-policy").label(), Some("Execution policy: Ask before protected actions"));
        let context = window.find("zen-composer-context").bounds();
        let send = window.find("send-prompt").bounds();
        assert!(
            context.right() <= send.left(),
            "the option cluster must end before the send action: {context:?} vs {send:?}"
        );
        assert!(
            send.top() <= context.center().y && context.center().y <= send.bottom(),
            "the option cluster shares the send action row: {context:?} vs {send:?}"
        );
        assert!(
            window.find("message-composer-frame").bounds().contains(&context.center()),
            "the option cluster sits inside the composer card"
        );

        window.press("ctrl-cmd-z", cx);
        window.render_frame(cx);
        assert!(window.try_find("zen-composer-context").is_none());
        assert!(window.try_find("attach-files").is_some());
        assert!(window.try_find("pick-model").is_some());
        assert!(window.try_find("composer-mode").is_some());
        assert!(window.try_find("composer-policy").is_some());
    })
    .unwrap();
}

#[gpui_kit::test]
fn zen_composer_accepts_keyboard_input_without_focus_ring(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        window.press("ctrl-cmd-z", cx);
        window.render_frame(cx);
        assert!(view.read(cx).zen_mode);

        // The card shows no focus ring; the caret still marks the field, so
        // focusing and typing must work exactly as in the default composer.
        window.click("message-composer", cx);
        window.input("Zen draft without ring", cx);
        window.render_frame(cx);

        let composer_focus = view.read(cx).composer.read(cx).focus_handle(cx).clone();
        assert!(composer_focus.contains_focused(window, cx));
        assert_eq!(view.read(cx).composer.read(cx).value(), "Zen draft without ring");
        assert!(window.find("message-composer-frame").bounds().size.height > px(0.));
    })
    .unwrap();
}
