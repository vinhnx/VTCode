use crate::core::timeout_detector::{OperationType, TIMEOUT_DETECTOR};
use crate::utils::current_timestamp;
use anyhow::Result;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Represents an error that occurred during execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionError {
    pub id: String,
    pub timestamp: u64,
    pub error_type: ErrorType,
    pub message: String,
    pub context: ErrorContext,
    pub recovery_attempts: Vec<RecoveryAttempt>,
    pub resolved: bool,
}

/// Type of error that can occur
///
/// Canonical error type used across both the global error recovery manager
/// and agent-specific error state tracking. Superset of all error categories.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Eq, Hash, PartialEq)]
pub enum ErrorType {
    ToolExecution,
    ApiCall,
    FileSystem,
    Network,
    Validation,
    CircuitBreaker,
    Timeout,
    PermissionDenied,
    InvalidArguments,
    ResourceNotFound,
    Other,
}

/// Context information about where and why the error occurred
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorContext {
    pub conversation_turn: usize,
    pub user_input: Option<String>,
    pub tool_name: Option<String>,
    pub tool_args: Option<Value>,
    pub api_request_size: Option<usize>,
    pub context_size: Option<usize>,
    pub stack_trace: Option<String>,
}

/// A recovery attempt that was made
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryAttempt {
    pub timestamp: u64,
    pub strategy: RecoveryStrategy,
    pub success: bool,
    pub result: String,
    pub new_context_size: Option<usize>,
}

/// Recovery strategy used to handle the error
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RecoveryStrategy {
    RetryWithBackoff {
        delay_ms: u64,
        attempt_number: usize,
    },

    SimplifyRequest {
        removed_parameters: Vec<String>,
    },
    AlternativeTool {
        original_tool: String,
        alternative_tool: String,
    },
    ContextReset {
        preserved_data: IndexMap<String, Value>,
    },
    ManualIntervention,
}

/// Error recovery manager
pub struct ErrorRecoveryManager {
    errors: Vec<ExecutionError>,
    recovery_strategies: IndexMap<ErrorType, Vec<RecoveryStrategy>>,
    operation_type_mapping: IndexMap<ErrorType, OperationType>,
}

impl Default for ErrorRecoveryManager {
    fn default() -> Self {
        Self::new()
    }
}

impl ErrorRecoveryManager {
    pub fn new() -> Self {
        // Pre-allocate with known capacity
        let mut recovery_strategies = IndexMap::with_capacity(2);
        let mut operation_type_mapping = IndexMap::with_capacity(11);

        // Define recovery strategies for different error types
        recovery_strategies.insert(
            ErrorType::ToolExecution,
            vec![
                RecoveryStrategy::RetryWithBackoff {
                    delay_ms: 1000,
                    attempt_number: 1,
                },
                RecoveryStrategy::AlternativeTool {
                    original_tool: String::new(),
                    alternative_tool: String::new(),
                },
            ],
        );

        recovery_strategies.insert(
            ErrorType::ApiCall,
            vec![
                RecoveryStrategy::RetryWithBackoff {
                    delay_ms: 2000,
                    attempt_number: 1,
                },
                RecoveryStrategy::ContextReset {
                    preserved_data: IndexMap::new(),
                },
            ],
        );

        // Map error types to operation types for timeout detector integration
        operation_type_mapping.insert(ErrorType::ToolExecution, OperationType::ToolExecution);
        operation_type_mapping.insert(ErrorType::ApiCall, OperationType::ApiCall);
        operation_type_mapping.insert(ErrorType::Network, OperationType::NetworkRequest);
        operation_type_mapping.insert(ErrorType::FileSystem, OperationType::FileOperation);
        operation_type_mapping.insert(ErrorType::Validation, OperationType::Processing);
        operation_type_mapping.insert(ErrorType::CircuitBreaker, OperationType::ToolExecution);
        operation_type_mapping.insert(ErrorType::Timeout, OperationType::Processing);
        operation_type_mapping.insert(ErrorType::PermissionDenied, OperationType::Processing);
        operation_type_mapping.insert(ErrorType::InvalidArguments, OperationType::Processing);
        operation_type_mapping.insert(ErrorType::ResourceNotFound, OperationType::FileOperation);
        operation_type_mapping.insert(ErrorType::Other, OperationType::Processing);

        Self {
            errors: Vec::with_capacity(16), // Pre-allocate for typical session
            recovery_strategies,
            operation_type_mapping,
        }
    }

    /// Record a new error
    pub fn record_error(
        &mut self,
        error_type: ErrorType,
        message: String,
        context: ErrorContext,
    ) -> String {
        // Use a more efficient ID generation with minimal formatting
        let error_count = self.errors.len();
        let timestamp_short = current_timestamp() % 10000;
        let error_id = format!("e{}_{}", error_count, timestamp_short);

        let error = ExecutionError {
            id: error_id.clone(),
            timestamp: current_timestamp(),
            error_type, // ErrorType is Copy now, no need to clone
            message,
            context,
            recovery_attempts: Vec::with_capacity(2), // Most errors have 1-2 recovery attempts
            resolved: false,
        };

        self.errors.push(error);
        error_id
    }

    /// Record a recovery attempt
    #[inline]
    pub fn record_recovery_attempt(
        &mut self,
        error_id: &str,
        strategy: RecoveryStrategy,
        success: bool,
        result: String,
        new_context_size: Option<usize>,
    ) {
        let attempt = RecoveryAttempt {
            timestamp: current_timestamp(),
            strategy,
            success,
            result,
            new_context_size,
        };

        if let Some(error) = self.errors.iter_mut().find(|e| e.id == error_id) {
            error.recovery_attempts.push(attempt);
            if success {
                error.resolved = true;
            }
        }
    }

    /// Get recovery strategies for a specific error type
    #[inline]
    pub fn get_recovery_strategies(&self, error_type: &ErrorType) -> &[RecoveryStrategy] {
        self.recovery_strategies
            .get(error_type)
            .map(|strategies| strategies.as_slice())
            .unwrap_or(&[])
    }

    /// Generate a context preservation plan
    pub fn generate_context_preservation_plan(
        &self,
        context_size: usize,
        error_count: usize,
    ) -> ContextPreservationPlan {
        let critical_errors = error_count > 5;

        let strategies = if critical_errors {
            vec![
                PreservationStrategy::SelectiveRetention {
                    preserve_decisions: true,
                    preserve_errors: true,
                },
                PreservationStrategy::ContextReset {
                    preserve_session_data: true,
                },
            ]
        } else {
            vec![PreservationStrategy::NoAction]
        };

        ContextPreservationPlan {
            current_context_size: context_size,
            error_count,
            recommended_strategies: strategies,
            urgency: if critical_errors {
                Urgency::Critical
            } else {
                Urgency::Low
            },
        }
    }

    /// Get error statistics
    pub fn get_error_statistics(&self) -> ErrorStatistics {
        let total_errors = self.errors.len();
        if total_errors == 0 {
            return ErrorStatistics {
                total_errors: 0,
                resolved_errors: 0,
                unresolved_errors: 0,
                errors_by_type: IndexMap::new(),
                avg_recovery_attempts: 0.0,
                recent_errors: Vec::new(),
            };
        }

        let resolved_errors = self.errors.iter().filter(|e| e.resolved).count();
        let unresolved_errors = total_errors - resolved_errors;

        // Use a more efficient approach for counting by type
        let mut errors_by_type = IndexMap::new();
        let mut total_attempts = 0usize;

        for error in &self.errors {
            *errors_by_type.entry(error.error_type).or_insert(0) += 1;
            total_attempts += error.recovery_attempts.len();
        }

        let avg_recovery_attempts = total_attempts as f64 / total_errors as f64;

        // Get recent errors more efficiently
        let recent_count = total_errors.min(5);
        let recent_errors: Vec<_> = self
            .errors
            .iter()
            .rev()
            .take(recent_count)
            .cloned()
            .collect();

        ErrorStatistics {
            total_errors,
            resolved_errors,
            unresolved_errors,
            errors_by_type,
            avg_recovery_attempts,
            recent_errors,
        }
    }

    /// Check if a specific error pattern is recurring
    pub fn detect_error_pattern(&self, error_type: &ErrorType, time_window_seconds: u64) -> bool {
        let now = current_timestamp();

        let recent_errors = self
            .errors
            .iter()
            .filter(|e| e.error_type == *error_type && (now - e.timestamp) < time_window_seconds)
            .count();

        recent_errors >= 3 // Consider it a pattern if 3+ similar errors in time window
    }

    /// Get the corresponding operation type for an error type
    pub fn get_operation_type(&self, error_type: &ErrorType) -> OperationType {
        self.operation_type_mapping
            .get(error_type)
            .cloned()
            .unwrap_or(OperationType::Processing)
    }

    /// Execute an operation with intelligent timeout detection and recovery
    pub async fn execute_with_recovery<F, Fut, T>(
        &mut self,
        operation_id: String,
        error_type: ErrorType,
        _context: ErrorContext,
        operation: F,
    ) -> Result<T, anyhow::Error>
    where
        F: Fn() -> Fut,
        Fut: std::future::Future<Output = Result<T, anyhow::Error>>,
    {
        let operation_type = self.get_operation_type(&error_type);

        TIMEOUT_DETECTOR
            .execute_with_timeout_retry(operation_id, operation_type, operation)
            .await
    }

    /// Check if an operation should be retried based on error analysis
    pub async fn should_retry_operation(
        &self,
        error_type: &ErrorType,
        error: &anyhow::Error,
        attempt: u32,
    ) -> bool {
        let operation_type = self.get_operation_type(error_type);
        TIMEOUT_DETECTOR
            .should_retry(&operation_type, error, attempt)
            .await
    }

    /// Get timeout statistics for monitoring and optimization
    pub async fn get_timeout_stats(&self) -> crate::core::timeout_detector::TimeoutStats {
        TIMEOUT_DETECTOR.get_stats().await
    }

    /// Configure timeout settings for a specific error type
    pub async fn configure_timeout_for_error_type(
        &self,
        error_type: ErrorType,
        config: crate::core::timeout_detector::TimeoutConfig,
    ) {
        let operation_type = self.get_operation_type(&error_type);
        TIMEOUT_DETECTOR.set_config(operation_type, config).await;
    }

    /// Generate enhanced recovery plan based on timeout detector insights
    pub async fn generate_enhanced_recovery_plan(
        &self,
        context_size: usize,
        error_count: usize,
    ) -> EnhancedContextPreservationPlan {
        let timeout_stats = self.get_timeout_stats().await;
        let base_plan = self.generate_context_preservation_plan(context_size, error_count);

        // Enhance the plan based on timeout detector insights
        let timeout_rate = if timeout_stats.total_operations > 0 {
            timeout_stats.timed_out_operations as f64 / timeout_stats.total_operations as f64
        } else {
            0.0
        };

        let retry_success_rate = if timeout_stats.total_retry_attempts > 0 {
            timeout_stats.successful_retries as f64 / timeout_stats.total_retry_attempts as f64
        } else {
            1.0
        };

        // Adjust urgency based on timeout patterns
        let _adjusted_urgency = if timeout_rate > 0.3 {
            // High timeout rate indicates systemic issues
            Urgency::Critical
        } else if retry_success_rate < 0.5 {
            // Low retry success rate indicates recovery issues
            Urgency::High
        } else {
            base_plan.urgency.clone()
        };

        EnhancedContextPreservationPlan {
            base_plan,
            timeout_rate,
            retry_success_rate,
            timeout_stats,
        }
    }

    /// Get the number of errors (for context preservation plan)
    pub fn error_count(&self) -> usize {
        self.errors.len()
    }
}

/// Plan for preserving context during error recovery
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextPreservationPlan {
    pub current_context_size: usize,
    pub error_count: usize,
    pub recommended_strategies: Vec<PreservationStrategy>,
    pub urgency: Urgency,
}

/// Strategy for preserving context
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PreservationStrategy {
    SelectiveRetention {
        preserve_decisions: bool,
        preserve_errors: bool,
    },
    ContextReset {
        preserve_session_data: bool,
    },
    NoAction,
}

/// Urgency level for context preservation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Urgency {
    Low,
    High,
    Critical,
}

/// Statistics about errors in the session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorStatistics {
    pub total_errors: usize,
    pub resolved_errors: usize,
    pub unresolved_errors: usize,
    pub errors_by_type: IndexMap<ErrorType, usize>,
    pub avg_recovery_attempts: f64,
    pub recent_errors: Vec<ExecutionError>,
}

/// Enhanced context preservation plan with timeout detector insights
#[derive(Debug, Clone)]
pub struct EnhancedContextPreservationPlan {
    pub base_plan: ContextPreservationPlan,
    pub timeout_rate: f64,
    pub retry_success_rate: f64,
    pub timeout_stats: crate::core::timeout_detector::TimeoutStats,
}
