use super::*;
use crate::desktop::inspector::InspectorTab;
use vtcode_desktop_protocol::{InteractionChoice, InteractionKind, InteractionRequest};

/// Production-faithful plan approval: `PlanApproval` kind with the service's
/// bounded summary lines plus the full normalized markdown the expander shows.
fn plan_approval_interaction(id: &str, plan_markdown: Option<&str>) -> InteractionRequest {
    InteractionRequest {
        session_id: "session".into(),
        turn_id: vtcode_desktop_protocol::SESSION_START_TURN_ID.into(),
        interaction_id: id.into(),
        kind: InteractionKind::PlanApproval,
        wizard: false,
        title: "Ready to code?".into(),
        details: vec![
            "A plan is ready to execute. Would you like to proceed?".into(),
            "Summary: Refine the plan viewer for clarity.".into(),
        ],
        choices: vec![
            InteractionChoice {
                id: "execute".into(),
                label: "Yes, implement this plan".into(),
                description: Some("Continue with the current context and confirmation policy.".into()),
                recommended: true,
            },
            InteractionChoice {
                id: "stay".into(),
                label: "No, stay in Plan mode".into(),
                description: Some("Return to planning and revise the plan.".into()),
                recommended: false,
            },
        ],
        steps: Vec::new(),
        selected_choice_id: Some("execute".into()),
        selected_choice_ids: vec!["execute".into()],
        plan_markdown: plan_markdown.map(str::to_string),
    }
}

#[gpui_kit::test]
fn plan_toggle_hides_without_viewable_markdown(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    // None, empty, and blank all hide: only real content earns the toggle.
    for (id, markdown) in [
        ("plan-none", None),
        ("plan-empty", Some("")),
        ("plan-blank", Some("  \n ")),
    ] {
        deliver(&view, plan_approval_interaction(id, markdown), cx);
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            assert!(window.try_find("interaction-plan-toggle").is_none(), "{id}: no toggle without viewable markdown");
            assert!(
                window.try_find("interaction-plan-proposal").is_none(),
                "{id}: no viewer without viewable markdown"
            );
        })
        .unwrap();
    }
}

#[gpui_kit::test]
fn plan_toggle_expands_and_collapses_the_full_proposal(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    deliver(&view, plan_approval_interaction("plan-1", Some("# Retry plan\n\nWrap generate in retry.")), cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("interaction-plan-toggle").label(), Some("Show full plan"));
        assert!(window.try_find("interaction-plan-proposal").is_none(), "collapsed until toggled");
        window.click("interaction-plan-toggle", cx);
        window.render_frame(cx);
        let viewer = window.find("interaction-plan-proposal");
        assert!(viewer.visible(), "the expanded viewer lays out");
        let spoken = viewer.label().unwrap_or_default();
        assert!(spoken.contains("Wrap generate in retry."), "plan body is exposed: {spoken}");
        assert_eq!(window.find("interaction-plan-toggle").label(), Some("Hide full plan"));
        window.click("interaction-plan-toggle", cx);
        window.render_frame(cx);
        assert!(window.try_find("interaction-plan-proposal").is_none(), "second click collapses");
    })
    .unwrap();
}

#[gpui_kit::test]
fn plan_expanded_resets_when_a_new_interaction_arrives(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    deliver(&view, plan_approval_interaction("plan-1", Some("# First plan\n\nFirst body.")), cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("interaction-plan-toggle", cx);
        window.render_frame(cx);
        assert!(window.find("interaction-plan-proposal").visible());
    })
    .unwrap();
    deliver(&view, plan_approval_interaction("plan-2", Some("# Second plan\n\nSecond body.")), cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("interaction-plan-toggle").label(), Some("Show full plan"));
        assert!(window.try_find("interaction-plan-proposal").is_none(), "a new interaction starts collapsed");
    })
    .unwrap();
}

#[gpui_kit::test]
fn plan_open_button_reveals_inspector_plan_tab(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    deliver(&view, plan_approval_interaction("plan-1", Some("# Retry plan\n\nWrap generate in retry.")), cx);
    // Arrange the precondition explicitly: the shared test preferences can
    // restore the inspector open from an earlier test in this process.
    view.update(cx, |view, _| view.inspector_open = false);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(!view.read(cx).inspector_open, "the inspector starts closed");
        assert_eq!(window.find("interaction-plan-open-inspector").label(), Some("Open in inspector"));
        window.click("interaction-plan-open-inspector", cx);
        window.render_frame(cx);
        assert!(view.read(cx).inspector_open, "the shortcut opens the inspector");
        assert_eq!(view.read(cx).inspector_tab, InspectorTab::Plan);
        let viewer = window.find("inspector-plan-proposal");
        assert!(viewer.visible(), "the side viewer lays out");
        let spoken = viewer.label().unwrap_or_default();
        assert!(spoken.contains("Wrap generate in retry."), "side viewer shows the same plan: {spoken}");
        assert!(window.try_find("inspector-plan-empty").is_none(), "no empty state while a plan waits");
    })
    .unwrap();
}

#[gpui_kit::test]
fn plan_tab_follows_the_live_interaction(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    deliver(&view, plan_approval_interaction("plan-1", Some("# First plan\n\nFirst body.")), cx);
    view.update(cx, |view, cx| view.open_inspector_tab(InspectorTab::Plan, cx));
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let viewer = window.find("inspector-plan-proposal");
        let spoken = viewer.label().unwrap_or_default();
        assert!(spoken.contains("First body."), "tab shows the pending plan: {spoken}");
    })
    .unwrap();
    // A superseding approval replaces the tab content; a plan-less prompt
    // falls back to Changes. The tab never shows a stale plan.
    deliver(&view, plan_approval_interaction("plan-2", Some("# Second plan\n\nSecond body.")), cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let viewer = window.find("inspector-plan-proposal");
        let spoken = viewer.label().unwrap_or_default();
        assert!(spoken.contains("Second body."), "tab tracks the newest plan: {spoken}");
        assert!(!spoken.contains("First body."), "no stale content lingers: {spoken}");
    })
    .unwrap();
    deliver(&view, plan_approval_interaction("plan-3", None), cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(view.read(cx).inspector_tab, InspectorTab::Changes, "selection falls back when the plan leaves");
        assert!(window.try_find("inspector-plan-tab").is_none(), "Plan tab leaves the rail with its plan");
    })
    .unwrap();
}

#[gpui_kit::test]
fn plan_tab_absent_from_rail_without_pending_approval(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    view.update(cx, |view, cx| view.open_inspector_tab(InspectorTab::Plan, cx));
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(view.read(cx).inspector_tab, InspectorTab::Changes, "opening a hidden tab is a no-op");
        let tabs = window.within("inspector-tabs").find_all_by_role(gpui_kit::Role::Tab);
        assert_eq!(tabs.len(), 5, "only the persistent tabs render");
        assert!(window.try_find("inspector-plan-tab").is_none(), "no Plan content without a plan");
    })
    .unwrap();
}

#[gpui_kit::test]
fn stranded_plan_selection_degrades_to_empty_state(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    // White-box the unreachable-by-construction fallback: force the
    // selection the rail guards normally prevent.
    view.update(cx, |view, _| {
        view.inspector_open = true;
        view.inspector_tab = InspectorTab::Plan;
    });
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.find("inspector-plan-empty").visible(), "stranded selection explains itself, never blanks");
        assert!(window.try_find("inspector-plan-proposal").is_none(), "no viewer without a plan");
    })
    .unwrap();
}

#[gpui_kit::test]
fn plan_tab_joins_keyboard_cycle_while_approval_pends(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    deliver(&view, plan_approval_interaction("plan-1", Some("# Plan\n\nBody.")), cx);
    view.update(cx, |view, cx| view.open_inspector_tab(InspectorTab::Changes, cx));
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        view.update(cx, |view, cx| view.inspector_focus.focus(window, cx));
        window.render_frame(cx);
        window.press("end", cx);
        window.render_frame(cx);
        assert_eq!(view.read(cx).inspector_tab, InspectorTab::Plan, "end reaches the joined tab");
        window.press("home", cx);
        window.render_frame(cx);
        assert_eq!(view.read(cx).inspector_tab, InspectorTab::Changes);
    })
    .unwrap();
}

#[gpui_kit::test]
fn interaction_header_cards_actions_share_left_spine(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    deliver(&view, plan_approval_interaction("plan-spine", Some("# Plan\n\nBody.")), cx);
    focus_panel(&view, window, cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let title = window.find("interaction-title-row").bounds();
        let card = window.find("choice-execute").bounds();
        let toggle = window.find("interaction-plan-toggle").bounds();
        let cancel = window.find("cancel-interaction").bounds();
        assert!(
            (title.origin.x - card.origin.x).abs() <= px(1.),
            "title and cards share one left spine: title={title:?} card={card:?}"
        );
        assert!(
            (title.origin.x - toggle.origin.x).abs() <= px(1.),
            "title and plan toggle share one left spine: title={title:?} toggle={toggle:?}"
        );
        assert!(
            (title.origin.x - cancel.origin.x).abs() <= px(1.),
            "title and decision row share one left spine: title={title:?} cancel={cancel:?}"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn plan_edit_button_hides_without_viewable_markdown(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    for (id, markdown) in [
        ("plan-none", None),
        ("plan-empty", Some("")),
        ("plan-blank", Some("  \n ")),
    ] {
        deliver(&view, plan_approval_interaction(id, markdown), cx);
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            assert!(
                window.try_find("interaction-plan-edit").is_none(),
                "{id}: no edit button without viewable markdown"
            );
        })
        .unwrap();
    }
}

#[gpui_kit::test]
fn plan_edit_round_trip_locks_panel_until_reshow(cx: &mut TestAppContext) {
    use crate::desktop::transport::InteractionResponseIdentity;

    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    view.update(cx, |view, _cx| {
        view.backend = Some(BackendHandle { commands });
    });
    deliver(&view, plan_approval_interaction("plan-1", Some("# First plan\n\nFirst body.")), cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("interaction-plan-edit").label(), Some("Edit plan"));
        window.click("interaction-plan-edit", cx);
        window.render_frame(cx);
        assert!(view.read(cx).interaction_edit_pending, "edit locks the panel");
        assert_eq!(view.read(cx).status, "Editing plan…");
        assert!(window.find("interaction-response-status").visible());
    })
    .unwrap();

    let BackendCommand::Request { method, params } = requests.try_recv().expect("edit sends a hotkey response") else {
        panic!("expected interaction response");
    };
    assert_eq!(method, vtcode_desktop_protocol::method::INTERACTION_RESPOND);
    assert_eq!(params["interactionId"], json!("plan-1"));
    assert_eq!(params["hotkey"], json!("launch_editor"));
    assert_eq!(params["choiceIds"], json!([]));
    assert_eq!(params["cancelled"], json!(false));
    assert!(requests.try_recv().is_err(), "edit sends exactly one response");

    // A second edit while locked sends nothing.
    view.update(cx, |view, cx| view.submit_interaction_hotkey(cx));
    assert!(requests.try_recv().is_err(), "the edit lock suppresses repeat signals");

    // The service accepted the edit: the panel stays on the editing copy
    // instead of clearing while the external editor is open.
    view.update(cx, |view, cx| {
        view.handle_backend_event(
            BackendEvent::InteractionResponded {
                identity: InteractionResponseIdentity {
                    session_id: "session".into(),
                    turn_id: vtcode_desktop_protocol::SESSION_START_TURN_ID.into(),
                    interaction_id: "plan-1".into(),
                    cancelled: false,
                },
                error: None,
            },
            cx,
        );
    });
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("interaction-plan-edit").is_some(), "the panel stays through the edit round-trip");
        assert_eq!(window.find("interaction-plan-toggle").disabled(), Some(true), "controls stay locked");
        assert_eq!(view.read(cx).status, "Editing plan…");
        assert!(view.read(cx).pending_interaction_response.is_some(), "the pending lock survives the ack");
    })
    .unwrap();

    // The re-shown approval unlocks the panel with the edited proposal.
    deliver(&view, plan_approval_interaction("plan-2", Some("# Second plan\n\nSecond body.")), cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(!view.read(cx).interaction_edit_pending, "re-show clears the edit lock");
        assert_ne!(window.find("interaction-plan-toggle").disabled(), Some(true), "re-show unlocks the controls");
        assert!(window.try_find("interaction-response-status").is_none(), "re-show clears the editing copy");
        window.click("interaction-plan-toggle", cx);
        window.render_frame(cx);
        let viewer = window.find("interaction-plan-proposal");
        let spoken = viewer.label().unwrap_or_default();
        assert!(spoken.contains("Second body."), "re-show carries the edited proposal: {spoken}");
    })
    .unwrap();
}

#[gpui_kit::test]
fn plan_edit_lock_releases_only_on_terminal_turn_end(cx: &mut TestAppContext) {
    use crate::desktop::transport::InteractionResponseIdentity;

    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    view.update(cx, |view, _cx| {
        view.backend = Some(BackendHandle { commands });
    });
    let ack = || BackendEvent::InteractionResponded {
        identity: InteractionResponseIdentity {
            session_id: "session".into(),
            turn_id: vtcode_desktop_protocol::SESSION_START_TURN_ID.into(),
            interaction_id: "plan-1".into(),
            cancelled: false,
        },
        error: None,
    };
    let turn_status = |turn_id: &str, status: &str, restarted: bool| {
        BackendEvent::TurnStatus(json!({
            "sessionId": "session",
            "turnId": turn_id,
            "status": status,
            "restarted": restarted,
        }))
    };
    let start_turn = vtcode_desktop_protocol::SESSION_START_TURN_ID;

    // Lock the panel in the edit round-trip.
    deliver(&view, plan_approval_interaction("plan-1", Some("# First plan\n\nFirst body.")), cx);
    view.update(cx, |view, cx| view.submit_interaction_hotkey(cx));
    assert!(matches!(requests.try_recv(), Ok(BackendCommand::Request { .. })), "edit sends a hotkey response");
    view.update(cx, |view, cx| view.handle_backend_event(ack(), cx));
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(view.read(cx).interaction_edit_pending, "ack locks the panel for the edit");
        // Blocked, recovered, running, and foreign-turn ends still hold (or
        // never held) this approval wait: the lock stays.
        for (turn_id, status, restarted) in [
            (start_turn, "blocked", false),
            (start_turn, "failed", true),
            (start_turn, "running", false),
            ("other-turn", "completed", false),
        ] {
            view.update(cx, |view, cx| view.handle_backend_event(turn_status(turn_id, status, restarted), cx));
            assert!(view.read(cx).interaction_edit_pending, "{status} keeps the edit lock");
        }
        // A terminal end of this turn without a re-show (stop, error) releases
        // the stranded panel instead of locking it on "Editing plan…" forever.
        view.update(cx, |view, cx| view.handle_backend_event(turn_status(start_turn, "failed", false), cx));
        window.render_frame(cx);
        assert!(!view.read(cx).interaction_edit_pending, "terminal failure releases the lock");
        assert!(view.read(cx).interaction.is_none(), "the stranded panel clears");
        assert!(window.try_find("interaction-plan-edit").is_none());
    })
    .unwrap();

    // Same release for the completed label on a fresh lock.
    deliver(&view, plan_approval_interaction("plan-1", Some("# First plan\n\nFirst body.")), cx);
    view.update(cx, |view, cx| view.submit_interaction_hotkey(cx));
    assert!(matches!(requests.try_recv(), Ok(BackendCommand::Request { .. })), "re-edit sends again");
    view.update(cx, |view, cx| view.handle_backend_event(ack(), cx));
    view.update(cx, |view, cx| view.handle_backend_event(turn_status(start_turn, "completed", false), cx));
    cx.update_window(window, |_, _, cx| {
        assert!(!view.read(cx).interaction_edit_pending, "terminal completion releases the lock");
        assert!(view.read(cx).interaction.is_none());
    })
    .unwrap();
}

#[gpui_kit::test]
fn plan_edit_rejection_unlocks_panel_for_retry(cx: &mut TestAppContext) {
    use crate::desktop::transport::InteractionResponseIdentity;

    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    view.update(cx, |view, _cx| {
        view.backend = Some(BackendHandle { commands });
    });
    deliver(&view, plan_approval_interaction("plan-1", Some("# First plan\n\nFirst body.")), cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("interaction-plan-edit", cx);
    })
    .unwrap();
    assert!(matches!(requests.try_recv(), Ok(BackendCommand::Request { .. })), "edit sends a hotkey response");

    view.update(cx, |view, cx| {
        view.handle_backend_event(
            BackendEvent::InteractionResponded {
                identity: InteractionResponseIdentity {
                    session_id: "session".into(),
                    turn_id: vtcode_desktop_protocol::SESSION_START_TURN_ID.into(),
                    interaction_id: "plan-1".into(),
                    cancelled: false,
                },
                error: Some("interaction hotkey is invalid".into()),
            },
            cx,
        );
    });
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(!view.read(cx).interaction_edit_pending, "rejection clears the edit lock");
        assert_eq!(view.read(cx).error.as_deref(), Some("interaction hotkey is invalid"));
        assert_ne!(window.find("interaction-plan-edit").disabled(), Some(true), "edit re-enables after the rejection");
        window.click("interaction-plan-edit", cx);
    })
    .unwrap();
    assert!(matches!(requests.try_recv(), Ok(BackendCommand::Request { .. })), "rejected edit retries");
}
