use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

/// RAII guard to ensure a background progress task is aborted when dropped
pub(crate) struct ProgressUpdateGuard {
    handle: Option<tokio::task::JoinHandle<()>>,
}

impl ProgressUpdateGuard {
    pub(crate) fn new(handle: tokio::task::JoinHandle<()>) -> Self {
        Self {
            handle: Some(handle),
        }
    }
}

impl Drop for ProgressUpdateGuard {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() {
            handle.abort();
        }
    }
}

/// Tracks the state of a long-running operation with detailed progress information
pub(crate) struct ProgressState {
    current: AtomicU64,
    total: AtomicU64,
    metadata: Mutex<ProgressMetadata>,
    is_complete: AtomicBool,
    start_time: Instant,
}

struct ProgressMetadata {
    message: String,
    estimated_remaining: Option<Duration>,
}

impl ProgressState {
    /// Create a new ProgressState
    pub fn new() -> Self {
        Self {
            current: AtomicU64::new(0),
            total: AtomicU64::new(0),
            metadata: Mutex::new(ProgressMetadata {
                message: String::new(),
                estimated_remaining: None,
            }),
            is_complete: AtomicBool::new(false),
            start_time: Instant::now(),
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

    /// Update the progress message
    pub async fn set_message(&self, message: String) {
        let mut metadata = self.metadata.lock().await;
        metadata.message = message;
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

            let mut metadata = self.metadata.lock().await;
            metadata.estimated_remaining = Some(Duration::from_secs_f64(remaining));
        }
    }

    /// Mark the operation as complete
    pub async fn complete(&self) {
        self.is_complete.store(true, Ordering::SeqCst);
        self.current
            .store(self.total.load(Ordering::SeqCst), Ordering::SeqCst);
    }

    /// Get the current progress state
    pub async fn get_progress(&self) -> (u64, u64, String, Option<Duration>) {
        let current = self.current.load(Ordering::SeqCst);
        let total = self.total.load(Ordering::SeqCst);
        let metadata = self.metadata.lock().await;
        let message = metadata.message.clone();
        let remaining = metadata.estimated_remaining;

        (current, total, message, remaining)
    }

    /// Check if the operation is complete
    pub fn is_complete(&self) -> bool {
        self.is_complete.load(Ordering::SeqCst)
    }
}

/// Provides a thread-safe way to report progress with enhanced features
#[derive(Clone)]
pub(crate) struct ProgressReporter {
    state: Arc<ProgressState>,
    last_progress: Arc<AtomicU64>,
}

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
        Arc::clone(&self.state)
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

    /// Update the progress message
    pub async fn set_message(&self, message: impl Into<String>) {
        self.state.set_message(message.into()).await;
    }

    /// Mark the operation as complete
    pub async fn complete(&self) {
        self.state.complete().await;
    }

    /// Get the current progress as a percentage (0-100)
    pub async fn percentage(&self) -> u8 {
        let (current, total, _, _) = self.state.get_progress().await;
        if total > 0 {
            ((current as f64 / total as f64) * 100.0).round() as u8
        } else {
            0
        }
    }

    /// Get the current progress information
    pub async fn progress_info(&self) -> ProgressInfo {
        let (current, total, message, eta) = self.state.get_progress().await;
        #[cfg(not(test))]
        let _ = current;
        let percentage = self.percentage().await;

        ProgressInfo {
            #[cfg(test)]
            current,
            total,
            percentage,
            message,
            eta,
            #[cfg(test)]
            is_complete: self.state.is_complete(),
        }
    }
}

/// Information about the current progress of an operation
#[derive(Debug, Clone)]
pub(crate) struct ProgressInfo {
    #[cfg(test)]
    pub current: u64,
    pub total: u64,
    pub percentage: u8,
    pub message: String,
    pub eta: Option<Duration>,
    #[cfg(test)]
    pub is_complete: bool,
}

impl ProgressInfo {
    /// Format the ETA as a human-readable string
    pub fn eta_formatted(&self) -> String {
        self.eta
            .map(format_eta)
            .unwrap_or_else(|| "Calculating...".to_string())
    }
}

/// Format an ETA duration as a human-readable string
fn format_eta(duration: Duration) -> String {
    let secs = duration.as_secs();
    if secs < 60 {
        format!("{}s", secs)
    } else if secs < 3600 {
        format!("{}m", (secs as f64 / 60.0).ceil() as u64)
    } else {
        let hours = secs / 3600;
        let minutes = (secs % 3600).div_ceil(60); // Round up to nearest minute
        if minutes >= 60 {
            format!("{}h", hours + 1)
        } else {
            format!("{}h {}m", hours, minutes)
        }
    }
}

/// Spawns a background task that updates the progress message with elapsed time
pub(crate) fn spawn_elapsed_time_updater(
    reporter: ProgressReporter,
    task_name: String,
    interval_ms: u64,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_millis(interval_ms));
        let start = Instant::now();
        let state = reporter.get_state();
        loop {
            interval.tick().await;
            if state.is_complete() {
                break;
            }
            let elapsed = start.elapsed();
            let duration_str = if elapsed.as_secs() < 60 {
                format!("{:.1}s", elapsed.as_secs_f64())
            } else {
                super::palettes::format_duration_label(elapsed)
            };

            reporter
                .set_message(format!(
                    "Running {}... ({} elapsed)",
                    task_name, duration_str
                ))
                .await;
        }
    })
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
        assert_eq!(progress.percentage().await, 0);
        let info = progress.progress_info().await;
        assert_eq!(info.current, 0);
        assert!(!info.is_complete);
    }

    #[tokio::test]
    async fn test_progress_updates() {
        let progress = ProgressReporter::new();
        progress.set_total(100).await;
        progress.set_progress(50).await;
        let info = progress.progress_info().await;
        assert_eq!(info.current, 50);
        assert_eq!(info.percentage, 50);
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
}
