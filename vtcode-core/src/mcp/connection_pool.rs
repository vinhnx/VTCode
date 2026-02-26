//! MCP connection pool for efficient provider management
//!
//! This module provides connection pooling and parallel initialization
//! for MCP providers to eliminate sequential connection bottlenecks.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{RwLock, Semaphore};
use tracing::{error, info, warn};

use super::{McpElicitationHandler, McpProvider};
use crate::config::mcp::{McpAllowListConfig, McpProviderConfig};
use rmcp::model::{ClientCapabilities, Implementation, InitializeRequestParams};

/// MCP connection pool for efficient provider management
pub struct McpConnectionPool {
    /// Active provider connections
    providers: Arc<RwLock<HashMap<String, Arc<McpProvider>>>>,
    /// Connection semaphore to limit concurrent connections
    connection_semaphore: Arc<Semaphore>,
    /// Maximum connections allowed concurrently
    max_concurrent_connections: usize,
    /// Connection timeout
    connection_timeout: Duration,
}

impl McpConnectionPool {
    pub fn new(max_concurrent_connections: usize, connection_timeout_seconds: u64) -> Self {
        Self {
            providers: Arc::new(RwLock::new(HashMap::new())),
            connection_semaphore: Arc::new(Semaphore::new(max_concurrent_connections)),
            max_concurrent_connections,
            connection_timeout: Duration::from_secs(connection_timeout_seconds),
        }
    }

    /// Initialize multiple providers in parallel with controlled concurrency
    pub async fn initialize_providers_parallel(
        &self,
        provider_configs: Vec<McpProviderConfig>,
        elicitation_handler: Option<Arc<dyn McpElicitationHandler>>,
        tool_timeout: Option<Duration>,
        allowlist_snapshot: &McpAllowListConfig,
    ) -> Result<Vec<(String, Arc<McpProvider>)>, McpPoolError> {
        use futures::future::join_all;

        // Create initialization tasks for each provider
        let tasks: Vec<_> = provider_configs
            .into_iter()
            .map(|config| {
                let elicitation_handler = elicitation_handler.clone();
                let allowlist_snapshot = allowlist_snapshot.clone();

                async move {
                    self.initialize_provider(
                        config,
                        elicitation_handler,
                        tool_timeout.unwrap_or(Duration::from_secs(30)),
                        allowlist_snapshot,
                    )
                    .await
                }
            })
            .collect();

        // Execute all tasks in parallel
        let results = join_all(tasks).await;

        // Collect successful connections
        let mut successful_providers = Vec::new();
        let mut errors = Vec::new();

        for result in results {
            match result {
                Ok((name, provider)) => {
                    successful_providers.push((name, provider));
                }
                Err(error) => {
                    errors.push(error);
                }
            }
        }

        if !errors.is_empty() {
            warn!("Some MCP provider connections failed: {:?}", errors);
        }

        Ok(successful_providers)
    }

    /// Initialize a single provider with connection pooling
    async fn initialize_provider(
        &self,
        config: McpProviderConfig,
        elicitation_handler: Option<Arc<dyn McpElicitationHandler>>,
        tool_timeout: Duration,
        allowlist_snapshot: McpAllowListConfig,
    ) -> Result<(String, Arc<McpProvider>), McpPoolError> {
        // Acquire semaphore permit to limit concurrent connections
        let _permit = self
            .connection_semaphore
            .acquire()
            .await
            .map_err(|e| McpPoolError::SemaphoreError(e.to_string()))?;

        info!("Initializing MCP provider '{}'", config.name);

        // Connect to provider with timeout
        let provider = tokio::time::timeout(
            self.connection_timeout,
            McpProvider::connect(config.clone(), elicitation_handler),
        )
        .await
        .map_err(|_| McpPoolError::ConnectionTimeout(config.name.clone()))?
        .map_err(|e| McpPoolError::ConnectionError(config.name.clone(), e.to_string()))?;

        // Initialize the provider with proper parameters
        let provider_startup_timeout = self.resolve_startup_timeout(&config);
        let initialize_params = build_pool_initialize_params(&provider);
        let tool_timeout_opt = Some(tool_timeout);

        if let Err(err) = provider
            .initialize(
                initialize_params,
                provider_startup_timeout,
                tool_timeout_opt,
                &allowlist_snapshot,
            )
            .await
        {
            return Err(McpPoolError::InitializationError(
                config.name.clone(),
                err.to_string(),
            ));
        }

        // Refresh tools
        if let Err(err) = provider
            .refresh_tools(&allowlist_snapshot, tool_timeout_opt)
            .await
        {
            warn!(
                "Failed to fetch tools for provider '{}': {}",
                config.name, err
            );
        }

        info!("Successfully initialized MCP provider '{}'", config.name);

        Ok((config.name.clone(), Arc::new(provider)))
    }

    /// Get a provider by name
    pub async fn get_provider(&self, name: &str) -> Option<Arc<McpProvider>> {
        let providers = self.providers.read().await;
        providers.get(name).cloned()
    }

    /// Get all active providers
    pub async fn get_all_providers(&self) -> Vec<Arc<McpProvider>> {
        let providers = self.providers.read().await;
        providers.values().cloned().collect()
    }

    /// Remove a provider from the pool
    pub async fn remove_provider(&self, name: &str) -> Option<Arc<McpProvider>> {
        let mut providers = self.providers.write().await;
        providers.remove(name)
    }

    /// Check if a provider exists in the pool
    pub async fn has_provider(&self, name: &str) -> bool {
        let providers = self.providers.read().await;
        providers.contains_key(name)
    }

    /// Get connection pool statistics
    pub async fn stats(&self) -> ConnectionPoolStats {
        let providers = self.providers.read().await;
        let semaphore = self.connection_semaphore.available_permits();

        ConnectionPoolStats {
            active_connections: providers.len(),
            available_permits: semaphore,
            max_connections: self.max_concurrent_connections,
        }
    }

    /// Shutdown all providers gracefully
    pub async fn shutdown_all(&self) {
        let mut providers = self.providers.write().await;

        for (name, provider) in providers.drain() {
            if let Err(err) = provider.shutdown().await {
                error!("Failed to shutdown MCP provider '{}': {}", name, err);
            }
        }
    }

    /// Resolve startup timeout based on provider configuration
    fn resolve_startup_timeout(&self, config: &McpProviderConfig) -> Option<Duration> {
        config.startup_timeout_ms.map(Duration::from_millis)
    }
}

/// Connection pool statistics
#[derive(Debug, Clone)]
pub struct ConnectionPoolStats {
    pub active_connections: usize,
    pub available_permits: usize,
    pub max_connections: usize,
}

/// Enhanced MCP manager with connection pooling
pub struct PooledMcpManager {
    /// Connection pool for providers
    pool: Arc<McpConnectionPool>,
    /// Tool discovery cache
    tool_cache: Arc<super::tool_discovery_cache::ToolDiscoveryCache>,
}

impl PooledMcpManager {
    pub fn new(
        max_concurrent_connections: usize,
        connection_timeout_seconds: u64,
        tool_cache_capacity: usize,
    ) -> Self {
        Self {
            pool: Arc::new(McpConnectionPool::new(
                max_concurrent_connections,
                connection_timeout_seconds,
            )),
            tool_cache: Arc::new(super::tool_discovery_cache::ToolDiscoveryCache::new(
                tool_cache_capacity,
            )),
        }
    }

    /// Initialize providers with pooling and caching
    pub async fn initialize_providers(
        &self,
        provider_configs: Vec<McpProviderConfig>,
        elicitation_handler: Option<Arc<dyn McpElicitationHandler>>,
        tool_timeout: Option<Duration>,
        allowlist_snapshot: &McpAllowListConfig,
    ) -> Result<Vec<(String, Arc<McpProvider>)>, McpPoolError> {
        // Initialize providers in parallel
        let providers = self
            .pool
            .initialize_providers_parallel(
                provider_configs,
                elicitation_handler,
                tool_timeout,
                allowlist_snapshot,
            )
            .await?;

        // Add providers to the pool
        let mut pool_providers = self.pool.providers.write().await;
        for (name, provider) in &providers {
            pool_providers.insert(name.clone(), provider.clone());
        }

        Ok(providers)
    }

    /// Execute a tool on a specific provider
    pub async fn execute_tool(
        &self,
        provider_name: &str,
        tool_name: &str,
        arguments: serde_json::Value,
        allowlist: &crate::config::mcp::McpAllowListConfig,
        tool_timeout: Option<std::time::Duration>,
    ) -> Result<serde_json::Value, McpPoolError> {
        let provider = self
            .pool
            .get_provider(provider_name)
            .await
            .ok_or_else(|| McpPoolError::ProviderNotFound(provider_name.to_string()))?;

        // Convert arguments to proper format
        let args_ref = &arguments;

        // Execute the tool with correct signature
        let result = provider
            .call_tool(tool_name, args_ref, tool_timeout, allowlist)
            .await
            .map_err(|e| {
                McpPoolError::ToolExecutionError(provider_name.to_string(), e.to_string())
            })?;

        // Convert result to JSON value
        Ok(serde_json::to_value(&result).unwrap_or(serde_json::Value::Null))
    }

    /// Check if a tool is read-only (safe to cache)
    #[allow(dead_code)]
    fn is_read_only_tool(&self, tool_name: &str) -> bool {
        // This is a simple heuristic - in practice, you might want to
        // check tool metadata or maintain a list of read-only tools
        matches!(
            tool_name,
            "read_file"
                | "list_directory"
                | "search_files"
                | "get_file_info"
                | "read_environment"
                | "get_system_info"
                | "search_code"
                | "analyze_code"
        )
    }

    /// Get pool statistics
    pub async fn stats(&self) -> PooledMcpStats {
        let pool_stats = self.pool.stats().await;
        let tool_cache_stats = self.tool_cache.stats();

        PooledMcpStats {
            connection_pool: pool_stats,
            tool_cache: tool_cache_stats,
        }
    }

    /// Shutdown all providers gracefully
    pub async fn shutdown(&self) {
        self.pool.shutdown_all().await;
    }
}

/// Pooled MCP manager statistics
#[derive(Debug, Clone)]
pub struct PooledMcpStats {
    pub connection_pool: ConnectionPoolStats,
    pub tool_cache: super::tool_discovery_cache::ToolCacheStats,
}

/// Build initialize params for an MCP provider
fn build_pool_initialize_params(_provider: &McpProvider) -> InitializeRequestParams {
    InitializeRequestParams {
        meta: None,
        capabilities: ClientCapabilities {
            ..Default::default()
        },
        client_info: Implementation {
            name: "vtcode".to_owned(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            title: None,
            icons: None,
            website_url: None,
        },
        protocol_version: rmcp::model::ProtocolVersion::V_2024_11_05,
    }
}

/// MCP connection pool errors
#[derive(Debug, thiserror::Error)]
pub enum McpPoolError {
    #[error("Connection timeout for provider '{0}'")]
    ConnectionTimeout(String),

    #[error("Connection error for provider '{0}': {1}")]
    ConnectionError(String, String),

    #[error("Initialization timeout for provider '{0}'")]
    InitializationTimeout(String),

    #[error("Initialization error for provider '{0}': {1}")]
    InitializationError(String, String),

    #[error("Provider not found: {0}")]
    ProviderNotFound(String),

    #[error("Tool execution error for provider '{0}': {1}")]
    ToolExecutionError(String, String),

    #[error("Semaphore error: {0}")]
    SemaphoreError(String),
}

#[cfg(test)]
pub mod tests {
    use super::*;

    #[tokio::test]
    async fn test_connection_pool_creation() {
        let pool = McpConnectionPool::new(5, 30);
        let stats = pool.stats().await;

        assert_eq!(stats.active_connections, 0);
        assert_eq!(stats.max_connections, 5);
        assert_eq!(stats.available_permits, 5);
    }

    #[tokio::test]
    async fn test_connection_pool_semaphore_limits() {
        let pool = McpConnectionPool::new(3, 30);

        // Acquire 3 permits
        let permit1 = pool.connection_semaphore.acquire().await.unwrap();
        let _permit2 = pool.connection_semaphore.acquire().await.unwrap();
        let _permit3 = pool.connection_semaphore.acquire().await.unwrap();

        let stats = pool.stats().await;
        assert_eq!(stats.available_permits, 0);

        // Try to acquire another (would block if not in test)
        drop(permit1);
        let _permit4 = pool.connection_semaphore.acquire().await.unwrap();

        let stats = pool.stats().await;
        assert_eq!(stats.available_permits, 0);
    }

    #[tokio::test]
    async fn test_pooled_manager_creation() {
        let manager = PooledMcpManager::new(10, 30, 100);
        let stats = manager.stats().await;

        assert_eq!(stats.connection_pool.max_connections, 10);
        assert_eq!(stats.connection_pool.active_connections, 0);
    }

    #[tokio::test]
    async fn test_read_only_tool_detection() {
        let manager = PooledMcpManager::new(5, 30, 50);

        assert!(manager.is_read_only_tool("read_file"));
        assert!(manager.is_read_only_tool("search_files"));
        assert!(manager.is_read_only_tool("get_system_info"));
        assert!(manager.is_read_only_tool("get_file_info"));

        assert!(!manager.is_read_only_tool("write_file"));
        assert!(!manager.is_read_only_tool("edit_file"));
        assert!(!manager.is_read_only_tool("execute_command"));
        assert!(!manager.is_read_only_tool("delete_file"));
    }

    #[test]
    fn test_connection_pool_error_display() {
        let error = McpPoolError::ConnectionTimeout("test_provider".to_string());
        assert!(error.to_string().contains("test_provider"));

        let error = McpPoolError::InitializationError(
            "auth".to_string(),
            "invalid credentials".to_string(),
        );
        assert!(error.to_string().contains("auth"));
        assert!(error.to_string().contains("invalid credentials"));
    }

    #[tokio::test]
    async fn test_pool_provider_not_found() {
        let pool = McpConnectionPool::new(5, 30);
        let provider = pool.get_provider("nonexistent").await;
        assert!(provider.is_none());
    }

    #[tokio::test]
    async fn test_pool_has_provider() {
        let pool = McpConnectionPool::new(5, 30);
        assert!(!pool.has_provider("test").await);
    }

    #[tokio::test]
    async fn test_pool_get_all_providers_empty() {
        let pool = McpConnectionPool::new(5, 30);
        let providers = pool.get_all_providers().await;
        assert_eq!(providers.len(), 0);
    }

    #[tokio::test]
    async fn test_pool_stats() {
        let pool = McpConnectionPool::new(7, 60);
        let stats = pool.stats().await;

        assert_eq!(stats.max_connections, 7);
        assert_eq!(stats.available_permits, 7);
        assert_eq!(stats.active_connections, 0);
    }
}
