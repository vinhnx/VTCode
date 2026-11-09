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
        plan_markdown: None,
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
        plan_markdown: None,
    }
}

/// Two-step wizard: the smallest flow that earns the progress eyebrow.
fn two_step_question() -> InteractionRequest {
    let mut interaction = question_interaction(false);
    interaction.interaction_id = "two-step".into();
    interaction.steps.push(InteractionStep {
        title: "Detail".into(),
        question: "How much detail?".into(),
        choices: vec![InteractionChoice {
            id: "third".into(),
            label: "Third option".into(),
            description: None,
            recommended: false,
        }],
        selected_choice_id: None,
        allow_freeform: false,
    });
    interaction
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
fn recommended_pill_marks_only_the_recommended_row(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    // Asymmetric pair: the first row is recommended, the second is not, so
    // the pill must appear exactly once.
    let mut interaction = question_interaction(false);
    interaction.choices[0].recommended = true;
    interaction.steps[0].choices[0].recommended = true;
    deliver(&view, interaction, cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let pill = window.find("choice-recommended-first");
        assert!(pill.visible(), "the recommended row carries the pill");
        // Headless snapshots expose no rendered text, so layout is the
        // oracle: the pill must occupy space, not just exist.
        assert!(pill.bounds().size.height > px(0.), "the pill lays out with nonzero height");
        assert!(window.try_find("choice-recommended-second").is_none(), "a plain row carries no pill");
        // The visual pill splits off the label, but screen readers still hear
        // the recommendation on the row itself.
        let first_row = window.find("interaction-question-choice-0");
        let first = first_row.label().unwrap_or_default();
        assert!(first.contains("Recommended"), "recommended row announces it: {first}");
        let second_row = window.find("interaction-question-choice-1");
        let second = second_row.label().unwrap_or_default();
        assert!(!second.contains("Recommended"), "plain row stays quiet: {second}");
    })
    .unwrap();
}

#[gpui_kit::test]
fn cancel_request_button_cancels_choice_interaction(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    view.update(cx, |view, _cx| {
        view.backend = Some(BackendHandle { commands });
    });
    deliver(&view, tool_loop_interaction(Some("grant-40")), cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(
            window.try_find("dismiss-interaction").is_none(),
            "the title row carries no dismiss control: Cancel request owns cancellation"
        );
        window.click("cancel-interaction", cx);
    })
    .unwrap();
    let BackendCommand::Request { method, params } = requests.try_recv().expect("cancel button cancels the prompt")
    else {
        panic!("expected interaction response");
    };
    assert_eq!(method, vtcode_desktop_protocol::method::INTERACTION_RESPOND);
    assert!(params["choiceId"].is_null(), "cancellation names no choice");
    assert_eq!(params["choiceIds"], json!([]));
    assert_eq!(params["cancelled"], json!(true));
    assert!(requests.try_recv().is_err(), "cancel sends exactly one response");
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
fn single_step_question_hides_stepper_and_counter(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    // Both answer modes: the header collapses whether or not a freeform box follows.
    // (Multi-step keeps both; see multi_step_question_progress_tracks_the_runtime_step_order.)
    for freeform in [false, true] {
        deliver(&view, question_interaction(freeform), cx);
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            // One step is not progress: no stepper, no "Question 1 of 1".
            for id in [
                "interaction-question-progress",
                "interaction-question-stepper",
                "question-step-counter",
            ] {
                assert!(window.try_find(id).is_none(), "{id} earns no line with a single step (freeform={freeform})");
            }
            // The decision itself still renders: options group plus both rows.
            assert!(window.find("interaction-question-options").visible());
            assert!(window.find("interaction-question-choice-0").visible());
            assert!(window.find("interaction-question-choice-1").visible());
            assert!(window.find("continue-question").visible());
        })
        .unwrap();
    }
}

#[gpui_kit::test]
fn hovering_choice_card_never_selects_or_commits(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    deliver(&view, question_interaction(false), cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.hover("interaction-question-choice-1", cx);
        window.render_frame(cx);
        assert_eq!(window.find("interaction-question-choice-0").selected(), Some(false));
        assert_eq!(window.find("interaction-question-choice-1").selected(), Some(false));
        assert!(view.read(cx).pending_interaction_response.is_none(), "hover never submits");
        // Hover leaves the card fully interactive: the same row still selects.
        window.click("interaction-question-choice-1", cx);
        window.render_frame(cx);
        assert_eq!(window.find("interaction-question-choice-0").selected(), Some(false));
        assert_eq!(window.find("interaction-question-choice-1").selected(), Some(true));
    })
    .unwrap();
}

#[gpui_kit::test]
fn interaction_decision_row_keeps_group_air_and_shared_baseline(cx: &mut TestAppContext) {
    use gpui_kit::component::theme::Theme;

    let (window, view) = open_desktop(cx);
    deliver(&view, question_interaction(false), cx);
    // Asymmetric extremes: the rem-based rhythm must hold at the smallest
    // and largest UI base sizes, not just the default.
    for base_size in [14., 24.] {
        cx.update(|cx| {
            Theme::update(cx, |theme| {
                theme.font_size = px(base_size);
            });
        });
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            let hint = window.find("interaction-keyboard-hint").bounds();
            let cancel = window.find("cancel-interaction").bounds();
            let submit = window.find("continue-question").bounds();
            // Panel gap_2 plus the decision row's own mt_2: one full rem of
            // group air between the options block and the actions.
            let air = cancel.origin.y - (hint.origin.y + hint.size.height);
            assert!(
                (air - px(base_size)).abs() <= px(1.),
                "decision row keeps lg air above it at {base_size}px: air={air:?} hint={hint:?} cancel={cancel:?}"
            );
            assert!(
                (cancel.origin.y - submit.origin.y).abs() <= px(1.),
                "cancel and submit share one baseline at {base_size}px: cancel={cancel:?} submit={submit:?}"
            );
        })
        .unwrap();
    }
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
            plan_markdown: None,
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

#[gpui_kit::test]
fn wizard_header_orders_counter_above_title_above_choices(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    deliver(&view, two_step_question(), cx);
    focus_panel(&view, window, cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("question-step-counter").label(), Some("Question 1 of 2"));
        let counter = window.find("question-step-counter").bounds();
        let title = window.find("interaction-title-row").bounds();
        let first_choice = window.find("interaction-question-choice-0").bounds();
        assert!(
            counter.origin.y < title.origin.y,
            "progress eyebrow sits above the title: counter={counter:?} title={title:?}"
        );
        assert!(
            title.origin.y < first_choice.origin.y,
            "title sits above the choices: title={title:?} first_choice={first_choice:?}"
        );
        assert!(
            window.try_find("interaction-approval-marker").is_none(),
            "a plain question stays neutral without the warning marker"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn approval_panel_shows_warning_marker_in_title_row(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    deliver(&view, tool_loop_interaction(Some("grant-40")), cx);
    focus_panel(&view, window, cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let _marker = window.within("interaction-title-row").find("interaction-approval-marker");
    })
    .unwrap();
}

#[gpui_kit::test]
fn choice_panel_hides_approval_marker(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let mut interaction = tool_loop_interaction(None);
    interaction.kind = InteractionKind::Choice;
    interaction.interaction_id = "neutral-choice".into();
    deliver(&view, interaction, cx);
    focus_panel(&view, window, cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("interaction-title-row").is_some(), "the title row renders for a plain choice");
        assert!(
            window.try_find("interaction-approval-marker").is_none(),
            "a plain choice stays neutral without the warning marker"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn tool_call_detail_renders_highlighted_code_without_raw_ticks(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let mut interaction = tool_loop_interaction(Some("grant-40"));
    interaction.details = vec![
        "Environment: default policy".into(),
        "`$ python3 -c 'print(1)'`".into(),
        "Run `vitest` to verify".into(),
    ];
    deliver(&view, interaction, cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        // Prose keeps the plain paragraph: visible, but with no accessible
        // label of its own (the code and Markdown branches both set one).
        let prose = window.find("interaction-detail-0");
        assert!(prose.visible());
        assert_eq!(prose.label(), None);
        // A lone `$` span becomes a labeled code block with the ticks gone.
        // The "Command: ..." prefix plus the stripped `$` prove the code
        // branch ran: the Markdown branch would announce the span verbatim.
        let code = window.find("interaction-detail-1");
        assert_eq!(code.role(), Some(gpui_kit::Role::Label));
        assert_eq!(code.label(), Some("Command: python3 -c 'print(1)'"));
        assert!(!code.label().unwrap_or_default().contains('`'), "no raw ticks survive");
        // Mixed spans render as Markdown, also without raw ticks.
        let mixed = window.find("interaction-detail-2");
        assert_eq!(mixed.label(), Some("Run vitest to verify"));
    })
    .unwrap();
}

#[gpui_kit::test]
fn unbreakable_text_stays_inside_the_panel_and_cards(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let mut interaction = tool_loop_interaction(Some("grant-40"));
    // Worst case on both surfaces: a spaceless token with no wrap opportunity.
    let token = "x".repeat(300);
    interaction.details = vec![format!("`$ deploy {token}`"), format!("plain {token}")];
    let description = Some(format!("Stop after {token}"));
    interaction.choices[2].description = description.clone();
    interaction.steps[0].choices[2].description = description;
    deliver(&view, interaction, cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let panel = window.find("runtime-interaction").bounds();
        for id in ["interaction-detail-0", "interaction-detail-1"] {
            let detail = window.find(id).bounds();
            let overhang = (detail.origin.x + detail.size.width) - (panel.origin.x + panel.size.width);
            assert!(overhang <= px(1.), "{id} must not escape the panel: detail={detail:?} panel={panel:?}");
        }
        // The digit chip trails the description row: if the row escaped the
        // card, the chip would clip past its edge (the reported bug).
        let card = window.find("choice-stop").bounds();
        let card_right = card.origin.x + card.size.width;
        let row = window.find("choice-desc-stop").bounds();
        let row_overhang = (row.origin.x + row.size.width) - card_right;
        assert!(row_overhang <= px(1.), "description row must not escape its card: row={row:?} card={card:?}");
        let chip = window.find("choice-digit-stop").bounds();
        let chip_overhang = (chip.origin.x + chip.size.width) - card_right;
        assert!(chip_overhang <= px(1.), "digit chip must not clip past the card edge: chip={chip:?} card={card:?}");
    })
    .unwrap();
}
