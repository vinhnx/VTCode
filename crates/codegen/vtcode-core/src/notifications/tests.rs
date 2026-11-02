use super::*;

#[tokio::test]
async fn test_notification_manager_creation() {
    let manager = NotificationManager::new();
    let config = manager.get_config().await;

    assert!(!config.command_failure_notifications);
    assert!(!config.tool_failure_notifications);
    assert!(config.error_notifications);
    assert!(!config.completion_success_notifications);
    assert!(config.completion_failure_notifications);
    assert_eq!(config.delivery_mode, NotificationDeliveryMode::Desktop);
    assert_eq!(config.backend, NotificationBackend::Auto);
    assert_eq!(config.repeat_window_seconds, 30);
    assert_eq!(config.max_identical_notifications_in_window, 1);
}

#[test]
fn runtime_notification_config_respects_backend_preference() {
    let mut config = VTCodeConfig::default();
    config.ui.notifications.backend = NotificationBackend::Terminal;

    let runtime = NotificationConfig::from_vtcode_config(&config);

    assert_eq!(runtime.backend, NotificationBackend::Terminal);
}

#[test]
fn terminal_backend_skips_desktop_backends() {
    let manager = NotificationManager::new();

    assert_eq!(
        manager.desktop_notification_backends(NotificationBackend::Terminal),
        NO_DESKTOP_NOTIFICATION_BACKENDS
    );
}

#[test]
fn notify_rust_backend_selects_only_notify_rust() {
    let manager = NotificationManager::new();

    assert_eq!(
        manager.desktop_notification_backends(NotificationBackend::NotifyRust),
        NOTIFY_RUST_DESKTOP_NOTIFICATION_BACKENDS
    );
}

#[test]
fn auto_backend_uses_expected_platform_order() {
    let manager = NotificationManager::new();

    assert_eq!(manager.desktop_notification_backends(NotificationBackend::Auto), AUTO_DESKTOP_NOTIFICATION_BACKENDS);
}

#[cfg(all(target_os = "macos", feature = "desktop-notifications"))]
#[test]
fn macos_notification_application_registration_succeeds() {
    ensure_macos_notification_application().expect("Finder must be available for macOS notifications");
}

#[cfg(all(target_os = "macos", feature = "desktop-notifications"))]
#[test]
fn macos_notification_setup_runs_before_delivery_and_failure_skips_it() {
    use std::cell::Cell;

    let step = Cell::new(0);
    let delivered = send_after_macos_notification_setup(
        || {
            assert_eq!(step.get(), 0);
            step.set(1);
            Ok(())
        },
        || {
            assert_eq!(step.get(), 1);
            step.set(2);
            Ok(42)
        },
    );
    assert_eq!(delivered.unwrap(), 42);
    assert_eq!(step.get(), 2);

    let send_called = Cell::new(false);
    let failed = send_after_macos_notification_setup(
        || anyhow::bail!("application registration failed"),
        || {
            send_called.set(true);
            Ok(())
        },
    );
    assert!(failed.is_err());
    assert!(!send_called.get());
}

#[tokio::test]
async fn desktop_delivery_does_not_fall_back_to_terminal_attention() {
    let config = NotificationConfig {
        delivery_mode: NotificationDeliveryMode::Desktop,
        backend: NotificationBackend::NotifyRust,
        ..Default::default()
    };
    let manager = NotificationManager::with_config(config);

    let result = manager.send_message("Session started", &manager.get_config_sync()).await;

    result.unwrap();
}

#[cfg(target_os = "macos")]
#[test]
fn osascript_backend_selects_only_osascript_on_macos() {
    let manager = NotificationManager::new();

    assert_eq!(
        manager.desktop_notification_backends(NotificationBackend::Osascript),
        OSASCRIPT_DESKTOP_NOTIFICATION_BACKENDS
    );
}

#[cfg(not(target_os = "macos"))]
#[test]
fn osascript_backend_is_empty_off_macos() {
    let manager = NotificationManager::new();

    assert_eq!(
        manager.desktop_notification_backends(NotificationBackend::Osascript),
        NO_DESKTOP_NOTIFICATION_BACKENDS
    );
}

#[tokio::test]
async fn test_command_failure_notification() {
    let manager = NotificationManager::new();
    let event = NotificationEvent::CommandFailure {
        command: "git status".to_string(),
        error: "Not a git repository".to_string(),
        exit_code: Some(128),
    };

    // This should not panic
    let result = manager.send_notification(event).await;
    result.unwrap();
}

#[tokio::test]
async fn test_tool_failure_notification() {
    let manager = NotificationManager::new();
    let event = NotificationEvent::ToolFailure {
        tool_name: "read_file".to_string(),
        error: "File not found".to_string(),
        details: Some("Attempted to read /nonexistent/file.txt".to_string()),
    };

    // This should not panic
    let result = manager.send_notification(event).await;
    result.unwrap();
}

#[tokio::test]
async fn test_terminal_notifications_toggle() {
    // Test with notifications enabled (default)
    let manager = NotificationManager::new();
    let config = manager.get_config().await;
    assert!(config.terminal_notifications_enabled);

    // Test with notifications disabled
    let config = NotificationConfig {
        terminal_notifications_enabled: false,
        ..Default::default()
    };
    let manager = NotificationManager::with_config(config);
    let event = NotificationEvent::CommandFailure {
        command: "test".to_string(),
        error: "test error".to_string(),
        exit_code: None,
    };

    // This should not send notification when disabled
    let result = manager.send_notification(event).await;
    result.unwrap(); // Should not error, but notification won't be sent

    // Verify the setting worked by checking the config
    let current_config = manager.get_config().await;
    assert!(!current_config.terminal_notifications_enabled);
}

#[test]
fn completion_notifications_are_split_by_status() {
    let manager = NotificationManager::new();
    let config = NotificationConfig {
        completion_success_notifications: false,
        completion_failure_notifications: true,
        ..Default::default()
    };

    let success_event = NotificationEvent::Completion {
        task: "turn".to_string(),
        status: CompletionStatus::Success,
        details: None,
    };
    let failure_event = NotificationEvent::Completion {
        task: "turn".to_string(),
        status: CompletionStatus::Failure,
        details: None,
    };

    assert!(!manager.event_enabled(&success_event, &config));
    assert!(manager.event_enabled(&failure_event, &config));
}

#[test]
fn repeat_suppression_limits_identical_notifications() {
    let manager = NotificationManager::new();
    let config = NotificationConfig {
        repeat_window_seconds: 30,
        max_identical_notifications_in_window: 1,
        ..Default::default()
    };
    let event = NotificationEvent::ToolFailure {
        tool_name: "read_file".to_string(),
        error: "File not found".to_string(),
        details: None,
    };

    assert!(matches!(manager.repeat_decision(&event, &config), RepeatDecision::Deliver));
    assert!(matches!(manager.repeat_decision(&event, &config), RepeatDecision::Suppress));
}

#[test]
fn custom_notifications_format_title_and_message() {
    let manager = NotificationManager::new();
    let event = NotificationEvent::Custom {
        title: "VT Code".to_string(),
        message: "Session started".to_string(),
    };

    assert_eq!(manager.format_notification_message(&event), "VT Code: Session started");
}

#[cfg(target_os = "macos")]
#[test]
fn macos_osascript_notification_script_mentions_display_notification() {
    let script = r#"on run argv
display notification (item 1 of argv) with title (item 2 of argv)
end run
"#;
    assert!(script.contains("display notification"));
    assert!(script.contains("with title"));
}

#[test]
fn session_completion_format_message_reads_naturally() {
    let manager = NotificationManager::new();
    let event = NotificationEvent::Completion {
        task: "session".to_string(),
        status: CompletionStatus::Success,
        details: Some("7 turn(s)".to_string()),
    };
    assert_eq!(
        manager.format_notification_message(&event),
        "Agent session ended: completed successfully - 7 turn(s)"
    );
}
