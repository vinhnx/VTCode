use std::sync::Arc;

use crate::agent::runloop::ui_list;
use crate::agent::runloop::ui_list::Tone;
use anyhow::Result;
use tokio::sync::Notify;
use vtcode_commons::modal_hints::{APPROVAL_NAVIGATE_DENY, APPROVAL_NAVIGATE_STOP};
use vtcode_core::config::constants::tool_limits::MAX_TOOL_LOOP_INCREMENT_PER_PROMPT;
use vtcode_core::core::interfaces::ui::UiSession;
use vtcode_ui::tui::app::{InlineHandle, ListOverlayRequest, TransientRequest, TransientSubmission};

use crate::agent::runloop::unified::overlay_prompt::{OverlayWaitOutcome, show_permission_and_wait};
use crate::agent::runloop::unified::state::CtrlCState;

pub(super) async fn prompt_session_limit_increase<S: UiSession + ?Sized>(
    handle: &InlineHandle,
    session: &mut S,
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
    max_limit: usize,
    agent_name: Option<&str>,
) -> Result<Option<usize>> {
    use vtcode_ui::tui::app::InlineListSelection;

    let description_lines = vec![
        format!("Session tool limit reached: {}", max_limit),
        format!("Current agent: {}", agent_name.unwrap_or("unknown")),
        "Grant an increase to retry the pending tool call in this turn.".to_string(),
        "Deny stops the call; reuse the outputs already gathered for the next response.".to_string(),
    ];

    let options = vec![
        ui_list::action(
            "+100 tool calls",
            "Increase the session limit by 100",
            None,
            Tone::Neutral,
            Some(InlineListSelection::SessionLimitIncrease(100)),
        )
        .with_search_value("increase 100 hundred plus more".to_string()),
        ui_list::action(
            "+50 tool calls",
            "Increase the session limit by 50",
            None,
            Tone::Neutral,
            Some(InlineListSelection::SessionLimitIncrease(50)),
        )
        .with_search_value("increase 50 fifty plus more".to_string()),
        ui_list::group_divider(),
        ui_list::action(
            "Deny",
            "Do not increase limit (stops tool execution)",
            None,
            Tone::Neutral,
            Some(InlineListSelection::ToolApproval(false)),
        )
        .with_search_value("deny no exit stop cancel".to_string()),
    ];

    prompt_limit_increase_modal(
        handle,
        session,
        ctrl_c_state,
        ctrl_c_notify,
        "Session Limit Reached".to_string(),
        description_lines,
        options,
        100,
        APPROVAL_NAVIGATE_DENY,
    )
    .await
}

/// Standard tool-loop grant candidates, largest first. Bounded by
/// `MAX_TOOL_LOOP_INCREMENT_PER_PROMPT` (50).
const TOOL_LOOP_GRANT_CANDIDATES: [usize; 3] = [50, 20, 10];

const _: () = {
    assert!(TOOL_LOOP_GRANT_CANDIDATES[0] <= MAX_TOOL_LOOP_INCREMENT_PER_PROMPT);
    assert!(TOOL_LOOP_GRANT_CANDIDATES[0] > TOOL_LOOP_GRANT_CANDIDATES[1]);
    assert!(TOOL_LOOP_GRANT_CANDIDATES[1] > TOOL_LOOP_GRANT_CANDIDATES[2]);
};

/// Search text for a grant option. Keeps the original number-word aliases
/// (`fifty`/`twenty`/`ten`) so type-ahead filtering still matches words as
/// well as digits; custom remainder options fall back to digits only.
fn tool_loop_search_value(increment: usize) -> String {
    let word = match increment {
        50 => " fifty",
        20 => " twenty",
        10 => " ten",
        _ => "",
    };
    format!("increase {increment}{word} plus more continue")
}

/// Viable grant increments for the remaining headroom below the hard cap.
///
/// Filters the standard candidates to those that fit, and prepends the exact
/// remaining headroom as a one-shot "reaches cap" option when it is smaller
/// than the largest candidate (e.g. remaining 40 -> `[40, 20, 10]`). When the
/// remaining headroom is smaller than every candidate (e.g. 5), returns just
/// the remainder so the turn can consume the cap without another prompt loop.
/// Returns empty when there is no headroom.
fn tool_loop_grant_options(remaining_headroom: usize) -> Vec<usize> {
    if remaining_headroom == 0 {
        return Vec::new();
    }
    let mut viable: Vec<usize> = TOOL_LOOP_GRANT_CANDIDATES
        .iter()
        .copied()
        .filter(|candidate| *candidate <= remaining_headroom)
        .collect();
    if viable.is_empty() {
        return vec![remaining_headroom];
    }
    let largest_candidate = TOOL_LOOP_GRANT_CANDIDATES[0];
    if remaining_headroom < largest_candidate && !viable.contains(&remaining_headroom) {
        viable.insert(0, remaining_headroom);
    }
    viable
}

pub(super) async fn prompt_tool_loop_limit_increase<S: UiSession + ?Sized>(
    handle: &InlineHandle,
    session: &mut S,
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
    max_limit: usize,
    hard_cap: usize,
    agent_name: Option<&str>,
) -> Result<Option<usize>> {
    use vtcode_ui::tui::app::{InlineListItem, InlineListSelection};

    let remaining_headroom = hard_cap.saturating_sub(max_limit);
    if remaining_headroom == 0 {
        return Ok(None);
    }
    let viable_increments = tool_loop_grant_options(remaining_headroom);
    if viable_increments.is_empty() {
        return Ok(None);
    }

    let description_lines = vec![
        format!("Maximum tool loops reached: {max_limit} (cap {hard_cap}, {remaining_headroom} remaining)"),
        format!("Current agent: {}", agent_name.unwrap_or("unknown")),
        "Grant more loops to continue this turn with the current agent.".to_string(),
        "Stop synthesizes from the outputs already gathered.".to_string(),
    ];

    let mut options: Vec<InlineListItem> = viable_increments
        .iter()
        .map(|increment| {
            let reaches_cap = *increment == remaining_headroom;
            let subtitle = if reaches_cap {
                format!("Continue with {increment} more tool loops (reaches cap)")
            } else {
                format!("Continue with {increment} more tool loops")
            };
            ui_list::action(
                format!("+{increment} tool loops"),
                subtitle,
                None,
                Tone::Neutral,
                Some(InlineListSelection::SessionLimitIncrease(*increment)),
            )
            .with_search_value(tool_loop_search_value(*increment))
        })
        .collect();
    options.push(ui_list::group_divider());
    options.push(
        ui_list::action(
            "Stop",
            "Stop the current turn and wait for input",
            None,
            Tone::Neutral,
            Some(InlineListSelection::ToolApproval(false)),
        )
        .with_search_value("stop no exit cancel done".to_string()),
    );

    let default_increment = viable_increments.first().copied().unwrap_or(remaining_headroom);

    prompt_limit_increase_modal(
        handle,
        session,
        ctrl_c_state,
        ctrl_c_notify,
        "Tool Loop Limit Reached".to_string(),
        description_lines,
        options,
        default_increment,
        APPROVAL_NAVIGATE_STOP,
    )
    .await
}

async fn prompt_limit_increase_modal<S: UiSession + ?Sized>(
    handle: &InlineHandle,
    session: &mut S,
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
    title: String,
    description_lines: Vec<String>,
    options: Vec<vtcode_ui::tui::app::InlineListItem>,
    default_increment: usize,
    footer_hint: &'static str,
) -> Result<Option<usize>> {
    use vtcode_ui::tui::app::InlineListSelection;

    // A bridge submission is deferred while the modal owns the input surface.
    // Re-show this limit prompt and continue waiting so that a transient
    // bridge event cannot accidentally become a denial of the grant.
    loop {
        let outcome = show_permission_and_wait(
            handle,
            session,
            TransientRequest::List(ListOverlayRequest {
                title: title.clone(),
                lines: description_lines.clone(),
                footer_hint: Some(footer_hint.to_string()),
                items: options.clone(),
                selected: Some(InlineListSelection::SessionLimitIncrease(default_increment)),
                search: None,
                hotkeys: Vec::new(),
                status: None,
            }),
            ctrl_c_state,
            ctrl_c_notify,
            |submission| match submission {
                TransientSubmission::Selection(InlineListSelection::SessionLimitIncrease(inc)) => Some(inc),
                // All grant options are strictly positive; zero is the
                // explicit Deny/Stop selection sentinel.
                TransientSubmission::Selection(InlineListSelection::ToolApproval(false)) => Some(0),
                TransientSubmission::Selection(_) => None,
                _ => None,
            },
        )
        .await?;

        match outcome {
            OverlayWaitOutcome::Submitted(0) => return Ok(None),
            OverlayWaitOutcome::Submitted(increment) => return Ok(Some(increment)),
            // Esc/Cancel is the user's explicit denial. Interrupt and Exit
            // remain distinct control-flow outcomes but also deny the grant.
            OverlayWaitOutcome::Cancelled | OverlayWaitOutcome::Interrupted | OverlayWaitOutcome::Exit => {
                return Ok(None);
            }
            OverlayWaitOutcome::Deferred => continue,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::collections::VecDeque;
    use tokio::sync::{Notify, mpsc};
    use vtcode_ui::tui::app::{InlineCommand, InlineEvent, InlineListSelection, TransientEvent, TransientSubmission};

    struct TestSession {
        handle: InlineHandle,
        events: VecDeque<InlineEvent>,
    }

    #[async_trait]
    impl UiSession for TestSession {
        fn inline_handle(&self) -> &InlineHandle {
            &self.handle
        }

        async fn next_event(&mut self) -> Option<InlineEvent> {
            self.events.pop_front()
        }
    }

    #[tokio::test]
    async fn deferred_bridge_event_reshows_limit_prompt_until_grant() {
        let (command_sender, mut command_receiver) = mpsc::unbounded_channel();
        let handle = InlineHandle::new_for_tests(command_sender);
        let mut session = TestSession {
            handle: handle.clone(),
            events: VecDeque::from([
                // A leaked mode-switch event belongs to the active turn and
                // must not dismiss the prompt.
                InlineEvent::CyclePrimaryAgent,
                InlineEvent::WebmcpSubmit("continue".into()),
                InlineEvent::Transient(TransientEvent::Submitted(TransientSubmission::Selection(
                    InlineListSelection::SessionLimitIncrease(50),
                ))),
            ]),
        };
        let ctrl_c_state = Arc::new(CtrlCState::new());
        let ctrl_c_notify = Arc::new(Notify::new());

        let result =
            prompt_session_limit_increase(&handle, &mut session, &ctrl_c_state, &ctrl_c_notify, 100, Some("build"))
                .await
                .expect("limit prompt should remain available after deferred input");

        assert_eq!(result, Some(50));
        let mut shown = 0;
        while let Ok(command) = command_receiver.try_recv() {
            if matches!(command, InlineCommand::ShowTransient { .. }) {
                shown += 1;
            }
        }
        assert_eq!(shown, 2, "the deferred bridge input should cause a re-show");
    }

    #[tokio::test]
    async fn cancel_is_an_explicit_denial() {
        let (command_sender, _command_receiver) = mpsc::unbounded_channel();
        let handle = InlineHandle::new_for_tests(command_sender);
        let mut session = TestSession {
            handle: handle.clone(),
            events: VecDeque::from([InlineEvent::Cancel]),
        };
        let ctrl_c_state = Arc::new(CtrlCState::new());
        let ctrl_c_notify = Arc::new(Notify::new());

        let result =
            prompt_session_limit_increase(&handle, &mut session, &ctrl_c_state, &ctrl_c_notify, 100, Some("build"))
                .await
                .expect("cancel should be handled as a denial");

        assert_eq!(result, None);
    }

    #[tokio::test]
    async fn deny_selection_is_an_explicit_denial() {
        let (command_sender, _command_receiver) = mpsc::unbounded_channel();
        let handle = InlineHandle::new_for_tests(command_sender);
        let mut session = TestSession {
            handle: handle.clone(),
            events: VecDeque::from([InlineEvent::Transient(TransientEvent::Submitted(
                TransientSubmission::Selection(InlineListSelection::ToolApproval(false)),
            ))]),
        };
        let ctrl_c_state = Arc::new(CtrlCState::new());
        let ctrl_c_notify = Arc::new(Notify::new());

        let result =
            prompt_session_limit_increase(&handle, &mut session, &ctrl_c_state, &ctrl_c_notify, 100, Some("build"))
                .await
                .expect("deny selection should be handled as a denial");

        assert_eq!(result, None);
    }

    #[test]
    fn tool_loop_grant_options_fit_remaining_headroom() {
        assert_eq!(tool_loop_grant_options(0), Vec::<usize>::new());
        assert_eq!(tool_loop_grant_options(50), vec![50, 20, 10]);
        assert_eq!(tool_loop_grant_options(100), vec![50, 20, 10]);
        assert_eq!(tool_loop_grant_options(60), vec![50, 20, 10]);
    }

    #[test]
    fn tool_loop_grant_options_offer_exact_remainder_below_max_candidate() {
        // Reported bug: remaining 40 offered +50, then clamped to +40.
        assert_eq!(tool_loop_grant_options(40), vec![40, 20, 10]);
        assert_eq!(tool_loop_grant_options(25), vec![25, 20, 10]);
        assert_eq!(tool_loop_grant_options(15), vec![15, 10]);
        assert_eq!(tool_loop_grant_options(20), vec![20, 10]);
        assert_eq!(tool_loop_grant_options(10), vec![10]);
    }

    #[test]
    fn tool_loop_grant_options_offer_single_remainder_when_below_smallest_candidate() {
        assert_eq!(tool_loop_grant_options(9), vec![9]);
        assert_eq!(tool_loop_grant_options(5), vec![5]);
        assert_eq!(tool_loop_grant_options(1), vec![1]);
    }

    #[test]
    fn tool_loop_search_value_keeps_number_word_aliases() {
        assert!(tool_loop_search_value(50).contains("fifty"));
        assert!(tool_loop_search_value(20).contains("twenty"));
        assert!(tool_loop_search_value(10).contains("ten"));
        assert!(tool_loop_search_value(40).contains("40"));
    }

    #[tokio::test]
    async fn at_cap_tool_loop_prompt_returns_denial_without_modal() {
        let (command_sender, mut command_receiver) = mpsc::unbounded_channel();
        let handle = InlineHandle::new_for_tests(command_sender);
        let mut session = TestSession { handle: handle.clone(), events: VecDeque::new() };
        let ctrl_c_state = Arc::new(CtrlCState::new());
        let ctrl_c_notify = Arc::new(Notify::new());

        let result = prompt_tool_loop_limit_increase(
            &handle,
            &mut session,
            &ctrl_c_state,
            &ctrl_c_notify,
            60,
            60,
            Some("build"),
        )
        .await
        .expect("at-cap prompt should fail closed without modal");

        assert_eq!(result, None);
        while let Ok(command) = command_receiver.try_recv() {
            assert!(!matches!(command, InlineCommand::ShowTransient { .. }), "at-cap must not show a grant modal");
        }
    }
}
