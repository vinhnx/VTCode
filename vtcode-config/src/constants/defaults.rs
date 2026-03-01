use super::{models, ui};

pub const DEFAULT_MODEL: &str = models::openai::GPT_5_3_CODEX;
pub const DEFAULT_CLI_MODEL: &str = models::openai::GPT_5_3_CODEX;
pub const DEFAULT_PROVIDER: &str = "openai";
pub const DEFAULT_API_KEY_ENV: &str = "OPENAI_API_KEY";
pub const DEFAULT_THEME: &str = "ciapre";
pub const DEFAULT_FULL_AUTO_MAX_TURNS: usize = 30;
pub const DEFAULT_MAX_TOOL_LOOPS: usize = 20;
pub const DEFAULT_MAX_REPEATED_TOOL_CALLS: usize = 2;
pub const DEFAULT_MAX_SEQUENTIAL_SPOOL_CHUNK_READS_PER_TURN: usize = 6;
pub const DEFAULT_MAX_CONSECUTIVE_BLOCKED_TOOL_CALLS_PER_TURN: usize = 3;
pub const DEFAULT_PTY_STDOUT_TAIL_LINES: usize = 20;
pub const DEFAULT_PTY_SCROLLBACK_LINES: usize = 400;
pub const DEFAULT_TOOL_OUTPUT_MODE: &str = ui::TOOL_OUTPUT_MODE_COMPACT;
pub const DEFAULT_MAX_CONVERSATION_TURNS: usize = 150;

pub const DEFAULT_PTY_OUTPUT_MAX_TOKENS: usize = 8_000;

/// Byte fuse for PTY output - secondary safeguard after token truncation.
/// Protects against edge cases where token estimation underestimates size.
pub const DEFAULT_PTY_OUTPUT_BYTE_FUSE: usize = 40 * 1024; // 40 KiB

pub const DEFAULT_MAX_TOOL_CALLS_PER_TURN: usize = 32;
pub const DEFAULT_MAX_TOOL_WALL_CLOCK_SECS: u64 = 600;
pub const DEFAULT_MAX_TOOL_RETRIES: u32 = 2;

/// Default macOS Gatekeeper auto-clear paths (relative or home-expanded)
pub const DEFAULT_GATEKEEPER_AUTO_CLEAR_PATHS: &[&str] = &[".vtcode/bin", "~/.vtcode/bin"];

/// Minimum interval between session progress snapshots (milliseconds)
pub const DEFAULT_SESSION_PROGRESS_MIN_INTERVAL_MS: u64 = 1000;

/// Minimum turns between session progress snapshots
pub const DEFAULT_SESSION_PROGRESS_MIN_TURN_DELTA: usize = 1;

/// Async trajectory log channel capacity (lines)
pub const DEFAULT_TRAJECTORY_LOG_CHANNEL_CAPACITY: usize = 1024;
