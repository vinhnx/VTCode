use super::*;

#[gpui_kit::test]
fn activity_groups_preserve_commentary_order_and_expand_live_output(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            for (id, role, title, text) in [
                ("intro", "assistant", "VT Code", "I will inspect the build."),
                ("read", "tool", "Read Cargo.toml", "completed\nmanifest"),
                ("build", "tool", "cargo check", "failed (exit 1)\nstructured details"),
                ("comment", "assistant", "VT Code", "The build needs a fix."),
                ("retry", "tool", "cargo check", "in progress\nretrying"),
            ] {
                view.transcript
                    .push(TranscriptItem::new(id.into(), role.into(), title.into(), text.into()));
            }
            view.transcript.set_output("build", Some("compiler error: missing name".into()));
        });
        window.render_frame(cx);
        assert_eq!(window.find("activity-group-read").label(), Some("Expand 2 tool actions · 1 failed"));
        assert_eq!(window.find("activity-group-read").checked(), Some(false));
        assert!(window.try_find("activity-group-running-indicator-read").is_none());
        assert_eq!(window.find("message-body-comment").label(), Some("VT Code: The build needs a fix."));
        assert_eq!(window.find("tool-retry").label(), Some("Show details for cargo check, status: in progress"));
        assert!(!view.read(cx).transcript.get(1).unwrap().activity_expanded);
        window.click("activity-group-read", cx);
        window.render_frame(cx);
        assert_eq!(window.find("activity-group-read").checked(), Some(true));
        assert_eq!(window.find("tool-read").label(), Some("Show details for Read Cargo.toml, status: completed"));
        window.click("tool-build", cx);
        window.render_frame(cx);
        assert_eq!(window.find("tool-output-build").label(), Some("compiler error: missing name"));
        window.click("copy-tool-output-build", cx);
        assert_eq!(cx.read_from_clipboard().unwrap().text().as_deref(), Some("compiler error: missing name"));
        view.update(cx, |view, _| {
            view.transcript.upsert(TranscriptItem::new(
                "read".into(),
                "tool".into(),
                "Read Cargo.toml".into(),
                "in progress\nupdated".into(),
            ));
            assert!(view.transcript.get(1).unwrap().activity_expanded, "streaming preserves group expansion");
            assert_eq!(view.transcript.activity_group(2), Some(1..3));
            assert_eq!(view.transcript.activity_group(4), None, "commentary breaks a group");
        });
        window.render_frame(cx);
        assert_eq!(window.find("activity-group-read").label(), Some("Collapse 2 tool actions · 1 running · 1 failed"));
        assert_eq!(window.find("activity-group-read").checked(), Some(true));
        assert!(window.try_find("activity-group-running-indicator-read").is_some());
        window.click("activity-group-read", cx);
        window.render_frame(cx);
        assert_eq!(window.find("activity-group-read").checked(), Some(false));
        assert!(!view.read(cx).transcript.get(1).unwrap().activity_expanded);
        assert!(view.read(cx).transcript.get(2).unwrap().expanded, "group collapse preserves detail choice");
    })
    .unwrap();
}

#[test]
fn activity_status_colors_follow_semantic_theme_tokens() {
    for palette in super::super::themes::all_themes() {
        let colors = gpui_kit::component::theme::ThemeColor {
            primary: palette.accent,
            success: palette.success,
            danger: palette.danger,
            warning: palette.warning,
            foreground: palette.foreground,
            popover: palette.background,
            ..Default::default()
        };

        for (state, expected) in [
            (ActivityState::Running, colors.primary),
            (ActivityState::Completed, colors.success),
            (ActivityState::Failed, colors.danger),
            (ActivityState::Interrupted, colors.warning),
        ] {
            assert_eq!(
                DesktopView::activity_status_color(state, colors),
                expected,
                "{} maps {state:?} to its semantic theme color",
                palette.id
            );
        }
        // Subdued rows use the brighter secondary ramp: readable everywhere,
        // at the 6:1 target wherever the palette allows it.
        let other = DesktopView::activity_status_color(ActivityState::Other, colors);
        let headroom = super::super::themes::contrast_ratio(colors.foreground, colors.popover);
        let ratio = super::super::themes::contrast_ratio(other, colors.popover);
        assert!(ratio >= 4.5, "{}: subdued status unreadable ({ratio:.2})", palette.id);
        assert!(
            ratio >= 6.0_f32.min(headroom) - 0.05,
            "{}: subdued status misses its target ({ratio:.2}, headroom {headroom:.2})",
            palette.id
        );
    }
}

#[gpui_kit::test]
fn collapsed_tool_group_keeps_following_content_close_to_its_summary(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            let mut diff = TranscriptItem::new("change".into(), "diff".into(), "README.md".into(), String::new());
            diff.diff = Some(super::super::DiffCard {
                files: vec![super::super::DiffFile {
                    path: "README.md".into(),
                    kind: super::super::DiffFileKind::Update,
                }],
                additions: Some(1),
                deletions: Some(1),
                incomplete: false,
                failed: false,
                patch: "".into(),
            });
            view.transcript.push(diff);
            for id in 0..20 {
                view.transcript.push(TranscriptItem::new(
                    format!("tool-{id}"),
                    "tool".into(),
                    format!("Command {id}").into(),
                    "completed\noutput".into(),
                ));
            }
            view.transcript.push(TranscriptItem::new(
                "done".into(),
                "assistant".into(),
                "VT Code".into(),
                "All checks completed.".into(),
            ));
        });
        window.render_frame(cx);

        let diff_row = window.find("transcript-row-change").bounds();
        let summary = window.find("activity-group-tool-0").bounds();
        let summary_label = window.find("activity-group-label-tool-0").bounds();
        let following_message = window.find("message-body-done").bounds();
        let diff_gap = summary.origin.y - (diff_row.origin.y + diff_row.size.height);
        assert!(diff_gap <= px(8.), "the diff and tool summary should remain visually grouped; got {diff_gap:?}");
        let gap = following_message.origin.y - (summary.origin.y + summary.size.height);
        assert!(gap <= px(48.), "collapsed tool rows should not reserve a row gap each; got {gap:?}");
        let label_inset = summary_label.origin.x - following_message.origin.x;
        assert!(
            label_inset >= px(0.) && label_inset <= px(48.),
            "the tool-group summary should align with transcript content; got {label_inset:?}"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn composer_model_name_uses_the_active_provider_catalog_entry(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.configuration = Some(
                serde_json::from_value(json!({
                    "provider": "provider-b", "model": "shared-id", "authenticated": true,
                    "credentialSetupCommand": "",
                }))
                .unwrap(),
            );
            view.models = [("provider-a", "Wrong provider model"), ("provider-b", "Readable Model")]
                .into_iter()
                .map(|(provider, name)| vtcode_desktop_protocol::ModelInfo {
                    provider: provider.into(),
                    id: "shared-id".into(),
                    display_name: name.into(),
                    context_window: None,
                    current: false,
                    recommended_rank: None,
                    capabilities: None,
                })
                .collect();
        });
        window.render_frame(cx);
        assert_eq!(window.find("pick-model").label(), Some("Readable Model"));
        view.update(cx, |view, _| view.models.clear());
        window.render_frame(cx);
        assert_eq!(
            window.find("pick-model").label(),
            Some("shared-id"),
            "custom IDs remain usable without catalog metadata"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn sidebar_changed_file_navigation_wraps_and_copies_the_selected_patch(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.inspector_open = true;
            let mut item = TranscriptItem::new("edits".into(), "diff".into(), "Edits".into(), String::new());
            item.diff = Some(super::super::DiffCard {
                files: Vec::new(), additions: Some(2), deletions: Some(2), incomplete: false, failed: false,
                patch: "--- a/first.rs\n+++ b/first.rs\n@@ -1 +1 @@\n-old\n+first\n--- a/second.rs\n+++ b/second.rs\n@@ -1 +1 @@\n-before\n+second\n".into(),
            });
            view.transcript.push(item);
            let mut unavailable = TranscriptItem::new("unavailable".into(), "diff".into(), "Edits".into(), String::new());
            unavailable.diff = Some(super::super::DiffCard {
                files: vec![super::super::DiffFile { path: "unavailable.rs".into(), kind: super::super::DiffFileKind::Update }],
                additions: None,
                deletions: None,
                incomplete: true,
                failed: false,
                patch: String::new(),
            });
            view.transcript.push(unavailable);
            view.changes = super::super::ChangesProjection::rebuild(&view.transcript);
            assert_eq!(view.changes.preview_count(), 2);
            view.select_change("first.rs".into(), cx);
        });
        window.render_frame(cx);
        view.update(cx, |view, cx| view.inspector_focus.focus(window, cx));
        window.within("inspector-navigation").press("alt-f7", cx);
        assert_eq!(view.read(cx).selected_change.as_deref(), Some("second.rs"));
        window.within("inspector-navigation").press("alt-shift-f7", cx);
        assert_eq!(view.read(cx).selected_change.as_deref(), Some("first.rs"));
        focus_element_by_tab(window, "next-change-file", cx);
        window.within("inspector-changes-tab").press("alt-f7", cx);
        assert_eq!(view.read(cx).selected_change.as_deref(), Some("second.rs"));
        window.within("inspector-changes-tab").press("alt-shift-f7", cx);
        assert_eq!(view.read(cx).selected_change.as_deref(), Some("first.rs"));
        window.click("next-change-file", cx);
        assert_eq!(view.read(cx).selected_change.as_deref(), Some("second.rs"));
        window.render_frame(cx);
        window.click("copy-change-patch", cx);
        let copied = cx.read_from_clipboard().unwrap().text().unwrap();
        assert!(copied.contains("+second"));
        window.click("next-change-file", cx);
        assert_eq!(view.read(cx).selected_change.as_deref(), Some("first.rs"));
        window.click("previous-change-file", cx);
        assert_eq!(view.read(cx).selected_change.as_deref(), Some("second.rs"));
        assert_eq!(window.find("open-change-file").label(), Some("Open file"));
    }).unwrap();
}

#[gpui_kit::test]
fn utilities_pane_projects_real_activity_and_routes_existing_workflows(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.inspector_open = true;
            view.inspector_tab = crate::desktop::inspector::InspectorTab::Utilities;
            view.attachments = vec![
                vtcode_desktop_protocol::TurnAttachment {
                    path: "src/main.rs".into(),
                    text: "fn main() {}".into(),
                },
                vtcode_desktop_protocol::TurnAttachment {
                    path: "src/lib.rs".into(),
                    text: "pub fn run() {}".into(),
                },
            ];
            for (id, status) in [
                ("read", "completed"),
                ("build", "failed (exit 1)"),
                ("search", "in progress"),
            ] {
                view.transcript.push(TranscriptItem::new(
                    id.into(),
                    "tool".into(),
                    id.into(),
                    format!("{status}\ndetails"),
                ));
            }
        });
        window.render_frame(cx);
        assert_eq!(window.find("utilities-pane").role(), Some(gpui_kit::Role::Group));
        assert_eq!(window.find("utilities-workspace-group").role(), Some(gpui_kit::Role::Group));
        assert!(window.find("utilities-workspace-group").label().is_some());
        assert_eq!(
            window.find("utilities-activity").label(),
            Some("Recent activity: 1 running · 1 done · 1 failed · 0 interrupted · 0 other")
        );
        assert_eq!(
            window.find("utilities-changes").label(),
            Some("Session changes: 0 files, 0 additions, 0 deletions")
        );
        assert_eq!(
            window
                .find(super::super::opaque_path_element_id("utilities-source", "src/main.rs"))
                .label(),
            Some("Open attached source: src/main.rs")
        );
        assert_eq!(
            window
                .find(super::super::opaque_path_element_id("utilities-source", "src/lib.rs"))
                .label(),
            Some("Open attached source: src/lib.rs")
        );
        view.update(cx, |view, _| view.attachments.reverse());
        window.render_frame(cx);
        assert_eq!(
            window
                .find(super::super::opaque_path_element_id("utilities-source", "src/main.rs"))
                .label(),
            Some("Open attached source: src/main.rs"),
            "source identity follows its path after reordering"
        );
        window.click("utilities-activity", cx);
        assert_eq!(view.read(cx).inspector_tab, crate::desktop::inspector::InspectorTab::Activity);
        window.render_frame(cx);
        assert_eq!(
            window.find("activity-status-chart").label(),
            Some("Recent activity statuses: 1 running · 1 done · 1 failed · 0 interrupted · 0 other")
        );
        assert!(window.try_find("activity-read").is_some(), "the virtual list renders visible activity rows");
        view.update(cx, |view, _| view.inspector_tab = crate::desktop::inspector::InspectorTab::Utilities);
        window.render_frame(cx);
        window.click("utilities-changes", cx);
        assert_eq!(view.read(cx).inspector_tab, crate::desktop::inspector::InspectorTab::Changes);
        view.update(cx, |view, _| view.inspector_tab = crate::desktop::inspector::InspectorTab::Utilities);
        window.render_frame(cx);
        window.click("utilities-browse-files", cx);
        assert_eq!(view.read(cx).inspector_tab, crate::desktop::inspector::InspectorTab::Files);
        assert_eq!(view.read(cx).attachments.len(), 2, "navigation preserves the draft sources");
    })
    .unwrap();
}

#[gpui_kit::test]
fn activity_tab_virtualizes_the_bounded_tool_history(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.inspector_open = true;
            view.inspector_tab = crate::desktop::inspector::InspectorTab::Activity;
            view.selected_session_id = Some("activity-history-thread".into());
            view.transcript.push(TranscriptItem::new(
                "activity-history-prompt".into(),
                "user".into(),
                "You".into(),
                "Review older tool actions".into(),
            ));
            for index in 0..=300 {
                view.transcript.push(TranscriptItem::new(
                    format!("tool-{index}"),
                    "tool".into(),
                    format!("tool {index}").into(),
                    "completed".into(),
                ));
            }
        });
        window.render_frame(cx);

        let list = window.find("activity-rows");
        assert_eq!(list.role(), Some(gpui_kit::Role::List));
        assert_eq!(list.label(), Some("Recent activity"));
        assert_eq!(
            window.find("activity-status-chart").label(),
            Some("Recent activity statuses: 0 running · 300 done · 0 failed · 0 interrupted · 0 other; older activity is not included")
        );
        let status = window.find("activity-history-status");
        assert_eq!(status.role(), Some(gpui_kit::Role::Status));
        assert_eq!(status.label(), Some("Showing the latest 300 tool actions."));
        let search = window.find("activity-search-history");
        assert_eq!(search.role(), Some(gpui_kit::Role::Button));
        assert_eq!(search.label(), Some("Search conversation"));

        let rendered_row_indexes = (0..=300)
            .filter(|index| window.try_find(format!("activity-tool-{index}")).is_some())
            .collect::<Vec<_>>();
        assert!(!rendered_row_indexes.is_empty(), "visible tool activity is rendered");
        assert!(rendered_row_indexes.len() < 300, "off-screen tool activity stays virtualized");
        let first_visible = rendered_row_indexes[0];
        let expected_label = format!("tool {first_visible}");
        assert_eq!(
            window.find(format!("activity-tool-{first_visible}")).label(),
            Some(expected_label.as_str()),
            "borrowing the row projection preserves its accessible label"
        );
        assert_eq!(window.find(format!("activity-tool-{first_visible}")).role(), Some(gpui_kit::Role::ListItem));

        window.click("activity-search-history", cx);
        window.render_frame(cx);
        assert_eq!(window.find("command-palette").label(), Some("Find in conversation"));
    })
    .unwrap();
}

#[gpui_kit::test]
fn running_activity_uses_the_kit_spinner_while_finished_rows_stay_static(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.inspector_tab = crate::desktop::inspector::InspectorTab::Activity;
            view.transcript.push(TranscriptItem::new(
                "running".into(),
                "tool".into(),
                "Running check".into(),
                "in progress\ncommand output".into(),
            ));
            view.transcript.push(TranscriptItem::new(
                "finished".into(),
                "tool".into(),
                "Finished check".into(),
                "completed\ncommand output".into(),
            ));
        });
        window.render_frame(cx);

        assert!(window.try_find("activity-indicator-running-spinner").is_some());
        assert!(window.try_find("activity-indicator-finished-spinner").is_none());
        assert_eq!(window.find("activity-running").label(), Some("Running check"));
    })
    .unwrap();
}

#[gpui_kit::test]
fn startup_collapses_panels_and_navigation_peek_is_transient(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        let fresh = cx.new(|cx| DesktopView::new(window, cx, ThemeMode::Dark));
        assert!(!fresh.read(cx).sidebar_open);
        assert!(!fresh.read(cx).inspector_open);
        view.update(cx, |view, cx| {
            view.preferences.inspector_open = Some(true);
            view.load_preferences(window, cx);
            view.sidebar_open = false;
            view.focus_handle.focus(window, cx);
        });
        window.render_frame(cx);
        assert!(!view.read(cx).inspector_open, "launch remains focused even after an earlier expanded inspector");
        window.hover("nav-home", cx);
        window.render_frame(cx);
        assert!(view.read(cx).navigation_peek);
        assert_eq!(window.find("nav-home").label(), Some("Projects and threads (expanded)"));
        assert_eq!(window.find("nav-home").checked(), Some(true));
        assert!(!view.read(cx).sidebar_open, "hover does not pin the sidebar");
        window.press("escape", cx);
        window.render_frame(cx);
        assert!(!view.read(cx).navigation_peek);
        assert_eq!(window.find("nav-home").label(), Some("Projects and threads"));
        assert_eq!(window.find("nav-home").checked(), Some(false));
        window.click("toggle-sidebar", cx);
        assert!(view.read(cx).sidebar_open);
        window.press("cmd-b", cx);
        assert!(!view.read(cx).sidebar_open);
    })
    .unwrap();
}

#[gpui_kit::test]
fn shortcut_settings_reject_conflicts_then_persist_and_reset(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update(|cx| cx.set_reduce_motion(true));
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.open_settings(window, cx);
        });
        window.render_frame(cx);
        select_settings_category(window, 2, cx);
        window.click("shortcut-edit-commands", cx);
        window.render_frame(cx);
        assert!(view.read(cx).shortcut_capture.is_some(), "pencil click arms capture");
        assert!(window.try_find("shortcut-capture-commands").is_some(), "armed capture mounts the pill");
        assert!(
            view.read(cx).shortcut_capture_focus.contains_focused(window, cx),
            "beginning capture focuses the pill"
        );
        window.press("cmd-n", cx);
        window.render_frame(cx);
        assert!(view.read(cx).shortcut_message.as_ref().unwrap().contains("already assigned"));
        assert!(view.read(cx).shortcut_capture.is_some(), "rejected chords keep capturing");
        assert!(crate::desktop::preferences::Preferences::load().shortcuts.is_empty());
        window.press("ctrl-shift-k", cx);
        window.render_frame(cx);
        assert!(view.read(cx).shortcut_capture.is_none());
        assert!(
            view.read(cx).shortcut_search_input.read(cx).focus_handle(cx).is_focused(window),
            "committing returns to search for the next row"
        );
        focus_element_by_tab(window, "save-shortcuts", cx);
        window.press("enter", cx);
        assert_eq!(
            crate::desktop::preferences::Preferences::load()
                .shortcuts
                .get("commands")
                .map(String::as_str),
            Some("ctrl-shift-k")
        );
        window.render_frame(cx);
        window.click("reset-shortcuts", cx);
        window.render_frame(cx);
        assert_eq!(view.read(cx).shortcut_draft.get("commands").map(String::as_str), Some("cmd-k"));
        assert_eq!(
            crate::desktop::preferences::Preferences::load()
                .shortcuts
                .get("commands")
                .map(String::as_str),
            Some("ctrl-shift-k"),
            "reset stages without saving"
        );
        window.click("cancel-shortcuts", cx);
        window.render_frame(cx);
        assert_eq!(
            view.read(cx).shortcut_draft.get("commands").map(String::as_str),
            Some("ctrl-shift-k"),
            "cancelling a staged reset keeps the saved binding"
        );
        window.click("reset-shortcuts", cx);
        window.render_frame(cx);
        window.click("save-shortcuts", cx);
        assert_eq!(
            crate::desktop::preferences::Preferences::load()
                .shortcuts
                .get("commands")
                .map(String::as_str),
            Some("cmd-k")
        );
        assert_eq!(view.read(cx).shortcut_draft.get("commands").map(String::as_str), Some("cmd-k"));
    })
    .unwrap();
}

#[gpui_kit::test]
fn settings_categories_are_keyboard_navigable_and_shortcuts_use_kit_kbd(cx: &mut TestAppContext) {
    cx.update(|cx| cx.set_reduce_motion(true));
    let (window, view) = open_desktop_sized(cx, 450., 900.);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.open_settings(window, cx));
        window.render_frame(cx);
        assert_eq!(window.find("settings-panel").role(), Some(gpui_kit::Role::Dialog));
        assert_eq!(window.find("settings-navigation").label(), Some("Settings categories"));
        assert!(window.try_find("appearance-settings").is_some());
        assert_eq!(window.find("appearance-visual-style-group").role(), Some(gpui_kit::Role::Group));
        assert_eq!(window.find("appearance-visual-style-group").label(), Some("Visual style"));
        assert_eq!(window.find("appearance-typography-group").role(), Some(gpui_kit::Role::Group));

        window.press("right", cx);
        window.render_frame(cx);
        assert!(window.try_find("accessibility-settings").is_some());
        assert_eq!(window.find("accessibility-display-group").role(), Some(gpui_kit::Role::Group));
        window.press("end", cx);
        window.render_frame(cx);
        window.render_frame(cx);
        assert!(window.try_find("storage-settings").is_some());
        assert_eq!(window.within("settings-tabs").find(6usize).selected(), Some(true));
        let scroll = &view.read(cx).settings_tabs_scroll;
        let offset = scroll.offset().x;
        assert!(
            horizontal_scroll_item_is_visible(scroll, 6),
            "End reveals the selected settings category; scroll offset={offset:?}"
        );
        assert!(offset < px(0.), "narrow settings tabs must scroll to the selected category");

        select_settings_category(window, 2, cx);
        assert!(window.try_find("keyboard-settings").is_some());
        assert_eq!(window.find("keyboard-shortcuts-group").role(), Some(gpui_kit::Role::Group));
        let expected_shortcut_label = format!(
            "Shortcut for Search commands: {}",
            gpui_kit::component::kbd::Kbd::format(&gpui_kit::Keystroke::parse("cmd-k").unwrap())
        );
        let shortcut = window.find("shortcut-badge-commands");
        assert_eq!(shortcut.role(), Some(gpui_kit::Role::Label));
        assert_eq!(shortcut.label(), Some(expected_shortcut_label.as_str()));
        assert!(window.try_find("shortcut-capture-commands").is_none(), "capture stays hidden until edit");

        window.click("shortcut-edit-commands", cx);
        window.render_frame(cx);
        assert!(window.try_find("shortcut-capture-commands").is_some());
        window.press("escape", cx);
        window.render_frame(cx);
        assert!(window.try_find("shortcut-capture-commands").is_none());
        assert!(window.try_find("keyboard-settings").is_some(), "escape cancels capture without dismissing settings");
        assert!(
            view.read(cx).shortcut_search_input.read(cx).focus_handle(cx).is_focused(window),
            "cancelling returns to search for the next row"
        );
        assert_eq!(
            view.read(cx).shortcut_draft.get("commands").map(String::as_str),
            Some("cmd-k"),
            "cancelling capture keeps the staged binding"
        );

        view.update(cx, |view, cx| {
            view.shortcut_search_input
                .update(cx, |input, cx| input.set_value("model picker", window, cx));
        });
        window.render_frame(cx);
        window.click("shortcut-remove-model", cx);
        window.render_frame(cx);
        assert_eq!(window.find("shortcut-badge-model").label(), Some("Shortcut for Choose model: not set"));
        window.click("cancel-shortcuts", cx);
        window.render_frame(cx);
        assert_eq!(
            crate::desktop::preferences::Preferences::load().shortcuts.get("model"),
            None,
            "cancelling discards unsaved bindings"
        );
        view.update(cx, |view, cx| {
            view.shortcut_search_input
                .update(cx, |input, cx| input.set_value("", window, cx));
        });
        window.render_frame(cx);
        assert_eq!(window.find("shortcut-badge-commands").label(), Some(expected_shortcut_label.as_str()));
    })
    .unwrap();
}

#[gpui_kit::test]
fn settings_sheet_and_category_tabs_fit_a_narrow_window(cx: &mut TestAppContext) {
    let (window, view) = open_desktop_sized(cx, 600., 700.);
    cx.update(|cx| cx.set_reduce_motion(true));
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        view.update(cx, |view, cx| view.open_settings(window, cx));
        window.render_frame(cx);
    })
    .unwrap();
    cx.update_window(window, |_, window, cx| {
        let sheet = window.find("sheet-content").bounds();
        let panel = window.find("settings-panel").bounds();
        let viewport = window.viewport_size();
        assert!(
            sheet.origin.x >= px(0.) && sheet.origin.y >= px(0.),
            "settings sheet starts outside viewport: sheet={sheet:?}, viewport={viewport:?}"
        );
        assert!(
            sheet.origin.x + sheet.size.width <= viewport.width,
            "settings sheet exceeds viewport width: sheet={sheet:?}, viewport={viewport:?}"
        );
        assert!(
            sheet.origin.y + sheet.size.height <= viewport.height,
            "settings sheet exceeds viewport height: sheet={sheet:?}, viewport={viewport:?}"
        );
        assert!(panel.size.width <= sheet.size.width);

        select_settings_category(window, 2, cx);
        window.render_frame(cx);
        let navigation = window.find("settings-navigation").bounds();
        let content = window.find("settings-content").bounds();
        assert!(navigation.origin.x >= panel.origin.x);
        assert!(navigation.origin.x + navigation.size.width <= panel.origin.x + panel.size.width);
        assert!(content.origin.x >= panel.origin.x);
        assert!(content.origin.x + content.size.width <= panel.origin.x + panel.size.width);
        assert!(window.try_find("keyboard-settings").is_some());
    })
    .unwrap();
}

#[gpui_kit::test]
fn settings_tab_bar_stays_fixed_while_settings_content_scrolls(cx: &mut TestAppContext) {
    let (window, view) = open_desktop_sized(cx, 600., 560.);
    cx.update(|cx| cx.set_reduce_motion(true));
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.preferences.set_ui_font_size(view.theme_mode, 24);
            view.apply_typography(cx);
            view.open_settings(window, cx);
        });
        window.render_frame(cx);
        window.render_frame(cx);
        select_settings_category(window, 2, cx);
        window.render_frame(cx);

        let sheet = window.find("sheet-content").bounds();
        let panel = window.find("settings-panel").bounds();
        assert!(
            panel.origin.y + panel.size.height <= sheet.origin.y + sheet.size.height,
            "large UI typography should keep settings within the sheet: panel={panel:?}, sheet={sheet:?}"
        );
        let navigation_before = window.find("settings-navigation").bounds();
        window.scroll("settings-content", gpui_kit::ScrollDelta::Pixels(point(px(0.), px(-240.))), cx);
        window.render_frame(cx);

        let offset = view.read(cx).settings_scroll.offset().y;
        assert!(offset != px(0.), "settings content should scroll independently");
        assert_eq!(window.find("settings-navigation").bounds(), navigation_before);
    })
    .unwrap();
}

#[gpui_kit::test]
fn settings_sheet_tracks_window_width_after_open(cx: &mut TestAppContext) {
    let (window, view) = open_desktop_sized(cx, 1_100., 900.);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.open_settings(window, cx));
        window.render_frame(cx);
    })
    .unwrap();

    cx.simulate_window_resize(window, size(px(600.), px(700.)));
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let sheet = window.find("sheet-content").bounds();
        let viewport = window.viewport_size();
        assert_eq!(sheet.size.width, viewport.width - px(24.), "sheet width follows the resized viewport");
    })
    .unwrap();
}

#[test]
fn native_titlebar_is_transparent_and_kit_owns_dragging() {
    let options = super::super::desktop_window_options();
    assert!(options.app_owns_titlebar_drag, "avoid duplicate AppKit/Kit double-click handling");
    assert_eq!(options.window_background, gpui_kit::WindowBackgroundAppearance::Opaque);
    let titlebar = options.titlebar.unwrap();
    assert!(titlebar.appears_transparent);
    assert_eq!(titlebar.traffic_light_position, Some(point(px(12.), px(17.))));
}

#[gpui_kit::test]
fn header_overflow_menu_is_accessible_and_preserves_the_draft(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        window.press("cmd-l", cx);
        window.input("Keep this draft", cx);
        window.render_frame(cx);
        assert_eq!(window.find("title-bar").bounds().size.height, px(48.));
        assert!(window.find("toggle-sidebar").bounds().origin.x >= px(80.), "reserve native traffic-light space");
        assert_eq!(window.find("toggle-inspector").label(), Some("Hide panel"));
        assert_eq!(window.find("search-commands").role(), Some(gpui_kit::Role::Button));
        assert_eq!(window.find_all("open-settings").len(), 1);
        assert!(window.find_all("nav-settings").is_empty(), "Settings has one header entry");
        assert!(window.find_all("navigation-overflow").is_empty(), "the duplicate rail menu is omitted");
        assert!(window.find_all("toggle-theme").is_empty());
        focus_element_by_tab(window, "search-commands", cx);
        window.press("right", cx);
        window.render_frame(cx);
        assert_eq!(window.find("toggle-inspector").focused(), Some(true));
        window.press("right", cx);
        window.render_frame(cx);
        assert_eq!(window.find("open-settings").focused(), Some(true));
        assert_eq!(window.find_all("header-overflow").len(), 1, "one global overflow trigger remains");
        assert_eq!(window.find("header-overflow").label(), Some("Window actions"));
        window.click("header-overflow", cx);
        window.render_frame(cx);
        assert_eq!(
            window
                .within("dropdown-menu:Name(\"header-overflow\")")
                .find("popup-menu")
                .role(),
            Some(gpui_kit::Role::Menu)
        );
        window.press("escape", cx);
        window.render_frame(cx);
        assert_eq!(view.read(cx).composer.read(cx).value(), "Keep this draft");
    })
    .unwrap();
}

#[gpui_kit::test]
fn compact_composer_caps_width_grows_with_text_and_preserves_controls(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.sidebar_open = false;
            view.inspector_open = false;
        });
        window.render_frame(cx);
        let compact = window.find("message-composer").bounds();
        assert!(compact.size.width <= px(960.));
        assert!(compact.size.height <= px(120.), "empty composer height: {:?}", compact.size.height);
        assert_eq!(window.find("send-prompt").disabled(), Some(true));
        assert!(window.find_all("composer-settings").is_empty(), "global settings has one header entry");
        assert_eq!(window.find_all("open-settings").len(), 1);
        assert_eq!(window.find("composer-policy").label(), Some("Execution policy: Permissions unavailable"));
        view.update(cx, |view, cx| {
            view.composer
                .update(cx, |input, cx| input.set_value("first\nsecond\nthird\nfourth\nfifth", window, cx));
        });
        window.render_frame(cx);
        let expanded = window.find("message-composer").bounds();
        assert!(expanded.size.height > compact.size.height, "multiline drafts should grow instead of clipping");
        assert!(expanded.size.height <= px(240.));
        assert_eq!(view.read(cx).composer.read(cx).value(), "first\nsecond\nthird\nfourth\nfifth");
        let long_draft = (1..=30).map(|line| format!("Draft line {line}")).collect::<Vec<_>>().join("\n");
        view.update(cx, |view, cx| {
            view.composer
                .update(cx, |input, cx| input.set_value(long_draft.clone(), window, cx));
        });
        window.render_frame(cx);
        let capped = window.find("message-composer").bounds();
        assert!(capped.size.height <= px(240.), "long drafts must scroll after six rows: {capped:?}");
        assert_eq!(view.read(cx).composer.read(cx).value(), long_draft);
        view.update(cx, |view, cx| {
            view.composer
                .update(cx, |input, cx| input.set_value("first\nsecond\nthird\nfourth\nfifth", window, cx));
        });
        window.render_frame(cx);
        window.click("open-settings", cx);
        assert!(view.read(cx).settings_open);
        assert_eq!(view.read(cx).composer.read(cx).value(), "first\nsecond\nthird\nfourth\nfifth");
    })
    .unwrap();
}

#[gpui_kit::test]
fn composer_accepts_keyboard_input_with_accent_focus_ring_disabled(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        window.click("message-composer", cx);
        window.input("Focused draft", cx);
        window.render_frame(cx);

        let composer_focus = view.read(cx).composer.read(cx).focus_handle(cx).clone();
        assert!(composer_focus.contains_focused(window, cx));
        assert_eq!(view.read(cx).composer.read(cx).value(), "Focused draft");
        assert!(window.find("message-composer-frame").bounds().size.height > px(0.));
    })
    .unwrap();
}

#[gpui_kit::test]
fn narrow_composer_wraps_actions_and_bounds_long_model_names(cx: &mut TestAppContext) {
    let (window, view) = open_desktop_sized(cx, 600., 800.);
    cx.update_window(window, |_, _window, cx| {
        view.update(cx, |view, _| {
            view.sidebar_open = false;
            view.inspector_open = false;
            view.configuration = Some(serde_json::from_value(json!({
                "provider": "local", "model": "a-very-long-model-name-with-a-large-context-window-and-extra-provider-description",
                "authenticated": true, "credentialSetupCommand": "", "sandbox": "Ask before protected actions",
            })).unwrap());
        });
    }).unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.viewport_size().width, px(600.));
        let panel = window.find("message-composer").bounds();
        for id in ["pick-model", "send-prompt", "attach-files"] {
            let control = window.find(id).bounds();
            assert!(
                control.origin.x >= panel.origin.x
                    && control.origin.x + control.size.width <= panel.origin.x + panel.size.width,
                "{id} must fit horizontally: control={control:?}, panel={panel:?}"
            );
            assert!(
                control.origin.y + control.size.height <= panel.origin.y + panel.size.height,
                "{id} must fit vertically"
            );
        }
        assert_eq!(window.find("composer-policy").label(), Some("Execution policy: Ask before protected actions"));
    })
    .unwrap();
}

#[gpui_kit::test]
fn connections_terminal_row_copies_the_resume_command(cx: &mut TestAppContext) {
    // Pure builder oracles: a selected session yields a directory-independent
    // resume command, no selection yields a plain terminal session.
    assert_eq!(DesktopView::terminal_command_for_session(Some("sess-1")), "vtcode --resume sess-1 --all");
    assert_eq!(DesktopView::terminal_command_for_session(None), "vtcode");

    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.selected_session_id = Some("sess-1".to_string());
            view.open_settings(window, cx);
        });
        window.render_frame(cx);
        select_settings_category(window, 4, cx);
        window.render_frame(cx);
        assert!(window.try_find("copy-terminal-command").is_some());
        window.click("copy-terminal-command", cx);
        assert_eq!(cx.read_from_clipboard().unwrap().text().as_deref(), Some("vtcode --resume sess-1 --all"));
    })
    .unwrap();
}

#[gpui_kit::test]
fn settings_toolbar_button_closes_an_open_sheet(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update(|cx| cx.set_reduce_motion(true));
    cx.update_window(window, |_, window, cx| {
        window.click("open-settings", cx);
        window.render_frame(cx);
        assert!(view.read(cx).settings_open);
        window.click("open-settings", cx);
        window.render_frame(cx);
        assert!(!view.read(cx).settings_open);
    })
    .unwrap();
}
