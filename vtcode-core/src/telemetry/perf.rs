use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use once_cell::sync::OnceCell;
use tokio::runtime::Handle;
use tracing::debug;

use crate::config::TelemetryConfig;
use crate::telemetry::{TelemetryEvent, TelemetryPipeline};

#[derive(Debug)]
pub struct PerfRecorder {
    enabled: AtomicBool,
    pipeline: Arc<TelemetryPipeline>,
}

static PERF_RECORDER: OnceCell<PerfRecorder> = OnceCell::new();

pub fn initialize_perf_telemetry(config: &TelemetryConfig) {
    let recorder = PerfRecorder {
        enabled: AtomicBool::new(config.perf_events),
        pipeline: Arc::new(TelemetryPipeline::new(config.clone())),
    };
    let _ = PERF_RECORDER.set(recorder);
}

pub fn enabled() -> bool {
    PERF_RECORDER
        .get()
        .map(|recorder| recorder.enabled.load(Ordering::Relaxed))
        .unwrap_or(false)
}

pub fn record_duration(name: &'static str, duration: Duration, tags: HashMap<String, String>) {
    record_value(name, duration.as_secs_f64() * 1000.0, tags);
}

pub fn record_value(name: &'static str, value: f64, tags: HashMap<String, String>) {
    let Some(recorder) = PERF_RECORDER.get() else {
        return;
    };
    if !recorder.enabled.load(Ordering::Relaxed) {
        return;
    }

    let mut event = TelemetryEvent::new(name, value);
    event.tags = tags;

    if let Ok(handle) = Handle::try_current() {
        let pipeline = Arc::clone(&recorder.pipeline);
        handle.spawn(async move {
            let _ = pipeline.record(event).await;
        });
    } else {
        debug!(name, "Skipping perf event (no runtime)");
    }
}

pub struct PerfSpan {
    name: &'static str,
    start: Instant,
    tags: HashMap<String, String>,
    enabled: bool,
}

impl PerfSpan {
    pub fn new(name: &'static str) -> Self {
        Self {
            name,
            start: Instant::now(),
            tags: HashMap::new(),
            enabled: enabled(),
        }
    }

    pub fn tag(&mut self, key: impl Into<String>, value: impl Into<String>) {
        if self.enabled {
            self.tags.insert(key.into(), value.into());
        }
    }
}

impl Drop for PerfSpan {
    fn drop(&mut self) {
        if self.enabled {
            record_duration(
                self.name,
                self.start.elapsed(),
                std::mem::take(&mut self.tags),
            );
        }
    }
}
