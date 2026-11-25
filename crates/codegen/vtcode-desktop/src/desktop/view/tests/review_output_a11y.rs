//! Accessible names for restored output-page status surfaces.

use super::*;

#[gpui_kit::test]
fn pending_historical_output_page_names_its_loading_status(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let suffix = vtcode_desktop_protocol::RESTORED_OUTPUT_TRUNCATION_SUFFIX;
    view.update(cx, |view, _| {
        view.selected_session_id = Some("archive-session".into());
        view.selected_output_id = Some("historical-output".into());
        view.inspector_open = true;
        view.inspector_tab = crate::desktop::inspector::InspectorTab::Output;
        let mut item = TranscriptItem::new(
            "historical-output".into(),
            "tool".into(),
            "Archived command".into(),
            "completed".into(),
        );
        item.output = Some(format!("valid prefix{suffix}").into());
        view.push_restored_activity(item);
        let _previous = view.restored_output_cursors.insert(
            "historical-output".into(),
            vtcode_desktop_protocol::SessionOutputCursor {
                item_id: "historical-output".into(),
                next_offset: 12,
                total_bytes: 20,
            },
        );
        view.output_page_pending = Some(super::super::PendingOutputPage {
            request_id: "output-page-1".into(),
            session_id: "archive-session".into(),
            item_id: "historical-output".into(),
            offset: 12,
        });
    });

    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let loading = window.find("output-page-loading");
        assert_eq!(loading.role(), Some(gpui_kit::Role::Status));
        assert_eq!(loading.label(), Some("Reading the next bounded page…"));
        assert!(window.try_find("output-page-error").is_none());
    })
    .unwrap();
}
