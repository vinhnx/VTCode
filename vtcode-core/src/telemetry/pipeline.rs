use std::collections::HashMap;
use std::time::SystemTime;

use anyhow::Result;
use tokio::sync::Mutex;

use crate::config::TelemetryConfig;

/// Single telemetry observation.
#[derive(Debug, Clone)]
pub struct TelemetryEvent {
    pub name: String,
    pub value: f64,
    pub timestamp: SystemTime,
    pub tags: HashMap<String, String>,
}

impl TelemetryEvent {
    pub fn new(name: impl Into<String>, value: f64) -> Self {
        Self {
            name: name.into(),
            value,
            timestamp: SystemTime::now(),
            tags: HashMap::new(),
        }
    }
}

/// In-memory telemetry pipeline suitable for dashboards.
#[derive(Debug)]
pub struct TelemetryPipeline {
    config: TelemetryConfig,
    events: Mutex<Vec<TelemetryEvent>>,
}

impl TelemetryPipeline {
    pub fn new(config: TelemetryConfig) -> Self {
        Self {
            config,
            events: Mutex::new(Vec::new()),
        }
    }

    pub async fn record(&self, event: TelemetryEvent) -> Result<()> {
        if !self.config.dashboards_enabled {
            return Ok(());
        }

        let mut events = self.events.lock().await;
        events.push(event);
        Ok(())
    }

    pub async fn snapshot(&self) -> Vec<TelemetryEvent> {
        let events = self.events.lock().await;
        events.clone()
    }
}
