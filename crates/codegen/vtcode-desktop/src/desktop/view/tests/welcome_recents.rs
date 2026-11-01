use super::*;

#[gpui_kit::test]
fn welcome_lists_recent_workspaces_for_one_click_resume(cx: &mut TestAppContext) {
    let target = tempfile::tempdir().unwrap();
    let target_path = target.path().to_path_buf();
    let target_display = target_path.display().to_string();
    let canonical_target = vtcode_commons::paths::canonicalize(&target_path).unwrap();
    let row_id = opaque_path_element_id("recent-workspace-welcome", &target_display);
    let mut recents = vec![target_display.clone()];
    recents.extend((1..=7).map(|index| format!("/tmp/vtcode-welcome-fake-{index}")));
    let (commands, mut received) = tokio::sync::mpsc::channel(1);
    let (window, view) = open_desktop(cx);
    view.update(cx, |view, cx| {
        view.workspace = None;
        view.zen_mode = true;
        view.backend = Some(BackendHandle { commands });
        cx.notify();
    });

    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        // No recents yet: only the folder picker action.
        assert!(window.try_find("zen-welcome").is_some());
        assert!(window.try_find("open-first-workspace").is_some());
        assert!(window.try_find(row_id.clone()).is_none());
        view.update(cx, |view, _| {
            view.preferences.recent_workspaces = recents.clone();
        });
        window.render_frame(cx);
        for path in &recents {
            let id = opaque_path_element_id("recent-workspace-welcome", path);
            assert!(window.try_find(id).is_some(), "all eight recents stay in the tree: {path}");
        }
        let full = window.find("welcome-recent-list").bounds().size.height;
        assert!(full <= px(240.), "a full house clamps to the cap: {full:?}");
        view.update(cx, |view, _| {
            view.preferences.recent_workspaces = vec![target_display.clone()];
        });
        window.render_frame(cx);
        let single = window.find("welcome-recent-list").bounds().size.height;
        assert!(single < full, "one row must sit below the clamped height: {single:?} vs {full:?}");
        assert_eq!(window.find(row_id.clone()).label(), Some(target_display.as_str()));
        // The shared builder serves the normal welcome too.
        view.update(cx, |view, _| view.zen_mode = false);
        window.render_frame(cx);
        assert!(window.try_find("welcome").is_some());
        assert_eq!(window.find(row_id.clone()).label(), Some(target_display.as_str()));
        window.within("welcome").click(row_id.clone(), cx);
        window.render_frame(cx);
    })
    .unwrap();
    cx.update(|cx| {
        assert!(view.read(cx).workspace_selection_pending);
    });
    cx.run_until_parked();
    cx.update(|cx| {
        assert!(!view.read(cx).workspace_selection_pending);
        assert_eq!(view.read(cx).pending_workspace.as_deref(), Some(canonical_target.as_path()));
    });
    assert!(matches!(received.try_recv(), Ok(BackendCommand::Stop)));
}
