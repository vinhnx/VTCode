use super::*;

#[test]
fn ctrl_c_state_check_cancellation_returns_error_on_exit() {
    let state = CtrlCState::new();

    // Get to exit state
    assert!(matches!(state.register_signal(), CtrlCSignal::Cancel));
    thread::sleep(Duration::from_millis(250));
    assert!(matches!(state.register_signal(), CtrlCSignal::Exit));

    // check_cancellation should return error
    assert!(state.check_cancellation().is_err());
}
#[test]
fn ctrl_c_state_check_cancellation_returns_error_on_cancel() {
    let state = CtrlCState::new();

    // Get to cancel state
    assert!(matches!(state.register_signal(), CtrlCSignal::Cancel));

    // check_cancellation should return error
    assert!(state.check_cancellation().is_err());
}
#[test]
fn ctrl_c_state_check_cancellation_ok_when_idle() {
    let state = CtrlCState::new();

    // check_cancellation should return Ok when idle
    assert!(state.check_cancellation().is_ok());
}
#[test]
fn ctrl_c_state_mark_cancel_handled_transitions_to_exit_armed() {
    let state = CtrlCState::new();

    // Get to cancel state
    assert!(matches!(state.register_signal(), CtrlCSignal::Cancel));
    assert!(state.is_cancel_requested());

    // Mark cancel handled should transition to ExitArmed
    state.mark_cancel_handled();

    // Should not be cancel requested anymore
    assert!(!state.is_cancel_requested());

    // Should not be exit requested yet
    assert!(!state.is_exit_requested());

    // Next Ctrl+C should exit
    assert!(matches!(state.register_signal(), CtrlCSignal::Exit));
    assert!(state.is_exit_requested());
}
#[test]
fn ctrl_c_state_local_cancel_does_not_arm_exit_window() {
    let state = CtrlCState::new();

    assert!(matches!(state.register_signal(), CtrlCSignal::Cancel));
    state.mark_cancel_handled();
    state.request_local_cancel();

    assert!(state.is_cancel_requested());
    assert!(!state.is_exit_requested());
}

#[test]
fn resetting_handled_cancel_preserves_pending_cancel_and_exit() {
    let state = CtrlCState::new();
    assert!(!state.reset_handled_cancel());
    state.request_local_cancel();
    assert!(!state.reset_handled_cancel());
    assert!(state.is_cancel_requested());
    state.mark_cancel_handled();
    assert!(state.reset_handled_cancel());
    assert!(!state.is_cancel_requested());
    state.request_exit();
    assert!(!state.reset_handled_cancel());
    assert!(state.is_exit_requested());
}
#[test]
fn ctrl_c_state_window_expires_after_one_second() {
    let state = CtrlCState::new();

    // First Ctrl+C
    assert!(matches!(state.register_signal(), CtrlCSignal::Cancel));

    // Wait for window to expire (1.1 seconds)
    thread::sleep(Duration::from_millis(1100));

    // Second Ctrl+C should cancel again, not exit
    assert!(matches!(state.register_signal(), CtrlCSignal::Cancel));
    assert!(state.is_cancel_requested());
    assert!(!state.is_exit_requested());
}
#[test]
fn ctrl_c_state_reset_preserves_exit() {
    let state = CtrlCState::new();

    // Get to exit state
    assert!(matches!(state.register_signal(), CtrlCSignal::Cancel));
    thread::sleep(Duration::from_millis(250));
    assert!(matches!(state.register_signal(), CtrlCSignal::Exit));
    assert!(state.is_exit_requested());

    // Only a new session may clear exit, by creating a new state.
    state.reset();

    assert!(!state.is_cancel_requested());
    assert!(state.is_exit_requested());
    assert!(state.check_cancellation().is_err());
}
#[test]
fn ctrl_c_state_atomic_operations_are_thread_safe() {
    // This test verifies that atomic operations work correctly under concurrency
    let state = Arc::new(CtrlCState::new());
    let mut handles = vec![];

    // Spawn multiple threads that all try to register signals
    for _ in 0..10 {
        let state = Arc::clone(&state);
        handles.push(thread::spawn(move || {
            for _ in 0..100 {
                let _ = state.register_signal();
            }
        }));
    }

    // Wait for all threads to complete
    for handle in handles {
        handle.join().unwrap();
    }

    // The state should be consistent (either cancel or exit requested)
    // The exact state depends on timing, but it should be valid
    assert!(state.is_cancel_requested() || state.is_exit_requested());
}
