use super::{prepare_responses_request_messages, record_terminal_turn_event, tool_loop_limit_reached};
use crate::core::agent::events::ExecEventRecorder;
use crate::core::agent::session::AgentSessionState;
use crate::core::agent::state::should_apply_local_tool_result_clearing;
use crate::core::agent::task::TaskOutcome;
use crate::exec::events::ThreadEvent;
use crate::llm::provider::{Message, records_responses_continuation_state};

#[test]
fn headless_local_clearing_gate_ignores_provider_context_edit_capability() {
    // Headless requests never attach native context management, so the
    // gate must run with context_edits == false even for Anthropic;
    // otherwise headless Anthropic gets neither native nor local
    // clearing and old tool bodies ride every request.
    assert!(should_apply_local_tool_result_clearing("anthropic", false, true));
    assert!(!should_apply_local_tool_result_clearing("anthropic", false, false));
    // Interactive semantics stay pinned: native edits exclude the local
    // pass, other providers use it regardless of capability.
    assert!(!should_apply_local_tool_result_clearing("anthropic", true, true));
    assert!(should_apply_local_tool_result_clearing("openai", true, true));
}

#[test]
fn failed_outcome_emits_only_turn_failed() {
    let mut recorder = ExecEventRecorder::new("thread", None, None);
    recorder.turn_started();

    record_terminal_turn_event(
        &mut recorder,
        &TaskOutcome::failed("boom".to_string(), vec![], None, None),
        Default::default(),
    );

    let events = recorder.into_events();
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, ThreadEvent::TurnFailed(_)))
            .count(),
        1
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, ThreadEvent::TurnCompleted(_)))
            .count(),
        0
    );
}

#[test]
fn successful_outcome_emits_only_turn_completed() {
    let mut recorder = ExecEventRecorder::new("thread", None, None);
    recorder.turn_started();

    record_terminal_turn_event(&mut recorder, &TaskOutcome::Success, Default::default());

    let events = recorder.into_events();
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, ThreadEvent::TurnCompleted(_)))
            .count(),
        1
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, ThreadEvent::TurnFailed(_)))
            .count(),
        0
    );
}

#[test]
fn disabled_tool_loop_limit_never_trips() {
    assert!(!tool_loop_limit_reached(1, 0));
    assert!(!tool_loop_limit_reached(32, 0));
}

#[test]
fn openai_prepare_responses_request_messages_keeps_full_history_without_previous_response_id() {
    let mut state = AgentSessionState::new("session".to_string(), 4, 4, 16_000);
    let prior_messages = vec![Message::user("hello".to_string())];
    let current_messages = vec![
        Message::user("hello".to_string()),
        Message::user("continue".to_string()),
    ];
    state.set_previous_response_chain("openai", "gpt-5.6-sol", Some("resp_123"), prior_messages);

    let (request_messages, previous_response_id) = prepare_responses_request_messages(
        &mut state.previous_response_chains,
        "openai",
        false,
        "gpt-5.6-sol",
        &current_messages,
    );

    assert_eq!(previous_response_id, None);
    assert_eq!(request_messages.as_ref(), current_messages.as_slice());
}

#[test]
fn openai_runner_success_path_does_not_record_previous_response_chain() {
    let mut state = AgentSessionState::new("session".to_string(), 4, 4, 16_000);
    let messages = vec![Message::user("hello".to_string())];

    if records_responses_continuation_state("openai", true) {
        state.set_previous_response_chain("openai", "gpt-5.6-sol", Some("resp_123"), messages);
    }

    assert_eq!(state.previous_response_chain_for("openai", "gpt-5.6-sol"), None);
}

#[test]
fn compatible_runner_success_path_does_not_record_previous_response_chain() {
    let mut state = AgentSessionState::new("session".to_string(), 4, 4, 16_000);
    let messages = vec![Message::user("hello".to_string())];

    if records_responses_continuation_state("mycorp", true) {
        state.set_previous_response_chain("mycorp", "gpt-5.6-sol", Some("resp_123"), messages);
    }

    assert_eq!(state.previous_response_chain_for("mycorp", "gpt-5.6-sol"), None);
}

#[test]
fn gemini_prepare_responses_request_messages_keeps_full_history() {
    let mut state = AgentSessionState::new("session".to_string(), 4, 4, 16_000);
    let prior_messages = vec![Message::user("hello".to_string())];
    let current_messages = vec![
        Message::user("hello".to_string()),
        Message::user("continue".to_string()),
    ];
    state.set_previous_response_chain("gemini", "gemini-2.5-pro", Some("resp_123"), prior_messages);

    let (request_messages, previous_response_id) = prepare_responses_request_messages(
        &mut state.previous_response_chains,
        "gemini",
        false,
        "gemini-2.5-pro",
        &current_messages,
    );

    assert_eq!(previous_response_id.as_deref(), Some("resp_123"));
    assert_eq!(request_messages.as_ref(), current_messages.as_slice());
}

#[test]
fn compatible_prepare_responses_request_messages_keeps_custom_provider_stateless() {
    let mut state = AgentSessionState::new("session".to_string(), 4, 4, 16_000);
    let prior_messages = vec![Message::user("hello".to_string())];
    let current_messages = vec![
        Message::user("hello".to_string()),
        Message::user("continue".to_string()),
    ];
    state.set_previous_response_chain("mycorp", "gpt-5.6-sol", Some("resp_123"), prior_messages);

    let (request_messages, previous_response_id) = prepare_responses_request_messages(
        &mut state.previous_response_chains,
        "mycorp",
        true,
        "gpt-5.6-sol",
        &current_messages,
    );

    assert_eq!(previous_response_id, None);
    assert_eq!(request_messages.as_ref(), current_messages.as_slice());
}
