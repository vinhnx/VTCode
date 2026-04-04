//! Push notification system for VT Code terminal clients
//! Handles important events like command failures, errors, policy approval requests,
//! human in the loop interactions, completion and requests.

use anyhow::Result;
use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
#[cfg(target_os = "macos")]
use std::io::Write;
#[cfg(target_os = "macos")]
use std::process::{Command, Stdio};
use std::sync::Arc;
#[cfg(all(feature = "desktop-notifications", target_os = "macos"))]
use std::sync::Once;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crate::config::loader::VTCodeConfig;
use crate::hooks::{LifecycleHookEngine, NotificationHookType};
use vtcode_config::{
    NotificationBackend, NotificationDeliveryMode, TerminalNotificationMethod,
    TuiNotificationEvent, TuiNotificationsConfig,
};

/// Types of important events that trigger notifications
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NotificationEvent {
    /// Generic ad-hoc notification
    Custom { title: String, message: String },
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
    /// Tool execution succeeded
    ToolSuccess {
        tool_name: String,
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
    /// Approval or elicitation prompt that should surface as a permission request
    PermissionPrompt { title: String, message: String },
    /// VT Code has been waiting for user input long enough to notify
    IdlePrompt { title: String, message: String },
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
    /// Enable tool failure notifications
    pub tool_failure_notifications: bool,
    /// Enable error notifications
    pub error_notifications: bool,
    /// Enable policy approval request notifications
    pub policy_approval_notifications: bool,
    /// Enable human in the loop notifications
    pub hitl_notifications: bool,
    /// Enable completion notifications for successful turns/tasks
    pub completion_success_notifications: bool,
    /// Enable completion notifications for partial/failure/cancelled turns/tasks
    pub completion_failure_notifications: bool,
    /// Enable request notifications
    pub request_notifications: bool,
    /// Enable tool success notifications
    pub tool_success_notifications: bool,
    /// Enable/disable all terminal notifications (overrides other settings)
    pub terminal_notifications_enabled: bool,
    /// Suppress notifications while terminal is focused.
    pub suppress_when_focused: bool,
    /// Delivery mode for notifications.
    pub delivery_mode: NotificationDeliveryMode,
    /// Preferred backend for desktop notification delivery.
    pub backend: NotificationBackend,
    /// Preferred terminal notification transport.
    pub notification_method: TerminalNotificationMethod,
    /// Time window for suppressing repeated identical notifications.
    pub repeat_window_seconds: u64,
    /// Maximum identical notifications allowed per suppression window.
    pub max_identical_notifications_in_window: u32,
}

impl Default for NotificationConfig {
    fn default() -> Self {
        Self {
            command_failure_notifications: false,
            tool_failure_notifications: false,
            error_notifications: true,
            policy_approval_notifications: true,
            hitl_notifications: true,
            completion_success_notifications: false,
            completion_failure_notifications: true,
            request_notifications: false,
            tool_success_notifications: false,
            terminal_notifications_enabled: true,
            suppress_when_focused: true,
            delivery_mode: NotificationDeliveryMode::Hybrid,
            backend: NotificationBackend::Auto,
            notification_method: TerminalNotificationMethod::Auto,
            repeat_window_seconds: 30,
            max_identical_notifications_in_window: 1,
        }
    }
}

impl NotificationConfig {
    /// Build runtime notification config from full VTCodeConfig.
    pub fn from_vtcode_config(config: &VTCodeConfig) -> Self {
        let notifications = &config.ui.notifications;
        let mut resolved = Self {
            command_failure_notifications: notifications
                .command_failure
                .unwrap_or(notifications.tool_failure),
            tool_failure_notifications: notifications.tool_failure,
            error_notifications: notifications.error,
            policy_approval_notifications: notifications
                .policy_approval
                .unwrap_or(notifications.hitl),
            hitl_notifications: notifications.hitl,
            completion_success_notifications: notifications
                .completion_success
                .unwrap_or(notifications.completion),
            completion_failure_notifications: notifications
                .completion_failure
                .unwrap_or(notifications.completion),
            request_notifications: notifications.request.unwrap_or(notifications.hitl),
            tool_success_notifications: notifications.tool_success,
            terminal_notifications_enabled: notifications.enabled,
            suppress_when_focused: notifications.suppress_when_focused,
            delivery_mode: notifications.delivery_mode,
            backend: notifications.backend,
            notification_method: config.tui.notification_method.unwrap_or_default(),
            repeat_window_seconds: notifications.repeat_window_seconds,
            max_identical_notifications_in_window: notifications.max_identical_in_window,
        };

        if let Some(tui_notifications) = &config.tui.notifications {
            match tui_notifications {
                TuiNotificationsConfig::Enabled(enabled) => {
                    resolved.terminal_notifications_enabled = *enabled;
                }
                TuiNotificationsConfig::Events(events) => {
                    let turn_complete = events.contains(&TuiNotificationEvent::AgentTurnComplete);
                    let approval_requested =
                        events.contains(&TuiNotificationEvent::ApprovalRequested);
                    resolved.terminal_notifications_enabled = true;
                    resolved.command_failure_notifications = false;
                    resolved.tool_failure_notifications = false;
                    resolved.error_notifications = false;
                    resolved.tool_success_notifications = false;
                    resolved.completion_success_notifications = turn_complete;
                    resolved.completion_failure_notifications = turn_complete;
                    resolved.policy_approval_notifications = approval_requested;
                    resolved.hitl_notifications = approval_requested;
                    resolved.request_notifications = approval_requested;
                }
            }
        }

        resolved
    }
}

#[derive(Debug)]
struct RepeatEntry {
    window_start: Instant,
    sent_in_window: u32,
}

impl RepeatEntry {
    fn new(now: Instant) -> Self {
        Self {
            window_start: now,
            sent_in_window: 0,
        }
    }
}

#[derive(Debug, Default)]
struct RepeatSuppressionState {
    entries: HashMap<String, RepeatEntry>,
}

#[derive(Debug)]
enum RepeatDecision {
    Deliver,
    Suppress,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DesktopNotificationBackend {
    #[cfg(target_os = "macos")]
    Osascript,
    NotifyRust,
}

#[cfg(target_os = "macos")]
const AUTO_DESKTOP_NOTIFICATION_BACKENDS: &[DesktopNotificationBackend] = &[
    DesktopNotificationBackend::Osascript,
    DesktopNotificationBackend::NotifyRust,
];
#[cfg(not(target_os = "macos"))]
const AUTO_DESKTOP_NOTIFICATION_BACKENDS: &[DesktopNotificationBackend] =
    &[DesktopNotificationBackend::NotifyRust];
#[cfg(target_os = "macos")]
const OSASCRIPT_DESKTOP_NOTIFICATION_BACKENDS: &[DesktopNotificationBackend] =
    &[DesktopNotificationBackend::Osascript];
const NOTIFY_RUST_DESKTOP_NOTIFICATION_BACKENDS: &[DesktopNotificationBackend] =
    &[DesktopNotificationBackend::NotifyRust];
const NO_DESKTOP_NOTIFICATION_BACKENDS: &[DesktopNotificationBackend] = &[];

/// Notification manager that handles sending notifications
pub struct NotificationManager {
    config: Arc<RwLock<NotificationConfig>>,
    /// Track if the terminal is currently focused/active
    terminal_focused: Arc<AtomicBool>,
    repeat_state: Arc<Mutex<RepeatSuppressionState>>,
}

impl NotificationManager {
    /// Create a new notification manager with default configuration
    pub fn new() -> Self {
        Self {
            config: Arc::new(RwLock::new(NotificationConfig::default())),
            terminal_focused: Arc::new(AtomicBool::new(false)), // Start as not focused
            repeat_state: Arc::new(Mutex::new(RepeatSuppressionState::default())),
        }
    }

    /// Create a new notification manager with custom configuration
    pub fn with_config(config: NotificationConfig) -> Self {
        Self {
            config: Arc::new(RwLock::new(config)),
            terminal_focused: Arc::new(AtomicBool::new(false)), // Start as not focused
            repeat_state: Arc::new(Mutex::new(RepeatSuppressionState::default())),
        }
    }

    /// Send a notification for an event
    pub async fn send_notification(&self, event: NotificationEvent) -> Result<()> {
        let config = self.config.read().clone();

        // Check if terminal notifications are enabled globally first
        if !config.terminal_notifications_enabled {
            return Ok(());
        }

        // Check if the terminal is currently focused/active
        // Only send notifications when the terminal is NOT active (user is not using it)
        let is_terminal_active = self.terminal_focused.load(Ordering::Relaxed);
        if is_terminal_active && config.suppress_when_focused {
            // Terminal is active, don't send notification to avoid interrupting the user
            return Ok(());
        }

        if !self.event_enabled(&event, &config) {
            return Ok(());
        }

        match self.repeat_decision(&event, &config) {
            RepeatDecision::Deliver => {
                self.send_notification_impl(&event, &config).await?;
                self.run_notification_hook_if_configured(&event).await;
            }
            RepeatDecision::Suppress => {
                return Ok(());
            }
        }

        Ok(())
    }

    fn event_enabled(&self, event: &NotificationEvent, config: &NotificationConfig) -> bool {
        match event {
            NotificationEvent::Custom { .. } => true,
            NotificationEvent::CommandFailure { .. } => config.command_failure_notifications,
            NotificationEvent::ToolFailure { .. } => config.tool_failure_notifications,
            NotificationEvent::ToolSuccess { .. } => config.tool_success_notifications,
            NotificationEvent::Error { .. } => config.error_notifications,
            NotificationEvent::PolicyApprovalRequest { .. } => config.policy_approval_notifications,
            NotificationEvent::HumanInTheLoop { .. } => config.hitl_notifications,
            NotificationEvent::PermissionPrompt { .. } => {
                config.policy_approval_notifications || config.hitl_notifications
            }
            NotificationEvent::IdlePrompt { .. } => config.request_notifications,
            NotificationEvent::Completion { status, .. } => match status {
                CompletionStatus::Success => config.completion_success_notifications,
                CompletionStatus::PartialSuccess
                | CompletionStatus::Failure
                | CompletionStatus::Cancelled => config.completion_failure_notifications,
            },
            NotificationEvent::Request { .. } => config.request_notifications,
        }
    }

    fn repeat_decision(
        &self,
        event: &NotificationEvent,
        config: &NotificationConfig,
    ) -> RepeatDecision {
        if config.repeat_window_seconds == 0 {
            return RepeatDecision::Deliver;
        }

        let Some(fingerprint) = self.repeat_fingerprint(event) else {
            return RepeatDecision::Deliver;
        };

        let window = Duration::from_secs(config.repeat_window_seconds.max(1));
        let max_allowed = config.max_identical_notifications_in_window.max(1);
        let now = Instant::now();

        let mut state = self.repeat_state.lock();

        if state.entries.len() > 1024 {
            state
                .entries
                .retain(|_, entry| now.duration_since(entry.window_start) < window);
        }

        let entry = state
            .entries
            .entry(fingerprint)
            .or_insert_with(|| RepeatEntry::new(now));

        if now.duration_since(entry.window_start) >= window {
            *entry = RepeatEntry::new(now);
        }

        if entry.sent_in_window < max_allowed {
            entry.sent_in_window += 1;
            RepeatDecision::Deliver
        } else {
            RepeatDecision::Suppress
        }
    }

    /// Internal method to send the actual notification
    async fn send_notification_impl(
        &self,
        event: &NotificationEvent,
        config: &NotificationConfig,
    ) -> Result<()> {
        let message = self.format_notification_message(event);
        self.send_message(&message, config).await
    }

    async fn run_notification_hook_if_configured(&self, event: &NotificationEvent) {
        let Some((notification_type, title, message)) = self.notification_hook_payload(event)
        else {
            return;
        };
        let Some(engine) = get_global_notification_hook_engine() else {
            return;
        };

        if let Err(error) = engine
            .run_notification(notification_type, title.as_str(), message.as_str())
            .await
        {
            tracing::warn!(
                error = %error,
                notification_type = notification_type.as_str(),
                "Failed to run notification lifecycle hook"
            );
        }
    }

    async fn send_message(&self, message: &str, config: &NotificationConfig) -> Result<()> {
        match config.delivery_mode {
            NotificationDeliveryMode::Terminal => {
                self.send_terminal_bell(message).await;
            }
            NotificationDeliveryMode::Hybrid => {
                self.send_terminal_bell(message).await;
                let _ = self.send_desktop_notification(message, config).await;
            }
            NotificationDeliveryMode::Desktop => {
                if !self.send_desktop_notification(message, config).await {
                    self.send_terminal_bell(message).await;
                }
            }
        }

        Ok(())
    }

    fn repeat_fingerprint(&self, event: &NotificationEvent) -> Option<String> {
        let event_type = match event {
            NotificationEvent::Custom { .. } => "custom",
            NotificationEvent::CommandFailure { .. } => "command_failure",
            NotificationEvent::ToolFailure { .. } => "tool_failure",
            NotificationEvent::ToolSuccess { .. } => "tool_success",
            NotificationEvent::Error { .. } => "error",
            NotificationEvent::Completion { .. } => "completion",
            NotificationEvent::IdlePrompt { .. } => "idle_prompt",
            NotificationEvent::Request { .. } => "request",
            NotificationEvent::PolicyApprovalRequest { .. }
            | NotificationEvent::HumanInTheLoop { .. }
            | NotificationEvent::PermissionPrompt { .. } => {
                return None;
            }
        };

        let normalized_message = self.normalize_message(&self.format_notification_message(event));
        Some(format!("{event_type}:{normalized_message}"))
    }

    fn normalize_message(&self, message: &str) -> String {
        message
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_ascii_lowercase()
    }

    /// Format a notification message based on the event
    fn format_notification_message(&self, event: &NotificationEvent) -> String {
        match event {
            NotificationEvent::Custom { title, message } => {
                let title = title.trim();
                if title.is_empty() {
                    message.clone()
                } else {
                    format!("{title}: {message}")
                }
            }
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
            NotificationEvent::ToolSuccess { tool_name, details } => {
                let details_str = details
                    .as_ref()
                    .map(|d| format!(" - {}", d))
                    .unwrap_or_default();
                format!("Tool '{}' completed{}", tool_name, details_str)
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
            NotificationEvent::PermissionPrompt { title, message } => {
                format!("{title}: {message}")
            }
            NotificationEvent::IdlePrompt { title, message } => {
                format!("{title}: {message}")
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
                if task == "turn" {
                    format!("Agent turn ended: {}{}", status_str, details_str)
                } else {
                    format!("Task '{}' {}{}", task, status_str, details_str)
                }
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
        use crate::utils::ansi_codes::notify_attention_with_terminal_method;
        let method = self.config.read().notification_method;
        notify_attention_with_terminal_method(true, Some(message), method);
    }

    async fn send_desktop_notification(&self, message: &str, config: &NotificationConfig) -> bool {
        tracing::info!("Notification: {}", message);

        for backend in self.desktop_notification_backends(config.backend) {
            if self.try_send_desktop_notification_backend(*backend, message) {
                return true;
            }
        }

        false
    }

    fn desktop_notification_backends(
        &self,
        backend: NotificationBackend,
    ) -> &'static [DesktopNotificationBackend] {
        match backend {
            NotificationBackend::Auto => AUTO_DESKTOP_NOTIFICATION_BACKENDS,
            NotificationBackend::Osascript => {
                #[cfg(target_os = "macos")]
                {
                    OSASCRIPT_DESKTOP_NOTIFICATION_BACKENDS
                }
                #[cfg(not(target_os = "macos"))]
                {
                    tracing::warn!("osascript notification backend is only supported on macOS");
                    NO_DESKTOP_NOTIFICATION_BACKENDS
                }
            }
            NotificationBackend::NotifyRust => NOTIFY_RUST_DESKTOP_NOTIFICATION_BACKENDS,
            NotificationBackend::Terminal => NO_DESKTOP_NOTIFICATION_BACKENDS,
        }
    }

    fn try_send_desktop_notification_backend(
        &self,
        backend: DesktopNotificationBackend,
        message: &str,
    ) -> bool {
        match backend {
            #[cfg(target_os = "macos")]
            DesktopNotificationBackend::Osascript => self.send_osascript_notification(message),
            DesktopNotificationBackend::NotifyRust => self.send_notify_rust_notification(message),
        }
    }

    #[cfg(target_os = "macos")]
    fn send_osascript_notification(&self, message: &str) -> bool {
        match send_macos_osascript_notification("VT Code", message) {
            Ok(()) => true,
            Err(error) => {
                tracing::warn!(error = %error, "Failed to send macOS osascript notification");
                false
            }
        }
    }

    fn send_notify_rust_notification(&self, message: &str) -> bool {
        #[cfg(feature = "desktop-notifications")]
        {
            #[cfg(target_os = "macos")]
            configure_macos_notification_application();

            use std::time::Duration;
            match notify_rust::Notification::new()
                .summary("VT Code")
                .body(message)
                .icon("dialog-information")
                .timeout(Duration::from_secs(5))
                .show()
            {
                Ok(notification) => {
                    tracing::debug!("Desktop notification sent: {:?}", notification);
                    true
                }
                Err(error) => {
                    tracing::warn!("Failed to send desktop notification: {}", error);
                    false
                }
            }
        }

        #[cfg(not(feature = "desktop-notifications"))]
        {
            let _ = message;
            tracing::warn!("notify_rust notification backend is unavailable in this build");
            false
        }
    }

    /// Update the notification configuration
    pub async fn update_config(&self, new_config: NotificationConfig) {
        self.update_config_sync(new_config);
    }

    /// Synchronously update notification configuration.
    pub fn update_config_sync(&self, new_config: NotificationConfig) {
        let mut config = self.config.write();
        *config = new_config;
    }

    /// Get the current notification configuration
    pub async fn get_config(&self) -> NotificationConfig {
        self.get_config_sync()
    }

    /// Get the current notification configuration synchronously.
    pub fn get_config_sync(&self) -> NotificationConfig {
        self.config.read().clone()
    }

    /// Update the terminal focus state - true if terminal is focused/active, false otherwise
    pub fn set_terminal_focused(&self, focused: bool) {
        self.terminal_focused.store(focused, Ordering::Relaxed);
    }

    /// Get the current terminal focus state
    pub fn is_terminal_focused(&self) -> bool {
        self.terminal_focused.load(Ordering::Relaxed)
    }

    fn notification_hook_payload(
        &self,
        event: &NotificationEvent,
    ) -> Option<(NotificationHookType, String, String)> {
        match event {
            NotificationEvent::PermissionPrompt { title, message } => Some((
                NotificationHookType::PermissionPrompt,
                title.clone(),
                message.clone(),
            )),
            NotificationEvent::IdlePrompt { title, message } => Some((
                NotificationHookType::IdlePrompt,
                title.clone(),
                message.clone(),
            )),
            _ => None,
        }
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
static GLOBAL_NOTIFICATION_HOOK_ENGINE: OnceLock<RwLock<Option<LifecycleHookEngine>>> =
    OnceLock::new();

/// Initialize the global notification manager
pub fn init_global_notification_manager() -> Result<()> {
    let manager = NotificationManager::new();
    GLOBAL_NOTIFICATION_MANAGER
        .set(manager)
        .map_err(|_| anyhow::anyhow!("Failed to set global notification manager"))
}

/// Initialize the global notification manager with explicit configuration.
pub fn init_global_notification_manager_with_config(config: NotificationConfig) -> Result<()> {
    let manager = NotificationManager::with_config(config);
    GLOBAL_NOTIFICATION_MANAGER
        .set(manager)
        .map_err(|_| anyhow::anyhow!("Failed to set global notification manager"))
}

/// Get a reference to the global notification manager
pub fn get_global_notification_manager() -> Option<&'static NotificationManager> {
    GLOBAL_NOTIFICATION_MANAGER.get()
}

pub fn set_global_notification_hook_engine(engine: Option<LifecycleHookEngine>) {
    let slot = GLOBAL_NOTIFICATION_HOOK_ENGINE.get_or_init(|| RwLock::new(None));
    *slot.write() = engine;
}

fn get_global_notification_hook_engine() -> Option<LifecycleHookEngine> {
    GLOBAL_NOTIFICATION_HOOK_ENGINE
        .get()
        .and_then(|slot| slot.read().clone())
}

/// Ensure the global manager is initialized, then apply updated configuration.
pub fn apply_global_notification_config(config: NotificationConfig) -> Result<()> {
    if let Some(manager) = get_global_notification_manager() {
        manager.update_config_sync(config);
        return Ok(());
    }
    init_global_notification_manager_with_config(config)
}

/// Build and apply notification settings from VTCodeConfig.
pub fn apply_global_notification_config_from_vtcode(config: &VTCodeConfig) -> Result<()> {
    let notification_config = NotificationConfig::from_vtcode_config(config);
    apply_global_notification_config(notification_config)
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

/// Send a notification immediately, even when the terminal is currently focused.
pub async fn send_global_notification_force(event: NotificationEvent) -> Result<(), anyhow::Error> {
    if let Some(manager) = get_global_notification_manager() {
        let original = manager.get_config().await;
        let mut forced = original.clone();
        forced.suppress_when_focused = false;
        manager.update_config(forced).await;
        let result = manager.send_notification(event).await;
        manager.update_config(original).await;
        result
    } else {
        let config = NotificationConfig {
            suppress_when_focused: false,
            ..NotificationConfig::default()
        };
        let manager = NotificationManager::with_config(config);
        manager.send_notification(event).await
    }
}

/// Set the terminal focus state using the global notification manager
pub fn set_global_terminal_focused(focused: bool) {
    if let Some(manager) = get_global_notification_manager() {
        manager.set_terminal_focused(focused);
    }
}

/// Check if the terminal is focused using the global notification manager
pub fn is_global_terminal_focused() -> bool {
    if let Some(manager) = get_global_notification_manager() {
        manager.is_terminal_focused()
    } else {
        false // Default to not focused if manager isn't initialized
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

/// Convenience function to send a tool success notification
pub async fn notify_tool_success(
    tool_name: &str,
    details: Option<&str>,
) -> Result<(), anyhow::Error> {
    let event = NotificationEvent::ToolSuccess {
        tool_name: tool_name.to_string(),
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

#[cfg(target_os = "macos")]
fn send_macos_osascript_notification(title: &str, message: &str) -> Result<()> {
    let mut child = Command::new("/usr/bin/osascript")
        .arg("-")
        .arg(message)
        .arg(title)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()?;

    let script = r#"on run argv
display notification (item 1 of argv) with title (item 2 of argv)
end run
"#;

    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| anyhow::anyhow!("failed to open osascript stdin"))?;
    stdin.write_all(script.as_bytes())?;
    drop(stdin);

    let output = child.wait_with_output()?;
    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if stderr.is_empty() {
            anyhow::bail!("osascript exited with status {}", output.status);
        }
        anyhow::bail!("osascript exited with status {}: {}", output.status, stderr);
    }
}

#[cfg(all(feature = "desktop-notifications", target_os = "macos"))]
fn configure_macos_notification_application() {
    static INIT: Once = Once::new();

    INIT.call_once(|| {
        let Some(bundle) = macos_notification_bundle_identifier(
            std::env::var("TERM_PROGRAM").ok().as_deref(),
            std::env::var("TERM").ok().as_deref(),
        ) else {
            return;
        };

        if let Err(error) = notify_rust::set_application(bundle) {
            tracing::warn!(
                bundle,
                error = %error,
                "Failed to configure macOS notification application"
            );
        } else {
            tracing::debug!(bundle, "Configured macOS notification application");
        }
    });
}

#[cfg(all(feature = "desktop-notifications", target_os = "macos"))]
fn macos_notification_bundle_identifier(
    term_program: Option<&str>,
    term: Option<&str>,
) -> Option<&'static str> {
    match term_program
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        Some(value) if value.eq_ignore_ascii_case("ghostty") => Some("com.mitchellh.ghostty"),
        Some(value) if value.eq_ignore_ascii_case("wezterm") => Some("com.github.wez.wezterm"),
        Some(value) if value.eq_ignore_ascii_case("alacritty") => Some("org.alacritty"),
        Some(value)
            if value.eq_ignore_ascii_case("apple_terminal")
                || value.eq_ignore_ascii_case("terminal") =>
        {
            Some("com.apple.Terminal")
        }
        Some(value)
            if value.eq_ignore_ascii_case("iterm.app")
                || value.eq_ignore_ascii_case("iterm2")
                || value.eq_ignore_ascii_case("iterm") =>
        {
            Some("com.googlecode.iterm2")
        }
        Some(value) if value.eq_ignore_ascii_case("kitty") => Some("net.kovidgoyal.kitty"),
        _ => None,
    }
    .or_else(
        || match term.map(str::trim).filter(|value| !value.is_empty()) {
            Some(value) if value.eq_ignore_ascii_case("xterm-ghostty") => {
                Some("com.mitchellh.ghostty")
            }
            _ => None,
        },
    )
}

#[cfg(test)]
mod tests {
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

        assert_eq!(
            manager.desktop_notification_backends(NotificationBackend::Auto),
            AUTO_DESKTOP_NOTIFICATION_BACKENDS
        );
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
        assert!(result.is_ok()); // Should not error, but notification won't be sent

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

        assert!(matches!(
            manager.repeat_decision(&event, &config),
            RepeatDecision::Deliver
        ));
        assert!(matches!(
            manager.repeat_decision(&event, &config),
            RepeatDecision::Suppress
        ));
    }

    #[test]
    fn custom_notifications_format_title_and_message() {
        let manager = NotificationManager::new();
        let event = NotificationEvent::Custom {
            title: "VT Code".to_string(),
            message: "Session started".to_string(),
        };

        assert_eq!(
            manager.format_notification_message(&event),
            "VT Code: Session started"
        );
    }

    #[cfg(all(feature = "desktop-notifications", target_os = "macos"))]
    #[test]
    fn macos_notification_bundle_identifier_maps_known_term_programs() {
        assert_eq!(
            macos_notification_bundle_identifier(Some("ghostty"), None),
            Some("com.mitchellh.ghostty")
        );
        assert_eq!(
            macos_notification_bundle_identifier(Some("Apple_Terminal"), None),
            Some("com.apple.Terminal")
        );
        assert_eq!(
            macos_notification_bundle_identifier(Some("iTerm.app"), None),
            Some("com.googlecode.iterm2")
        );
        assert_eq!(
            macos_notification_bundle_identifier(Some("WezTerm"), None),
            Some("com.github.wez.wezterm")
        );
        assert_eq!(
            macos_notification_bundle_identifier(None, Some("xterm-ghostty")),
            Some("com.mitchellh.ghostty")
        );
        assert_eq!(
            macos_notification_bundle_identifier(Some("unknown"), None),
            None
        );
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
}
