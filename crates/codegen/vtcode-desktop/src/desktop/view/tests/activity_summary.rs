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
        assert_eq!(window.find("activity-build").bounds().size.height, px(44.));
        assert_eq!(window.find("activity-output-build").label(), Some("View output: cargo check"));
        assert!(window.try_find("activity-output-read").is_none());
    })
    .unwrap();
}

#[gpui_kit::test]
fn activity_tab_leads_with_newest_and_filters_by_legend_status(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.inspector_open = true;
            view.inspector_tab = crate::desktop::inspector::InspectorTab::Activity;
            // Asymmetric statuses across distinct titles: order and filter
            // assertions cannot pass on identical rows.
            for (id, title, text) in [
                ("read", "Read Cargo.toml", "completed\nmanifest"),
                ("build", "cargo check", "failed (exit 1)\nstructured details"),
                ("retry", "cargo check", "in progress\nretrying"),
            ] {
                view.transcript
                    .push(TranscriptItem::new(id.into(), "tool".into(), title.into(), text.into()));
            }
        });
        window.render_frame(cx);

        let newest_top = window.find("activity-retry").bounds().origin.y;
        let oldest_top = window.find("activity-read").bounds().origin.y;
        assert!(newest_top < oldest_top, "the newest entry leads the list");
        assert_eq!(window.find("activity-shown-count").label(), Some("3 shown"));
        assert_eq!(window.find("activity-summary-failed").role(), Some(gpui_kit::Role::Button));
        assert_eq!(window.find("activity-summary-failed").checked(), Some(false));

        window.click("activity-summary-failed", cx);
        window.render_frame(cx);

        assert_eq!(window.find("activity-summary-failed").checked(), Some(true));
        assert_eq!(window.find("activity-shown-count").label(), Some("1 of 3 shown"));
        assert!(window.try_find("activity-build").is_some(), "matching rows stay visible");
        assert!(window.try_find("activity-read").is_none(), "other statuses leave the list");
        assert!(window.try_find("activity-retry").is_none(), "other statuses leave the list");
        assert_eq!(
            window.find("activity-status-chart").label(),
            Some("Recent activity statuses: 1 running · 1 done · 1 failed · 0 interrupted · 0 other"),
            "the legend keeps full counts while filtered"
        );

        window.click("activity-summary-failed", cx);
        window.render_frame(cx);

        assert_eq!(window.find("activity-summary-failed").checked(), Some(false));
        assert_eq!(window.find("activity-shown-count").label(), Some("3 shown"));
        assert!(window.try_find("activity-read").is_some(), "clearing the filter restores every row");
        assert!(window.try_find("activity-retry").is_some(), "clearing the filter restores every row");
    })
    .unwrap();
}

#[gpui_kit::test]
fn activity_filter_stays_clearable_when_its_status_hits_zero(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.inspector_open = true;
            view.inspector_tab = crate::desktop::inspector::InspectorTab::Activity;
            for (id, title, text) in [
                ("read", "Read Cargo.toml", "completed\nmanifest"),
                ("retry", "cargo check", "in progress\nretrying"),
            ] {
                view.transcript
                    .push(TranscriptItem::new(id.into(), "tool".into(), title.into(), text.into()));
            }
        });
        window.render_frame(cx);

        window.click("activity-summary-running", cx);
        window.render_frame(cx);
        assert!(window.try_find("activity-retry").is_some());
        assert!(window.try_find("activity-read").is_none());

        // The running row finishes while filtered: its legend entry must stay
        // so the filter never strands.
        view.update(cx, |view, _| {
            view.transcript.upsert(TranscriptItem::new(
                "retry".into(),
                "tool".into(),
                "cargo check".into(),
                "completed\ndone".into(),
            ));
        });
        window.render_frame(cx);

        assert_eq!(window.find("activity-summary-running").label(), Some("Running: 0"));
        assert_eq!(window.find("activity-summary-running").checked(), Some(true));
        assert_eq!(window.find("activity-shown-count").label(), Some("0 of 2 shown"));
        assert!(window.try_find("activity-retry").is_none());
        assert!(window.try_find("activity-read").is_none());

        window.click("activity-summary-running", cx);
        window.render_frame(cx);

        assert!(window.try_find("activity-summary-running").is_none(), "zero-count entries leave with the filter");
        assert!(window.try_find("activity-read").is_some());
        assert!(window.try_find("activity-retry").is_some());
    })
    .unwrap();
}

#[gpui_kit::test]
fn activity_tab_follows_newest_until_the_reader_scrolls_away(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.inspector_open = true;
            view.inspector_tab = crate::desktop::inspector::InspectorTab::Activity;
            for index in 0..40 {
                view.transcript.push(TranscriptItem::new(
                    format!("tool-{index}"),
                    "tool".into(),
                    format!("tool {index}").into(),
                    "completed".into(),
                ));
            }
        });
        window.render_frame(cx);

        assert!(window.try_find("activity-tool-39").is_some(), "the newest entry leads");
        assert!(window.try_find("activity-tool-0").is_none(), "older entries start offscreen");
        assert!(window.try_find("activity-jump-latest").is_none(), "no jump button while pinned to the top");

        view.read(cx).activity_scroll.scroll_to_item(39, gpui_kit::ScrollStrategy::Top);
        // The deferred scroll lands in layout; a second frame renders the
        // jump button from the settled offset.
        window.render_frame(cx);
        window.render_frame(cx);

        assert!(window.try_find("activity-tool-0").is_some(), "scrolling reaches the oldest entry");
        assert!(window.try_find("activity-tool-39").is_none(), "the head leaves the rendered range");
        assert_eq!(window.find("activity-jump-latest").label(), Some("Jump to latest activity"));

        // New entries while scrolled away must not yank the reader's offset.
        view.update(cx, |view, _| {
            view.transcript.push(TranscriptItem::new(
                "tool-40".into(),
                "tool".into(),
                "tool 40".into(),
                "in progress".into(),
            ));
        });
        window.render_frame(cx);

        assert!(window.try_find("activity-jump-latest").is_some(), "the live edge stays one click away");
        assert!(window.try_find("activity-tool-40").is_none(), "the new head does not steal the viewport");

        window.click("activity-jump-latest", cx);
        window.render_frame(cx);
        window.render_frame(cx);

        assert!(window.try_find("activity-tool-40").is_some(), "jumping resumes the live edge");
        assert!(window.try_find("activity-jump-latest").is_none(), "the button hides once pinned again");

        // New entries while pinned appear at the top without any action.
        view.update(cx, |view, _| {
            view.transcript.push(TranscriptItem::new(
                "tool-41".into(),
                "tool".into(),
                "tool 41".into(),
                "completed".into(),
            ));
        });
        window.render_frame(cx);

        assert!(window.try_find("activity-tool-41").is_some(), "pinned readers see new entries immediately");
        assert!(window.try_find("activity-jump-latest").is_none(), "following needs no jump button");
    })
    .unwrap();
}
