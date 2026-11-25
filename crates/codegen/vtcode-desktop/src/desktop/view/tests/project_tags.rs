use super::*;
use gpui_kit::Modifiers;

fn seed_projects(view: &Entity<DesktopView>, cx: &mut gpui_kit::App, entries: &[(&str, &[&str])]) {
    view.update(cx, |view, cx| {
        view.preferences.recent_workspaces = entries.iter().map(|(path, _)| (*path).to_owned()).collect();
        view.preferences.workspace_tags.workspaces.clear();
        view.preferences.workspace_tags.active.clear();
        for (path, tags) in entries {
            let _ = view
                .preferences
                .workspace_tags
                .set_workspace_tags(path, tags.iter().map(|tag| (*tag).to_owned()).collect());
        }
        cx.notify();
    });
}

fn session(id: &str, workspace: &str) -> vtcode_desktop_protocol::SessionSummary {
    vtcode_desktop_protocol::SessionSummary {
        session_id: id.into(),
        title: Some(format!("Thread {id}")),
        workspace: workspace.into(),
        completed: true,
        resumable: true,
        size_bytes: 0,
        updated_at: Some(20),
    }
}

#[gpui_kit::test]
fn tag_trigger_labels_the_active_context_and_popover_lists_counts(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        seed_projects(
            &view,
            cx,
            &[
                ("/work/alpha", &["work", "infra"]),
                ("/work/beta", &["infra"]),
                ("/home/x", &[]),
            ],
        );
        view.update(cx, |view, cx| view.toggle_tag_filter("work", false, window, cx));
        view.update(cx, |view, cx| view.toggle_tag_filter("infra", true, window, cx));
        window.render_frame(cx);
        assert_eq!(view.read(cx).tag_active_label(), "work +1");
        let trigger = window.find("project-tag-filter");
        let label = trigger.label().unwrap_or_default();
        assert!(label.contains("work +1"), "trigger shows the active context: {label}");

        window.click("project-tag-filter", cx);
        window.render_frame(cx);
        assert!(window.try_find("tag-filter-work").is_some());
        assert!(window.try_find("tag-filter-infra").is_some());
        let infra_row = window.find("tag-filter-infra");
        let infra = infra_row.label().unwrap_or_default();
        assert!(infra.contains("2"), "popover shows the project count: {infra}");
    })
    .unwrap();
}

#[gpui_kit::test]
fn plain_click_selects_one_context_and_shift_click_combines(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        seed_projects(&view, cx, &[("/work/alpha", &["work"]), ("/work/beta", &["infra"])]);
        window.render_frame(cx);
        window.click("project-tag-filter", cx);
        window.render_frame(cx);
        window.click("tag-filter-work", cx);
        window.render_frame(cx);
        assert_eq!(view.read(cx).preferences.workspace_tags.active, ["work".to_owned()]);
        assert_eq!(view.read(cx).visible_recent_workspaces(), ["/work/alpha".to_owned()]);

        // The popover stays open so a second context can be combined immediately.
        window.click_with_modifiers("tag-filter-infra", Modifiers::shift(), cx);
        window.render_frame(cx);
        assert_eq!(view.read(cx).preferences.workspace_tags.active, ["work".to_owned(), "infra".to_owned()]);
        assert_eq!(view.read(cx).visible_recent_workspaces().len(), 2);

        // A plain click narrows the combined context to one tag; clicking the
        // sole active tag again clears it.
        window.click("tag-filter-work", cx);
        window.render_frame(cx);
        assert_eq!(view.read(cx).preferences.workspace_tags.active, ["work".to_owned()]);
        window.click("tag-filter-work", cx);
        window.render_frame(cx);
        assert!(view.read(cx).preferences.workspace_tags.active.is_empty());
        assert_eq!(view.read(cx).visible_recent_workspaces().len(), 2);
    })
    .unwrap();
}

#[gpui_kit::test]
fn tag_filter_hides_listed_threads_without_dropping_history(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        seed_projects(&view, cx, &[("/work/alpha", &["work"]), ("/work/beta", &["infra"])]);
        view.update(cx, |view, _| {
            view.sessions = vec![
                session("alpha-thread", "/work/alpha"),
                session("beta-thread", "/work/beta"),
            ];
        });
        view.update(cx, |view, cx| view.toggle_tag_filter("work", false, window, cx));
        window.render_frame(cx);

        let filtered = view.read(cx).context_sessions();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].session_id, "alpha-thread");
        assert_eq!(view.read(cx).sessions.len(), 2, "raw history stays intact");
        assert!(window.try_find("session-row-beta-thread").is_none());
        assert!(window.try_find("session-row-alpha-thread").is_some());
    })
    .unwrap();
}

#[gpui_kit::test]
fn new_workspace_inherits_only_a_single_active_tag(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        seed_projects(&view, cx, &[("/work/alpha", &["work"]), ("/work/beta", &["infra"])]);
        view.update(cx, |view, cx| view.toggle_tag_filter("work", false, window, cx));
        view.update(cx, |view, cx| view.prepare_workspace(Path::new("/work/new"), cx));
        assert_eq!(view.read(cx).preferences.workspace_tags.workspaces["/work/new"], ["work"]);

        view.update(cx, |view, cx| view.toggle_tag_filter("infra", true, window, cx));
        view.update(cx, |view, cx| view.prepare_workspace(Path::new("/work/second"), cx));
        assert!(
            !view.read(cx).preferences.workspace_tags.workspaces.contains_key("/work/second"),
            "an ambiguous context never auto-tags"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn tag_pick_moves_only_an_untouched_new_thread_page(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        seed_projects(&view, cx, &[("/work/alpha", &["work"]), ("/work/beta", &["infra"])]);
        view.update(cx, |view, _| {
            view.workspace = Some(PathBuf::from("/work/beta"));
        });
        // An untouched landing page moves into the selected context.
        view.update(cx, |view, cx| view.toggle_tag_filter("work", false, window, cx));
        assert!(view.read(cx).workspace_selection_pending, "an untouched page arms the workspace switch");

        // An open conversation blocks it: the filter applies in place.
        view.update(cx, |view, _| {
            view.workspace_selection_pending = false;
            view.selected_session_id = Some("open-thread".to_owned());
        });
        view.update(cx, |view, cx| view.toggle_tag_filter("infra", false, window, cx));
        assert!(!view.read(cx).workspace_selection_pending, "an open thread is never displaced by a tag pick");
    })
    .unwrap();
}

#[gpui_kit::test]
fn evicted_workspaces_leave_no_phantom_tags_or_filters(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, _, cx| {
        seed_projects(&view, cx, &[("/work/alpha", &["work"]), ("/work/beta", &["infra"])]);
        view.update(cx, |view, _| {
            // Two active tags suppress per-project auto-tagging below.
            view.preferences.workspace_tags.active = vec!["work".to_owned(), "infra".to_owned()];
        });
        for index in 0..9 {
            let path = format!("/work/project-{index}");
            view.update(cx, |view, cx| view.prepare_workspace(Path::new(&path), cx));
        }
        let state = &view.read(cx).preferences.workspace_tags;
        assert!(!state.workspaces.contains_key("/work/alpha"), "evicted project tags are dropped");
        assert!(!state.workspaces.contains_key("/work/beta"), "the other evicted project is dropped too");
        assert_eq!(state.active, ["work".to_owned()], "the evicted-only 'infra' tag is pruned");
        assert!(
            state.active.iter().all(|tag| state.all_tags().contains_key(tag)),
            "every remaining active tag belongs to a recent project"
        );
        assert!(!view.read(cx).visible_recent_workspaces().is_empty(), "the pruned filter still matches projects");
    })
    .unwrap();
}

#[gpui_kit::test]
fn tag_manager_edits_tags_and_escape_restores_focus(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        seed_projects(&view, cx, &[("/work/alpha", &["work"])]);
        window.render_frame(cx);
        window.click("project-tag-filter", cx);
        window.render_frame(cx);
        window.click("tag-manage-open", cx);
        window.render_frame(cx);
        assert!(window.try_find("tag-manager-dialog").is_some());
        assert!(view.read(cx).tag_manager_focus.is_focused(window));

        window.click(opaque_path_element_id("tag-manager-edit", "/work/alpha"), cx);
        window.render_frame(cx);
        assert!(window.try_find("tag-manager-add").is_some());

        window.press("escape", cx);
        window.render_frame(cx);
        assert!(window.try_find("tag-manager-dialog").is_none(), "Escape dismisses the topmost layer");
        assert!(!view.read(cx).tag_manager_open);
    })
    .unwrap();
}

#[gpui_kit::test]
fn tag_section_collapses_and_radio_rows_select_a_context(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        seed_projects(&view, cx, &[("/work/alpha", &["dev"]), ("/work/beta", &["infra"])]);
        window.render_frame(cx);
        assert!(window.try_find("tag-section-toggle").is_some());
        assert!(window.try_find("tag-section-dev").is_some());
        assert!(window.try_find("tag-section-infra").is_some());

        window.click("tag-section-toggle", cx);
        window.render_frame(cx);
        assert!(window.try_find("tag-section-dev").is_none(), "collapsed section hides its rows");

        window.click("tag-section-toggle", cx);
        window.render_frame(cx);
        window.click("tag-section-dev", cx);
        window.render_frame(cx);
        assert_eq!(view.read(cx).preferences.workspace_tags.active, ["dev".to_owned()]);
        assert_eq!(view.read(cx).visible_recent_workspaces(), ["/work/alpha".to_owned()]);
    })
    .unwrap();
}

#[gpui_kit::test]
fn all_tags_row_opens_the_searchable_multi_select_picker(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        seed_projects(&view, cx, &[("/work/alpha", &["dev"]), ("/work/beta", &["infra"])]);
        window.render_frame(cx);
        window.click("tag-section-all", cx);
        window.render_frame(cx);
        assert!(view.read(cx).tag_all_open, "the section row opens its own picker");
        assert!(!view.read(cx).tag_filter_open, "the compact picker stays closed");
        assert!(window.try_find("tag-filter-dev").is_some());
        assert!(window.try_find("tag-filter-infra").is_some());

        window.click("tag-filter-dev", cx);
        window.render_frame(cx);
        assert_eq!(view.read(cx).preferences.workspace_tags.active, ["dev".to_owned()]);

        window.click_with_modifiers("tag-filter-infra", Modifiers::shift(), cx);
        window.render_frame(cx);
        assert_eq!(
            view.read(cx).preferences.workspace_tags.active,
            ["dev".to_owned(), "infra".to_owned()],
            "Shift+click from the section picker combines contexts"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn section_picker_closes_before_the_manager_or_on_collapse(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        seed_projects(&view, cx, &[("/work/alpha", &["dev"])]);
        window.render_frame(cx);
        window.click("tag-section-all", cx);
        window.render_frame(cx);
        assert!(view.read(cx).tag_all_open);

        window.click("tag-manage-open", cx);
        window.render_frame(cx);
        assert!(view.read(cx).tag_manager_open);
        assert!(!view.read(cx).tag_all_open, "a popover must not paint above the modal scrim");
        assert!(window.try_find("tag-filter-list").is_none());

        window.press("escape", cx);
        window.render_frame(cx);
        window.click("tag-section-all", cx);
        window.render_frame(cx);
        assert!(view.read(cx).tag_all_open);
        view.update(cx, |view, cx| {
            view.tag_search
                .update(cx, |input, cx| input.set_value("stale query", window, cx));
        });
        window.click("tag-section-toggle", cx);
        window.render_frame(cx);
        assert!(!view.read(cx).tag_all_open, "folding the section retires its picker");
        assert_eq!(view.read(cx).tag_search.read(cx).value(), "", "a prop-driven close resets the query");
    })
    .unwrap();
}

#[gpui_kit::test]
fn section_picker_dismisses_with_escape(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        seed_projects(&view, cx, &[("/work/alpha", &["dev"])]);
        window.render_frame(cx);
        window.click("tag-section-all", cx);
        window.render_frame(cx);
        assert!(view.read(cx).tag_all_open);
        window.press("escape", cx);
        window.render_frame(cx);
        assert!(!view.read(cx).tag_all_open, "Escape dismisses the picker");
    })
    .unwrap();
}

#[gpui_kit::test]
fn tag_section_caps_quick_rows_and_keeps_the_full_picker(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        let tags = ["t0", "t1", "t2", "t3", "t4", "t5", "t6", "t7", "t8", "t9"];
        seed_projects(&view, cx, &[("/work/alpha", &tags)]);
        window.render_frame(cx);
        for tag in tags.iter().take(8) {
            assert!(window.try_find(format!("tag-section-{tag}")).is_some(), "{tag} is a quick row");
        }
        // An active tag outside the alphabetical window is promoted into it.
        view.update(cx, |view, _| {
            view.preferences.workspace_tags.active = vec!["t9".to_owned()];
        });
        window.render_frame(cx);
        assert!(window.try_find("tag-section-t9").is_some(), "the active tag stays visible");
        assert!(window.try_find("tag-section-t7").is_none(), "the cap still applies");

        view.update(cx, |view, _| {
            view.preferences.workspace_tags.active.clear();
        });
        window.render_frame(cx);
        assert!(window.try_find("tag-section-t8").is_none(), "the section must not starve the thread list");
        assert!(window.try_find("tag-section-all").is_some());

        window.click("tag-section-all", cx);
        window.render_frame(cx);
        assert!(window.try_find("tag-filter-t9").is_some(), "the full picker still lists every tag");
    })
    .unwrap();
}

#[gpui_kit::test]
fn reselecting_a_radio_row_keeps_the_context(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        seed_projects(&view, cx, &[("/work/alpha", &["dev"]), ("/work/beta", &["infra"])]);
        window.render_frame(cx);
        window.click("tag-section-dev", cx);
        window.render_frame(cx);
        assert_eq!(view.read(cx).preferences.workspace_tags.active, ["dev".to_owned()]);

        window.click("tag-section-dev", cx);
        window.render_frame(cx);
        assert_eq!(
            view.read(cx).preferences.workspace_tags.active,
            ["dev".to_owned()],
            "re-clicking the selected radio must not clear the context"
        );

        window.click("tag-section-infra", cx);
        window.render_frame(cx);
        assert_eq!(
            view.read(cx).preferences.workspace_tags.active,
            ["infra".to_owned()],
            "choosing another radio replaces the single context"
        );
    })
    .unwrap();
}
