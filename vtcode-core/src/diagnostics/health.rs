use std::collections::VecDeque;
use std::time::SystemTime;

use anyhow::Result;

/// Individual health datapoint for predictive detection.
#[derive(Debug, Clone)]
pub struct HealthSample {
    pub timestamp: SystemTime,
    pub latency_ms: u64,
    pub success: bool,
}

impl HealthSample {
    pub fn new(latency_ms: u64, success: bool) -> Self {
        Self {
            timestamp: SystemTime::now(),
            latency_ms,
            success,
        }
    }
}

/// Sliding-window monitor that surfaces degradation.
#[derive(Debug)]
pub struct PredictiveMonitor {
    samples: VecDeque<HealthSample>,
    max_samples: usize,
    failure_threshold: f32,
    latency_budget_ms: u64,
}

impl PredictiveMonitor {
    pub fn new(max_samples: usize, failure_threshold: f32, latency_budget_ms: u64) -> Self {
        Self {
            samples: VecDeque::with_capacity(max_samples),
            max_samples,
            failure_threshold,
            latency_budget_ms,
        }
    }

    pub fn record(&mut self, sample: HealthSample) {
        self.samples.push_back(sample);
        if self.samples.len() > self.max_samples {
            self.samples.pop_front();
        }
    }

    pub fn failure_rate(&self) -> f32 {
        if self.samples.is_empty() {
            return 0.0;
        }

        let failures = self.samples.iter().filter(|s| !s.success).count();
        failures as f32 / self.samples.len() as f32
    }

    pub fn latency_p95(&self) -> Option<u64> {
        if self.samples.is_empty() {
            return None;
        }

        let mut latencies: Vec<u64> = self.samples.iter().map(|s| s.latency_ms).collect();
        latencies.sort_unstable();

        let idx = ((latencies.len() as f32) * 0.95).ceil() as usize - 1;
        latencies.get(idx).cloned()
    }

    pub fn is_degrading(&self) -> bool {
        let failure_rate = self.failure_rate();
        let latency_p95 = self.latency_p95().unwrap_or(0);

        failure_rate >= self.failure_threshold || latency_p95 > self.latency_budget_ms
    }
}

/// Structured diagnostic summary for external reporting.
#[derive(Debug, Clone)]
pub struct DiagnosticReport {
    pub failure_rate: f32,
    pub latency_p95: Option<u64>,
    pub degrading: bool,
}

impl DiagnosticReport {
    pub fn from_monitor(monitor: &PredictiveMonitor) -> Self {
        Self {
            failure_rate: monitor.failure_rate(),
            latency_p95: monitor.latency_p95(),
            degrading: monitor.is_degrading(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_degradation() {
        let mut monitor = PredictiveMonitor::new(10, 0.2, 100);
        for _ in 0..5 {
            monitor.record(HealthSample::new(50, true));
        }
        for _ in 0..3 {
            monitor.record(HealthSample::new(120, false));
        }

        assert!(monitor.is_degrading());
    }
}
