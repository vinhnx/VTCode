use super::*;
use vtcode_desktop_protocol::{InteractionChoice, InteractionKind, InteractionRequest, InteractionStep};

fn tool_loop_choices() -> Vec<InteractionChoice> {
    vec![
        InteractionChoice {
            id: "grant-40".into(),
            label: "+40 tool loops".into(),
            description: Some("Continue with 40 more tool loops (reaches cap)".into()),
            recommended: false,
        },
        InteractionChoice {
            id: "grant-20".into(),
            label: "+20 tool loops".into(),
            description: Some("Continue with 20 more tool loops".into()),
            recommended: false,
        },
        InteractionChoice {
            id: "stop".into(),
            label: "Stop".into(),
            description: Some("Stop the turn and synthesize from the outputs already gathered".into()),
            recommended: false,
        },
    ]
}

/// Production-faithful tool-loop grant prompt: Approval kind (the Stop row
/// carries a deny selection), choices mirrored on the first step exactly as
/// the service pump sends them.
fn tool_loop_interaction(selected: Option<&str>) -> InteractionRequest {
    let choices = tool_loop_choices();
    InteractionRequest {
        session_id: "session".into(),
        turn_id: vtcode_desktop_protocol::SESSION_START_TURN_ID.into(),
        interaction_id: "tool-loop".into(),
        kind: InteractionKind::Approval,
        wizard: false,
        title: "Tool Loop Limit Reached".into(),
        details: vec![
            "Maximum tool loops reached: 20 (cap 60, 40 remaining)".into(),
            "Current agent: build".into(),
        ],
        choices: choices.clone(),
        steps: vec![InteractionStep {
            title: "Tool Loop Limit Reached".into(),
            question: "Maximum tool loops reached: 20 (cap 60, 40 remaining)".into(),
            choices,
            selected_choice_id: selected.map(str::to_string),
            allow_freeform: false,
        }],
        selected_choice_id: selected.map(str::to_string),
        selected_choice_ids: selected.map(|id| vec![id.to_string()]).unwrap_or_default(),
    }
}

fn question_interaction(freeform: bool) -> InteractionRequest {
    let choices = vec![
        InteractionChoice {
            id: "first".into(),
            label: "First option".into(),
            description: Some("First description".into()),
            recommended: false,
        },
        InteractionChoice {
            id: "second".into(),
            label: "Second option".into(),
            description: Some("Second description".into()),
            recommended: false,
        },
    ];
    InteractionRequest {
        session_id: "session".into(),
        turn_id: vtcode_desktop_protocol::SESSION_START_TURN_ID.into(),
        interaction_id: "question".into(),
        kind: InteractionKind::Question,
        wizard: false,
        title: "Pick one".into(),
        details: vec!["Choose the scope".into()],
        choices: choices.clone(),
        steps: vec![InteractionStep {
            title: "Scope".into(),
            question: "Which scope?".into(),
            choices,
            selected_choice_id: None,
            allow_freeform: freeform,
        }],
        selected_choice_id: None,
        selected_choice_ids: Vec::new(),
    }
}

fn deliver(view: &Entity<DesktopView>, interaction: InteractionRequest, cx: &mut TestAppContext) {
    view.update(cx, |view, cx| {
        view.selected_session_id = Some(interaction.session_id.clone());
        view.handle_backend_event(BackendEvent::Interaction(interaction), cx);
    });
}

fn focus_panel(view: &Entity<DesktopView>, window: AnyWindowHandle, cx: &mut TestAppContext) {
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        view.update(cx, |view, cx| view.interaction_focus.focus(window, cx));
        window.render_frame(cx);
    })
    .unwrap();
}

#[gpui_kit::test]
fn choice_rows_show_inline_descriptions_with_service_default_preselected(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    deliver(&view, tool_loop_interaction(Some("grant-20")), cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let options = window.find("interaction-choice-options");
        assert_eq!(options.role(), Some(gpui_kit::Role::RadioGroup));
        assert_eq!(options.label(), Some("Tool Loop Limit Reached"));
        // The service-selected row — not the first row — carries the default.
        assert_eq!(window.find("choice-grant-40").selected(), Some(false));
        assert_eq!(window.find("choice-grant-20").selected(), Some(true));
        assert_eq!(window.find("choice-stop").selected(), Some(false));
        // Every description renders inline in the tree: no hover tooltip owns
        // decision-critical copy.
        for id in ["choice-desc-grant-40", "choice-desc-grant-20", "choice-desc-stop"] {
            assert!(window.find(id).visible(), "{id} description renders inline");
        }
        assert!(window.find("interaction-keyboard-hint").visible());
    })
    .unwrap();
}

#[gpui_kit::test]
fn choice_default_falls_back_to_first_row_without_service_selection(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    deliver(&view, tool_loop_interaction(None), cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("choice-grant-40").selected(), Some(true));
        assert_eq!(window.find("choice-grant-20").selected(), Some(false));
        assert_eq!(window.find("choice-stop").selected(), Some(false));
    })
    .unwrap();
}

#[gpui_kit::test]
fn digit_shortcut_commits_the_numbered_choice(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    view.update(cx, |view, _cx| {
        view.backend = Some(BackendHandle { commands });
    });
    deliver(&view, tool_loop_interaction(Some("grant-40")), cx);
    focus_panel(&view, window, cx);
    cx.update_window(window, |_, window, cx| {
        window.press("2", cx);
    })
    .unwrap();
    let BackendCommand::Request { method, params } = requests.try_recv().expect("digit commits a response") else {
        panic!("expected interaction response");
    };
    assert_eq!(method, vtcode_desktop_protocol::method::INTERACTION_RESPOND);
    assert_eq!(params["choiceId"], json!("grant-20"));
    assert_eq!(params["choiceIds"], json!(["grant-20"]));
    assert_eq!(params["cancelled"], json!(false));
    assert!(requests.try_recv().is_err(), "one digit sends exactly one response");
    cx.update_window(window, |_, _, cx| {
        assert_eq!(view.read(cx).composer.read(cx).value(), "", "digits never leak into the composer");
    })
    .unwrap();
}

#[gpui_kit::test]
fn escape_cancels_choice_interaction(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    view.update(cx, |view, _cx| {
        view.backend = Some(BackendHandle { commands });
    });
    deliver(&view, tool_loop_interaction(Some("grant-40")), cx);
    focus_panel(&view, window, cx);
    cx.update_window(window, |_, window, cx| {
        window.press("escape", cx);
    })
    .unwrap();
    let BackendCommand::Request { method, params } = requests.try_recv().expect("escape cancels the prompt") else {
        panic!("expected interaction response");
    };
    assert_eq!(method, vtcode_desktop_protocol::method::INTERACTION_RESPOND);
    assert!(params["choiceId"].is_null(), "cancellation names no choice");
    assert_eq!(params["choiceIds"], json!([]));
    assert_eq!(params["cancelled"], json!(true));
    assert!(requests.try_recv().is_err(), "escape sends exactly one response");
}

#[gpui_kit::test]
fn digits_never_commit_while_freeform_answer_is_focused(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    view.update(cx, |view, _cx| {
        view.backend = Some(BackendHandle { commands });
    });
    deliver(&view, question_interaction(true), cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(
            window.try_find("interaction-keyboard-hint").is_none(),
            "no hint promises shortcuts while the freeform answer owns the keys"
        );
        view.update(cx, |view, cx| {
            view.question_answer.read(cx).focus_handle(cx).focus(window, cx);
        });
        assert!(
            view.read(cx).question_answer.read(cx).focus_handle(cx).is_focused(window),
            "the freeform answer must hold focus for this regression to bite"
        );
        window.press("1", cx);
        window.press("2", cx);
    })
    .unwrap();
    assert!(requests.try_recv().is_err(), "typing digits must not commit a row");
    view.update(cx, |view, _cx| {
        assert!(view.interaction.is_some(), "the prompt stays open");
        assert!(view.pending_interaction_response.is_none(), "nothing is in flight");
    });
}

#[gpui_kit::test]
fn clicking_choice_row_selects_without_submitting(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    view.update(cx, |view, _cx| {
        view.backend = Some(BackendHandle { commands });
    });
    // Preselect the first row, click the last: the highlight must move across
    // the full list, not just to a neighbor.
    deliver(&view, tool_loop_interaction(Some("grant-40")), cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("choice-grant-40").selected(), Some(true));
        window.click("choice-stop", cx);
        window.render_frame(cx);
        assert_eq!(window.find("choice-grant-40").selected(), Some(false));
        assert_eq!(window.find("choice-grant-20").selected(), Some(false));
        assert_eq!(window.find("choice-stop").selected(), Some(true));
    })
    .unwrap();
    assert!(requests.try_recv().is_err(), "selecting a row never submits");
    view.update(cx, |view, _cx| {
        assert_eq!(view.effective_immediate_choice_id().as_deref(), Some("stop"));
        assert!(view.pending_interaction_response.is_none(), "nothing is in flight");
    });
}

#[gpui_kit::test]
fn confirm_submits_preselected_default_row(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    view.update(cx, |view, _cx| {
        view.backend = Some(BackendHandle { commands });
    });
    // The service preselects the middle row. Clicking the checked card is a
    // Kit-level no-op, so without Confirm this default could never be
    // submitted by mouse — the dead-end this regression pins.
    deliver(&view, tool_loop_interaction(Some("grant-20")), cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("choice-grant-20").selected(), Some(true));
        window.click("choice-grant-20", cx);
        window.render_frame(cx);
    })
    .unwrap();
    assert!(requests.try_recv().is_err(), "clicking the checked row sends nothing");
    cx.update_window(window, |_, window, cx| {
        window.click("confirm-interaction", cx);
    })
    .unwrap();
    let BackendCommand::Request { method, params } = requests.try_recv().expect("confirm submits a response") else {
        panic!("expected interaction response");
    };
    assert_eq!(method, vtcode_desktop_protocol::method::INTERACTION_RESPOND);
    assert_eq!(params["choiceId"], json!("grant-20"));
    assert_eq!(params["choiceIds"], json!(["grant-20"]));
    assert_eq!(params["cancelled"], json!(false));
    assert!(requests.try_recv().is_err(), "confirm sends exactly one response");
}

#[gpui_kit::test]
fn confirming_choice_row_disables_panel_while_response_pending(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    view.update(cx, |view, _cx| {
        view.backend = Some(BackendHandle { commands });
    });
    deliver(&view, tool_loop_interaction(Some("grant-40")), cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("choice-grant-20", cx);
        window.render_frame(cx);
        window.click("confirm-interaction", cx);
        window.click("confirm-interaction", cx);
        window.render_frame(cx);
        for id in [
            "choice-grant-40",
            "choice-grant-20",
            "choice-stop",
            "cancel-interaction",
            "confirm-interaction",
        ] {
            assert_eq!(window.find(id).disabled(), Some(true), "{id} disables while pending");
        }
    })
    .unwrap();
    let BackendCommand::Request { method, params } = requests.try_recv().expect("confirm commits a response") else {
        panic!("expected interaction response");
    };
    assert_eq!(method, vtcode_desktop_protocol::method::INTERACTION_RESPOND);
    assert_eq!(params["choiceIds"], json!(["grant-20"]));
    assert!(requests.try_recv().is_err(), "repeat confirms send exactly one response");
}

#[gpui_kit::test]
fn confirm_ignores_wizard_questions(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    view.update(cx, |view, _cx| {
        view.backend = Some(BackendHandle { commands });
    });
    deliver(&view, question_interaction(false), cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(
            window.try_find("confirm-interaction").is_none(),
            "wizards commit through Next/Submit, never an immediate Confirm"
        );
        view.update(cx, |view, cx| view.confirm_immediate_choice(window, cx));
    })
    .unwrap();
    assert!(requests.try_recv().is_err(), "confirm never submits a wizard step");
    view.update(cx, |view, _cx| {
        assert!(view.interaction.is_some(), "the wizard stays open");
        assert!(view.pending_interaction_response.is_none(), "nothing is in flight");
    });
}

#[gpui_kit::test]
fn failed_confirm_keeps_selection_for_retry(cx: &mut TestAppContext) {
    use crate::desktop::transport::InteractionResponseIdentity;

    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    view.update(cx, |view, _cx| {
        view.backend = Some(BackendHandle { commands });
    });
    deliver(&view, tool_loop_interaction(Some("grant-40")), cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("choice-stop", cx);
        window.render_frame(cx);
        window.click("confirm-interaction", cx);
    })
    .unwrap();
    let BackendCommand::Request { params, .. } = requests.try_recv().expect("first confirm sends") else {
        panic!("expected interaction response");
    };
    assert_eq!(params["choiceIds"], json!(["stop"]));
    // The service rejects the decision: the prompt stays open with the same
    // row highlighted, so Confirm retries instead of forcing a reselect.
    view.update(cx, |view, cx| {
        view.handle_backend_event(
            BackendEvent::InteractionResponded {
                identity: InteractionResponseIdentity {
                    session_id: "session".into(),
                    turn_id: vtcode_desktop_protocol::SESSION_START_TURN_ID.into(),
                    interaction_id: "tool-loop".into(),
                    cancelled: false,
                },
                error: Some("Decision rejected".into()),
            },
            cx,
        );
    });
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("choice-stop").selected(), Some(true));
        assert_ne!(window.find("confirm-interaction").disabled(), Some(true), "confirm re-enables after the rejection");
        window.click("confirm-interaction", cx);
    })
    .unwrap();
    let BackendCommand::Request { params, .. } = requests.try_recv().expect("retry confirm sends") else {
        panic!("expected interaction response");
    };
    assert_eq!(params["choiceIds"], json!(["stop"]));
    assert!(requests.try_recv().is_err(), "retry sends exactly one response");
}

#[gpui_kit::test]
fn digit_selects_without_submitting_for_question_steps(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    view.update(cx, |view, _cx| {
        view.backend = Some(BackendHandle { commands });
    });
    deliver(&view, question_interaction(false), cx);
    focus_panel(&view, window, cx);
    cx.update_window(window, |_, window, cx| {
        window.press("2", cx);
        window.render_frame(cx);
        assert_eq!(window.find("interaction-question-choice-0").selected(), Some(false));
        assert_eq!(window.find("interaction-question-choice-1").selected(), Some(true));
    })
    .unwrap();
    assert!(requests.try_recv().is_err(), "selecting a question row never submits");
    view.update(cx, |view, _cx| {
        assert_eq!(
            view.interaction_choice_ids.first().and_then(Option::as_deref),
            Some("second"),
            "the digit updates the controlled step selection"
        );
    });
}

#[gpui_kit::test]
fn empty_choice_panel_hides_hint_but_keeps_escape_cancel(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    view.update(cx, |view, _cx| {
        view.backend = Some(BackendHandle { commands });
    });
    deliver(
        &view,
        InteractionRequest {
            session_id: "session".into(),
            turn_id: vtcode_desktop_protocol::SESSION_START_TURN_ID.into(),
            interaction_id: "notice".into(),
            kind: InteractionKind::Other,
            wizard: false,
            title: "Nothing to pick".into(),
            details: vec!["An informational notice with no rows.".into()],
            choices: Vec::new(),
            steps: Vec::new(),
            selected_choice_id: None,
            selected_choice_ids: Vec::new(),
        },
        cx,
    );
    focus_panel(&view, window, cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(
            window.try_find("interaction-keyboard-hint").is_none(),
            "no hint promises shortcuts when no rows exist"
        );
        assert!(window.try_find("confirm-interaction").is_none(), "no confirm button renders when no rows exist");
        window.press("escape", cx);
    })
    .unwrap();
    let BackendCommand::Request { method, params } = requests.try_recv().expect("escape cancels the notice") else {
        panic!("expected interaction response");
    };
    assert_eq!(method, vtcode_desktop_protocol::method::INTERACTION_RESPOND);
    assert_eq!(params["cancelled"], json!(true));
}
