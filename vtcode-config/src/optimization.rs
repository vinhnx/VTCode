//! Configuration for performance optimization features

use serde::{Deserialize, Serialize};

/// Configuration for all optimization features
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OptimizationConfig {
    /// Memory pool configuration
    #[serde(default)]
    pub memory_pool: MemoryPoolConfig,

    /// Tool registry optimization settings
    #[serde(default)]
    pub tool_registry: ToolRegistryConfig,

    /// Async pipeline configuration
    #[serde(default)]
    pub async_pipeline: AsyncPipelineConfig,

    /// LLM client optimization settings
    #[serde(default)]
    pub llm_client: LLMClientConfig,

    /// Agent execution optimization
    #[serde(default)]
    pub agent_execution: AgentExecutionConfig,

    /// Performance profiling settings
    #[serde(default)]
    pub profiling: ProfilingConfig,

    /// File read cache configuration
    #[serde(default)]
    pub file_read_cache: FileReadCacheConfig,

    /// Read-only command result cache
    #[serde(default)]
    pub command_cache: CommandCacheConfig,
}

/// File read cache configuration
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileReadCacheConfig {
    /// Enable file read caching
    pub enabled: bool,

    /// Minimum file size (bytes) before caching
    pub min_size_bytes: usize,

    /// Maximum cached file size (bytes)
    pub max_size_bytes: usize,

    /// Cache TTL in seconds
    pub ttl_secs: u64,

    /// Maximum number of cached entries
    pub max_entries: usize,
}

/// Read-only command cache configuration
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandCacheConfig {
    /// Enable command caching
    pub enabled: bool,

    /// Cache TTL in milliseconds
    pub ttl_ms: u64,

    /// Maximum number of cached entries
    pub max_entries: usize,

    /// Allowlist of command prefixes eligible for caching
    pub allowlist: Vec<String>,
}

/// Memory pool configuration
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryPoolConfig {
    /// Enable memory pool (can be disabled for debugging)
    pub enabled: bool,

    /// Maximum number of strings to pool
    pub max_string_pool_size: usize,

    /// Maximum number of Values to pool
    pub max_value_pool_size: usize,

    /// Maximum number of Vec<String> to pool
    pub max_vec_pool_size: usize,
}

/// Tool registry optimization configuration
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolRegistryConfig {
    /// Enable optimized registry
    pub use_optimized_registry: bool,

    /// Maximum concurrent tool executions
    pub max_concurrent_tools: usize,

    /// Hot cache size for frequently used tools
    pub hot_cache_size: usize,

    /// Tool execution timeout in seconds
    pub default_timeout_secs: u64,
}

/// Async pipeline configuration
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AsyncPipelineConfig {
    /// Enable request batching
    pub enable_batching: bool,

    /// Enable result caching
    pub enable_caching: bool,

    /// Maximum batch size for tool requests
    pub max_batch_size: usize,

    /// Batch timeout in milliseconds
    pub batch_timeout_ms: u64,

    /// Result cache size
    pub cache_size: usize,
}

/// LLM client optimization configuration
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LLMClientConfig {
    /// Enable connection pooling
    pub enable_connection_pooling: bool,

    /// Enable response caching
    pub enable_response_caching: bool,

    /// Enable request batching
    pub enable_request_batching: bool,

    /// Connection pool size
    pub connection_pool_size: usize,

    /// Response cache size
    pub response_cache_size: usize,

    /// Response cache TTL in seconds
    pub cache_ttl_secs: u64,

    /// Rate limit: requests per second
    pub rate_limit_rps: f64,

    /// Rate limit burst capacity
    pub rate_limit_burst: usize,
}

/// Agent execution optimization configuration
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentExecutionConfig {
    /// Enable optimized agent execution loop
    pub use_optimized_loop: bool,

    /// Enable performance prediction
    pub enable_performance_prediction: bool,

    /// State transition history size
    pub state_history_size: usize,

    /// Resource monitoring interval in milliseconds
    pub resource_monitor_interval_ms: u64,

    /// Maximum memory usage in MB
    pub max_memory_mb: u64,

    /// Maximum execution time in seconds
    pub max_execution_time_secs: u64,

    /// Idle detection timeout in milliseconds (0 to disable)
    /// When the agent is idle for this duration, it will enter a low-power state
    pub idle_timeout_ms: u64,

    /// Back-off duration in milliseconds when no work is pending
    /// This reduces CPU usage during idle periods
    pub idle_backoff_ms: u64,

    /// Maximum consecutive idle cycles before entering deep sleep
    pub max_idle_cycles: usize,
}

/// Performance profiling configuration
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfilingConfig {
    /// Enable performance profiling
    pub enabled: bool,

    /// Resource monitoring interval in milliseconds
    pub monitor_interval_ms: u64,

    /// Maximum benchmark history size
    pub max_history_size: usize,

    /// Auto-export results to file
    pub auto_export_results: bool,

    /// Export file path
    pub export_file_path: String,

    /// Enable regression testing
    pub enable_regression_testing: bool,

    /// Maximum allowed performance regression percentage
    pub max_regression_percent: f64,
}

impl Default for MemoryPoolConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_string_pool_size: 64,
            max_value_pool_size: 32,
            max_vec_pool_size: 16,
        }
    }
}

impl Default for FileReadCacheConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            min_size_bytes: crate::constants::optimization::FILE_READ_CACHE_MIN_SIZE_BYTES,
            max_size_bytes: crate::constants::optimization::FILE_READ_CACHE_MAX_SIZE_BYTES,
            ttl_secs: crate::constants::optimization::FILE_READ_CACHE_TTL_SECS,
            max_entries: crate::constants::optimization::FILE_READ_CACHE_MAX_ENTRIES,
        }
    }
}

impl Default for CommandCacheConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            ttl_ms: crate::constants::optimization::COMMAND_CACHE_TTL_MS,
            max_entries: crate::constants::optimization::COMMAND_CACHE_MAX_ENTRIES,
            allowlist: crate::constants::optimization::COMMAND_CACHE_ALLOWLIST
                .iter()
                .map(|s| s.to_string())
                .collect(),
        }
    }
}

impl Default for ToolRegistryConfig {
    fn default() -> Self {
        Self {
            use_optimized_registry: true, // Enable by default for better performance
            max_concurrent_tools: 4,
            hot_cache_size: 16,
            default_timeout_secs: 180,
        }
    }
}

impl Default for AsyncPipelineConfig {
    fn default() -> Self {
        Self {
            enable_batching: false, // Conservative default
            enable_caching: true,
            max_batch_size: 5,
            batch_timeout_ms: 100,
            cache_size: 100,
        }
    }
}

impl Default for LLMClientConfig {
    fn default() -> Self {
        Self {
            enable_connection_pooling: false, // Conservative default
            enable_response_caching: true,
            enable_request_batching: false, // Conservative default
            connection_pool_size: 4,
            response_cache_size: 50,
            cache_ttl_secs: 300,
            rate_limit_rps: 10.0,
            rate_limit_burst: 20,
        }
    }
}

impl Default for AgentExecutionConfig {
    fn default() -> Self {
        Self {
            use_optimized_loop: true, // Enable by default for better performance
            enable_performance_prediction: false, // Conservative default
            state_history_size: 1000,
            resource_monitor_interval_ms: 100,
            max_memory_mb: 1024,
            max_execution_time_secs: 300,
            idle_timeout_ms: 5000, // 5 seconds idle timeout
            idle_backoff_ms: 100,  // 100ms backoff during idle
            max_idle_cycles: 10,   // 10 consecutive idle cycles before deep sleep
        }
    }
}

impl Default for ProfilingConfig {
    fn default() -> Self {
        Self {
            enabled: false, // Disabled by default to avoid overhead
            monitor_interval_ms: 100,
            max_history_size: 1000,
            auto_export_results: false,
            export_file_path: "benchmark_results.json".to_string(),
            enable_regression_testing: false,
            max_regression_percent: 10.0,
        }
    }
}

impl OptimizationConfig {
    /// Get optimized configuration for development
    pub fn development() -> Self {
        Self {
            memory_pool: MemoryPoolConfig {
                enabled: true,
                ..Default::default()
            },
            tool_registry: ToolRegistryConfig {
                use_optimized_registry: true,
                max_concurrent_tools: 2,
                ..Default::default()
            },
            async_pipeline: AsyncPipelineConfig {
                enable_batching: true,
                enable_caching: true,
                max_batch_size: 3,
                ..Default::default()
            },
            llm_client: LLMClientConfig {
                enable_connection_pooling: true,
                enable_response_caching: true,
                connection_pool_size: 2,
                rate_limit_rps: 5.0,
                ..Default::default()
            },
            agent_execution: AgentExecutionConfig {
                use_optimized_loop: true,
                enable_performance_prediction: false, // Disabled for dev
                max_memory_mb: 512,
                idle_timeout_ms: 2000, // Shorter idle timeout for development
                idle_backoff_ms: 50,   // Shorter backoff for development
                max_idle_cycles: 5,    // Fewer idle cycles for development
                ..Default::default()
            },
            profiling: ProfilingConfig {
                enabled: true, // Enabled for development
                auto_export_results: true,
                ..Default::default()
            },
            file_read_cache: FileReadCacheConfig::default(),
            command_cache: CommandCacheConfig::default(),
        }
    }

    /// Get optimized configuration for production
    pub fn production() -> Self {
        Self {
            memory_pool: MemoryPoolConfig {
                enabled: true,
                max_string_pool_size: 128,
                max_value_pool_size: 64,
                max_vec_pool_size: 32,
            },
            tool_registry: ToolRegistryConfig {
                use_optimized_registry: true,
                max_concurrent_tools: 8,
                hot_cache_size: 32,
                default_timeout_secs: 300,
            },
            async_pipeline: AsyncPipelineConfig {
                enable_batching: true,
                enable_caching: true,
                max_batch_size: 10,
                batch_timeout_ms: 50,
                cache_size: 200,
            },
            llm_client: LLMClientConfig {
                enable_connection_pooling: true,
                enable_response_caching: true,
                enable_request_batching: true,
                connection_pool_size: 8,
                response_cache_size: 100,
                cache_ttl_secs: 600,
                rate_limit_rps: 20.0,
                rate_limit_burst: 50,
            },
            agent_execution: AgentExecutionConfig {
                use_optimized_loop: true,
                enable_performance_prediction: true,
                state_history_size: 2000,
                resource_monitor_interval_ms: 50,
                max_memory_mb: 2048,
                max_execution_time_secs: 600,
                idle_timeout_ms: 10000, // Longer idle timeout for production
                idle_backoff_ms: 200,   // Longer backoff for production
                max_idle_cycles: 20,    // More idle cycles for production
            },
            profiling: ProfilingConfig {
                enabled: false, // Disabled in production unless needed
                monitor_interval_ms: 1000,
                max_history_size: 500,
                auto_export_results: false,
                export_file_path: "/var/log/vtcode/benchmark_results.json".to_string(),
                enable_regression_testing: true,
                max_regression_percent: 5.0,
            },
            file_read_cache: FileReadCacheConfig {
                enabled: true,
                min_size_bytes: crate::constants::optimization::FILE_READ_CACHE_PROD_MIN_SIZE_BYTES,
                max_size_bytes: crate::constants::optimization::FILE_READ_CACHE_PROD_MAX_SIZE_BYTES,
                ttl_secs: crate::constants::optimization::FILE_READ_CACHE_PROD_TTL_SECS,
                max_entries: crate::constants::optimization::FILE_READ_CACHE_PROD_MAX_ENTRIES,
            },
            command_cache: CommandCacheConfig {
                enabled: true,
                ttl_ms: crate::constants::optimization::COMMAND_CACHE_PROD_TTL_MS,
                max_entries: crate::constants::optimization::COMMAND_CACHE_PROD_MAX_ENTRIES,
                allowlist: crate::constants::optimization::COMMAND_CACHE_PROD_ALLOWLIST
                    .iter()
                    .map(|s| s.to_string())
                    .collect(),
            },
        }
    }
}
