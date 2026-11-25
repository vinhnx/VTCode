use super::*;
use gpui_kit::Modifiers;

fn seed_projects(view: &Entity<DesktopView>, cx: &mut gpui_kit::App, entries: &[(&str, &[&str])]) {
    view.update(cx, |view, cx| {
        view.preferences.recent_workspaces = entries.iter().map(|(path, _)| (*path).to_owned()).collect();
        view.preferences.workspace_tags.workspaces.clear();
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
