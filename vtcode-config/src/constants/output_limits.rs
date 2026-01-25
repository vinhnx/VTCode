/// Maximum size for single agent message payloads (bytes).
pub const MAX_AGENT_MESSAGES_SIZE: usize = 10 * 1024 * 1024;
/// Maximum size for entire message history payloads (bytes).
pub const MAX_ALL_MESSAGES_SIZE: usize = 50 * 1024 * 1024;
/// Maximum size per line (bytes).
pub const MAX_LINE_LENGTH: usize = 1024 * 1024;
/// Default message count limit.
pub const DEFAULT_MESSAGE_LIMIT: usize = 10_000;
/// Maximum message count limit.
pub const MAX_MESSAGE_LIMIT: usize = 50_000;
