use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityMetrics {
    pub pii_detections: u64,
    pub tokens_created: u64,
    pub pattern_distribution: HashMap<String, u64>,
    pub audit_events: VecDeque<AuditEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEvent {
    pub event_type: String,
    pub pattern_type: String,
    pub severity: String,
    pub timestamp: DateTime<Utc>,
}

impl SecurityMetrics {
    pub fn new() -> Self {
        Self {
            pii_detections: 0,
            tokens_created: 0,
            pattern_distribution: HashMap::new(),
            audit_events: VecDeque::with_capacity(100),
        }
    }

    pub fn record_detection(&mut self, pattern_type: String) {
        self.pii_detections += 1;
        *self.pattern_distribution.entry(pattern_type).or_insert(0) += 1;
    }

    pub fn record_tokenization(&mut self, token_count: usize) {
        self.tokens_created += token_count as u64;
    }

    pub fn record_audit_event(&mut self, event_type: String, severity: String) {
        let event = AuditEvent {
            event_type,
            pattern_type: String::new(),
            severity,
            timestamp: Utc::now(),
        };

        if self.audit_events.len() >= 100 {
            self.audit_events.pop_front();
        }
        self.audit_events.push_back(event);
    }

    pub fn detection_rate(&self) -> f64 {
        if !self.audit_events.is_empty() {
            self.pii_detections as f64 / self.audit_events.len() as f64
        } else {
            0.0
        }
    }

    pub fn get_pattern_count(&self, pattern_type: &str) -> u64 {
        self.pattern_distribution.get(pattern_type).copied().unwrap_or(0)
    }

    pub fn get_most_detected_pattern(&self) -> Option<String> {
        self.pattern_distribution
            .iter()
            .max_by_key(|(_, count)| *count)
            .map(|(name, _)| name.clone())
    }

    pub fn get_high_severity_events(&self) -> Vec<AuditEvent> {
        self.audit_events
            .iter()
            .filter(|e| e.severity == "high" || e.severity == "critical")
            .cloned()
            .collect()
    }
}

impl Default for SecurityMetrics {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_record_detection() {
        let mut metrics = SecurityMetrics::new();
        metrics.record_detection("email".to_string());
        metrics.record_detection("ssn".to_string());
        metrics.record_detection("email".to_string());

        assert_eq!(metrics.pii_detections, 3);
        assert_eq!(metrics.get_pattern_count("email"), 2);
        assert_eq!(metrics.get_pattern_count("ssn"), 1);
    }

    #[test]
    fn test_record_tokenization() {
        let mut metrics = SecurityMetrics::new();
        metrics.record_tokenization(5);
        metrics.record_tokenization(3);

        assert_eq!(metrics.tokens_created, 8);
    }

    #[test]
    fn test_record_audit_event() {
        let mut metrics = SecurityMetrics::new();
        metrics.record_audit_event("pii_detected".to_string(), "info".to_string());
        metrics.record_audit_event("tokenized".to_string(), "info".to_string());

        assert_eq!(metrics.audit_events.len(), 2);
    }

    #[test]
    fn test_detection_rate() {
        let mut metrics = SecurityMetrics::new();
        metrics.record_detection("email".to_string());
        metrics.record_detection("email".to_string());
        metrics.record_audit_event("test".to_string(), "info".to_string());

        let rate = metrics.detection_rate();
        assert!(rate > 0.0);
    }

    #[test]
    fn test_most_detected_pattern() {
        let mut metrics = SecurityMetrics::new();
        metrics.record_detection("email".to_string());
        metrics.record_detection("email".to_string());
        metrics.record_detection("email".to_string());
        metrics.record_detection("ssn".to_string());

        assert_eq!(metrics.get_most_detected_pattern(), Some("email".to_string()));
    }

    #[test]
    fn test_high_severity_events() {
        let mut metrics = SecurityMetrics::new();
        metrics.record_audit_event("event1".to_string(), "low".to_string());
        metrics.record_audit_event("event2".to_string(), "high".to_string());
        metrics.record_audit_event("event3".to_string(), "critical".to_string());
        metrics.record_audit_event("event4".to_string(), "info".to_string());

        let high_severity = metrics.get_high_severity_events();
        assert_eq!(high_severity.len(), 2);
    }

    #[test]
    fn test_audit_events_limit() {
        let mut metrics = SecurityMetrics::new();
        for i in 0..150 {
            metrics.record_audit_event(format!("event_{}", i), "info".to_string());
        }

        assert_eq!(metrics.audit_events.len(), 100);
    }
}
