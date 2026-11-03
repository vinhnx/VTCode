use super::*;
use gpui_kit::component::diff::DiffMode;
use vtcode_exec_events::{
    FileChangeItem, FileUpdateChange, ItemCompletedEvent, PatchApplyStatus, PatchChangeKind, ThreadEvent, ThreadItem,
    ThreadItemDetails,
};

fn change_notification(id: &str, path: &str, patch: &str) -> BackendEvent {
    BackendEvent::ThreadEvent(vtcode_desktop_protocol::ThreadEventNotification {
        session_id: "review-session".into(),
        turn_id: None,
        event: vtcode_exec_events::VersionedThreadEvent::new(ThreadEvent::ItemCompleted(ItemCompletedEvent {
            item: ThreadItem {
                context: None,
                id: id.into(),
                details: ThreadItemDetails::FileChange(Box::new(FileChangeItem {
                    changes: vec![FileUpdateChange { path: path.into(), kind: PatchChangeKind::Update }],
                    status: PatchApplyStatus::Completed,
                    unified_diff: Some(patch.into()),
                    diff_incomplete: Some(false),
                    additions: None,
                    deletions: None,
                })),
            },
        })),
    })
}

#[gpui_kit::test]
fn inspector_diff_toggle_persists_and_relaunch_seeds_it(cx: &mut TestAppContext) {
    const PATCH: &str = "--- a/src/app.rs\n+++ b/src/app.rs\n@@ -1 +1 @@\n-old\n+new\n";

    let prefs_path = crate::desktop::preferences::install_test_preferences_path();
    let (window, view) = open_desktop_with_preferences_path(cx, prefs_path.clone());
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.selected_session_id = Some("review-session".into());
            view.handle_backend_event(change_notification("edit", "src/app.rs", PATCH), cx);
        });
        window.render_frame(cx);
        window.click(opaque_path_element_id("change-row", "src/app.rs"), cx);
        assert_eq!(view.read(cx).change_viewer.as_ref().unwrap().read(cx).mode(), DiffMode::Unified);
        window.click("expand-change-review", cx);
        window.render_frame(cx);
        window.within("diff-review-panel").click("review-split", cx);
        window.render_frame(cx);
        assert_eq!(view.read(cx).change_viewer.as_ref().unwrap().read(cx).mode(), DiffMode::Split);
    })
    .unwrap();
    assert_eq!(
        crate::desktop::preferences::Preferences::load().diff_mode(),
        DiffMode::Split,
        "toggling the diff layout persists the choice"
    );

    // Simulate a relaunch: a fresh window seeds new viewers from the saved layout.
    let (relaunched_window, relaunched) = open_desktop_with_preferences_path(cx, prefs_path);
    cx.update_window(relaunched_window, |_, relaunched_window, cx| {
        relaunched.update(cx, |relaunched, cx| {
            relaunched.selected_session_id = Some("review-session".into());
            relaunched.handle_backend_event(change_notification("edit", "src/app.rs", PATCH), cx);
        });
        relaunched_window.render_frame(cx);
        relaunched_window.click(opaque_path_element_id("change-row", "src/app.rs"), cx);
        assert_eq!(
            relaunched.read(cx).change_viewer.as_ref().unwrap().read(cx).mode(),
            DiffMode::Split,
            "a relaunched viewer starts in the saved layout without another toggle"
        );
    })
    .unwrap();
}
