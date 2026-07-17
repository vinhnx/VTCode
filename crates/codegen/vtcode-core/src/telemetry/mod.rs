//! Telemetry pipeline for real-time KPIs and historical benchmarking.

pub mod perf;
mod pipeline;

pub use pipeline::{TelemetryEvent, TelemetryPipeline};

/// Memory pool performance telemetry
#[derive(Debug, Clone)]
pub struct MemoryPoolTelemetry {
    pub string_hit_rate: f64,
    pub value_hit_rate: f64,
    pub vec_hit_rate: f64,
    pub total_allocations_avoided: usize,
}
