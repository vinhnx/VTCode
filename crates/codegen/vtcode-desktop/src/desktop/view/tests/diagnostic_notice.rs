use super::*;
use crate::desktop::view::NoticeSeverity;
use vtcode_exec_events::{Diagnostic, DiagnosticSeverity, ReasoningItem, ThreadItem, ThreadItemDetails};

fn diagnostic_item(id: &str, details: &str, severity: DiagnosticSeverity) -> ThreadItem {
    ThreadItem {
        id: id.into(),
        context: None,
        details: ThreadItemDetails::Reasoning(Box::new(ReasoningItem {
            text: "Tool 'task_tracker' failed.".into(),
            stage: Some("diagnosis".into()),
            diagnostic: Some(Box::new(Diagnostic {
                severity,
                code: "tool.execution_failed".into(),
                summary: "Tool 'task_tracker' failed.".into(),
                agent_details: details.into(),
                correlation_id: id.into(),
                actions: Vec::new(),
            })),
        })),
    }
}

#[gpui_kit::test]
fn reasoning_diagnostic_projects_to_collapsible_notice(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let item =
        diagnostic_item("tool-diagnosis:task-tracker", "Likely cause: duplicate create.", DiagnosticSeverity::Warning);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.update_runtime_item(&item);
        });
        window.render_frame(cx);

        let row = view
            .read(cx)
            .transcript
            .items()
            .find(|row| row.id == "tool-diagnosis:task-tracker")
            .cloned()
            .expect("diagnostic projects a transcript row");
        assert!(row.is_notice(), "a diagnostic payload renders as a severity notice");
        assert_eq!(row.title.as_str(), "Tool 'task_tracker' failed.");
        assert!(!row.expanded, "diagnostic details start collapsed");
        assert!(row.text.contains("duplicate create"), "collapsed rows retain the details payload");

        let toggle = window.find("notice-details-tool-diagnosis:task-tracker");
        assert_eq!(toggle.role(), Some(gpui_kit::Role::Button));
        let header = window.find("notice-header-tool-diagnosis:task-tracker").bounds();
        let toggle_bounds = toggle.bounds();
        assert!(
            toggle_bounds.origin.y >= header.origin.y && toggle_bounds.bottom() <= header.bottom(),
            "details toggle shares the title row: toggle={toggle_bounds:?} header={header:?}"
        );
        assert!(
            window.try_find("notice-details-body-tool-diagnosis:task-tracker").is_none(),
            "collapsed notices hide the details body"
        );
        window.click("notice-details-tool-diagnosis:task-tracker", cx);
        window.render_frame(cx);
        assert!(
            view.read(cx)
                .transcript
                .items()
                .find(|row| row.id == "tool-diagnosis:task-tracker")
                .is_some_and(|row| row.expanded),
            "clicking Show details expands the notice"
        );
        assert!(
            window.try_find("notice-details-body-tool-diagnosis:task-tracker").is_some(),
            "expanded notices render the details body"
        );
        assert!(
            window
                .find("notice-tool-diagnosis:task-tracker")
                .label()
                .is_some_and(|label| label.contains("duplicate create")),
            "the notice exposes the details to assistive tech"
        );

        // A streaming update refreshes the payload without collapsing it.
        view.update(cx, |view, _| {
            view.update_runtime_item(&diagnostic_item(
                "tool-diagnosis:task-tracker",
                "Likely cause: duplicate create. Attempts: 2.",
                DiagnosticSeverity::Warning,
            ));
        });
        let row = view
            .read(cx)
            .transcript
            .items()
            .find(|row| row.id == "tool-diagnosis:task-tracker")
            .cloned()
            .expect("diagnostic row survives updates");
        assert!(row.expanded, "streaming updates preserve expansion");
        assert!(row.text.contains("Attempts: 2"), "streaming updates refresh the details");
    })
    .unwrap();
}

#[gpui_kit::test]
fn reasoning_diagnostic_severity_maps_to_notice_tone(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.update_runtime_item(&diagnostic_item("diag-err", "boom.", DiagnosticSeverity::Error));
            view.update_runtime_item(&diagnostic_item("diag-info", "fyi.", DiagnosticSeverity::Info));
        });
        window.render_frame(cx);

        for (id, expected) in [("diag-err", NoticeSeverity::Error), ("diag-info", NoticeSeverity::Info)] {
            let row = view
                .read(cx)
                .transcript
                .items()
                .find(|row| row.id == id)
                .cloned()
                .unwrap_or_else(|| panic!("{id} projects a transcript row"));
            assert!(row.is_notice(), "{id} renders as a severity notice");
            assert_eq!(row.notice_severity, expected, "{id} keeps its diagnostic tone");
        }
    })
    .unwrap();
}

#[gpui_kit::test]
fn reasoning_without_diagnostic_stays_assistant_message(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let item = ThreadItem {
        id: "planning-thought".into(),
        context: None,
        details: ThreadItemDetails::Reasoning(Box::new(ReasoningItem {
            text: "Considering two approaches.".into(),
            stage: Some("analysis".into()),
            diagnostic: None,
        })),
    };
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.update_runtime_item(&item);
        });
        window.render_frame(cx);

        let row = view
            .read(cx)
            .transcript
            .items()
            .find(|row| row.id == "planning-thought")
            .cloned()
            .expect("reasoning projects a transcript row");
        assert!(row.is_assistant(), "payload-less reasoning keeps its assistant rendering");
        assert!(
            window.try_find("notice-details-planning-thought").is_none(),
            "payload-less reasoning offers no details toggle"
        );
    })
    .unwrap();
}
