use super::*;

#[test]
fn ctrl_c_state_escalates_to_exit_within_window() {
    let state = CtrlCState::new();

    assert!(matches!(state.register_signal(), CtrlCSignal::Cancel));
    thread::sleep(Duration::from_millis(250));
    assert!(matches!(state.register_signal(), CtrlCSignal::Exit));
}
#[test]
fn ctrl_c_state_reset_clears_exit_window() {
    let state = CtrlCState::new();

    assert!(matches!(state.register_signal(), CtrlCSignal::Cancel));
    state.reset();
    thread::sleep(Duration::from_millis(250));

    assert!(matches!(state.register_signal(), CtrlCSignal::Cancel));
    assert!(state.is_cancel_requested());
    assert!(!state.is_exit_requested());
}
#[test]
fn ctrl_c_state_mark_cancel_handled_keeps_exit_window_armed() {
    let state = CtrlCState::new();

    assert!(matches!(state.register_signal(), CtrlCSignal::Cancel));
    state.mark_cancel_handled();
    thread::sleep(Duration::from_millis(250));

    assert!(matches!(state.register_signal(), CtrlCSignal::Exit));
    assert!(state.is_exit_requested());
}
#[test]
fn ctrl_c_state_allows_immediate_exit_after_cancel_handled() {
    let state = CtrlCState::new();

    assert!(matches!(state.register_signal(), CtrlCSignal::Cancel));
    state.mark_cancel_handled();

    assert!(matches!(state.register_signal(), CtrlCSignal::Exit));
    assert!(state.is_exit_requested());
}
#[test]
fn ctrl_c_state_escalation_is_priority_guarantee() {
    // This test verifies the priority guarantee: double Ctrl+C always exits
    // Note: The 200ms debounce window prevents immediate escalation
    let state = CtrlCState::new();

    // First Ctrl+C - should always cancel
    assert!(matches!(state.register_signal(), CtrlCSignal::Cancel));
    assert!(state.is_cancel_requested());
    assert!(!state.is_exit_requested());

    // Second Ctrl+C within 1 second (but after 200ms debounce) - should exit
    thread::sleep(Duration::from_millis(250));
    assert!(matches!(state.register_signal(), CtrlCSignal::Exit));
    assert!(!state.is_cancel_requested());
    assert!(state.is_exit_requested());
}
#[test]
fn ctrl_c_state_debounce_prevents_accidental_escalation() {
    let state = CtrlCState::new();

    // First Ctrl+C
    assert!(matches!(state.register_signal(), CtrlCSignal::Cancel));

    // Rapid second Ctrl+C (within 200ms debounce window)
    // Should still cancel, not exit
    thread::sleep(Duration::from_millis(50));
    assert!(matches!(state.register_signal(), CtrlCSignal::Cancel));
    assert!(state.is_cancel_requested());
    assert!(!state.is_exit_requested());
}
#[test]
fn ctrl_c_state_exit_is_always_processed() {
    // This test verifies that exit signals are always processed
    let state = CtrlCState::new();

    // Get to exit state
    assert!(matches!(state.register_signal(), CtrlCSignal::Cancel));
    thread::sleep(Duration::from_millis(250));
    assert!(matches!(state.register_signal(), CtrlCSignal::Exit));

    // Subsequent Ctrl+C should always return Exit
    assert!(matches!(state.register_signal(), CtrlCSignal::Exit));
    assert!(matches!(state.register_signal(), CtrlCSignal::Exit));
    assert!(matches!(state.register_signal(), CtrlCSignal::Exit));
    assert!(state.is_exit_requested());
}
