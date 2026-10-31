use super::*;

#[gpui_kit::test]
fn activity_summary_legend_lists_nonzero_statuses_and_rows_stay_compact(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.inspector_open = true;
            view.inspector_tab = crate::desktop::inspector::InspectorTab::Activity;
            // Asymmetric counts across every rendered branch: running (spinner),
            // done x2, failed (output action), other; interrupted stays zero.
            for (id, title, text) in [
                ("read", "Read Cargo.toml", "completed\nmanifest"),
                ("build", "cargo check", "failed (exit 1)\nstructured details"),
                ("retry", "cargo check", "in progress\nretrying"),
                ("verify", "Verify lockfile", "verification requested\nwaiting"),
                ("lint", "cargo clippy", "completed\nwarnings"),
            ] {
                view.transcript
                    .push(TranscriptItem::new(id.into(), "tool".into(), title.into(), text.into()));
            }
            view.transcript.set_output("build", Some("compiler error: missing name".into()));
        });
        window.render_frame(cx);

        assert_eq!(
            window.find("activity-status-chart").label(),
            Some("Recent activity statuses: 1 running · 2 done · 1 failed · 0 interrupted · 1 other")
        );
        assert_eq!(window.find("activity-summary-running").label(), Some("Running: 1"));
        assert_eq!(window.find("activity-summary-done").label(), Some("Done: 2"));
        assert_eq!(window.find("activity-summary-failed").label(), Some("Failed: 1"));
        assert_eq!(window.find("activity-summary-other").label(), Some("Other: 1"));
        assert!(
            window.try_find("activity-summary-interrupted").is_none(),
            "zero-count statuses stay out of the legend"
        );
        assert_eq!(window.find("activity-build").bounds().size.height, px(60.));
        assert_eq!(window.find("activity-output-build").label(), Some("View output for cargo check"));
        assert!(window.try_find("activity-output-read").is_none());
    })
    .unwrap();
}
