pub const DEFAULT_ENABLED: bool = true;
pub const DEFAULT_CACHE_DIR: &str = ".vtcode/cache/prompts";
pub const DEFAULT_MAX_ENTRIES: usize = 1_000;
pub const DEFAULT_MAX_AGE_DAYS: u64 = 30;
pub const DEFAULT_AUTO_CLEANUP: bool = true;
pub const DEFAULT_MIN_QUALITY_THRESHOLD: f64 = 0.7;

pub const OPENAI_MIN_PREFIX_TOKENS: u32 = 1_024;
pub const GEMINI_MIN_PREFIX_TOKENS: u32 = 1_024;
pub const OPENAI_IDLE_EXPIRATION_SECONDS: u64 = 60 * 60; // 1 hour max reuse window

pub const ANTHROPIC_DEFAULT_TTL_SECONDS: u64 = 5 * 60; // 5 minutes
pub const ANTHROPIC_EXTENDED_TTL_SECONDS: u64 = 60 * 60; // 1 hour option
pub const ANTHROPIC_TOOLS_TTL_SECONDS: u64 = 60 * 60; // 1 hour for tools/system
pub const ANTHROPIC_MESSAGES_TTL_SECONDS: u64 = 5 * 60; // 5 minutes for messages
pub const ANTHROPIC_MAX_BREAKPOINTS: u8 = 4;
pub const ANTHROPIC_MIN_MESSAGE_LENGTH_FOR_CACHE: usize = 256;

pub const GEMINI_EXPLICIT_DEFAULT_TTL_SECONDS: u64 = 60 * 60; // 1 hour default for explicit caches

pub const OPENROUTER_CACHE_DISCOUNT_ENABLED: bool = true;
pub const XAI_CACHE_ENABLED: bool = true;
pub const DEEPSEEK_CACHE_ENABLED: bool = true;
pub const ZAI_CACHE_ENABLED: bool = false;
pub const MOONSHOT_CACHE_ENABLED: bool = true;
