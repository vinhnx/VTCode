use anyhow::{Context, Result};
use std::fmt::{self, Display};
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use tokio::time::{Duration, timeout};
use tracing::{error, info, warn};
use vtcode_core::config::mcp::McpClientConfig;
use vtcode_core::mcp_client::{McpClient, McpClientStatus};

use crate::agent::runloop::mcp_events::McpEvent;

/// Represents the initialization status of MCP components
#[derive(Clone)]
pub enum McpInitStatus {
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
    #[allow(dead_code)]
    pub fn is_ready(&self) -> bool {
        matches!(self, McpInitStatus::Ready { .. })
    }

    #[allow(dead_code)]
    pub fn get_client(&self) -> Option<&Arc<McpClient>> {
        match self {
            McpInitStatus::Ready { client } => Some(client),
            _ => None,
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use vtcode_core::config::mcp::McpClientConfig;

    #[tokio::test]
    async fn test_async_mcp_manager_creation() {
        let config = McpClientConfig::default();
        let event_callback: Arc<dyn Fn(McpEvent) + Send + Sync> = Arc::new(|_event| {});

        let manager = AsyncMcpManager::new(config, event_callback);
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
}

/// Async manager for MCP client initialization and lifecycle
pub struct AsyncMcpManager {
    /// Configuration for MCP client
    config: McpClientConfig,
    /// Current initialization status
    status: Arc<RwLock<McpInitStatus>>,
    /// Mutex to prevent multiple concurrent initializations
    initialization_mutex: Arc<Mutex<()>>,
    /// Event callback for MCP events
    event_callback: Arc<dyn Fn(McpEvent) + Send + Sync>,
}

impl AsyncMcpManager {
    pub fn new(
        config: McpClientConfig,
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
            status: Arc::new(RwLock::new(init_status)),
            initialization_mutex: Arc::new(Mutex::new(())),
            event_callback,
        }
    }

    /// Start async initialization of MCP client
    pub fn start_initialization(&self) -> Result<()> {
        if !self.config.enabled {
            // If MCP is disabled, set status immediately
            let mut status_guard = self.status.blocking_write();
            *status_guard = McpInitStatus::Disabled;
            return Ok(());
        }

        // Clone what we need for the async task
        let config = self.config.clone();
        let status = Arc::clone(&self.status);
        let mutex = Arc::clone(&self.initialization_mutex);
        let event_callback = Arc::clone(&self.event_callback);

        // Spawn the initialization task
        tokio::spawn(async move {
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
            match Self::initialize_mcp_client(config, event_callback).await {
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

        Ok(())
    }

    async fn initialize_mcp_client(
        config: McpClientConfig,
        event_callback: Arc<dyn Fn(McpEvent) + Send + Sync>,
    ) -> Result<McpClient> {
        info!(
            "Initializing MCP client with {} providers",
            config.providers.len()
        );

        // Validate configuration before initializing
        if let Err(e) = vtcode_core::mcp_client::validate_mcp_config(&config) {
            warn!("MCP configuration validation error: {e}");
        }

        let mut client = McpClient::new(config);

        // Set up elicitation handler
        use crate::agent::runloop::mcp_elicitation::InteractiveMcpElicitationHandler;
        client.set_elicitation_handler(Arc::new(InteractiveMcpElicitationHandler::new()));

        // Initialize with timeout
        match timeout(Duration::from_secs(30), client.initialize()).await {
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
                "MCP client initialization timed out after 30 seconds"
            )),
        }
    }

    /// Get current status
    pub async fn get_status(&self) -> McpInitStatus {
        self.status.read().await.clone()
    }

    /// Get current status reference
    #[allow(dead_code)]
    pub fn get_status_arc(&self) -> Arc<RwLock<McpInitStatus>> {
        Arc::clone(&self.status)
    }

    /// Get current MCP client status (runtime status)
    #[allow(dead_code)]
    pub async fn get_client_status(&self) -> Option<McpClientStatus> {
        match self.get_status().await {
            McpInitStatus::Ready { client } => Some(client.get_status()),
            _ => None,
        }
    }

    /// Shutdown MCP client if active
    pub async fn shutdown(&self) -> Result<()> {
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
