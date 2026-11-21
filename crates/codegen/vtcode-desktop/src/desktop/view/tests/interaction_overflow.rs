//! Interaction overflow guards: shared-Escape cancellation and the capped,
//! scrollable interaction body for long choice lists.

use super::interaction_choices::tool_loop_interaction;
use super::*;

/// `interaction` with 24 filler rows appended past the tool-loop choices.
/// The render path reads the first step's choices for non-wizard prompts, so
/// the rows go there — mirrored onto the top-level list like the service
/// sends them. Filler rows carry descriptions in one variant to make them
/// tall enough to overflow the cap.
fn long_interaction(descriptions: bool) -> vtcode_desktop_protocol::InteractionRequest {
    let mut interaction = tool_loop_interaction(Some("grant-40"));
    for index in 0..24 {
        let choice = vtcode_desktop_protocol::InteractionChoice {
            id: format!("bulk-{index}"),
            label: format!("Bulk choice {index}"),
            description: descriptions.then(|| "Filler row that only exists to push the list past the cap".to_string()),
            recommended: false,
        };
        interaction.choices.push(choice.clone());
        interaction.steps[0].choices.push(choice);
    }
    interaction
}

#[gpui_kit::test]
fn escape_cancels_the_interaction_without_panel_focus(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    view.update(cx, |view, _cx| {
        view.backend = Some(BackendHandle { commands });
    });
    deliver(&view, tool_loop_interaction(Some("grant-40")), cx);
    // The shared escape chain owns dismissal wherever focus lives: with the
    // composer focused (not the panel), Escape still cancels the pending
    // approval instead of arming the draft clear.
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.composer.update(cx, |state, cx| state.focus(window, cx));
        });
        window.press("escape", cx);
    })
    .unwrap();
    let BackendCommand::Request { method, params } = requests
        .try_recv()
        .expect("escape from the composer cancels the pending prompt")
    else {
        panic!("expected interaction response");
    };
    assert_eq!(method, vtcode_desktop_protocol::method::INTERACTION_RESPOND);
    assert_eq!(params["cancelled"], json!(true));
    assert!(requests.try_recv().is_err(), "the shared escape sends exactly one response");
}

#[gpui_kit::test]
fn long_choice_lists_scroll_inside_a_capped_panel(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    // Far past the cap: the body must scroll instead of stretching the
    // composer into the transcript.
    deliver(&view, long_interaction(true), cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let body = window.find("interaction-body").bounds();
        assert!(body.size.height <= px(361.), "the scroll body caps at the shared list height: body={body:?}");
        let panel = window.find("runtime-interaction").bounds();
        assert!(panel.size.height < px(560.), "header, capped body, and decision row stay compact: panel={panel:?}");
        // The decision row stays pinned below the scroll region: cancelling
        // and confirming never scroll out of reach.
        let cancel = window.find("cancel-interaction").bounds();
        let body_bottom = body.origin.y + body.size.height;
        assert!(cancel.origin.y >= body_bottom, "decision row sits below the scroll body");
        let hint = window.find("interaction-keyboard-hint").bounds();
        assert!(hint.origin.y >= body_bottom, "keyboard hint stays pinned below the scroll body");
    })
    .unwrap();
}

#[gpui_kit::test]
fn a_new_interaction_opens_the_scrolled_body_at_its_top(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    deliver(&view, long_interaction(false), cx);
    // Simulate the reader having scrolled the long list before responding.
    view.update(cx, |view, _| {
        view.interaction_scroll.set_offset(point(px(0.), px(240.)));
        assert_eq!(view.interaction_scroll.offset().y, px(240.));
    });
    // A follow-up prompt renders fresh content: the stale offset must not
    // carry into it. The follow-up needs its own interaction id — the
    // delivery handler dedupes repeats of the same prompt.
    let mut follow_up = long_interaction(false);
    follow_up.interaction_id = "tool-loop-follow-up".into();
    deliver(&view, follow_up, cx);
    view.update(cx, |view, _| {
        assert_eq!(view.interaction_scroll.offset().y, px(0.), "a new interaction opens its body at the top");
    });
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("runtime-interaction").is_some());
    })
    .unwrap();
}

/// A crowded approval (the command-confirmation shape from the bug report:
/// environment row, `sh` command block, and two prose details, plus a long
/// choice list) must show every detail at full height. Before the fix the
/// detail wrappers used `overflow_hidden`, which made them collapse to the
/// font's em box (or zero under flex pressure) and clipped the text's
/// ascenders and wrapped lines. Overflow belongs to the scroll body.
#[gpui_kit::test]
fn crowded_interaction_never_clips_detail_text(cx: &mut TestAppContext) {
    let (window, view) = open_desktop_sized(cx, 1600., 1000.);
    let mut interaction = tool_loop_interaction(Some("grant-40"));
    interaction.title = "Would you like to run the following command?".into();
    interaction.details = vec![
        "Environment: default policy".into(),
        "`$ bun x markdownlint-cli2 README.md`".into(),
        "No additional details were provided. Review the action above before allowing.".into(),
        "Choose how to handle this run:".into(),
    ];
    let long_desc = "Sync facts and mermaid sketch to checkout truth -> files: [ README.md ] -> verify: [ rg -n \"0-185-0\" README.md ]";
    for choice in interaction.choices.iter_mut().chain(interaction.steps[0].choices.iter_mut()) {
        choice.description = Some(long_desc.to_string());
    }
    for index in 0..8 {
        let choice = vtcode_desktop_protocol::InteractionChoice {
            id: format!("bulk-{index}"),
            label: format!("Bulk choice {index}"),
            description: Some(long_desc.to_string()),
            recommended: false,
        };
        interaction.choices.push(choice.clone());
        interaction.steps[0].choices.push(choice);
    }
    deliver(&view, interaction, cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let body = window.find("interaction-body").bounds();
        assert!(body.size.height <= px(361.), "body stays capped: {body:?}");
        // Every detail keeps a full line box; the code block is taller still.
        for index in 0..4 {
            let detail = window.find(format!("interaction-detail-{index}")).bounds();
            assert!(detail.size.height >= px(20.), "detail {index} must keep its full line height, got {detail:?}");
        }
        let code = window.find("interaction-detail-1").bounds();
        assert!(code.size.height > px(40.), "the command block keeps its full height: {code:?}");
        // The panel scrolls instead of shaving text off the details.
        let max_offset = view.read(cx).interaction_scroll.max_offset();
        assert!(max_offset.y > px(0.), "crowded content scrolls inside the capped body: {max_offset:?}");
    })
    .unwrap();
}

#[gpui_kit::test]
fn arrow_navigation_keeps_the_highlighted_choice_in_view(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    // 27 rows (3 tool-loop + 24 bulk): bulk-9 sits at index 12.
    deliver(&view, long_interaction(true), cx);
    focus_panel(&view, window, cx);
    // Walking the highlight down past the capped body must reveal each
    // landing row: Confirm commits the highlight, so an off-screen highlight
    // would submit a choice the reader never saw.
    cx.update_window(window, |_, window, cx| {
        for _ in 0..12 {
            window.press("down", cx);
            window.render_frame(cx);
        }
        let body = window.find("interaction-body").bounds();
        let highlighted = window.find("choice-bulk-9").bounds();
        assert!(
            highlighted.top() >= body.top() - px(1.) && highlighted.bottom() <= body.bottom() + px(1.),
            "the highlighted row stays inside the capped body: row={highlighted:?} body={body:?}"
        );
    })
    .unwrap();
    view.update(cx, |view, _| {
        assert!(view.interaction_scroll.offset().y < px(0.), "walking past the cap scrolls the body");
    });
    // Walking back up reveals each landing row again (minimal scroll: the
    // body may rest a little below its top while the highlight stays visible).
    cx.update_window(window, |_, window, cx| {
        for _ in 0..12 {
            window.press("up", cx);
            window.render_frame(cx);
        }
        let body = window.find("interaction-body").bounds();
        let highlighted = window.find("choice-grant-40").bounds();
        assert!(
            highlighted.top() >= body.top() - px(1.) && highlighted.bottom() <= body.bottom() + px(1.),
            "walking back up keeps the first row visible: row={highlighted:?} body={body:?}"
        );
    })
    .unwrap();
}
