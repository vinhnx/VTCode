//! Interaction overflow guards: shared-Escape cancellation and the capped,
//! scrollable interaction body for long choice lists.

use super::interaction_choices::tool_loop_interaction;
use super::*;

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
    let mut interaction = tool_loop_interaction(Some("grant-40"));
    // Far past the cap: the body must scroll instead of stretching the
    // composer into the transcript.
    for index in 0..24 {
        let id = format!("bulk-{index}");
        interaction.choices.push(vtcode_desktop_protocol::InteractionChoice {
            id: id.clone(),
            label: format!("Bulk choice {index}"),
            description: Some("Filler row that only exists to push the list past the cap".into()),
            recommended: false,
        });
    }
    deliver(&view, interaction, cx);
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
