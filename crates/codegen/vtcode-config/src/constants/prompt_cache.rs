pub const DEFAULT_ENABLED: bool = true;
pub const DEFAULT_CACHE_DIR: &str = ".vtcode/cache/prompts";
pub const DEFAULT_MAX_ENTRIES: usize = 1_000;
pub const DEFAULT_MAX_AGE_DAYS: u64 = 30;
pub const DEFAULT_AUTO_CLEANUP: bool = true;
pub const DEFAULT_MIN_QUALITY_THRESHOLD: f64 = 0.7;
pub(crate) const DEFAULT_CACHE_FRIENDLY_PROMPT_SHAPING: bool = true;

pub(crate) const OPENAI_MIN_PREFIX_TOKENS: u32 = 1_024;
pub(crate) const GEMINI_MIN_PREFIX_TOKENS: u32 = 1_024;
pub(crate) const OPENAI_IDLE_EXPIRATION_SECONDS: u64 = 60 * 60; // 1 hour max reuse window

pub const ANTHROPIC_DEFAULT_TTL_SECONDS: u64 = 5 * 60; // 5 minutes
pub(crate) const ANTHROPIC_EXTENDED_TTL_SECONDS: u64 = 60 * 60; // 1 hour option
pub(crate) const ANTHROPIC_TOOLS_TTL_SECONDS: u64 = 60 * 60; // 1 hour for tools/system
pub(crate) const ANTHROPIC_MESSAGES_TTL_SECONDS: u64 = 5 * 60; // 5 minutes for messages
pub(crate) const ANTHROPIC_MAX_BREAKPOINTS: u8 = 4;
pub(crate) const ANTHROPIC_MIN_MESSAGE_LENGTH_FOR_CACHE: usize = 256;

pub(crate) const GEMINI_EXPLICIT_DEFAULT_TTL_SECONDS: u64 = 60 * 60; // 1 hour default for explicit caches

/// Approximate provider-side prompt cache lifetimes used for the cache-gap
/// warning (advisory only). A pause longer than these likely means the next
/// request re-pays full cache-creation cost.
pub(crate) const ANTHROPIC_CACHE_GAP_WARNING_SECONDS: u64 = 5 * 60; // ephemeral 5m TTL
pub(crate) const OPENAI_CACHE_GAP_WARNING_SECONDS: u64 = 10 * 60; // in-memory cache window
pub(crate) const DEFAULT_CACHE_GAP_WARNING_SECONDS: u64 = 5 * 60;

pub const OPENROUTER_CACHE_DISCOUNT_ENABLED: bool = true;
pub const XAI_CACHE_ENABLED: bool = true;
pub const DEEPSEEK_CACHE_ENABLED: bool = true;
pub(crate) const ZAI_CACHE_ENABLED: bool = false;
pub(crate) const MOONSHOT_CACHE_ENABLED: bool = true;
