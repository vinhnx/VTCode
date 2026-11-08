use super::*;
use crate::desktop::view::NoticeSeverity;
use gpui_kit::component::theme::Theme;

#[gpui_kit::test]
fn unbreakable_text_stays_inside_transcript_bodies(cx: &mut TestAppContext) {
    let (window, view) = open_desktop_sized(cx, 1600., 900.);
    // Pinned explicitly: the wrap-height thresholds below are derived from the
    // 16px base size, so they must not depend on the ambient default.
    cx.update(|cx| {
        Theme::update(cx, |theme| {
            theme.font_size = px(16.);
        });
    });
    // Worst case on every surface: a spaceless token with no wrap opportunity,
    // mirroring `unbreakable_text_stays_inside_the_panel_and_cards`.
    let token = "x".repeat(300);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.sidebar_open = false;
            view.inspector_open = false;
            view.transcript.push(TranscriptItem::new(
                "assistant-token".into(),
                "assistant".into(),
                "VT Code".into(),
                format!("Before {token} after"),
            ));
            view.transcript.push(TranscriptItem::new(
                "user-token".into(),
                "user".into(),
                "You".into(),
                format!("Before {token} after"),
            ));
            view.transcript.push(TranscriptItem::notice(
                "notice-token".into(),
                "Heads up".into(),
                format!("Before {token} after"),
                NoticeSeverity::Info,
            ));
            view.transcript.push(TranscriptItem::new(
                "tool-token".into(),
                "tool".into(),
                "Long output".into(),
                "completed".into(),
            ));
            view.transcript
                .set_output("tool-token", Some(format!("Before {token} after").into()));
        });
        window.render_frame(cx);
        window.click("tool-tool-token", cx);
        window.render_frame(cx);

        // Right edges pin the box structure; heights prove each token wrapped
        // instead of clipping (one 16px line clears at most ~26px even at 1.6
        // leading, so anything past 40px is multiple lines on every surface).
        // Labels prove the full token survives for assistive tech and copy.
        for zen in [false, true] {
            if zen {
                window.press("ctrl-cmd-z", cx);
                window.render_frame(cx);
            }
            assert_eq!(view.read(cx).zen_mode, zen, "zen toggle must land before measuring");
            let mode = if zen { "zen" } else { "normal" };
            for id in ["assistant-token", "user-token"] {
                let column = window.find(format!("transcript-content-{id}")).bounds();
                let element = window.find(format!("message-body-{id}"));
                let body = element.bounds();
                let overhang = body.right() - column.right();
                assert!(
                    overhang <= px(1.),
                    "{mode} {id} body must not escape the conversation column: body={body:?} column={column:?}"
                );
                assert!(
                    body.size.height > px(40.),
                    "{mode} {id} token must wrap to multiple lines, not clip: height={:?}",
                    body.size.height
                );
                let label = element.label().unwrap_or_default();
                assert!(label.contains(&token), "{mode} {id} accessible text keeps the token");
            }

            let notice_column = window.find("transcript-content-notice-token").bounds();
            let notice = window.find("notice-notice-token");
            let notice_overhang = notice.bounds().right() - notice_column.right();
            assert!(
                notice_overhang <= px(1.),
                "{mode} notice must not escape the conversation column: panel={:?} column={notice_column:?}",
                notice.bounds()
            );
            // The text row alone: the panel carries a marker header, so only
            // the text height proves wrapping (one text_sm line is ~23px).
            let notice_text = window.find("notice-text-notice-token").bounds();
            assert!(
                notice_text.size.height > px(40.),
                "{mode} notice token must wrap to multiple lines, not clip: height={notice_text:?}"
            );
            let notice_label = notice.label().unwrap_or_default();
            assert!(notice_label.contains(&token), "{mode} notice accessible text keeps the token");

            let tool_column = window.find("transcript-content-tool-token").bounds();
            let tool = window.find("tool-output-tool-token");
            let tool_overhang = tool.bounds().right() - tool_column.right();
            assert!(
                tool_overhang <= px(1.),
                "{mode} tool output must not escape the conversation column: body={:?} column={tool_column:?}",
                tool.bounds()
            );
            // Mono 14px at default leading (~23px/line) plus 16px of vertical
            // padding: a clipped single line clears ~39px, the token ~106px.
            assert!(
                tool.bounds().size.height > px(60.),
                "{mode} tool token must wrap to multiple lines, not clip: height={:?}",
                tool.bounds().size.height
            );
            let tool_label = tool.label().unwrap_or_default();
            assert!(tool_label.contains(&token), "{mode} tool accessible text keeps the token");
        }
    })
    .unwrap();

    // Fenced code takes a separate Kit path (Inline in a padded block with no
    // horizontal scroll) and earns its own window: the first transcript
    // already fills its viewport, so a fifth tall row would mount below the
    // fold instead of rendering.
    let (code_window, code_view) = open_desktop_sized(cx, 1600., 900.);
    cx.update_window(code_window, |_, window, cx| {
        code_view.update(cx, |view, _| {
            view.sidebar_open = false;
            view.inspector_open = false;
            view.transcript.push(TranscriptItem::new(
                "assistant-code".into(),
                "assistant".into(),
                "VT Code".into(),
                format!("```text\n{token}\n```"),
            ));
        });
        window.render_frame(cx);
        for zen in [false, true] {
            if zen {
                window.press("ctrl-cmd-z", cx);
                window.render_frame(cx);
            }
            assert_eq!(code_view.read(cx).zen_mode, zen, "zen toggle must land before measuring");
            let mode = if zen { "zen" } else { "normal" };
            let code_column = window.find("transcript-content-assistant-code").bounds();
            let code = window.find("message-body-assistant-code");
            let overhang = code.bounds().right() - code_column.right();
            assert!(
                overhang <= px(1.),
                "{mode} code block must not escape the conversation column: body={:?} column={code_column:?}",
                code.bounds()
            );
            assert!(
                code.bounds().size.height > px(40.),
                "{mode} code token must wrap to multiple lines, not clip: height={:?}",
                code.bounds().size.height
            );
            let label = code.label().unwrap_or_default();
            assert!(label.contains(&token), "{mode} code accessible text keeps the token");
        }
    })
    .unwrap();
}

#[gpui_kit::test]
fn assistant_prose_measure_follows_the_column_minus_title_inset(cx: &mut TestAppContext) {
    let (window, view) = open_desktop_sized(cx, 1600., 900.);
    view.update(cx, |view, _| {
        view.sidebar_open = false;
        view.inspector_open = false;
        view.transcript.push(TranscriptItem::new(
            "measure-prose".into(),
            "assistant".into(),
            "VT Code".into(),
            "Prose measure probe.".into(),
        ));
    });
    // Readability contract: the assistant body is the column minus the 2rem
    // title-spine inset per side (704px/~88 chars normal, 616px/~77 chars zen
    // at the default size). Narrowing the column or widening the inset past
    // this breaks the 80-column code fit the constants promise, so the
    // relationship is pinned exactly, not as a loose band. Asymmetric
    // extremes: the rem-based inset scales with the base size while the
    // column caps stay fixed, so drift shows most at the ends.
    for base_size in [14., 16., 24.] {
        cx.update(|cx| {
            Theme::update(cx, |theme| {
                theme.font_size = px(base_size);
            });
        });
        let inset = 4. * base_size;
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            let column = window.find("transcript-content-measure-prose").bounds();
            let body = window.find("message-body-measure-prose").bounds();
            assert!(
                (column.size.width - px(768.)).abs() <= px(1.),
                "normal column holds its cap at {base_size}px: {column:?}"
            );
            assert!(
                (body.size.width - px(768. - inset)).abs() <= px(1.),
                "normal prose keeps the title inset on both sides at {base_size}px: body={body:?} column={column:?}"
            );
            window.press("ctrl-cmd-z", cx);
            window.render_frame(cx);
            assert!(view.read(cx).zen_mode, "zen toggle must land at {base_size}px");
            let zen_column = window.find("transcript-content-measure-prose").bounds();
            let zen_body = window.find("message-body-measure-prose").bounds();
            assert!(
                (zen_column.size.width - px(680.)).abs() <= px(1.),
                "zen column holds its cap at {base_size}px: {zen_column:?}"
            );
            assert!(
                (zen_body.size.width - px(680. - inset)).abs() <= px(1.),
                "zen prose keeps the title inset on both sides at {base_size}px: body={zen_body:?} column={zen_column:?}"
            );
            window.press("ctrl-cmd-z", cx);
            window.render_frame(cx);
            assert!(!view.read(cx).zen_mode, "zen toggle must release at {base_size}px");
        })
        .unwrap();
    }
}

#[gpui_kit::test]
fn wide_tables_stay_inside_the_conversation_column(cx: &mut TestAppContext) {
    let (window, view) = open_desktop_sized(cx, 1600., 900.);
    cx.update(|cx| {
        Theme::update(cx, |theme| {
            theme.font_size = px(16.);
        });
    });
    // Asymmetric stress: six columns plus one very long cell, so both the
    // column-shrink path and the cell-wrap path engage at once. The short
    // twin isolates the wrap growth from the table chrome, which a bare
    // height threshold cannot separate.
    let long = "y".repeat(200);
    let source =
        format!("| A | B | C | D | E | F |\n| --- | --- | --- | --- | --- | --- |\n| {long} | b | c | d | e | f |\n");
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.sidebar_open = false;
            view.inspector_open = false;
            view.transcript.push(TranscriptItem::new(
                "assistant-table".into(),
                "assistant".into(),
                "VT Code".into(),
                source,
            ));
            view.transcript.push(TranscriptItem::new(
                "assistant-table-short".into(),
                "assistant".into(),
                "VT Code".into(),
                "| A | B |\n| --- | --- |\n| a | b |\n".into(),
            ));
        });
        window.render_frame(cx);
        for zen in [false, true] {
            if zen {
                window.press("ctrl-cmd-z", cx);
                window.render_frame(cx);
            }
            assert_eq!(view.read(cx).zen_mode, zen, "zen toggle must land before measuring");
            let mode = if zen { "zen" } else { "normal" };
            let column = window.find("transcript-content-assistant-table").bounds();
            let element = window.find("message-body-assistant-table");
            let body = element.bounds();
            let overhang = body.right() - column.right();
            assert!(
                overhang <= px(1.),
                "{mode} wide table must not escape the conversation column: body={body:?} column={column:?}"
            );
            let short = window.find("message-body-assistant-table-short").bounds();
            assert!(
                body.size.height - short.size.height > px(20.),
                "{mode} long table cell must wrap past its short twin: body={body:?} short={short:?}"
            );
            let label = element.label().unwrap_or_default();
            assert!(label.contains(&long), "{mode} table accessible text keeps the cell");
        }
    })
    .unwrap();
}
