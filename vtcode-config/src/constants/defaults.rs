use super::{models, ui};

pub const DEFAULT_MODEL: &str = models::anthropic::CLAUDE_SONNET_4_5;
pub const DEFAULT_CLI_MODEL: &str = models::anthropic::CLAUDE_SONNET_4_5;
pub const DEFAULT_PROVIDER: &str = "anthropic";
pub const DEFAULT_API_KEY_ENV: &str = "ANTHROPIC_API_KEY";
pub const DEFAULT_THEME: &str = "ciapre-dark";
pub const DEFAULT_FULL_AUTO_MAX_TURNS: usize = 30;
pub const DEFAULT_MAX_TOOL_LOOPS: usize = 200;
pub const DEFAULT_MAX_REPEATED_TOOL_CALLS: usize = 3;
pub const DEFAULT_PTY_STDOUT_TAIL_LINES: usize = 20;
pub const DEFAULT_PTY_SCROLLBACK_LINES: usize = 400;
pub const DEFAULT_TOOL_OUTPUT_MODE: &str = ui::TOOL_OUTPUT_MODE_COMPACT;

pub const DEFAULT_PTY_OUTPUT_MAX_TOKENS: usize = 8_000;

/// Byte fuse for PTY output - secondary safeguard after token truncation.
/// Protects against edge cases where token estimation underestimates size.
pub const DEFAULT_PTY_OUTPUT_BYTE_FUSE: usize = 40 * 1024; // 40 KiB

pub const DEFAULT_MAX_TOOL_CALLS_PER_TURN: usize = 32;
pub const DEFAULT_MAX_TOOL_WALL_CLOCK_SECS: u64 = 600;
pub const DEFAULT_MAX_TOOL_RETRIES: u32 = 2;
