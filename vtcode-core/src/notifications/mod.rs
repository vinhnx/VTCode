//! Push notification system for VTCode terminal clients
//! Handles important events like command failures, errors, policy approval requests,
//! human in the loop interactions, completion and requests.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Types of important events that trigger notifications
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NotificationEvent {
    /// Command execution failed
    CommandFailure {
        command: String,
        error: String,
        exit_code: Option<i32>,
    },
    /// Tool execution failed
    ToolFailure {
        tool_name: String,
        error: String,
        details: Option<String>,
    },
    /// General error occurred
    Error {
        message: String,
        context: Option<String>,
    },
    /// Policy approval required for action
    PolicyApprovalRequest { action: String, details: String },
    /// Human in the loop interaction required
    HumanInTheLoop { prompt: String, context: String },
    /// Task or operation completed
    Completion {
        task: String,
        status: CompletionStatus,
        details: Option<String>,
    },
    /// Request received
    Request {
        request_type: String,
        details: String,
    },
}

/// Status of a completed task
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CompletionStatus {
    Success,
    PartialSuccess,
    Failure,
    Cancelled,
}

/// Notification configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationConfig {
    /// Enable command failure notifications
    pub command_failure_notifications: bool,
    /// Enable error notifications
    pub error_notifications: bool,
    /// Enable policy approval request notifications
    pub policy_approval_notifications: bool,
    /// Enable human in the loop notifications
    pub hitl_notifications: bool,
    /// Enable completion notifications
    pub completion_notifications: bool,
    /// Enable request notifications
    pub request_notifications: bool,
    /// Use terminal bell for notifications
    pub use_terminal_bell: bool,
    /// Use rich notifications (desktop notifications if supported)
    pub use_rich_notifications: bool,
    /// Enable/disable all terminal notifications (overrides other settings)
    pub terminal_notifications_enabled: bool,
}

impl Default for NotificationConfig {
    fn default() -> Self {
        Self {
            command_failure_notifications: true,
            error_notifications: true,
            policy_approval_notifications: true,
            hitl_notifications: true,
            completion_notifications: true,
            request_notifications: true,
            use_terminal_bell: true,
            use_rich_notifications: true,
            terminal_notifications_enabled: true,
        }
    }
}

/// Notification manager that handles sending notifications
pub struct NotificationManager {
    config: Arc<RwLock<NotificationConfig>>,
}

impl NotificationManager {
    /// Create a new notification manager with default configuration
    pub fn new() -> Self {
        Self {
            config: Arc::new(RwLock::new(NotificationConfig::default())),
        }
    }

    /// Create a new notification manager with custom configuration
    pub fn with_config(config: NotificationConfig) -> Self {
        Self {
            config: Arc::new(RwLock::new(config)),
        }
    }

    /// Send a notification for an event
    pub async fn send_notification(&self, event: NotificationEvent) -> Result<()> {
        let config = self.config.read().await;

        // Check if terminal notifications are enabled globally first
        if !config.terminal_notifications_enabled {
            return Ok(());
        }

        match &event {
            NotificationEvent::CommandFailure { .. } => {
                if config.command_failure_notifications {
                    self.send_notification_impl(&event, &config).await?;
                }
            }
            NotificationEvent::ToolFailure { .. } => {
                if config.command_failure_notifications {
                    // Using same config as command failures
                    self.send_notification_impl(&event, &config).await?;
                }
            }
            NotificationEvent::Error { .. } => {
                if config.error_notifications {
                    self.send_notification_impl(&event, &config).await?;
                }
            }
            NotificationEvent::PolicyApprovalRequest { .. } => {
                if config.policy_approval_notifications {
                    self.send_notification_impl(&event, &config).await?;
                }
            }
            NotificationEvent::HumanInTheLoop { .. } => {
                if config.hitl_notifications {
                    self.send_notification_impl(&event, &config).await?;
                }
            }
            NotificationEvent::Completion { .. } => {
                if config.completion_notifications {
                    self.send_notification_impl(&event, &config).await?;
                }
            }
            NotificationEvent::Request { .. } => {
                if config.request_notifications {
                    self.send_notification_impl(&event, &config).await?;
                }
            }
        }

        Ok(())
    }

    /// Internal method to send the actual notification
    async fn send_notification_impl(
        &self,
        event: &NotificationEvent,
        config: &NotificationConfig,
    ) -> Result<()> {
        // Check if terminal notifications are enabled
        if !config.terminal_notifications_enabled {
            return Ok(());
        }

        // Format the notification message based on the event type
        let message = self.format_notification_message(event);

        // Send terminal bell if configured
        if config.use_terminal_bell {
            self.send_terminal_bell(&message).await;
        }

        // Send rich notification if configured
        if config.use_rich_notifications {
            self.send_rich_notification(&message).await;
        }

        Ok(())
    }

    /// Format a notification message based on the event
    fn format_notification_message(&self, event: &NotificationEvent) -> String {
        match event {
            NotificationEvent::CommandFailure {
                command,
                error,
                exit_code,
            } => {
                let exit_code_str = exit_code
                    .map(|code| format!(" (exit code: {})", code))
                    .unwrap_or_default();
                format!(
                    "Command failed: {}{} - Error: {}",
                    command, exit_code_str, error
                )
            }
            NotificationEvent::ToolFailure {
                tool_name,
                error,
                details,
            } => {
                let details_str = details
                    .as_ref()
                    .map(|d| format!(" - Details: {}", d))
                    .unwrap_or_default();
                format!("Tool '{}' failed: {}{}", tool_name, error, details_str)
            }
            NotificationEvent::Error { message, context } => {
                let context_str = context
                    .as_ref()
                    .map(|ctx| format!(" [{}]", ctx))
                    .unwrap_or_default();
                format!("Error occurred{}: {}", context_str, message)
            }
            NotificationEvent::PolicyApprovalRequest { action, details } => {
                format!("Policy approval required: {} - {}", action, details)
            }
            NotificationEvent::HumanInTheLoop { prompt, context } => {
                format!("Human input required: {} [Context: {}]", prompt, context)
            }
            NotificationEvent::Completion {
                task,
                status,
                details,
            } => {
                let status_str = match status {
                    CompletionStatus::Success => "completed successfully",
                    CompletionStatus::PartialSuccess => "partially completed",
                    CompletionStatus::Failure => "failed",
                    CompletionStatus::Cancelled => "was cancelled",
                };
                let details_str = details
                    .as_ref()
                    .map(|d| format!(" - {}", d))
                    .unwrap_or_default();
                format!("Task '{}' {}{}", task, status_str, details_str)
            }
            NotificationEvent::Request {
                request_type,
                details,
            } => {
                format!("New {} request: {}", request_type, details)
            }
        }
    }

    /// Send a terminal bell notification
    async fn send_terminal_bell(&self, message: &str) {
        use crate::utils::ansi_codes::notify_attention;
        notify_attention(true, Some(message));
    }

    /// Send a rich notification (desktop notifications when available)
    async fn send_rich_notification(&self, message: &str) {
        // Log the notification for terminal output
        tracing::info!("Notification: {}", message);

        // Attempt to send a desktop notification if the notify-rust feature is available
        #[cfg(feature = "desktop-notifications")]
        {
            use std::time::Duration;
            match notify_rust::Notification::new()
                .summary("VT Code")
                .body(message)
                .icon("dialog-information")
                .timeout(Duration::from_secs(5)) // 5 seconds
                .show()
            {
                Ok(notification) => {
                    tracing::debug!("Desktop notification sent: {:?}", notification);
                }
                Err(e) => {
                    tracing::warn!("Failed to send desktop notification: {}", e);
                }
            }
        }
    }

    /// Update the notification configuration
    pub async fn update_config(&self, new_config: NotificationConfig) {
        let mut config = self.config.write().await;
        *config = new_config;
    }

    /// Get the current notification configuration
    pub async fn get_config(&self) -> NotificationConfig {
        let config = self.config.read().await;
        config.clone()
    }
}

impl Default for NotificationManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Global notification manager instance for easy access
use std::sync::OnceLock;

static GLOBAL_NOTIFICATION_MANAGER: OnceLock<NotificationManager> = OnceLock::new();

/// Initialize the global notification manager
pub fn init_global_notification_manager() -> Result<(), ()> {
    let manager = NotificationManager::new();
    GLOBAL_NOTIFICATION_MANAGER.set(manager).map_err(|_| ())
}

/// Get a reference to the global notification manager
pub fn get_global_notification_manager() -> Option<&'static NotificationManager> {
    GLOBAL_NOTIFICATION_MANAGER.get()
}

/// Send a notification using the global notification manager
pub async fn send_global_notification(event: NotificationEvent) -> Result<(), anyhow::Error> {
    if let Some(manager) = get_global_notification_manager() {
        manager.send_notification(event).await
    } else {
        // If global manager isn't initialized, create a temporary one for this notification
        let manager = NotificationManager::new();
        manager.send_notification(event).await
    }
}

/// Convenience function to send a tool failure notification
pub async fn notify_tool_failure(
    tool_name: &str,
    error: &str,
    details: Option<&str>,
) -> Result<(), anyhow::Error> {
    let event = NotificationEvent::ToolFailure {
        tool_name: tool_name.to_string(),
        error: error.to_string(),
        details: details.map(|s| s.to_string()),
    };
    send_global_notification(event).await
}

/// Convenience function to send a command failure notification
pub async fn notify_command_failure(
    command: &str,
    error: &str,
    exit_code: Option<i32>,
) -> Result<(), anyhow::Error> {
    let event = NotificationEvent::CommandFailure {
        command: command.to_string(),
        error: error.to_string(),
        exit_code,
    };
    send_global_notification(event).await
}

/// Convenience function to send an error notification
pub async fn notify_error(message: &str, context: Option<&str>) -> Result<(), anyhow::Error> {
    let event = NotificationEvent::Error {
        message: message.to_string(),
        context: context.map(|s| s.to_string()),
    };
    send_global_notification(event).await
}

/// Convenience function to send a human in the loop notification
pub async fn notify_human_in_the_loop(prompt: &str, context: &str) -> Result<(), anyhow::Error> {
    let event = NotificationEvent::HumanInTheLoop {
        prompt: prompt.to_string(),
        context: context.to_string(),
    };
    send_global_notification(event).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_notification_manager_creation() {
        let manager = NotificationManager::new();
        let config = manager.get_config().await;

        assert!(config.command_failure_notifications);
        assert!(config.error_notifications);
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
        assert!(result.is_ok());
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
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_terminal_notifications_toggle() {
        // Test with notifications enabled (default)
        let manager = NotificationManager::new();
        let config = manager.get_config().await;
        assert!(config.terminal_notifications_enabled);

        // Test with notifications disabled
        let mut config = NotificationConfig::default();
        config.terminal_notifications_enabled = false;
        let manager = NotificationManager::with_config(config);
        let event = NotificationEvent::CommandFailure {
            command: "test".to_string(),
            error: "test error".to_string(),
            exit_code: None,
        };

        // This should not send notification when disabled
        let result = manager.send_notification(event).await;
        assert!(result.is_ok()); // Should not error, but notification won't be sent

        // Verify the setting worked by checking the config
        let current_config = manager.get_config().await;
        assert!(!current_config.terminal_notifications_enabled);
    }
}
