use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use portable_pty::PtySize;

/// Callback type for receiving streaming PTY output chunks
pub type PtyOutputCallback = Arc<dyn Fn(&str) + Send + Sync>;

pub struct PtyCommandRequest {
    pub command: Vec<String>,
    pub working_dir: PathBuf,
    pub timeout: Duration,
    pub size: PtySize,
    pub max_tokens: Option<usize>,
    /// Optional callback for receiving real-time output chunks.
    /// When set, output will be streamed line-by-line as it arrives.
    pub output_callback: Option<PtyOutputCallback>,
}

impl PtyCommandRequest {
    /// Create a new request with streaming callback
    pub fn with_streaming(
        command: Vec<String>,
        working_dir: PathBuf,
        timeout: Duration,
        callback: PtyOutputCallback,
    ) -> Self {
        Self {
            command,
            working_dir,
            timeout,
            size: PtySize::default(),
            max_tokens: None,
            output_callback: Some(callback),
        }
    }
}

pub struct PtyCommandResult {
    pub exit_code: i32,
    pub output: String,
    pub duration: Duration,
    pub size: PtySize,
    pub applied_max_tokens: Option<usize>,
}
