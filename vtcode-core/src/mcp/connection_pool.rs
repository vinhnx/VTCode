//! MCP connection pool for efficient provider management
//!
//! This module provides connection pooling and parallel initialization
//! for MCP providers to eliminate sequential connection bottlenecks.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{RwLock, Semaphore};
use tracing::{error, info, warn};

use super::provider::McpProvider;
use super::types::{McpProviderConfig, McpToolInfo};

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
        elicitation_handler: Option<Arc<dyn super::elicitation::ElicitationHandler>>,
        initialize_params: super::types::InitializeParams,
        tool_timeout: Duration,
        allowlist_snapshot: &std::collections::HashSet<String>,
    ) -> Result<Vec<(String, Arc<McpProvider>)>, McpPoolError> {
        use futures::future::join_all;
        
        // Create initialization tasks for each provider
        let tasks: Vec<_> = provider_configs
            .into_iter()
            .map(|config| {
                let elicitation_handler = elicitation_handler.clone();
                let initialize_params = initialize_params.clone();
                let allowlist_snapshot = allowlist_snapshot.clone();
                
                async move {
                    self.initialize_provider(
                        config,
                        elicitation_handler,
                        initialize_params,
                        tool_timeout,
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
        elicitation_handler: Option<Arc<dyn super::elicitation::ElicitationHandler>>,
        initialize_params: super::types::InitializeParams,
        tool_timeout: Duration,
        allowlist_snapshot: std::collections::HashSet<String>,
    ) -> Result<(String, Arc<McpProvider>), McpPoolError> {
        // Acquire semaphore permit to limit concurrent connections
        let _permit = self.connection_semaphore
            .acquire()
            .await
            .map_err(|e| McpPoolError::SemaphoreError(e.to_string()))?;

        info!("Initializing MCP provider '{}'", config.name);

        // Connect to provider with timeout
        let provider = tokio::time::timeout(
            self.connection_timeout,
            McpProvider::connect(config.clone(), elicitation_handler)
        )
        .await
        .map_err(|_| McpPoolError::ConnectionTimeout(config.name.clone()))?
        .map_err(|e| McpPoolError::ConnectionError(config.name.clone(), e.to_string()))?;

        // Initialize the provider
        let provider_startup_timeout = self.resolve_startup_timeout(&config);
        
        tokio::time::timeout(
            provider_startup_timeout,
            provider.initialize(initialize_params, provider_startup_timeout, tool_timeout, &allowlist_snapshot)
        )
        .await
        .map_err(|_| McpPoolError::InitializationTimeout(config.name.clone()))?
        .map_err(|e| McpPoolError::InitializationError(config.name.clone(), e.to_string()))?;

        // Refresh tools
        if let Err(err) = provider.refresh_tools(&allowlist_snapshot, tool_timeout).await {
            warn!("Failed to fetch tools for provider '{}': {}", config.name, err);
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
    fn resolve_startup_timeout(&self, config: &McpProviderConfig) -> Duration {
        config.startup_timeout
            .map(Duration::from_secs)
            .unwrap_or(Duration::from_secs(30))
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
    tool_cache: Arc<super::tool_discovery::ToolDiscoveryCache>,
}

impl PooledMcpManager {
    pub fn new(
        max_concurrent_connections: usize,
        connection_timeout_seconds: u64,
        tool_cache_capacity: usize,
    ) -> Self {
        Self {
            pool: Arc::new(McpConnectionPool::new(max_concurrent_connections, connection_timeout_seconds)),
            tool_cache: Arc::new(super::tool_discovery::ToolDiscoveryCache::new(tool_cache_capacity)),
        }
    }

    /// Initialize providers with pooling and caching
    pub async fn initialize_providers(
        &self,
        provider_configs: Vec<McpProviderConfig>,
        elicitation_handler: Option<Arc<dyn super::elicitation::ElicitationHandler>>,
        initialize_params: super::types::InitializeParams,
        tool_timeout: Duration,
        allowlist_snapshot: &std::collections::HashSet<String>,
    ) -> Result<Vec<(String, Arc<McpProvider>)>, McpPoolError> {
        // Initialize providers in parallel
        let providers = self.pool
            .initialize_providers_parallel(
                provider_configs,
                elicitation_handler,
                initialize_params,
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

    /// Execute a tool with caching
    pub async fn execute_tool(
        &self,
        provider_name: &str,
        tool_name: &str,
        arguments: serde_json::Value,
    ) -> Result<serde_json::Value, McpPoolError> {
        let provider = self.pool.get_provider(provider_name).await
            .ok_or_else(|| McpPoolError::ProviderNotFound(provider_name.to_string()))?;

        // Check tool cache first
        let cache_key = format!("{}:{}", provider_name, tool_name);
        if let Some(cached_result) = self.tool_cache.get_cached_result(&cache_key).await {
            return Ok(cached_result);
        }

        // Execute the tool
        let result = provider
            .call_tool(tool_name, arguments)
            .await
            .map_err(|e| McpPoolError::ToolExecutionError(provider_name.to_string(), e.to_string()))?;

        // Cache the result for read-only tools
        if self.is_read_only_tool(tool_name) {
            self.tool_cache.cache_result(cache_key, result.clone()).await;
        }

        Ok(result)
    }

    /// Check if a tool is read-only (safe to cache)
    fn is_read_only_tool(&self, tool_name: &str) -> bool {
        // This is a simple heuristic - in practice, you might want to
        // check tool metadata or maintain a list of read-only tools
        matches!(tool_name,
            "read_file" | "list_directory" | "search_files" | "get_file_info" |
            "read_environment" | "get_system_info" | "search_code" | "analyze_code"
        )
    }

    /// Get pool statistics
    pub async fn stats(&self) -> PooledMcpStats {
        let pool_stats = self.pool.stats().await;
        let tool_cache_stats = self.tool_cache.stats().await;
        
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
    pub tool_cache: super::tool_discovery::ToolCacheStats,
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
mod tests {
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
        assert!(!manager.is_read_only_tool("write_file"));
        assert!(!manager.is_read_only_tool("edit_file"));
    }
}