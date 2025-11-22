//! Error types and observability for core improvements
//!
//! Provides structured error handling with context and observability hooks.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Result type for tool improvements operations
pub type ImprovementResult<T> = Result<T, ImprovementError>;

/// Structured errors for tool improvements
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImprovementError {
    /// Error kind
    pub kind: ErrorKind,
    
    /// Context information
    pub context: String,
    
    /// Original source error (if any)
    #[serde(skip)]
    pub source: Option<String>,
    
    /// Operation that failed
    pub operation: String,
    
    /// Severity level
    pub severity: ErrorSeverity,
}

/// Error classifications
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorKind {
    // Scoring errors
    ScoringFailed,
    InvalidMetadata,
    UnsupportedToolType,
    
    // Selection errors
    SelectionFailed,
    NoViableCandidate,
    ContextMissing,
    
    // Fallback errors
    ChainExecutionFailed,
    AllFallbacksFailed,
    TimeoutExceeded,
    
    // Cache errors
    CacheOperationFailed,
    CacheCorrupted,
    SerializationFailed,
    
    // Context errors
    PatternDetectionFailed,
    ContextTruncated,
    
    // Correlation errors
    IntentExtractionFailed,
    CorrelationFailed,
    
    // Configuration errors
    ConfigurationInvalid,
    ConfigurationMissing,
}

/// Error severity level
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorSeverity {
    /// Recoverable, operation should retry
    Warning,
    /// Operation failed but service continues
    Error,
    /// System integrity compromised
    Critical,
}

impl ImprovementError {
    /// Create a new error
    pub fn new(kind: ErrorKind, context: impl Into<String>, operation: impl Into<String>) -> Self {
        Self {
            kind,
            context: context.into(),
            source: None,
            operation: operation.into(),
            severity: ErrorSeverity::Error,
        }
    }

    /// Add source error context
    pub fn with_source(mut self, source: impl fmt::Display) -> Self {
        self.source = Some(source.to_string());
        self
    }

    /// Set severity level
    pub fn with_severity(mut self, severity: ErrorSeverity) -> Self {
        self.severity = severity;
        self
    }

    /// Check if error is recoverable
    pub fn is_recoverable(&self) -> bool {
        self.severity <= ErrorSeverity::Error
    }

    /// Format for logging
    pub fn to_log_entry(&self) -> String {
        format!(
            "[{}] {} - {} ({}{})",
            match self.severity {
                ErrorSeverity::Warning => "WARN",
                ErrorSeverity::Error => "ERROR",
                ErrorSeverity::Critical => "CRIT",
            },
            self.operation,
            self.context,
            match self.kind {
                ErrorKind::ScoringFailed => "scoring_failed",
                ErrorKind::SelectionFailed => "selection_failed",
                ErrorKind::ChainExecutionFailed => "chain_failed",
                ErrorKind::CacheOperationFailed => "cache_failed",
                ErrorKind::ConfigurationInvalid => "config_invalid",
                _ => "unknown",
            },
            self.source
                .as_ref()
                .map(|s| format!(": {}", s))
                .unwrap_or_default()
        )
    }
}

impl fmt::Display for ImprovementError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}: {} ({})",
            self.operation, self.context, self.to_log_entry()
        )
    }
}

impl std::error::Error for ImprovementError {}

/// Observability event for tool improvements
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImprovementEvent {
    /// Event type
    pub event_type: EventType,
    
    /// Component that generated event
    pub component: String,
    
    /// Detailed message
    pub message: String,
    
    /// Metric value (if applicable)
    pub metric: Option<f32>,
    
    /// Timestamp (unix seconds)
    pub timestamp: u64,
}

/// Types of observable events
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EventType {
    // Scoring events
    ResultScored,
    ScoreDegraded,
    
    // Selection events
    ToolSelected,
    SelectionAlternative,
    
    // Fallback events
    FallbackAttempt,
    FallbackSuccess,
    ChainAborted,
    
    // Cache events
    CacheHit,
    CacheMiss,
    CacheEvicted,
    
    // Context events
    PatternDetected,
    RedundancyDetected,
    
    // Correlation events
    IntentExtracted,
    FulfillmentAssessed,
    
    // Error events
    ErrorOccurred,
    ErrorRecovered,
}

/// Observability sink for receiving events
pub trait ObservabilitySink: Send + Sync {
    /// Record an event
    fn record_event(&self, event: ImprovementEvent);
    
    /// Record an error
    fn record_error(&self, error: &ImprovementError);
    
    /// Record a metric
    fn record_metric(&self, component: &str, name: &str, value: f32);
}

/// No-op observability sink (for when observability is disabled)
pub struct NoOpSink;

impl ObservabilitySink for NoOpSink {
    fn record_event(&self, _event: ImprovementEvent) {}
    fn record_error(&self, _error: &ImprovementError) {}
    fn record_metric(&self, _component: &str, _name: &str, _value: f32) {}
}

/// Logging-based observability sink
pub struct LoggingSink;

impl ObservabilitySink for LoggingSink {
    fn record_event(&self, event: ImprovementEvent) {
        match event.event_type {
            EventType::ErrorOccurred => {
                tracing::error!(
                    component = %event.component,
                    event_type = ?event.event_type,
                    message = %event.message,
                    metric = event.metric,
                    timestamp = event.timestamp,
                    "improvement_event"
                );
            }
            EventType::PatternDetected => {
                tracing::debug!(
                    component = %event.component,
                    event_type = ?event.event_type,
                    message = %event.message,
                    metric = event.metric,
                    timestamp = event.timestamp,
                    "improvement_event"
                );
            }
            EventType::CacheHit => {
                tracing::trace!(
                    component = %event.component,
                    event_type = ?event.event_type,
                    message = %event.message,
                    metric = event.metric,
                    timestamp = event.timestamp,
                    "improvement_event"
                );
            }
            _ => {
                tracing::info!(
                    component = %event.component,
                    event_type = ?event.event_type,
                    message = %event.message,
                    metric = event.metric,
                    timestamp = event.timestamp,
                    "improvement_event"
                );
            }
        }
    }

    fn record_error(&self, error: &ImprovementError) {
        tracing::error!(
            operation = %error.operation,
            severity = ?error.severity,
            context = %error.context,
            source = ?error.source,
            "improvement_error: {}",
            error
        );
    }

    fn record_metric(&self, component: &str, name: &str, value: f32) {
        tracing::debug!(
            component = %component,
            metric = %name,
            value = value,
            "metric recorded"
        );
    }
}

/// Global observability context
pub struct ObservabilityContext {
    sink: Box<dyn ObservabilitySink>,
}

impl ObservabilityContext {
    /// Create with no-op sink
    pub fn noop() -> Self {
        Self {
            sink: Box::new(NoOpSink),
        }
    }

    /// Create with logging sink
    pub fn logging() -> Self {
        Self {
            sink: Box::new(LoggingSink),
        }
    }

    /// Record event
    pub fn event(
        &self,
        event_type: EventType,
        component: impl Into<String>,
        message: impl Into<String>,
        metric: Option<f32>,
    ) {
        let event = ImprovementEvent {
            event_type,
            component: component.into(),
            message: message.into(),
            metric,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        };
        self.sink.record_event(event);
    }

    /// Record error
    pub fn error(&self, error: &ImprovementError) {
        self.sink.record_error(error);
    }

    /// Record metric
    pub fn metric(&self, component: &str, name: &str, value: f32) {
        self.sink.record_metric(component, name, value);
    }
}

impl Default for ObservabilityContext {
    fn default() -> Self {
        Self::noop()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_creation() {
        let err = ImprovementError::new(
            ErrorKind::ScoringFailed,
            "result score too low",
            "score_result",
        );

        assert_eq!(err.kind, ErrorKind::ScoringFailed);
        assert_eq!(err.severity, ErrorSeverity::Error);
        assert!(err.is_recoverable());
    }

    #[test]
    fn test_error_severity() {
        let err = ImprovementError::new(
            ErrorKind::CacheCorrupted,
            "cache state invalid",
            "cache_read",
        )
        .with_severity(ErrorSeverity::Critical);

        assert_eq!(err.severity, ErrorSeverity::Critical);
        assert!(!err.is_recoverable());
    }

    #[test]
    fn test_error_logging() {
        let err = ImprovementError::new(
            ErrorKind::SelectionFailed,
            "no candidates available",
            "select_tool",
        )
        .with_source("context is empty");

        let log = err.to_log_entry();
        assert!(log.contains("ERROR"));
        assert!(log.contains("select_tool"));
    }

    #[test]
    fn test_observability_sink() {
        let sink = NoOpSink;
        let event = ImprovementEvent {
            event_type: EventType::ToolSelected,
            component: "selector".to_string(),
            message: "selected grep_file".to_string(),
            metric: Some(0.95),
            timestamp: 0,
        };

        // Should not panic
        sink.record_event(event);
    }
}
