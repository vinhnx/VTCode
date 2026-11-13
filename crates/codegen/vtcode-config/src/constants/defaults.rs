use super::{models, ui};

pub const DEFAULT_MODEL: &str = models::openrouter::DEFAULT_MODEL;
pub const DEFAULT_CLI_MODEL: &str = models::openrouter::DEFAULT_MODEL;
pub const DEFAULT_PROVIDER: &str = "openrouter";
pub const DEFAULT_API_KEY_ENV: &str = "OPENROUTER_API_KEY";
pub const DEFAULT_THEME: &str = "ciapre";
pub(crate) use super::tool_limits::DEFAULT_FULL_AUTO_MAX_TURNS;
pub use super::tool_limits::DEFAULT_MAX_TOOL_LOOPS;
pub const DEFAULT_MAX_REPEATED_TOOL_CALLS: usize = 2;
pub const DEFAULT_MAX_SEQUENTIAL_SPOOL_CHUNK_READS_PER_TURN: usize = 6;
pub const DEFAULT_MAX_CONSECUTIVE_BLOCKED_TOOL_CALLS_PER_TURN: usize = 8;
pub const DEFAULT_PTY_STDOUT_TAIL_LINES: usize = 20;
pub const DEFAULT_PTY_SCROLLBACK_LINES: usize = 400;
pub const DEFAULT_TOOL_OUTPUT_MODE: &str = ui::TOOL_OUTPUT_MODE_COMPACT;
pub use super::tool_limits::DEFAULT_MAX_CONVERSATION_TURNS;
pub const DEFAULT_PRIMARY_AGENT_NAME: &str = "build";

pub const DEFAULT_PTY_OUTPUT_MAX_TOKENS: usize = 8_000;

/// Byte fuse for PTY output - secondary safeguard after token truncation.
/// Protects against edge cases where token estimation underestimates size.
pub const DEFAULT_PTY_OUTPUT_BYTE_FUSE: usize = 40 * 1024; // 40 KiB

pub(crate) const DEFAULT_MAX_TOOL_WALL_CLOCK_SECS: u64 = 600;
pub const DEFAULT_MAX_TOOL_RETRIES: u32 = 2;

/// Minimum interval between session progress snapshots (milliseconds)
pub const DEFAULT_SESSION_PROGRESS_MIN_INTERVAL_MS: u64 = 1000;

/// Minimum turns between session progress snapshots
pub const DEFAULT_SESSION_PROGRESS_MIN_TURN_DELTA: usize = 1;

/// Async trajectory log channel capacity (lines)
pub const DEFAULT_TRAJECTORY_LOG_CHANNEL_CAPACITY: usize = 1024;

/// Maximum payload bytes retained by the async trajectory log actor.
pub const DEFAULT_TRAJECTORY_LOG_BUFFER_BYTES: usize = 4 * 1024 * 1024;

/// Interval between best-effort trajectory log flushes.
pub const DEFAULT_TRAJECTORY_LOG_FLUSH_INTERVAL_MS: u64 = 250;

/// Maximum number of rotated trajectory log files to keep
pub const DEFAULT_TRAJECTORY_MAX_FILES: usize = 50;

/// Maximum age in days for rotated trajectory log files
pub const DEFAULT_TRAJECTORY_MAX_AGE_DAYS: u64 = 14;

/// Maximum total size in MB for all trajectory log files in a workspace
pub const DEFAULT_TRAJECTORY_MAX_SIZE_MB: u64 = 50;
