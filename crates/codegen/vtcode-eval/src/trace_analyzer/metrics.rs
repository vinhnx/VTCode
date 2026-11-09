use std::collections::BTreeMap;

use serde_json::Value;

use super::model::{LatencyStatistics, TokenUsage};

pub(super) const MAX_LATENCY_SAMPLES: usize = 4096;

#[derive(Default)]
pub(super) struct UsageAccounting {
    per_turn: TokenUsage,
    other: TokenUsage,
    thread_aggregate: Option<TokenUsage>,
    saw_per_turn_usage: bool,
}

impl UsageAccounting {
    pub(super) fn record(&mut self, event_type: Option<&str>, object: &serde_json::Map<String, Value>) {
        let Some(sample) = super::usage_sample(object) else {
            return;
        };

        match event_type {
            Some("thread.completed") => self.thread_aggregate = Some(sample),
            Some("turn.completed" | "turn.failed" | "turn/end") => {
                self.saw_per_turn_usage = true;
                super::add_usage(&mut self.per_turn, &sample);
            }
            _ => super::add_usage(&mut self.other, &sample),
        }
    }

    pub(super) fn finish(self) -> TokenUsage {
        if self.saw_per_turn_usage {
            self.per_turn
        } else if let Some(thread_aggregate) = self.thread_aggregate {
            thread_aggregate
        } else {
            self.other
        }
    }
}

#[derive(Default)]
pub(super) struct LatencyAccumulator {
    samples: Vec<u64>,
    count: u64,
    total_ms: u64,
    max_ms: Option<u64>,
}

impl LatencyAccumulator {
    pub(super) fn record(&mut self, latency_ms: u64) {
        self.count = self.count.saturating_add(1);
        self.total_ms = self.total_ms.saturating_add(latency_ms);
        self.max_ms = Some(self.max_ms.map_or(latency_ms, |max_ms| max_ms.max(latency_ms)));

        if self.samples.len() < MAX_LATENCY_SAMPLES {
            self.samples.push(latency_ms);
            return;
        }

        let candidate = super::deterministic_reservoir_index(self.count);
        if let Some(slot) = self.samples.get_mut(candidate as usize) {
            *slot = latency_ms;
        }
    }

    pub(super) fn finish(mut self) -> LatencyStatistics {
        if self.count == 0 {
            return LatencyStatistics::default();
        }

        self.samples.sort_unstable();
        let percentile = |percent: usize| {
            let index = (self.samples.len() - 1).saturating_mul(percent).div_ceil(100);
            #[expect(
                clippy::indexing_slicing,
                reason = "count > 0 guarantees a nonempty sample (the first record() always pushes) and percent <= 100 keeps index <= len - 1"
            )]
            let value = self.samples[index];
            value
        };
        LatencyStatistics {
            count: self.count,
            total_ms: self.total_ms,
            mean_ms: Some(self.total_ms as f64 / self.count as f64),
            p50_ms: Some(percentile(50)),
            p95_ms: Some(percentile(95)),
            max_ms: self.max_ms,
        }
    }
}

#[derive(Default)]
pub(super) struct LifecycleTiming {
    pending_steps: BTreeMap<u64, u64>,
}

impl LifecycleTiming {
    pub(super) fn record(
        &mut self,
        event_type: Option<&str>,
        outer: &serde_json::Map<String, Value>,
        payload: &serde_json::Map<String, Value>,
        latencies: &mut LatencyAccumulator,
    ) {
        let timestamp = super::number_field_from(outer, payload, &["time"]);
        let step = super::number_field_from(payload, outer, &["step"]);
        match event_type {
            Some("step/start") => {
                if let (Some(step), Some(timestamp)) = (step, timestamp)
                    && self.pending_steps.len() < MAX_LATENCY_SAMPLES
                {
                    self.pending_steps.insert(step, timestamp);
                }
            }
            Some("step/end") => {
                if let Some(step) = step
                    && let Some(timestamp) = timestamp
                    && let Some(start) = self.pending_steps.remove(&step)
                    && timestamp >= start
                {
                    latencies.record(timestamp - start);
                }
            }
            _ => {}
        }
    }
}
