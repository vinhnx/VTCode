use super::*;

#[gpui_kit::test]
fn menu_bar_setting_keeps_the_window_and_draft_when_closed(cx: &mut TestAppContext) {
    cx.update(|cx| cx.set_reduce_motion(true));
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.composer
                .update(cx, |input, cx| input.set_value("keep this draft", window, cx));
            view.settings_tab = super::super::settings::SettingsTab::VtCode;
            view.open_settings(window, cx);
        });
        window.render_frame(cx);
        window.scroll("settings-content", gpui_kit::ScrollDelta::Pixels(point(px(0.), px(-3000.))), cx);
        window.render_frame(cx);
        assert_eq!(window.find("show-in-menu-bar").checked(), Some(false));
        window.click("show-in-menu-bar", cx);
        window.render_frame(cx);
        assert_eq!(window.find("show-in-menu-bar").checked(), Some(true));
        assert!(crate::desktop::preferences::Preferences::load().show_in_menu_bar);
        view.update(cx, |view, cx| view.close_settings(window, cx));
        window.render_frame(cx);
        window.dispatch_action(Box::new(super::super::CloseWindow), cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update(|cx| {
        assert_eq!(cx.windows().len(), 1);
        assert_eq!(view.read(cx).composer.read(cx).value(), "keep this draft");
        assert!(super::super::menu_bar::enabled(cx));
    });
}

#[gpui_kit::test]
fn contrast_slider_keyboard_changes_persist_and_reset(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.open_settings(window, cx));
        window.render_frame(cx);
        window.scroll("settings-content", gpui_kit::ScrollDelta::Pixels(point(px(0.), px(-3000.))), cx);
        window.render_frame(cx);
        view.read(cx).contrast_focus.clone().focus(window, cx);
        window.press("right", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(view.read(cx).preferences.contrast(), 1);
        assert_eq!(crate::desktop::preferences::Preferences::load().contrast(), 1);
        let slider_id = ("slider", view.read(cx).contrast_slider.entity_id());
        assert_eq!(window.find(slider_id).role(), Some(gpui_kit::Role::Slider));
        assert_eq!(window.find(slider_id).label(), Some("Contrast"));
        let track = window.find("slider-bar-container").bounds();
        window.click_at("slider-bar-container", point(track.size.width * 0.75, track.size.height / 2.), cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(view.read(cx).preferences.contrast() > 50, "pointer selection updates the preference");
        let before = view.read(cx).preferences.contrast();
        window.press("right", cx);
        window.render_frame(cx);
        assert_eq!(view.read(cx).contrast_slider.read(cx).value().start(), f32::from(before + 1));
        window.scroll("settings-content", gpui_kit::ScrollDelta::Pixels(point(px(0.), px(3000.))), cx);
        window.render_frame(cx);
        window.scroll("settings-content", gpui_kit::ScrollDelta::Pixels(point(px(0.), px(-1600.))), cx);
        window.render_frame(cx);
        window.click("reset-appearance", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_entity(&view, |view, _| assert_eq!(view.preferences.contrast(), 0));
    assert_eq!(crate::desktop::preferences::Preferences::load().contrast(), 0);
}

#[gpui_kit::test]
fn contrast_survives_theme_change_and_restores_from_preferences(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        let slider = view.read(cx).contrast_slider.clone();
        slider.update(cx, |slider, cx| slider.set_value(67.0, window, cx));
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.apply_theme("solarized-light".into(), window, cx));
        assert_eq!(view.read(cx).preferences.contrast(), 67);
        let changed_foreground = cx.theme().colors.foreground;
        let _mode = super::super::themes::apply_with_contrast("solarized-light", 0, Some(window), cx);
        assert_ne!(changed_foreground, cx.theme().colors.foreground);
        view.update(cx, |view, cx| view.load_preferences(window, cx));
        assert_eq!(cx.theme().colors.foreground, changed_foreground);
        assert_eq!(crate::desktop::preferences::Preferences::load().contrast(), 67);
    })
    .unwrap();
}

#[gpui_kit::test]
fn translucent_sidebar_toggle_persists_and_reset_restores_opaque(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.open_settings(window, cx));
        window.render_frame(cx);
        window.scroll("settings-content", gpui_kit::ScrollDelta::Pixels(point(px(0.), px(-1600.))), cx);
        window.render_frame(cx);
        assert_eq!(window.find("translucent-sidebar").checked(), Some(false));
        window.click("translucent-sidebar", cx);
        window.render_frame(cx);
        assert_eq!(window.find("translucent-sidebar").checked(), Some(true));
        assert!(view.read(cx).preferences.translucent_sidebar);
        assert!(crate::desktop::preferences::Preferences::load().translucent_sidebar);
        let root = window.root::<gpui_kit::base::Root>().flatten().unwrap();
        assert!(root.update(cx, |root, _| root.style().background.is_some()));
        window.click("reset-appearance", cx);
        window.render_frame(cx);
        assert!(!view.read(cx).preferences.translucent_sidebar);
        assert!(!crate::desktop::preferences::Preferences::load().translucent_sidebar);
        assert!(root.update(cx, |root, _| root.style().background.is_none()));
    })
    .unwrap();
}

#[gpui_kit::test]
fn partial_history_warnings_remain_visible_when_no_rows_were_restored(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    view.update(cx, |view, cx| {
        view.inspector_open = true;
        view.restored_review_history_incomplete = true;
        view.restored_activity_history_incomplete = true;
        view.inspector_tab = crate::desktop::inspector::InspectorTab::Activity;
        cx.notify();
    });

    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let warning = window.find("activity-history-warning");
        assert_eq!(warning.role(), Some(gpui_kit::Role::Alert));
        assert_eq!(warning.label(), Some("Saved tool history is partial. Some output may be missing or shortened."));

        view.update(cx, |view, _| view.inspector_tab = crate::desktop::inspector::InspectorTab::Output);
        window.render_frame(cx);
        let warning = window.find("activity-history-warning");
        assert_eq!(warning.role(), Some(gpui_kit::Role::Alert));

        view.update(cx, |view, _| view.inspector_tab = crate::desktop::inspector::InspectorTab::Changes);
        window.render_frame(cx);
        let warning = window.find("review-history-warning");
        assert_eq!(warning.role(), Some(gpui_kit::Role::Alert));
        assert_eq!(warning.label(), Some("Saved change history is partial. Some historical patches are unavailable."));

        view.update(cx, |view, _| view.reset_session_state());
        window.render_frame(cx);
        assert!(window.try_find("review-history-warning").is_none());
        assert!(window.try_find("activity-history-warning").is_none());
    })
    .unwrap();
}

#[gpui_kit::test]
fn untracked_review_renders_added_empty_and_spaced_paths_and_reports_omissions(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(8);
    let patch = "diff --git a/empty.rs b/empty.rs\nnew file mode 100644\ndiff --git a/new file.rs b/new file.rs\nnew file mode 100644\n--- /dev/null\n+++ b/new file.rs\n@@ -0,0 +1 @@\n+new content\n";
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.service_info = Some(vtcode_desktop_protocol::InitializeResult {
                protocol: vtcode_desktop_protocol::PROTOCOL_VERSION,
                workspace: "/workspace".into(), server_version: "test".into(),
                slash_commands: Vec::new(),
            });
            view.open_workspace_review(window, cx);
        });
        window.render_frame(cx);
        window.click("workspace-review-include-untracked", cx);
        window.click("refresh-workspace-review", cx);
        let BackendCommand::Request { params, .. } = requests.try_recv().unwrap() else { panic!("review request") };
        assert_eq!(params["includeUntracked"], true);
        view.update(cx, |view, cx| {
            view.handle_backend_event(BackendEvent::Response {
                method: vtcode_desktop_protocol::method::WORKSPACE_REVIEW.into(), error: None,
                result: Some(json!({"requestId": params["requestId"], "workspace":"/workspace", "capturedAt":"now", "includeUntracked":true, "patch":patch, "truncated":false, "note":"Known omissions: 0 untracked files"})),
            }, cx);
            let viewer = view.workspace_review.as_ref().unwrap().viewer.as_ref().unwrap().read(cx);
            assert_eq!(viewer.files().len(), 2);
            assert_eq!(viewer.files()[0].path().as_str(), "empty.rs");
            assert_eq!(viewer.files()[1].path().as_str(), "new file.rs");
            assert_eq!(viewer.files()[1].additions(), 1);
        });
        window.render_frame(cx);
        assert_eq!(window.find("workspace-review-summary").label(), Some("2 files changed · +1 · −0"));
        window.click("copy-workspace-review", cx);
        assert_eq!(cx.read_from_clipboard().unwrap().text().as_deref(), Some(patch));
        window.click("refresh-workspace-review", cx);
        let BackendCommand::Request { params, .. } = requests.try_recv().unwrap() else { panic!("review request") };
        view.update(cx, |view, cx| view.handle_backend_event(BackendEvent::Response {
            method: vtcode_desktop_protocol::method::WORKSPACE_REVIEW.into(), error: None,
            result: Some(json!({"requestId": params["requestId"], "workspace":"/workspace", "capturedAt":"now", "includeUntracked":true, "patch":"", "truncated":true, "note":"Known omissions: 1 untracked file"})),
        }, cx));
        window.render_frame(cx);
        assert_eq!(window.find("workspace-review-clean").label(), Some("No previewable changes"));
        assert_eq!(window.find("copy-workspace-review").disabled(), Some(true));
    }).unwrap();
}

#[gpui_kit::test]
fn working_tree_review_is_independent_refreshable_and_rejects_stale_results(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update(|cx| cx.set_reduce_motion(true));
    let (commands, mut requests) = tokio::sync::mpsc::channel(8);
    let patch = "diff --git a/a.rs b/a.rs\n--- a/a.rs\n+++ b/a.rs\n@@ -1 +1 @@\n-old\n+new\ndiff --git a/b.rs b/b.rs\n--- a/b.rs\n+++ b/b.rs\n@@ -1 +1 @@\n-before\n+after\n";
    let snapshot = |request_id: &str, workspace: &str| BackendEvent::WorkspaceReviewResponse {
        request_id: request_id.into(),
        error: None,
        result: Some(
            json!({"requestId": request_id, "workspace": workspace, "capturedAt": "2026-10-24T00:00:00Z", "patch": patch, "truncated": false, "note": "Tracked changes against HEAD"}),
        ),
    };
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.backend = Some(BackendHandle { commands });
            view.service_info = Some(vtcode_desktop_protocol::InitializeResult {
                protocol: vtcode_desktop_protocol::PROTOCOL_VERSION,
                workspace: "/workspace".into(),
                server_version: "test".into(),
                slash_commands: Vec::new(),
            });
            view.inspector_open = true;
        });
        window.render_frame(cx);
        window.click("review-working-tree", cx);
        window.render_frame(cx);
        assert!(requests.try_recv().is_err(), "opening review must wait for an explicit capture");
        window.click("workspace-review-include-untracked", cx);
        window.render_frame(cx);
        view.update(cx, |view, cx| {
            view.workspace_review
                .as_ref()
                .unwrap()
                .scope_input
                .update(cx, |input, cx| input.set_value("src", window, cx));
        });
        window.click("refresh-workspace-review", cx);
        window.render_frame(cx);
        assert!(view.read(cx).current_route() == Route::Review);
        assert_eq!(window.find("diff-review-panel").label(), Some("Working tree review"));
        assert_eq!(window.find("refresh-workspace-review").disabled(), Some(true));
        let loading = window.find("workspace-review-loading");
        assert_eq!(loading.role(), Some(gpui_kit::Role::Status));
        assert_eq!(loading.label(), Some("Reading Git changes"));
        assert!(window.find("workspace-review-loading-label").visible());
        cx.set_reduce_motion(false);
        window.render_frame(cx);
        assert!(window.find("workspace-review-loading-label").visible());
        cx.set_reduce_motion(true);
        window.render_frame(cx);
        let BackendCommand::Request { method, params } = requests.try_recv().unwrap() else {
            panic!("review request")
        };
        assert_eq!(method, vtcode_desktop_protocol::method::WORKSPACE_REVIEW);
        assert_eq!(params["scope"], "src");
        assert_eq!(params["includeUntracked"], true);
        assert_eq!(window.find("workspace-review-include-untracked").disabled(), Some(true));
        let mut request_id = params["requestId"].as_str().unwrap().to_string();
        view.update(cx, |view, cx| {
            view.handle_backend_event(BackendEvent::WorkspaceReviewResponse {
                request_id: "stale".into(),
                result: None,
                error: Some("stale failure".into()),
            }, cx);
            assert!(view.workspace_review.as_ref().unwrap().pending);
            view.handle_backend_event(BackendEvent::WorkspaceReviewResponse {
                request_id: "stale".into(),
                result: Some(json!({ "malformed": true })),
                error: None,
            }, cx);
            assert!(view.workspace_review.as_ref().unwrap().pending);
            view.handle_backend_event(snapshot("stale", "/workspace"), cx);
            assert!(view.workspace_review.as_ref().unwrap().pending);
            view.handle_backend_event(snapshot(&request_id, "/other-workspace"), cx);
            assert!(!view.workspace_review.as_ref().unwrap().pending);
            view.refresh_workspace_review(cx);
            let BackendCommand::Request { params, .. } = requests.try_recv().unwrap() else {
                panic!("review retry request")
            };
            request_id = params["requestId"].as_str().unwrap().to_string();
            view.handle_backend_event(snapshot(&request_id, "/workspace"), cx);
            let review = view.workspace_review.as_ref().unwrap();
            assert!(!review.pending);
            assert_eq!(review.viewer.as_ref().unwrap().read(cx).files().len(), 2);
            assert!(view.transcript.is_empty());
            assert!(view.changes.is_empty());
            assert!(view.selected_session_id.is_none());
            // Session projection invalidation cannot close a Git review.
            view.change_viewer_dirty = true;
            view.refresh_selected_change(cx);
            assert!(view.current_route() == Route::Review);
        });
        window.render_frame(cx);
        let layout_options = window.find("workspace-review-layout-options");
        assert_eq!(layout_options.role(), Some(gpui_kit::Role::Group));
        assert_eq!(layout_options.label(), Some("Diff layout"));
        assert_eq!(window.find("workspace-review-unified").checked(), Some(true));
        assert_eq!(window.find("workspace-review-split").checked(), Some(false));
        let panel = window.find("diff-review-panel").bounds();
        let diff = window.find("workspace-review-diff").bounds();
        assert!(diff.size.width > px(400.), "workspace review keeps a usable diff width: {diff:?} in {panel:?}");
        assert!(
            diff.right() >= panel.right() - px(60.),
            "diff view fills the panel width: {diff:?} in {panel:?}"
        );
        let summary = window.find("workspace-review-summary");
        assert_eq!(summary.role(), Some(gpui_kit::Role::Status));
        assert_eq!(summary.label(), Some("2 files changed · +2 · −2"));
        assert_eq!(window.find("workspace-review-current-file").label(), Some("File 1 of 2 · a.rs"));
        focus_element_by_tab(window, "workspace-review-next-file", cx);
        assert_eq!(window.find("workspace-review-current-file").label(), Some("File 1 of 2 · a.rs"));
        window.within("diff-review-panel").press("alt-f7", cx);
        assert_eq!(window.find("workspace-review-current-file").label(), Some("File 2 of 2 · b.rs"));
        window.within("diff-review-panel").press("alt-shift-f7", cx);
        assert_eq!(window.find("workspace-review-current-file").label(), Some("File 1 of 2 · a.rs"));
        window.click("workspace-review-previous-file", cx);
        window.render_frame(cx);
        assert_eq!(window.find("workspace-review-current-file").label(), Some("File 2 of 2 · b.rs"));
        window.click("workspace-review-next-file", cx);
        window.render_frame(cx);
        assert_eq!(window.find("workspace-review-current-file").label(), Some("File 1 of 2 · a.rs"));
        window.click("workspace-review-next-file", cx);
        window.click("workspace-review-split", cx);
        window.render_frame(cx);
        assert_eq!(window.find("workspace-review-unified").checked(), Some(false));
        assert_eq!(window.find("workspace-review-split").checked(), Some(true));
        assert_eq!(
            view.read(cx)
                .workspace_review
                .as_ref()
                .unwrap()
                .viewer
                .as_ref()
                .unwrap()
                .read(cx)
                .mode(),
            gpui_kit::component::diff::DiffMode::Split
        );
        window.click("copy-workspace-review", cx);
        assert_eq!(cx.read_from_clipboard().unwrap().text().as_deref(), Some(patch));
        window.click("refresh-workspace-review", cx);
        let BackendCommand::Request { params, .. } = requests.try_recv().unwrap() else {
            panic!("refresh request")
        };
        let refreshed_id = params["requestId"].as_str().unwrap().to_string();
        assert_ne!(refreshed_id, request_id);
        view.update(cx, |view, cx| {
            view.handle_backend_event(snapshot(&request_id, "/workspace"), cx);
            assert!(view.workspace_review.as_ref().unwrap().pending);
            // Reordering must preserve the path, not its old index.
            let (first, second) = patch.split_once("diff --git a/b.rs").unwrap();
            let reordered = format!("diff --git a/b.rs{second}{first}");
            let mut response = snapshot(&refreshed_id, "/workspace");
            if let BackendEvent::WorkspaceReviewResponse { result: Some(result), .. } = &mut response {
                result["patch"] = json!(reordered);
            }
            view.handle_backend_event(response, cx);
            assert_eq!(
                view.workspace_review.as_ref().unwrap().viewer.as_ref().unwrap().read(cx).mode(),
                gpui_kit::component::diff::DiffMode::Split
            );
        });
        window.click("copy-workspace-review", cx);
        let reordered = "diff --git a/b.rs b/b.rs\n--- a/b.rs\n+++ b/b.rs\n@@ -1 +1 @@\n-before\n+after\ndiff --git a/a.rs b/a.rs\n--- a/a.rs\n+++ b/a.rs\n@@ -1 +1 @@\n-old\n+new\n";
        assert_eq!(cx.read_from_clipboard().unwrap().text().as_deref(), Some(reordered));
        window.render_frame(cx);
        assert_eq!(window.find("workspace-review-current-file").label(), Some("File 1 of 2 · b.rs"));
        window.click("refresh-workspace-review", cx);
        let BackendCommand::Request { params, .. } = requests.try_recv().unwrap() else {
            panic!("refresh request")
        };
        view.update(cx, |view, cx| {
            let mut response = snapshot(params["requestId"].as_str().unwrap(), "/workspace");
            if let BackendEvent::WorkspaceReviewResponse { result: Some(result), .. } = &mut response {
                result["patch"] = json!(patch.split_once("diff --git a/b.rs").unwrap().0);
            }
            view.handle_backend_event(response, cx);
        });
        window.render_frame(cx);
        assert_eq!(window.find("workspace-review-current-file").label(), Some("File 1 of 1 · a.rs"));
        assert_eq!(window.find("workspace-review-previous-file").disabled(), Some(true));
        assert_eq!(window.find("workspace-review-next-file").disabled(), Some(true));
        for _ in 0..10 {
            window.press("tab", cx);
            window.render_frame(cx);
        }
        assert!(view.read(cx).review_focus.contains_focused(window, cx));
        window.press("escape", cx);
        window.render_frame(cx);
        assert!(view.read(cx).current_route() != Route::Review);
        assert!(!view.read(cx).review_focus.contains_focused(window, cx));
        assert!(view.read(cx).workspace_review.is_none());
        view.update(cx, |view, cx| {
            view.handle_backend_event(snapshot(&refreshed_id, "/workspace"), cx);
            assert!(view.workspace_review.is_none());
        });
    })
    .unwrap();
}

#[gpui_kit::test]
fn working_tree_review_distinguishes_partial_clean_and_unavailable_snapshots(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, _requests) = tokio::sync::mpsc::channel(8);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.service_info = Some(vtcode_desktop_protocol::InitializeResult {
                protocol: vtcode_desktop_protocol::PROTOCOL_VERSION,
                workspace: "/workspace".into(),
                server_version: "test".into(),
                slash_commands: Vec::new(),
            });
            view.open_workspace_review(window, cx);
            view.refresh_workspace_review(cx);
            let id = view.workspace_review.as_ref().unwrap().request_id.clone();
            view.accept_workspace_review(
                vtcode_desktop_protocol::WorkspaceReviewResult {
                    include_untracked: false,
                    scope: String::new(),
                    request_id: id,
                    workspace: "/workspace".into(),
                    captured_at: "now".into(),
                    patch: Some("partial patch\n".into()),
                    truncated: true,
                    note: "Tracked changes".into(),
                },
                cx,
            );
            assert!(view.workspace_review.as_ref().unwrap().viewer.is_none());
        });
        window.render_frame(cx);
        assert!(window.find("workspace-review-raw").bounds().size.height > px(300.));
        assert_eq!(window.find("workspace-review-split").disabled(), Some(true));
        window.click("copy-workspace-review", cx);
        assert_eq!(cx.read_from_clipboard().unwrap().text().as_deref(), Some("partial patch\n"));
        for patch in [Some(String::new()), None] {
            view.update(cx, |view, cx| {
                view.refresh_workspace_review(cx);
                let id = view.workspace_review.as_ref().unwrap().request_id.clone();
                view.accept_workspace_review(
                    vtcode_desktop_protocol::WorkspaceReviewResult {
                        include_untracked: false,
                        scope: String::new(),
                        request_id: id,
                        workspace: "/workspace".into(),
                        captured_at: "now".into(),
                        patch: patch.clone(),
                        truncated: false,
                        note: "Scope or unavailable reason".into(),
                    },
                    cx,
                );
                assert_eq!(view.workspace_review.as_ref().unwrap().snapshot.as_ref().unwrap().patch, patch);
            });
            window.render_frame(cx);
            assert_eq!(window.find("copy-workspace-review").disabled(), Some(true));
            if patch.is_some() {
                assert_eq!(window.find("workspace-review-clean").label(), Some("No tracked changes"));
            } else {
                assert_eq!(window.find("workspace-review-unavailable").label(), Some("Git review unavailable"));
            }
        }
        view.update(cx, |view, cx| {
            view.refresh_workspace_review(cx);
            view.handle_backend_event(BackendEvent::Exited { status: Some(1), authentication_required: false }, cx);
            assert!(!view.workspace_review.as_ref().unwrap().pending);
            view.reset_session_state();
            assert!(view.workspace_review.is_none());
            assert!(view.current_route() != Route::Review);
        });
    })
    .unwrap();
}

#[gpui_kit::test]
fn working_tree_review_searches_changed_files_without_closing_the_review(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, _requests) = tokio::sync::mpsc::channel(4);
    let patch = "diff --git a/a.rs b/a.rs\n--- a/a.rs\n+++ b/a.rs\n@@ -1 +1 @@\n-old\n+new\ndiff --git a/b.rs b/b.rs\n--- a/b.rs\n+++ b/b.rs\n@@ -1 +1 @@\n-before\n+after\n";
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.service_info = Some(vtcode_desktop_protocol::InitializeResult {
                protocol: vtcode_desktop_protocol::PROTOCOL_VERSION,
                workspace: "/workspace".into(),
                server_version: "test".into(),
                slash_commands: Vec::new(),
            });
            view.open_workspace_review(window, cx);
            view.refresh_workspace_review(cx);
            let request_id = view.workspace_review.as_ref().unwrap().request_id.clone();
            view.accept_workspace_review(
                vtcode_desktop_protocol::WorkspaceReviewResult {
                    include_untracked: false,
                    scope: String::new(),
                    request_id,
                    workspace: "/workspace".into(),
                    captured_at: "now".into(),
                    patch: Some(patch.into()),
                    truncated: false,
                    note: "Tracked changes against HEAD".into(),
                },
                cx,
            );
        });
        window.render_frame(cx);
        assert_eq!(window.find("workspace-review-current-file").label(), Some("File 1 of 2 · a.rs"));

        window.click("workspace-review-current-file", cx);
        window.render_frame(cx);
        view.update(cx, |view, cx| view.focus_palette_state(window, cx));
        window.render_frame(cx);
        assert!(view.read(cx).current_route() == Route::Review);
        assert_eq!(window.find("command-palette").label(), Some("Choose changed file"));
        window.within("command-palette").input("b.rs", cx);
        window.render_frame(cx);
        let target_action = super::super::PaletteAction::SelectWorkspaceReviewFile("b.rs".into());
        assert!(window.try_find(palette_action_id(&target_action)).is_some());

        window.within("command-palette").press("escape", cx);
        window.render_frame(cx);
        assert!(view.read(cx).palette.is_some(), "Escape clears the search before dismissing the picker");
        assert!(view.read(cx).palette_query.is_empty());
        window.within("command-palette").press("escape", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(view.read(cx).palette.is_none());
        assert!(
            view.read(cx).current_route() == Route::Review,
            "Escape dismisses the picker before the underlying review"
        );
        assert_eq!(window.find("workspace-review-current-file").label(), Some("File 1 of 2 · a.rs"));

        window.click("workspace-review-current-file", cx);
        window.render_frame(cx);
        view.update(cx, |view, cx| view.focus_palette_state(window, cx));
        window.render_frame(cx);
        window.within("command-palette").input("b.rs", cx);
        window.render_frame(cx);
        window.within("command-palette").press("enter", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let review = view.read(cx);
        assert!(review.current_route() == Route::Review);
        assert!(review.palette.is_none());
        assert_eq!(review.workspace_review.as_ref().unwrap().selected_file.as_deref(), Some("b.rs"));
        assert_eq!(window.find("workspace-review-current-file").label(), Some("File 2 of 2 · b.rs"));
    })
    .unwrap();
}

#[gpui_kit::test]
fn working_tree_queue_failure_preserves_agent_and_draft_and_allows_retry(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(1);
    commands
        .try_send(BackendCommand::Request { method: "occupied".into(), params: json!({}) })
        .unwrap();
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.service_info = Some(vtcode_desktop_protocol::InitializeResult {
                protocol: vtcode_desktop_protocol::PROTOCOL_VERSION,
                workspace: "/workspace".into(),
                server_version: "test".into(),
                slash_commands: Vec::new(),
            });
            view.running = true;
            view.composer
                .update(cx, |input, cx| input.set_value("Keep this draft", window, cx));
            view.open_workspace_review(window, cx);
            view.refresh_workspace_review(cx);
            assert!(!view.workspace_review.as_ref().unwrap().pending);
            assert!(view.running);
            assert_eq!(view.composer.read(cx).value().to_string(), "Keep this draft");
        });
        window.render_frame(cx);
        assert_eq!(window.find("workspace-review-unavailable").label(), Some("Git review unavailable"));
        let _occupied = requests.try_recv().unwrap();
        window.click("refresh-workspace-review", cx);
        assert!(view.read(cx).workspace_review.as_ref().unwrap().pending);
        assert!(requests.try_recv().is_ok());
        assert!(view.read(cx).running);
    })
    .unwrap();
}

#[gpui_kit::test]
fn transcript_accessibility_exposes_content_and_distinguishes_tool_actions(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.transcript.push(TranscriptItem::new(
                "answer".into(),
                "assistant".into(),
                "VT Code".into(),
                "Tests passed: **7**. Run `cargo test`.".into(),
            ));
            view.transcript.push(TranscriptItem::new(
                "check".into(),
                "tool".into(),
                "cargo check".into(),
                "Finished successfully".into(),
            ));
        });
        window.render_frame(cx);
        assert_eq!(window.find("transcript-row-answer").role(), None);
        assert_eq!(window.find("transcript-row-check").role(), Some(gpui_kit::Role::ListItem));
        assert_eq!(window.find("message-body-answer").role(), Some(gpui_kit::Role::Label));
        assert_eq!(window.find("message-body-answer").label(), Some("VT Code: Tests passed: 7. Run cargo test."));
        assert_eq!(
            window.find("tool-check").label(),
            Some("Show details for cargo check, status: Finished successfully")
        );
        window.click("tool-check", cx);
        window.render_frame(cx);
        assert_eq!(
            window.find("tool-check").label(),
            Some("Hide details for cargo check, status: Finished successfully")
        );
        assert_eq!(window.find("tool-output-check").label(), Some("Finished successfully"));
        window.click("tool-check", cx);
        assert!(!view.read(cx).transcript.get(1).unwrap().expanded);
        window.hover("transcript-row-answer", cx);
        window.render_frame(cx);
        window.click("copy-message-answer", cx);
        assert_eq!(cx.read_from_clipboard().unwrap().text().as_deref(), Some("Tests passed: **7**. Run `cargo test`."));
    })
    .unwrap();
}

#[gpui_kit::test]
fn appearance_accessibility_exposes_current_values_after_adjustment(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update(|cx| cx.set_reduce_motion(true));
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.open_settings(window, cx));
        window.render_frame(cx);
        assert_eq!(window.find("font-ui-value").label(), Some("UI font size: 16 pixels"));
        assert_eq!(window.find("font-code-value").label(), Some("Code font size: 14 pixels"));
        let preview_height = window.find("appearance-code-preview").bounds().size.height;
        assert!(preview_height > px(0.), "the highlighted code sample should have rendered layout");
        window.click("font-ui-1", cx);
        window.render_frame(cx);
        window.click("font-code-1", cx);
        window.render_frame(cx);
        assert_eq!(window.find("font-ui-value").label(), Some("UI font size: 17 pixels"));
        assert_eq!(window.find("font-code-value").label(), Some("Code font size: 15 pixels"));
        assert!(
            window.find("appearance-code-preview").bounds().size.height > preview_height,
            "changing code size should re-layout the syntax-highlighted preview"
        );
        assert!(window.find("choose-ui-font").label().unwrap().contains("current:"));
        assert!(window.find("choose-code-font").label().unwrap().contains("current:"));
    })
    .unwrap();
}

#[gpui_kit::test]
fn reduced_motion_setting_overrides_and_restores_the_system_value(cx: &mut TestAppContext) {
    cx.update(|cx| cx.set_reduce_motion(true));
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.preferences.reduced_motion = crate::desktop::preferences::ReducedMotionPreference::System;
            cx.set_reduce_motion(false);
            view.set_reduced_motion(crate::desktop::preferences::ReducedMotionPreference::System, cx);
            assert!(cx.reduce_motion(), "selecting System again refreshes its captured system value");
            view.open_settings(window, cx);
        });
        window.render_frame(cx);
        select_settings_category(window, 1, cx);
        window.scroll("settings-content", gpui_kit::ScrollDelta::Pixels(point(px(0.), px(-240.))), cx);
        window.render_frame(cx);
        assert_eq!(window.find("reduce-motion-system").label(), Some("Reduce motion: System (selected)"));
        assert_eq!(window.find("reduce-motion-system").checked(), Some(true));
        assert_eq!(window.find("reduce-motion-off").checked(), Some(false));

        window.click("reduce-motion-off", cx);
        window.render_frame(cx);
        assert!(!cx.reduce_motion());
        assert_eq!(window.find("reduce-motion-off").label(), Some("Reduce motion: Off (selected)"));
        assert_eq!(window.find("reduce-motion-off").checked(), Some(true));
        assert_eq!(window.find("reduce-motion-system").checked(), Some(false));
        assert_eq!(
            crate::desktop::preferences::Preferences::load().reduced_motion,
            crate::desktop::preferences::ReducedMotionPreference::Off
        );

        window.click("reduce-motion-system", cx);
        window.render_frame(cx);
        assert!(cx.reduce_motion(), "System restores the system value captured before overrides");
        assert_eq!(window.find("reduce-motion-system").label(), Some("Reduce motion: System (selected)"));
        assert_eq!(
            crate::desktop::preferences::Preferences::load().reduced_motion,
            crate::desktop::preferences::ReducedMotionPreference::System
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn navigation_accessibility_distinguishes_current_workspace_and_thread(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let alpha_id = opaque_path_element_id("workspace", "/project/alpha");
    let beta_id = opaque_path_element_id("workspace", "/project/beta");
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.workspace = Some(PathBuf::from("/project/alpha"));
            view.preferences.recent_workspaces = vec!["/project/alpha".into(), "/project/beta".into()];
            view.sessions = ["First", "Second"]
                .into_iter()
                .map(|title| vtcode_desktop_protocol::SessionSummary {
                    session_id: title.into(),
                    title: Some(title.into()),
                    workspace: "/project/alpha".into(),
                    completed: false,
                    resumable: true,
                    size_bytes: 0,
                    updated_at: None,
                })
                .collect();
            view.selected_session_id = Some("Second".into());
        });
        window.render_frame(cx);
        let navigation = window.find("primary-navigation");
        assert_eq!(navigation.role(), Some(gpui_kit::Role::Navigation));
        assert_eq!(navigation.label(), Some("VT Code navigation"));
        assert_eq!(window.find("project-thread-sidebar").role(), Some(gpui_kit::Role::Group));
        assert_eq!(window.find("project-thread-sidebar").label(), Some("Workspace, projects, and recent threads"));
        assert_eq!(window.find(alpha_id.clone()).label(), Some("alpha (current workspace)"));
        assert_eq!(window.find(beta_id.clone()).label(), Some("beta"));
        assert_eq!(window.find("session-First").label(), Some("First"));
        assert_eq!(window.find("session-Second").label(), Some("Second (current thread)"));
        view.update(cx, |view, _| {
            view.selected_session_id = Some("First".into());
            view.sessions.reverse();
            view.preferences.recent_workspaces.reverse();
        });
        window.render_frame(cx);
        assert_eq!(window.find(alpha_id.clone()).label(), Some("alpha (current workspace)"));
        assert_eq!(window.find(beta_id.clone()).label(), Some("beta"));
        assert_eq!(window.find("session-First").label(), Some("First (current thread)"));
        assert_eq!(window.find("session-Second").label(), Some("Second"));
    })
    .unwrap();
}

#[gpui_kit::test]
fn navigation_position_switch_moves_the_expanded_panel_and_persists(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        // Routes unmount the workspace, so measure the initial layout before opening settings.
        view.update(cx, |view, _| {
            view.sidebar_open = true;
        });
        window.render_frame(cx);
        let initial_navigation = window.find("primary-navigation").bounds();
        let initial_workspace = window.find("workspace-surface").bounds();
        let initial_panel = window.find("project-thread-sidebar").bounds();
        let initial_rail_button = window.find("nav-home").bounds();
        assert!(initial_workspace.right() < initial_navigation.left());
        assert!(initial_panel.right() < initial_rail_button.left());

        view.update(cx, |view, cx| view.open_settings(window, cx));
        window.render_frame(cx);
        window.scroll("settings-content", gpui_kit::ScrollDelta::Pixels(point(px(0.), px(-1600.))), cx);
        window.render_frame(cx);

        assert!(view.read(cx).preferences.navigation_on_right(), "navigation defaults to the right");
        assert_eq!(window.find("navigation-on-right").checked(), Some(true));

        window.click("navigation-on-right", cx);
        window.render_frame(cx);
        assert_eq!(window.find("navigation-on-right").checked(), Some(false));
        assert_eq!(view.read(cx).preferences.navigation_on_right, Some(false));

        window.press("escape", cx);
        window.render_frame(cx);
        let navigation = window.find("primary-navigation").bounds();
        let workspace = window.find("workspace-surface").bounds();
        let panel = window.find("project-thread-sidebar").bounds();
        let rail_button = window.find("nav-home").bounds();
        assert!(navigation.right() < workspace.left());
        assert!(rail_button.right() < panel.left());
        assert!(!crate::desktop::preferences::Preferences::load().navigation_on_right());
    })
    .unwrap();
}

#[gpui_kit::test]
fn hidden_navigation_moves_its_actions_to_the_header_and_can_be_restored(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    view.update(cx, |view, _| {
        view.preferences.navigation_visible = Some(false);
        view.sidebar_open = true;
        view.inspector_open = false;
    });

    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("navigation-rail").is_none());
        assert!(window.try_find("project-thread-sidebar").is_none());
        assert!(window.try_find("top-new-thread").is_some());
        assert!(window.try_find("top-utilities").is_some());
        assert_eq!(window.find("toggle-sidebar").label(), Some("Show navigation"));
        // The zen toggle keeps its single trailing header slot even while
        // both side panels are collapsed. The full-height sidebar narrows the
        // toolbar when restored, so the slot is its trailing inset, not its
        // absolute origin.
        assert_eq!(window.find("zen-mode-toggle").label(), Some("Enter Zen mode"));
        let collapsed_bar = window.find("title-bar").bounds();
        let collapsed_slot = window.find("zen-mode-toggle").bounds();

        window.click("toggle-sidebar", cx);
        window.render_frame(cx);
        assert!(view.read(cx).preferences.navigation_visible());
        assert!(window.try_find("navigation-rail").is_some());
        assert!(window.try_find("project-thread-sidebar").is_some());
        assert_eq!(window.find("zen-mode-toggle").label(), Some("Enter Zen mode"));
        let restored_bar = window.find("title-bar").bounds();
        let restored_slot = window.find("zen-mode-toggle").bounds();
        assert_eq!(
            restored_bar.right() - restored_slot.right(),
            collapsed_bar.right() - collapsed_slot.right(),
            "restoring navigation must keep the zen toggle in its trailing slot"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn zen_mode_is_active_on_fresh_launch(cx: &mut TestAppContext) {
    // No suite baseline: this observes the constructor default untouched.
    let (window, view) = open_desktop_sized_with_preferences_path(cx, 1280., 900., isolate_preferences(), false);

    cx.update_window(window, |_, window, cx| {
        assert!(view.read(cx).zen_mode);
        window.render_frame(cx);
        // Navigation defaults to visible, so its absence proves zen.
        assert!(window.try_find("navigation-rail").is_none());
        assert!(window.try_find("welcome").is_none());
        // No workspace yet: the picker row stays as the only visible path
        // to choose one.
        assert!(window.try_find("composer-context").is_some());
        assert!(window.try_find("send-prompt").is_some());

        view.update(cx, |view, _| view.workspace = Some(std::env::temp_dir()));
        window.render_frame(cx);
        assert!(view.read(cx).zen_mode);
        assert!(window.try_find("welcome").is_none());
        assert!(window.try_find("composer-context").is_none());
    })
    .unwrap();
}

#[gpui_kit::test]
fn long_thread_titles_truncate_within_the_header_in_narrow_windows(cx: &mut TestAppContext) {
    let workspace = tempfile::TempDir::new().expect("temporary workspace");
    let (window, view) = open_desktop_sized(cx, 700., 800.);
    view.update(cx, |view, _| {
        view.workspace = Some(workspace.path().to_path_buf());
        view.sessions = vec![vtcode_desktop_protocol::SessionSummary {
            session_id: "narrow-thread".into(),
            title: Some("A very long thread title that must truncate within the header row".into()),
            workspace: workspace.path().display().to_string(),
            completed: false,
            resumable: true,
            size_bytes: 0,
            updated_at: None,
        }];
        view.selected_session_id = Some("narrow-thread".into());
        view.transcript.push(TranscriptItem::new(
            "narrow-message".into(),
            "assistant".into(),
            "VT Code".into(),
            "Conversation remains visible.".into(),
        ));
        view.preferences.navigation_visible = Some(false);
        view.inspector_open = false;
    });

    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let title = window.find("thread-title").bounds();
        let header = window.find("thread-header").bounds();
        assert!(
            title.right() <= header.right(),
            "long titles must truncate inside the header: {title:?} vs {header:?}"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn zen_toggle_floats_above_the_edge_to_edge_surface_and_redocks_on_exit(cx: &mut TestAppContext) {
    let workspace = tempfile::TempDir::new().expect("temporary workspace");
    let (window, view) = open_desktop(cx);
    view.update(cx, |view, _| {
        view.workspace = Some(workspace.path().to_path_buf());
        view.sessions = vec![vtcode_desktop_protocol::SessionSummary {
            session_id: "collapsed-thread".into(),
            title: Some("Collapsed panels thread".into()),
            workspace: workspace.path().display().to_string(),
            completed: false,
            resumable: true,
            size_bytes: 0,
            updated_at: None,
        }];
        view.selected_session_id = Some("collapsed-thread".into());
        view.transcript.push(TranscriptItem::new(
            "collapsed-message".into(),
            "assistant".into(),
            "VT Code".into(),
            "Conversation remains visible.".into(),
        ));
        view.preferences.navigation_visible = Some(false);
        view.inspector_open = false;
    });

    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("navigation-rail").is_none());
        assert_eq!(window.find("zen-mode-toggle").label(), Some("Enter Zen mode"));
        let entered = window.find("zen-mode-toggle").bounds();
        assert!(
            entered.size.width >= px(24.) && entered.size.height >= px(24.),
            "zen toggle must keep a comfortable pointer target: {entered:?}"
        );
        assert!(window.find("title-bar").bounds().contains(&entered.center()), "zen toggle sits in the title bar");

        window.press("ctrl-cmd-z", cx);
        window.render_frame(cx);
        assert_eq!(window.find("zen-mode-toggle").label(), Some("Exit Zen mode"));
        assert!(window.try_find("title-bar").is_none(), "zen drops the toolbar");
        let zen = window.find("zen-mode-toggle").bounds();
        assert_eq!(zen.size, entered.size, "enter and exit share one frame");
        let strip = window.find("zen-drag-strip").bounds();
        assert!(strip.contains(&zen.center()), "the exit floats above the drag strip");
        let surface = window.find("workspace-surface").bounds();
        assert_eq!(surface.origin, point(px(0.), px(40.)), "zen content starts below the drag strip");

        window.click("zen-mode-toggle", cx);
        window.render_frame(cx);
        assert_eq!(window.find("zen-mode-toggle").label(), Some("Enter Zen mode"));
        let restored = window.find("zen-mode-toggle").bounds();
        assert_eq!(restored.right(), entered.right());
        assert_eq!(restored.top(), entered.top());
        assert!(window.find("title-bar").bounds().contains(&restored.center()), "exit redocks the toggle");
        assert!(window.try_find("thread-header").is_some());
    })
    .unwrap();
}

#[gpui_kit::test]
fn rail_utilities_hold_their_lane_when_the_rail_expands(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    view.update(cx, |view, _| {
        view.workspace = Some(std::env::temp_dir());
        view.sidebar_open = false;
    });

    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("nav-new-thread").is_some());
        let collapsed = window.find("nav-utilities").bounds();

        view.update(cx, |view, _| view.sidebar_open = true);
        window.render_frame(cx);
        assert!(window.try_find("project-thread-sidebar").is_some());
        assert!(window.try_find("nav-new-thread").is_none());
        let expanded = window.find("nav-utilities").bounds();
        assert_eq!(expanded.origin, collapsed.origin, "expanding the rail must not move Utilities");
        assert_eq!(expanded.size, collapsed.size, "the reserved lane keeps one frame");
    })
    .unwrap();
}
