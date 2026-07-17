/// Shared byte budget used when loading project documentation and instruction files.
pub const DEFAULT_MAX_BYTES: usize = 16 * 1024;

/// Soft token budget for the fully composed system prompt (character-based
/// estimate, ~4 chars/token). When the prompt exceeds this, VT Code warns and,
/// if `agent.trim_system_prompt` is enabled, trims low-priority sections.
pub const DEFAULT_MAX_SYSTEM_PROMPT_TOKENS: u64 = 8_000;
