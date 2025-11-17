/// Test that PtySessionGuard automatically decrements session count when dropped
#[test]
fn pty_session_guard_auto_cleanup() {
    use std::path::PathBuf;
    use vtcode_core::config::PtyConfig;
    use vtcode_core::tools::registry::PtySessionManager;

    let config = PtyConfig {
        enabled: true,
        max_sessions: 5,
        ..Default::default()
    };

    let manager = PtySessionManager::new(PathBuf::from("."), config);

    // Initially should be 0 active sessions
    assert_eq!(manager.active_sessions(), 0);

    // Start a session
    {
        let _guard = manager.start_session().expect("should start session");
        assert_eq!(manager.active_sessions(), 1);

        // The guard exists here

        // When we exit this scope, the guard is dropped and count decrements
    }

    // After guard is dropped, count should be back to 0
    assert_eq!(
        manager.active_sessions(),
        0,
        "session count should auto-decrement when guard is dropped"
    );
}

/// Test that multiple sessions can be tracked
#[test]
fn pty_session_guard_multiple_sessions() {
    use std::path::PathBuf;
    use vtcode_core::config::PtyConfig;
    use vtcode_core::tools::registry::PtySessionManager;

    let config = PtyConfig {
        enabled: true,
        max_sessions: 10,
        ..Default::default()
    };

    let manager = PtySessionManager::new(PathBuf::from("."), config);

    let _guard1 = manager.start_session().expect("session 1");
    assert_eq!(manager.active_sessions(), 1);

    let _guard2 = manager.start_session().expect("session 2");
    assert_eq!(manager.active_sessions(), 2);

    let _guard3 = manager.start_session().expect("session 3");
    assert_eq!(manager.active_sessions(), 3);

    drop(_guard2);
    assert_eq!(manager.active_sessions(), 2);

    drop(_guard1);
    assert_eq!(manager.active_sessions(), 1);

    drop(_guard3);
    assert_eq!(manager.active_sessions(), 0);
}

/// Test that session limit is enforced
#[test]
fn pty_session_guard_max_sessions() {
    use std::path::PathBuf;
    use vtcode_core::config::PtyConfig;
    use vtcode_core::tools::registry::PtySessionManager;

    let config = PtyConfig {
        enabled: true,
        max_sessions: 2,
        ..Default::default()
    };

    let manager = PtySessionManager::new(PathBuf::from("."), config);

    let _guard1 = manager.start_session().expect("session 1");
    let _guard2 = manager.start_session().expect("session 2");

    // Should fail: max sessions reached
    let result = manager.start_session();
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("Maximum PTY sessions")
    );

    // After dropping one, should succeed
    drop(_guard1);
    let _guard3 = manager
        .start_session()
        .expect("session 3 after freeing slot");
    assert_eq!(manager.active_sessions(), 2);
}
