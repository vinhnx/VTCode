use std::path::PathBuf;
use std::time::Duration;

use portable_pty::PtySize;

pub struct PtyCommandRequest {
    pub command: Vec<String>,
    pub working_dir: PathBuf,
    pub timeout: Duration,
    pub size: PtySize,
    pub max_tokens: Option<usize>,
}

pub struct PtyCommandResult {
    pub exit_code: i32,
    pub output: String,
    pub duration: Duration,
    pub size: PtySize,
    pub applied_max_tokens: Option<usize>,
}
