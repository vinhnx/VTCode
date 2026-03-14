use anyhow::{Context, Result};
use std::fmt::{self, Display};
use std::sync::{Arc, RwLock as StdRwLock};
use tokio::sync::{Mutex, RwLock};
use tokio::time::{Duration, timeout};
use tracing::{error, info, warn};
use vtcode_core::config::mcp::McpClientConfig;
use vtcode_core::exec_policy::{AskForApproval, RejectConfig};
use vtcode_core::mcp::McpClient;

use crate::agent::runloop::mcp_events::McpEvent;

pub(crate) fn approval_policy_from_human_in_the_loop(human_in_the_loop: bool) -> AskForApproval {
    if human_in_the_loop {
        AskForApproval::OnRequest
    } else {
        AskForApproval::Reject(RejectConfig {
            sandbox_approval: true,
            rules: true,
            request_permissions: false,
            mcp_elicitations: true,
        })
    }
}

/// Represents the initialization status of MCP components
#[derive(Clone)]
pub(crate) enum McpInitStatus {
    /// MCP is not enabled
    Disabled,
    /// MCP initialization is in progress
    Initializing { progress: String },
    /// MCP initialization completed successfully
    Ready { client: Arc<McpClient> },
    /// MCP initialization failed
    Error { message: String },
}

impl McpInitStatus {
    #[cfg(test)]
    pub fn is_ready(&self) -> bool {
        matches!(self, McpInitStatus::Ready { .. })
    }

    pub fn is_error(&self) -> bool {
        matches!(self, McpInitStatus::Error { .. })
    }

    pub fn get_error_message(&self) -> Option<&str> {
        match self {
            McpInitStatus::Error { message } => Some(message),
            _ => None,
        }
    }

    #[cfg(test)]
    pub fn is_initializing(&self) -> bool {
        matches!(self, McpInitStatus::Initializing { .. })
    }
}

impl fmt::Debug for McpInitStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            McpInitStatus::Disabled => f.write_str("Disabled"),
            McpInitStatus::Initializing { progress } => f
                .debug_struct("Initializing")
                .field("progress", progress)
                .finish(),
            McpInitStatus::Ready { .. } => f.write_str("Ready { client: <redacted> }"),
            McpInitStatus::Error { message } => {
                f.debug_struct("Error").field("message", message).finish()
            }
        }
    }
}

impl Display for McpInitStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            McpInitStatus::Disabled => write!(f, "MCP is disabled"),
            McpInitStatus::Initializing { progress } => write!(f, "MCP initializing: {}", progress),
            McpInitStatus::Ready { .. } => write!(f, "MCP ready with active connections"),
            McpInitStatus::Error { message } => write!(f, "MCP error: {}", message),
        }
    }
}

/// Async manager for MCP client initialization and lifecycle
pub(crate) struct AsyncMcpManager {
    /// Configuration for MCP client
    config: McpClientConfig,
    /// Whether to ring terminal bell for HITL prompts
    hitl_notification_bell: bool,
    /// Approval policy used by MCP elicitation handling.
    approval_policy: Arc<StdRwLock<AskForApproval>>,
    /// Current initialization status
    status: Arc<RwLock<McpInitStatus>>,
    /// Mutex to prevent multiple concurrent initializations
    initialization_mutex: Arc<Mutex<()>>,
    /// Event callback for MCP events
    event_callback: Arc<dyn Fn(McpEvent) + Send + Sync>,
    /// Handle for the background initialization task, aborted on drop.
    init_task: std::sync::Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl AsyncMcpManager {
    pub(crate) fn new(
        config: McpClientConfig,
        hitl_notification_bell: bool,
        approval_policy: AskForApproval,
        event_callback: Arc<dyn Fn(McpEvent) + Send + Sync>,
    ) -> Self {
        let init_status = if config.enabled {
            McpInitStatus::Initializing {
                progress: "Initializing MCP client...".to_string(),
            }
        } else {
            McpInitStatus::Disabled
        };

        Self {
            config,
            hitl_notification_bell,
            approval_policy: Arc::new(StdRwLock::new(approval_policy)),
            status: Arc::new(RwLock::new(init_status)),
            initialization_mutex: Arc::new(Mutex::new(())),
            event_callback,
            init_task: std::sync::Mutex::new(None),
        }
    }

    /// Start async initialization of MCP client
    pub(crate) fn start_initialization(&self) -> Result<()> {
        if !self.config.enabled {
            // If MCP is disabled, set status immediately
            let mut status_guard = self.status.blocking_write();
            *status_guard = McpInitStatus::Disabled;
            return Ok(());
        }

        if let Ok(guard) = self.init_task.lock()
            && let Some(existing_task) = guard.as_ref()
            && !existing_task.is_finished()
        {
            return Ok(());
        }

        // Clone what we need for the async task
        let config = self.config.clone();
        let status = Arc::clone(&self.status);
        let mutex = Arc::clone(&self.initialization_mutex);
        let event_callback = Arc::clone(&self.event_callback);
        let hitl_notification_bell = self.hitl_notification_bell;
        let approval_policy = Arc::clone(&self.approval_policy);

        // Spawn the initialization task. Store the JoinHandle so it can be
        // aborted on drop — prevents an orphan task if the manager is dropped
        // before initialization completes.
        let init_handle = tokio::spawn(async move {
            // Acquire the mutex to prevent concurrent initializations
            let _guard = mutex.lock().await;

            // Check if already initialized while mutex was being acquired
            {
                let current_status = status.read().await;
                if matches!(*current_status, McpInitStatus::Ready { .. }) {
                    return; // Already initialized
                }
            }

            // Update status to initializing
            {
                let mut status_guard = status.write().await;
                *status_guard = McpInitStatus::Initializing {
                    progress: "Connecting to MCP providers...".to_string(),
                };
            }

            // Initialize MCP client
            match Self::initialize_mcp_client(
                config,
                hitl_notification_bell,
                approval_policy,
                event_callback,
            )
            .await
            {
                Ok(client) => {
                    let mut status_guard = status.write().await;
                    *status_guard = McpInitStatus::Ready {
                        client: Arc::new(client),
                    };
                    info!("MCP client initialized successfully");
                }
                Err(e) => {
                    let error_msg = e.to_string();
                    let mcp_error = if error_msg.contains("No such process")
                        || error_msg.contains("ESRCH")
                        || error_msg.contains("EPIPE")
                        || error_msg.contains("Broken pipe")
                        || error_msg.contains("write EPIPE")
                    {
                        format!("MCP server startup failed: {}", e)
                    } else {
                        format!("MCP initialization error: {}", e)
                    };

                    let mut status_guard = status.write().await;
                    *status_guard = McpInitStatus::Error { message: mcp_error };
                    error!("MCP client initialization failed: {}", e);
                }
            }
        });
        if let Ok(mut guard) = self.init_task.lock() {
            *guard = Some(init_handle);
        }

        Ok(())
    }

    async fn initialize_mcp_client(
        config: McpClientConfig,
        hitl_notification_bell: bool,
        approval_policy: Arc<StdRwLock<AskForApproval>>,
        event_callback: Arc<dyn Fn(McpEvent) + Send + Sync>,
    ) -> Result<McpClient> {
        info!(
            "Initializing MCP client with {} providers",
            config.providers.len()
        );

        // Validate configuration before initializing
        if let Err(e) = vtcode_core::mcp::validate_mcp_config(&config) {
            warn!("MCP configuration validation error: {e}");
        }

        // Get startup timeout from config, default to 30 seconds
        let startup_timeout_secs = config.startup_timeout_seconds.unwrap_or(30);
        let startup_timeout = Duration::from_secs(startup_timeout_secs);

        let mut client = McpClient::new(config);

        // Set up elicitation handler
        use crate::agent::runloop::mcp_elicitation::InteractiveMcpElicitationHandler;
        client.set_elicitation_handler(Arc::new(InteractiveMcpElicitationHandler::new(
            hitl_notification_bell,
            approval_policy,
        )));

        // Initialize with timeout
        match timeout(startup_timeout, client.initialize()).await {
            Ok(Ok(())) => {
                info!("MCP client initialized successfully");

                // Add event reporting for tools
                if let Ok(mcp_tools) = client.list_tools().await {
                    info!("Found {} MCP tools", mcp_tools.len());
                    for mcp_tool in mcp_tools {
                        let mut event =
                            McpEvent::new(mcp_tool.provider.clone(), mcp_tool.name.clone(), None);
                        event.success(None);
                        (*event_callback)(event);
                    }
                } else {
                    warn!("Failed to discover MCP tools after initialization");
                }

                Ok(client)
            }
            Ok(Err(e)) => Err(e).context("MCP client initialization failed"),
            Err(_) => Err(anyhow::anyhow!(
                "MCP client initialization timed out after {} seconds",
                startup_timeout_secs
            )),
        }
    }

    /// Get current status
    pub(crate) async fn get_status(&self) -> McpInitStatus {
        self.status.read().await.clone()
    }

    pub(crate) fn approval_policy(&self) -> AskForApproval {
        match self.approval_policy.read() {
            Ok(policy) => *policy,
            Err(poisoned) => {
                warn!("MCP approval policy lock was poisoned; continuing with last known value");
                *poisoned.into_inner()
            }
        }
    }

    pub(crate) fn set_approval_policy(&self, approval_policy: AskForApproval) {
        let mut policy_guard = match self.approval_policy.write() {
            Ok(policy) => policy,
            Err(poisoned) => {
                warn!("MCP approval policy lock was poisoned during update; recovering");
                poisoned.into_inner()
            }
        };
        *policy_guard = approval_policy;
    }

    /// Shutdown MCP client if active
    pub(crate) async fn shutdown(&self) -> Result<()> {
        match self.get_status().await {
            McpInitStatus::Ready { client } => {
                if let Err(e) = client.shutdown().await {
                    let error_msg = e.to_string();
                    if error_msg.contains("EPIPE")
                        || error_msg.contains("Broken pipe")
                        || error_msg.contains("write EPIPE")
                    {
                        info!(
                            "MCP client shutdown encountered pipe errors (normal): {}",
                            e
                        );
                    } else {
                        warn!("Failed to shutdown MCP client cleanly: {}", e);
                    }
                }
                Ok(())
            }
            McpInitStatus::Disabled => {
                info!("MCP is disabled, no shutdown needed");
                Ok(())
            }
            _ => {
                info!("MCP not initialized, no shutdown needed");
                Ok(())
            }
        }
    }
}

impl Drop for AsyncMcpManager {
    fn drop(&mut self) {
        // Abort the background init task so it doesn't outlive the manager.
        if let Ok(mut guard) = self.init_task.lock()
            && let Some(task) = guard.take()
        {
            task.abort();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vtcode_core::config::mcp::McpClientConfig;
    use vtcode_core::exec_policy::RejectConfig;

    #[tokio::test]
    async fn test_async_mcp_manager_creation() {
        let config = McpClientConfig::default();
        let event_callback: Arc<dyn Fn(McpEvent) + Send + Sync> = Arc::new(|_event| {});

        let manager = AsyncMcpManager::new(config, true, AskForApproval::OnRequest, event_callback);
        let status = manager.get_status().await;

        // With default config, MCP should be disabled
        assert!(matches!(status, McpInitStatus::Disabled));
    }

    #[tokio::test]
    async fn test_mcp_init_status_display() {
        let disabled_status = McpInitStatus::Disabled;
        assert_eq!(disabled_status.to_string(), "MCP is disabled");

        let initializing_status = McpInitStatus::Initializing {
            progress: "Connecting...".to_string(),
        };
        assert_eq!(
            initializing_status.to_string(),
            "MCP initializing: Connecting..."
        );

        let error_status = McpInitStatus::Error {
            message: "Connection failed".to_string(),
        };
        assert_eq!(error_status.to_string(), "MCP error: Connection failed");
    }

    #[tokio::test]
    async fn test_start_initialization_skips_when_task_already_running() {
        let config = McpClientConfig {
            enabled: true,
            ..McpClientConfig::default()
        };
        let event_callback: Arc<dyn Fn(McpEvent) + Send + Sync> = Arc::new(|_event| {});
        let manager = AsyncMcpManager::new(config, true, AskForApproval::OnRequest, event_callback);

        let blocker = tokio::spawn(async {
            tokio::time::sleep(Duration::from_secs(60)).await;
        });
        let blocker_id = blocker.id();
        if let Ok(mut guard) = manager.init_task.lock() {
            *guard = Some(blocker);
        }

        manager
            .start_initialization()
            .expect("start should succeed");

        if let Ok(mut guard) = manager.init_task.lock() {
            let task = guard
                .as_ref()
                .expect("running init task should still be present");
            assert_eq!(
                task.id(),
                blocker_id,
                "start_initialization should not replace a running init task"
            );
            assert!(
                !task.is_finished(),
                "existing task should still be running after skipped start"
            );
            if let Some(task) = guard.take() {
                task.abort();
            }
        } else {
            panic!("failed to lock init_task");
        }
    }

    #[tokio::test]
    async fn test_mcp_init_status_helpers() {
        let disabled_status = McpInitStatus::Disabled;
        assert!(!disabled_status.is_ready());
        assert!(!disabled_status.is_error());
        assert!(disabled_status.get_error_message().is_none());

        let error_status = McpInitStatus::Error {
            message: "Test error".to_string(),
        };
        assert!(!error_status.is_ready());
        assert!(error_status.is_error());
        assert_eq!(error_status.get_error_message(), Some("Test error"));

        let initializing_status = McpInitStatus::Initializing {
            progress: "Init...".to_string(),
        };
        assert!(initializing_status.is_initializing());
        assert!(!initializing_status.is_ready());
    }

    #[test]
    fn test_approval_policy_mapping_from_hitl() {
        assert_eq!(
            approval_policy_from_human_in_the_loop(true),
            AskForApproval::OnRequest
        );
        assert_eq!(
            approval_policy_from_human_in_the_loop(false),
            AskForApproval::Reject(RejectConfig {
                sandbox_approval: true,
                rules: true,
                request_permissions: false,
                mcp_elicitations: true,
            })
        );
    }

    #[test]
    fn test_set_approval_policy_updates_policy() {
        let config = McpClientConfig::default();
        let event_callback: Arc<dyn Fn(McpEvent) + Send + Sync> = Arc::new(|_event| {});

        let manager = AsyncMcpManager::new(config, true, AskForApproval::OnRequest, event_callback);
        assert_eq!(manager.approval_policy(), AskForApproval::OnRequest);

        manager.set_approval_policy(AskForApproval::Never);
        assert_eq!(manager.approval_policy(), AskForApproval::Never);
    }
}
