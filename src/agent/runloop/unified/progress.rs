use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering},
};
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

/// Tracks the state of a long-running operation with detailed progress information
#[allow(dead_code)]
pub struct ProgressState {
    current: AtomicU64,
    total: AtomicU64,
    message: Mutex<String>,
    is_complete: AtomicBool,
    start_time: Instant,
    last_update: Mutex<Instant>,
    estimated_remaining: Mutex<Option<Duration>>,
    stage: AtomicU8,
    stage_name: Mutex<String>,
}

#[allow(dead_code)]
impl ProgressState {
    /// Create a new ProgressState
    pub fn new() -> Self {
        Self {
            current: AtomicU64::new(0),
            total: AtomicU64::new(0),
            message: Mutex::new(String::new()),
            is_complete: AtomicBool::new(false),
            start_time: Instant::now(),
            last_update: Mutex::new(Instant::now()),
            estimated_remaining: Mutex::new(None),
            stage: AtomicU8::new(0),
            stage_name: Mutex::new(String::new()),
        }
    }

    /// Set the total number of work units
    pub async fn set_total(&self, total: u64) {
        self.total.store(total, Ordering::SeqCst);
        self.update_eta().await;
    }

    /// Set the current progress
    pub async fn set_progress(&self, current: u64) {
        self.current.store(current, Ordering::SeqCst);
        self.update_eta().await;
    }

    /// Increment the current progress by delta
    pub async fn increment(&self, delta: u64) {
        self.current.fetch_add(delta, Ordering::SeqCst);
        self.update_eta().await;
    }

    /// Update the progress message
    pub async fn set_message(&self, message: String) {
        let mut msg = self.message.lock().await;
        *msg = message;
    }

    /// Set the current stage and its name
    pub async fn set_stage(&self, stage: u8, name: &str) {
        self.stage.store(stage, Ordering::SeqCst);
        let mut stage_name = self.stage_name.lock().await;
        *stage_name = name.to_string();
    }

    /// Calculate and update the estimated time remaining
    async fn update_eta(&self) {
        let current = self.current.load(Ordering::SeqCst) as f64;
        let total = self.total.load(Ordering::SeqCst) as f64;

        if current > 0.0 && total > 0.0 {
            let elapsed = self.start_time.elapsed();
            let progress_ratio = current / total;
            let estimated_total = elapsed.as_secs_f64() / progress_ratio;
            let remaining = (estimated_total - elapsed.as_secs_f64()).max(0.0);

            let mut est = self.estimated_remaining.lock().await;
            *est = Some(Duration::from_secs_f64(remaining));
        }
    }

    /// Mark the operation as complete
    pub async fn complete(&self) {
        self.is_complete.store(true, Ordering::SeqCst);
        self.current
            .store(self.total.load(Ordering::SeqCst), Ordering::SeqCst);
        self.stage.store(100, Ordering::SeqCst);
        let mut stage_name = self.stage_name.lock().await;
        *stage_name = "Completed".to_string();
    }

    /// Get the current progress state
    pub async fn get_progress(&self) -> (u64, u64, String, Option<Duration>, u8, String) {
        let current = self.current.load(Ordering::SeqCst);
        let total = self.total.load(Ordering::SeqCst);
        let message = self.message.lock().await.clone();
        let remaining = *self.estimated_remaining.lock().await;
        let stage = self.stage.load(Ordering::SeqCst);
        let stage_name = self.stage_name.lock().await.clone();

        (current, total, message, remaining, stage, stage_name)
    }

    /// Check if the operation is complete
    pub fn is_complete(&self) -> bool {
        self.is_complete.load(Ordering::SeqCst)
    }

    /// Check if enough time has passed since last update to warrant a UI refresh
    pub async fn should_update(&self) -> bool {
        let last_update = self.last_update.lock().await;
        last_update.elapsed() >= Duration::from_millis(100)
    }
}

/// Provides a thread-safe way to report progress with enhanced features
#[derive(Clone)]
#[allow(dead_code)]
pub struct ProgressReporter {
    state: Arc<ProgressState>,
    last_progress: Arc<AtomicU64>,
}

#[allow(dead_code)]
impl ProgressReporter {
    /// Create a new ProgressReporter
    pub fn new() -> Self {
        Self {
            state: Arc::new(ProgressState::new()),
            last_progress: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Get a reference to the underlying ProgressState
    pub fn get_state(&self) -> Arc<ProgressState> {
        self.state.clone()
    }

    /// Set the total number of work units
    pub async fn set_total(&self, total: u64) {
        self.state.set_total(total).await;
    }

    /// Set the current progress
    pub async fn set_progress(&self, current: u64) {
        self.state.set_progress(current).await;
        self.last_progress.store(current, Ordering::SeqCst);
    }

    /// Get the current progress
    pub fn current_progress(&self) -> u64 {
        self.last_progress.load(Ordering::SeqCst)
    }

    /// Increment the current progress by delta and return the new current value
    pub async fn increment(&self, delta: u64) -> u64 {
        let new_current = self.state.current.fetch_add(delta, Ordering::SeqCst) + delta;
        self.last_progress.store(new_current, Ordering::SeqCst);
        self.state.update_eta().await;
        new_current
    }

    /// Update the progress message
    pub async fn set_message(&self, message: impl Into<String>) {
        self.state.set_message(message.into()).await;
    }

    /// Set the current stage and its name
    pub async fn set_stage(&self, stage: u8, name: &str) {
        self.state.set_stage(stage, name).await;
    }

    /// Get the current stage information
    pub async fn stage_info(&self) -> (u8, String) {
        let (_, _, _, _, stage, stage_name) = self.state.get_progress().await;
        (stage, stage_name)
    }

    /// Get the estimated time remaining
    pub async fn eta(&self) -> Option<Duration> {
        let (_, _, _, remaining, _, _) = self.state.get_progress().await;
        remaining
    }

    /// Format the estimated time remaining as a string
    pub async fn eta_formatted(&self) -> String {
        match self.eta().await {
            Some(duration) => format_eta(duration),
            None => "Calculating...".to_string(),
        }
    }

    /// Mark the operation as complete
    pub async fn complete(&self) {
        self.state.complete().await;
    }

    /// Get the current progress as a percentage (0-100)
    pub async fn percentage(&self) -> u8 {
        let (current, total, ..) = self.state.get_progress().await;
        if total > 0 {
            ((current as f64 / total as f64) * 100.0).round() as u8
        } else {
            0
        }
    }

    /// Get the elapsed time since the operation started
    pub fn elapsed(&self) -> Duration {
        self.state.start_time.elapsed()
    }

    /// Get the current progress information
    pub async fn progress_info(&self) -> ProgressInfo {
        let (current, total, message, eta, stage, stage_name) = self.state.get_progress().await;
        let percentage = self.percentage().await;
        let elapsed = self.elapsed();

        ProgressInfo {
            current,
            total,
            percentage,
            message,
            eta,
            stage,
            stage_name,
            is_complete: self.state.is_complete(),
            elapsed,
        }
    }
}

/// Information about the current progress of an operation
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ProgressInfo {
    pub current: u64,
    pub total: u64,
    pub percentage: u8,
    pub message: String,
    pub eta: Option<Duration>,
    pub stage: u8,
    pub stage_name: String,
    pub is_complete: bool,
    pub elapsed: Duration,
}

#[allow(dead_code)]
impl ProgressInfo {
    /// Format the ETA as a human-readable string
    pub fn eta_formatted(&self) -> String {
        self.eta
            .map(format_eta)
            .unwrap_or_else(|| "Calculating...".to_string())
    }

    /// Format the elapsed time as a human-readable string
    pub fn elapsed_formatted(&self) -> String {
        format_duration(self.elapsed)
    }
}

/// Format a duration as a human-readable string
#[allow(dead_code)]
fn format_duration(duration: Duration) -> String {
    let secs = duration.as_secs();
    if secs < 60 {
        format!("{}s", secs)
    } else if secs < 3600 {
        format!("{}m {:02}s", secs / 60, secs % 60)
    } else {
        let hours = secs / 3600;
        let minutes = (secs % 3600) / 60;
        format!("{}h {:02}m", hours, minutes)
    }
}

/// Format an ETA duration as a human-readable string
#[allow(dead_code)]
fn format_eta(duration: Duration) -> String {
    let secs = duration.as_secs();
    if secs < 60 {
        format!("{}s", secs)
    } else if secs < 3600 {
        format!("{}m", (secs as f64 / 60.0).ceil() as u64)
    } else {
        let hours = secs / 3600;
        let minutes = (secs % 3600 + 59) / 60; // Round up to nearest minute
        if minutes >= 60 {
            format!("{}h", hours + 1)
        } else {
            format!("{}h {}m", hours, minutes)
        }
    }
}

impl Default for ProgressReporter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::Duration;

    #[tokio::test]
    async fn test_progress_basics() {
        let progress = ProgressReporter::new();
        assert_eq!(progress.current_progress(), 0);
        assert_eq!(progress.percentage().await, 0);
        let info = progress.progress_info().await;
        assert!(!info.is_complete);
    }

    #[tokio::test]
    async fn test_progress_updates() {
        let progress = ProgressReporter::new();
        progress.set_total(100).await;
        progress.set_progress(50).await;
        assert_eq!(progress.current_progress(), 50);
        assert_eq!(progress.percentage().await, 50);
    }

    #[tokio::test]
    async fn test_stage_management() {
        let progress = ProgressReporter::new();
        progress.set_stage(30, "Processing").await;
        let (stage, stage_name) = progress.stage_info().await;
        assert_eq!(stage, 30);
        assert_eq!(stage_name, "Processing");
    }

    #[tokio::test]
    async fn test_eta_calculation() {
        let progress = ProgressReporter::new();
        progress.set_total(100).await;
        progress.set_progress(50).await;

        // Simulate some time passing
        tokio::time::sleep(Duration::from_millis(100)).await;

        progress.set_progress(75).await;

        // ETA should be calculated based on the rate of progress
        let info = progress.progress_info().await;
        assert!(info.eta.is_some());

        // Should be able to format the ETA
        let eta_str = info.eta_formatted();
        assert!(!eta_str.is_empty());
    }

    #[tokio::test]
    async fn test_completion() {
        let progress = ProgressReporter::new();
        progress.set_total(100).await;
        progress.set_progress(100).await;
        progress.complete().await;

        let info = progress.progress_info().await;
        assert!(info.is_complete);
        assert_eq!(info.percentage, 100);
    }

    #[tokio::test]
    async fn test_concurrent_updates() {
        let progress = Arc::new(ProgressReporter::new());
        progress.set_total(100).await;

        let handles: Vec<_> = (0..10)
            .map(|_i| {
                let progress = progress.clone();
                tokio::spawn(async move {
                    for _ in 0..10 {
                        progress.increment(1).await;
                        tokio::time::sleep(Duration::from_millis(10)).await;
                    }
                })
            })
            .collect();

        for handle in handles {
            handle.await.unwrap();
        }

        assert_eq!(progress.current_progress(), 100);
    }
}
