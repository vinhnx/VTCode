// Metrics collection and observability for MCP execution system
//
// Tracks performance, effectiveness, and security across all execution steps:
// - Tool discovery (hit rate, response time)
// - Code execution (duration, success rate, memory)
// - SDK generation (overhead, caching)
// - Data filtering (reduction ratio, token savings)
// - Skill usage (adoption, reuse patterns)
// - PII detection (pattern matches, audit trail)

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use std::time::Instant;

pub mod discovery_metrics;
pub mod execution_metrics;
pub mod sdk_metrics;
pub mod filtering_metrics;
pub mod skill_metrics;
pub mod security_metrics;

pub use discovery_metrics::DiscoveryMetrics;
pub use execution_metrics::ExecutionMetrics;
pub use sdk_metrics::SdkMetrics;
pub use filtering_metrics::FilteringMetrics;
pub use skill_metrics::SkillMetrics;
pub use security_metrics::SecurityMetrics;

/// Central metrics collector for all MCP execution activities
#[derive(Clone)]
pub struct MetricsCollector {
    discovery: Arc<Mutex<DiscoveryMetrics>>,
    execution: Arc<Mutex<ExecutionMetrics>>,
    sdk: Arc<Mutex<SdkMetrics>>,
    filtering: Arc<Mutex<FilteringMetrics>>,
    skills: Arc<Mutex<SkillMetrics>>,
    security: Arc<Mutex<SecurityMetrics>>,
    start_time: Instant,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsSummary {
    pub timestamp: DateTime<Utc>,
    pub session_duration_ms: u64,
    pub discovery: DiscoveryMetrics,
    pub execution: ExecutionMetrics,
    pub sdk: SdkMetrics,
    pub filtering: FilteringMetrics,
    pub skills: SkillMetrics,
    pub security: SecurityMetrics,
}

impl MetricsCollector {
    /// Create a new metrics collector
    pub fn new() -> Self {
        Self {
            discovery: Arc::new(Mutex::new(DiscoveryMetrics::new())),
            execution: Arc::new(Mutex::new(ExecutionMetrics::new())),
            sdk: Arc::new(Mutex::new(SdkMetrics::new())),
            filtering: Arc::new(Mutex::new(FilteringMetrics::new())),
            skills: Arc::new(Mutex::new(SkillMetrics::new())),
            security: Arc::new(Mutex::new(SecurityMetrics::new())),
            start_time: Instant::now(),
        }
    }

    // ========== Discovery Metrics ==========

    /// Record a tool discovery query
    pub fn record_discovery_query(&self, keyword: String, result_count: u64, response_time_ms: u64) {
        if let Ok(mut metrics) = self.discovery.lock() {
            metrics.record_query(keyword, result_count, response_time_ms);
        }
    }

    /// Record a failed discovery query
    pub fn record_discovery_failure(&self, keyword: String) {
        if let Ok(mut metrics) = self.discovery.lock() {
            metrics.record_failure(keyword);
        }
    }

    /// Record a discovery cache hit
    pub fn record_discovery_cache_hit(&self) {
        if let Ok(mut metrics) = self.discovery.lock() {
            metrics.record_cache_hit();
        }
    }

    // ========== Execution Metrics ==========

    /// Record the start of a code execution
    pub fn record_execution_start(&self, language: String) {
        if let Ok(mut metrics) = self.execution.lock() {
            metrics.record_start(language);
        }
    }

    /// Record successful execution completion
    pub fn record_execution_complete(&self, language: String, duration_ms: u64, memory_mb: u64) {
        if let Ok(mut metrics) = self.execution.lock() {
            metrics.record_complete(language, duration_ms, memory_mb, true);
        }
    }

    /// Record failed execution
    pub fn record_execution_failure(&self, language: String, duration_ms: u64) {
        if let Ok(mut metrics) = self.execution.lock() {
            metrics.record_failure(language, duration_ms);
        }
    }

    /// Record execution timeout
    pub fn record_execution_timeout(&self, language: String, duration_ms: u64) {
        if let Ok(mut metrics) = self.execution.lock() {
            metrics.record_timeout(language, duration_ms);
        }
    }

    /// Record result size for filtering calculation
    pub fn record_result_size(&self, size_bytes: usize) {
        if let Ok(mut metrics) = self.execution.lock() {
            metrics.record_result_size(size_bytes);
        }
    }

    // ========== SDK Metrics ==========

    /// Record SDK generation
    pub fn record_sdk_generation(&self, generation_time_ms: u64, tools_count: u64) {
        if let Ok(mut metrics) = self.sdk.lock() {
            metrics.record_generation(generation_time_ms, tools_count);
        }
    }

    /// Record SDK cache utilization
    pub fn record_sdk_cache_hit(&self) {
        if let Ok(mut metrics) = self.sdk.lock() {
            metrics.record_cache_hit();
        }
    }

    // ========== Filtering Metrics ==========

    /// Record a filtering operation
    pub fn record_filtering_operation(
        &self,
        operation_type: String,
        input_size: u64,
        output_size: u64,
        duration_ms: u64,
    ) {
        if let Ok(mut metrics) = self.filtering.lock() {
            metrics.record_operation(operation_type, input_size, output_size, duration_ms);
        }
    }

    // ========== Skill Metrics ==========

    /// Record skill execution
    pub fn record_skill_execution(&self, skill_name: String, duration_ms: u64, success: bool) {
        if let Ok(mut metrics) = self.skills.lock() {
            metrics.record_execution(skill_name, duration_ms, success);
        }
    }

    /// Record skill creation
    pub fn record_skill_created(&self, skill_name: String, language: String) {
        if let Ok(mut metrics) = self.skills.lock() {
            metrics.record_created(skill_name, language);
        }
    }

    /// Record skill deletion
    pub fn record_skill_deleted(&self, skill_name: String) {
        if let Ok(mut metrics) = self.skills.lock() {
            metrics.record_deleted(skill_name);
        }
    }

    // ========== Security Metrics ==========

    /// Record PII pattern detection
    pub fn record_pii_detection(&self, pattern_type: String) {
        if let Ok(mut metrics) = self.security.lock() {
            metrics.record_detection(pattern_type);
        }
    }

    /// Record tokenization
    pub fn record_pii_tokenization(&self, token_count: usize) {
        if let Ok(mut metrics) = self.security.lock() {
            metrics.record_tokenization(token_count);
        }
    }

    /// Record audit event
    pub fn record_audit_event(&self, event_type: String, severity: String) {
        if let Ok(mut metrics) = self.security.lock() {
            metrics.record_audit_event(event_type, severity);
        }
    }

    // ========== Queries ==========

    /// Get current discovery metrics snapshot
    pub fn get_discovery_metrics(&self) -> DiscoveryMetrics {
        self.discovery
            .lock()
            .map(|m| m.clone())
            .unwrap_or_else(|_| DiscoveryMetrics::new())
    }

    /// Get current execution metrics snapshot
    pub fn get_execution_metrics(&self) -> ExecutionMetrics {
        self.execution
            .lock()
            .map(|m| m.clone())
            .unwrap_or_else(|_| ExecutionMetrics::new())
    }

    /// Get current SDK metrics snapshot
    pub fn get_sdk_metrics(&self) -> SdkMetrics {
        self.sdk
            .lock()
            .map(|m| m.clone())
            .unwrap_or_else(|_| SdkMetrics::new())
    }

    /// Get current filtering metrics snapshot
    pub fn get_filtering_metrics(&self) -> FilteringMetrics {
        self.filtering
            .lock()
            .map(|m| m.clone())
            .unwrap_or_else(|_| FilteringMetrics::new())
    }

    /// Get current skill metrics snapshot
    pub fn get_skill_metrics(&self) -> SkillMetrics {
        self.skills
            .lock()
            .map(|m| m.clone())
            .unwrap_or_else(|_| SkillMetrics::new())
    }

    /// Get current security metrics snapshot
    pub fn get_security_metrics(&self) -> SecurityMetrics {
        self.security
            .lock()
            .map(|m| m.clone())
            .unwrap_or_else(|_| SecurityMetrics::new())
    }

    /// Get comprehensive summary of all metrics
    pub fn get_summary(&self) -> MetricsSummary {
        MetricsSummary {
            timestamp: Utc::now(),
            session_duration_ms: self.start_time.elapsed().as_millis() as u64,
            discovery: self.get_discovery_metrics(),
            execution: self.get_execution_metrics(),
            sdk: self.get_sdk_metrics(),
            filtering: self.get_filtering_metrics(),
            skills: self.get_skill_metrics(),
            security: self.get_security_metrics(),
        }
    }

    // ========== Export ==========

    /// Export metrics as JSON
    pub fn export_json(&self) -> anyhow::Result<serde_json::Value> {
        let summary = self.get_summary();
        Ok(serde_json::to_value(summary)?)
    }

    /// Export metrics in Prometheus format
    pub fn export_prometheus(&self) -> String {
        let discovery = self.get_discovery_metrics();
        let execution = self.get_execution_metrics();
        let filtering = self.get_filtering_metrics();
        let skills = self.get_skill_metrics();
        let security = self.get_security_metrics();

        let mut output = String::new();

        // Discovery metrics
        output.push_str(&format!(
            "# HELP vtcode_discovery_queries_total Total tool discovery queries\n\
             # TYPE vtcode_discovery_queries_total counter\n\
             vtcode_discovery_queries_total {}\n\n",
            discovery.total_queries
        ));

        output.push_str(&format!(
            "# HELP vtcode_discovery_hit_rate Hit rate of discovery queries\n\
             # TYPE vtcode_discovery_hit_rate gauge\n\
             vtcode_discovery_hit_rate {}\n\n",
            discovery.hit_rate()
        ));

        // Execution metrics
        output.push_str(&format!(
            "# HELP vtcode_execution_total Total code executions\n\
             # TYPE vtcode_execution_total counter\n\
             vtcode_execution_total {}\n\n",
            execution.total_executions
        ));

        output.push_str(&format!(
            "# HELP vtcode_execution_duration_ms Code execution average duration\n\
             # TYPE vtcode_execution_duration_ms gauge\n\
             vtcode_execution_duration_ms {}\n\n",
            execution.avg_duration_ms()
        ));

        // Filtering metrics
        output.push_str(&format!(
            "# HELP vtcode_filtering_operations_total Total filtering operations\n\
             # TYPE vtcode_filtering_operations_total counter\n\
             vtcode_filtering_operations_total {}\n\n",
            filtering.total_operations
        ));

        output.push_str(&format!(
            "# HELP vtcode_context_tokens_saved Estimated tokens saved by filtering\n\
             # TYPE vtcode_context_tokens_saved counter\n\
             vtcode_context_tokens_saved {}\n\n",
            filtering.estimated_tokens_saved()
        ));

        // Skills metrics
        output.push_str(&format!(
            "# HELP vtcode_skills_total Total saved skills\n\
             # TYPE vtcode_skills_total gauge\n\
             vtcode_skills_total {}\n\n",
            skills.total_skills
        ));

        output.push_str(&format!(
            "# HELP vtcode_skill_reuse_ratio Ratio of skill reuse\n\
             # TYPE vtcode_skill_reuse_ratio gauge\n\
             vtcode_skill_reuse_ratio {}\n\n",
            skills.reuse_ratio()
        ));

        // Security metrics
        output.push_str(&format!(
            "# HELP vtcode_pii_detections_total Total PII patterns detected\n\
             # TYPE vtcode_pii_detections_total counter\n\
             vtcode_pii_detections_total {}\n\n",
            security.pii_detections
        ));

        output.push_str(&format!(
            "# HELP vtcode_tokens_created_total Total PII tokens created\n\
             # TYPE vtcode_tokens_created_total counter\n\
             vtcode_tokens_created_total {}\n\n",
            security.tokens_created
        ));

        output
    }
}

impl Default for MetricsCollector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_collector_creation() {
        let collector = MetricsCollector::new();
        let summary = collector.get_summary();
        assert_eq!(summary.discovery.total_queries, 0);
        assert_eq!(summary.execution.total_executions, 0);
    }

    #[test]
    fn test_discovery_metrics_recording() {
        let collector = MetricsCollector::new();
        collector.record_discovery_query("file".to_string(), 5, 50);
        
        let metrics = collector.get_discovery_metrics();
        assert_eq!(metrics.total_queries, 1);
        assert!(metrics.avg_response_time_ms() > 0);
    }

    #[test]
    fn test_execution_metrics_recording() {
        let collector = MetricsCollector::new();
        collector.record_execution_start("python3".to_string());
        collector.record_execution_complete("python3".to_string(), 1000, 50);
        
        let metrics = collector.get_execution_metrics();
        assert_eq!(metrics.total_executions, 1);
        assert_eq!(metrics.successful_executions, 1);
        assert_eq!(metrics.avg_duration_ms(), 1000);
    }

    #[test]
    fn test_metrics_summary_export() {
        let collector = MetricsCollector::new();
        collector.record_discovery_query("test".to_string(), 3, 30);
        collector.record_pii_detection("email".to_string());
        
        let summary = collector.get_summary();
        assert!(summary.session_duration_ms >= 0);
        assert_eq!(summary.discovery.total_queries, 1);
        assert_eq!(summary.security.pii_detections, 1);
    }

    #[test]
    fn test_prometheus_export() {
        let collector = MetricsCollector::new();
        collector.record_execution_complete("python3".to_string(), 500, 40);
        
        let prometheus = collector.export_prometheus();
        assert!(prometheus.contains("vtcode_execution_total"));
        assert!(prometheus.contains("vtcode_execution_duration_ms"));
    }

    #[test]
    fn test_json_export() {
        let collector = MetricsCollector::new();
        collector.record_discovery_query("test".to_string(), 2, 25);
        
        let json = collector.export_json().unwrap();
        assert!(json.get("timestamp").is_some());
        assert!(json.get("discovery").is_some());
    }
}
