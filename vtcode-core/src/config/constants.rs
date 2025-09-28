/// Prompt path constants to avoid hardcoding throughout the codebase
pub mod prompts {
    pub const DEFAULT_SYSTEM_PROMPT_PATH: &str = "prompts/system.md";
    pub const CODER_SYSTEM_PROMPT_PATH: &str = "prompts/coder_system.md";
}

/// Model ID constants to sync with docs/models.json
pub mod models {
    // Google/Gemini models
    pub mod google {
        pub const DEFAULT_MODEL: &str = "gemini-2.5-flash-preview-05-20";
        pub const SUPPORTED_MODELS: &[&str] = &[
            "gemini-2.5-flash-preview-05-20",
            "gemini-2.5-pro",
            "gemini-2.5-flash",
            "gemini-2.5-flash-lite",
        ];

        // Convenience constants for commonly used models
        pub const GEMINI_2_5_FLASH_PREVIEW: &str = "gemini-2.5-flash-preview-05-20";
        pub const GEMINI_2_5_PRO: &str = "gemini-2.5-pro";
        pub const GEMINI_2_5_FLASH: &str = "gemini-2.5-flash";
        pub const GEMINI_2_5_FLASH_LITE: &str = "gemini-2.5-flash-lite";
    }

    // OpenAI models (from docs/models.json)
    pub mod openai {
        pub const DEFAULT_MODEL: &str = "gpt-5";
        pub const SUPPORTED_MODELS: &[&str] = &[
            "gpt-5",
            "gpt-5-codex",
            "gpt-5-mini",
            "gpt-5-nano",
            "codex-mini-latest",
            "o4-mini",
            "o4-mini-deep-research",
            "o3",
            "o1",
            "o1-mini",
            "o1-preview",
            "o1-pro",
        ];

        /// Models that support the OpenAI reasoning API extensions
        pub const REASONING_MODELS: &[&str] = &[
            O4_MINI,
            O4_MINI_DEEP_RESEARCH,
            O3,
            O1,
            O1_PRO,
            O1_PREVIEW,
            O1_MINI,
        ];

        // Convenience constants for commonly used models
        pub const GPT_5: &str = "gpt-5";
        pub const GPT_5_CODEX: &str = "gpt-5-codex";
        pub const GPT_5_MINI: &str = "gpt-5-mini";
        pub const GPT_5_NANO: &str = "gpt-5-nano";
        pub const CODEX_MINI_LATEST: &str = "codex-mini-latest";
        pub const CODEX_MINI: &str = "codex-mini";
        pub const O4_MINI: &str = "o4-mini";
        pub const O4_MINI_DEEP_RESEARCH: &str = "o4-mini-deep-research";
        pub const O3: &str = "o3";
        pub const O1: &str = "o1";
        pub const O1_PRO: &str = "o1-pro";
        pub const O1_PREVIEW: &str = "o1-preview";
        pub const O1_MINI: &str = "o1-mini";
    }

    // OpenRouter models (extensible via vtcode.toml)
    pub mod openrouter {
        pub const DEFAULT_MODEL: &str = "x-ai/grok-code-fast-1";
        pub const SUPPORTED_MODELS: &[&str] = &[
            "x-ai/grok-code-fast-1",
            "x-ai/grok-4-fast:free",
            "qwen/qwen3-coder",
            "deepseek/deepseek-chat-v3.1",
            "openai/gpt-5",
            "openai/gpt-5-codex",
            "anthropic/claude-sonnet-4",
        ];

        /// Models that expose reasoning traces via OpenRouter APIs
        pub const REASONING_MODELS: &[&str] = &[
            X_AI_GROK_4_FAST_FREE,
            OPENAI_GPT_5,
            OPENAI_GPT_5_CODEX,
            ANTHROPIC_CLAUDE_SONNET_4,
        ];

        pub const X_AI_GROK_CODE_FAST_1: &str = "x-ai/grok-code-fast-1";
        pub const X_AI_GROK_4_FAST_FREE: &str = "x-ai/grok-4-fast:free";
        pub const QWEN3_CODER: &str = "qwen/qwen3-coder";
        pub const DEEPSEEK_DEEPSEEK_CHAT_V3_1: &str = "deepseek/deepseek-chat-v3.1";
        pub const OPENAI_GPT_5: &str = "openai/gpt-5";
        pub const OPENAI_GPT_5_CODEX: &str = "openai/gpt-5-codex";
        pub const ANTHROPIC_CLAUDE_SONNET_4: &str = "anthropic/claude-sonnet-4";
    }

    // Anthropic models (from docs/models.json) - Updated for tool use best practices
    pub mod anthropic {
        // Standard model for straightforward tools - Sonnet 4 preferred for most use cases
        pub const DEFAULT_MODEL: &str = "claude-sonnet-4-20250514";
        pub const SUPPORTED_MODELS: &[&str] = &[
            "claude-opus-4-1-20250805", // Latest: Opus 4.1 (2025-08-05)
            "claude-sonnet-4-20250514", // Latest: Sonnet 4 (2025-05-14)
        ];

        // Convenience constants for commonly used models
        pub const CLAUDE_OPUS_4_1_20250805: &str = "claude-opus-4-1-20250805";
        pub const CLAUDE_SONNET_4_20250514: &str = "claude-sonnet-4-20250514";
    }

    // xAI models
    pub mod xai {
        pub const DEFAULT_MODEL: &str = "grok-2-latest";
        pub const SUPPORTED_MODELS: &[&str] = &[
            "grok-2-latest",
            "grok-2",
            "grok-2-mini",
            "grok-2-reasoning",
            "grok-2-vision",
        ];

        pub const GROK_2_LATEST: &str = "grok-2-latest";
        pub const GROK_2: &str = "grok-2";
        pub const GROK_2_MINI: &str = "grok-2-mini";
        pub const GROK_2_REASONING: &str = "grok-2-reasoning";
        pub const GROK_2_VISION: &str = "grok-2-vision";
    }

    // Backwards compatibility - keep old constants working
    pub const GEMINI_2_5_FLASH_PREVIEW: &str = google::GEMINI_2_5_FLASH_PREVIEW;
    pub const GEMINI_2_5_FLASH: &str = google::GEMINI_2_5_FLASH;
    pub const GEMINI_2_5_PRO: &str = google::GEMINI_2_5_PRO;
    pub const GEMINI_2_5_FLASH_LITE: &str = google::GEMINI_2_5_FLASH_LITE;
    pub const GPT_5: &str = openai::GPT_5;
    pub const GPT_5_CODEX: &str = openai::GPT_5_CODEX;
    pub const GPT_5_MINI: &str = openai::GPT_5_MINI;
    pub const GPT_5_NANO: &str = openai::GPT_5_NANO;
    pub const CODEX_MINI: &str = openai::CODEX_MINI;
    pub const CODEX_MINI_LATEST: &str = openai::CODEX_MINI_LATEST;
    pub const CLAUDE_OPUS_4_1_20250805: &str = anthropic::CLAUDE_OPUS_4_1_20250805;
    pub const CLAUDE_SONNET_4_20250514: &str = anthropic::CLAUDE_SONNET_4_20250514;
    pub const OPENROUTER_X_AI_GROK_CODE_FAST_1: &str = openrouter::X_AI_GROK_CODE_FAST_1;
    pub const OPENROUTER_X_AI_GROK_4_FAST_FREE: &str = openrouter::X_AI_GROK_4_FAST_FREE;
    pub const OPENROUTER_QWEN3_CODER: &str = openrouter::QWEN3_CODER;
    pub const OPENROUTER_DEEPSEEK_CHAT_V3_1: &str = openrouter::DEEPSEEK_DEEPSEEK_CHAT_V3_1;
    pub const OPENROUTER_OPENAI_GPT_5: &str = openrouter::OPENAI_GPT_5;
    pub const OPENROUTER_OPENAI_GPT_5_CODEX: &str = openrouter::OPENAI_GPT_5_CODEX;
    pub const OPENROUTER_ANTHROPIC_CLAUDE_SONNET_4: &str = openrouter::ANTHROPIC_CLAUDE_SONNET_4;
    pub const XAI_GROK_2_LATEST: &str = xai::GROK_2_LATEST;
    pub const XAI_GROK_2: &str = xai::GROK_2;
    pub const XAI_GROK_2_MINI: &str = xai::GROK_2_MINI;
    pub const XAI_GROK_2_REASONING: &str = xai::GROK_2_REASONING;
    pub const XAI_GROK_2_VISION: &str = xai::GROK_2_VISION;
}

/// Prompt caching defaults shared across features and providers
pub mod prompt_cache {
    pub const DEFAULT_ENABLED: bool = true;
    pub const DEFAULT_CACHE_DIR: &str = ".vtcode/cache/prompts";
    pub const DEFAULT_MAX_ENTRIES: usize = 1_000;
    pub const DEFAULT_MAX_AGE_DAYS: u64 = 30;
    pub const DEFAULT_AUTO_CLEANUP: bool = true;
    pub const DEFAULT_MIN_QUALITY_THRESHOLD: f64 = 0.7;

    pub const OPENAI_MIN_PREFIX_TOKENS: u32 = 1_024;
    pub const OPENAI_IDLE_EXPIRATION_SECONDS: u64 = 60 * 60; // 1 hour max reuse window

    pub const ANTHROPIC_DEFAULT_TTL_SECONDS: u64 = 5 * 60; // 5 minutes
    pub const ANTHROPIC_EXTENDED_TTL_SECONDS: u64 = 60 * 60; // 1 hour option
    pub const ANTHROPIC_MAX_BREAKPOINTS: u8 = 4;

    pub const GEMINI_MIN_PREFIX_TOKENS: u32 = 1_024;
    pub const GEMINI_EXPLICIT_DEFAULT_TTL_SECONDS: u64 = 60 * 60; // 1 hour default for explicit caches

    pub const OPENROUTER_CACHE_DISCOUNT_ENABLED: bool = true;
    pub const XAI_CACHE_ENABLED: bool = true;
    pub const DEEPSEEK_CACHE_ENABLED: bool = true;
}

/// Model validation and helper functions
pub mod model_helpers {
    use super::models;

    /// Get supported models for a provider
    pub fn supported_for(provider: &str) -> Option<&'static [&'static str]> {
        match provider {
            "google" | "gemini" => Some(models::google::SUPPORTED_MODELS),
            "openai" => Some(models::openai::SUPPORTED_MODELS),
            "anthropic" => Some(models::anthropic::SUPPORTED_MODELS),
            "openrouter" => Some(models::openrouter::SUPPORTED_MODELS),
            "xai" => Some(models::xai::SUPPORTED_MODELS),
            _ => None,
        }
    }

    /// Get default model for a provider
    pub fn default_for(provider: &str) -> Option<&'static str> {
        match provider {
            "google" | "gemini" => Some(models::google::DEFAULT_MODEL),
            "openai" => Some(models::openai::DEFAULT_MODEL),
            "anthropic" => Some(models::anthropic::DEFAULT_MODEL),
            "openrouter" => Some(models::openrouter::DEFAULT_MODEL),
            "xai" => Some(models::xai::DEFAULT_MODEL),
            _ => None,
        }
    }

    /// Validate if a model is supported by a provider
    pub fn is_valid(provider: &str, model: &str) -> bool {
        supported_for(provider)
            .map(|list| list.iter().any(|m| *m == model))
            .unwrap_or(false)
    }
}

/// Default configuration values
pub mod defaults {
    use super::{models, ui};

    pub const DEFAULT_MODEL: &str = models::google::GEMINI_2_5_FLASH_PREVIEW;
    pub const DEFAULT_CLI_MODEL: &str = models::google::GEMINI_2_5_FLASH_PREVIEW;
    pub const DEFAULT_PROVIDER: &str = "gemini";
    pub const DEFAULT_API_KEY_ENV: &str = "GEMINI_API_KEY";
    pub const DEFAULT_THEME: &str = "ciapre-dark";
    pub const DEFAULT_MAX_TOOL_LOOPS: usize = 100;
    pub const ANTHROPIC_DEFAULT_MAX_TOKENS: u32 = 4_096;
    pub const DEFAULT_PTY_STDOUT_TAIL_LINES: usize = 20;
    pub const DEFAULT_TOOL_OUTPUT_MODE: &str = ui::TOOL_OUTPUT_MODE_COMPACT;
}

pub mod ui {
    pub const TOOL_OUTPUT_MODE_COMPACT: &str = "compact";
    pub const TOOL_OUTPUT_MODE_FULL: &str = "full";
    pub const DEFAULT_INLINE_VIEWPORT_ROWS: u16 = 16;
    pub const SLASH_SUGGESTION_LIMIT: usize = 6;
}

/// Reasoning effort configuration constants
pub mod reasoning {
    pub const LOW: &str = "low";
    pub const MEDIUM: &str = "medium";
    pub const HIGH: &str = "high";
    pub const ALLOWED_LEVELS: &[&str] = &[LOW, MEDIUM, HIGH];
}

/// Message role constants to avoid hardcoding strings
pub mod message_roles {
    pub const SYSTEM: &str = "system";
    pub const USER: &str = "user";
    pub const ASSISTANT: &str = "assistant";
    pub const TOOL: &str = "tool";
}

/// URL constants for API endpoints
pub mod urls {
    pub const GEMINI_API_BASE: &str = "https://generativelanguage.googleapis.com/v1beta";
    pub const OPENAI_API_BASE: &str = "https://api.openai.com/v1";
    pub const ANTHROPIC_API_BASE: &str = "https://api.anthropic.com/v1";
    pub const ANTHROPIC_API_VERSION: &str = "2023-06-01";
    pub const OPENROUTER_API_BASE: &str = "https://openrouter.ai/api/v1";
    pub const XAI_API_BASE: &str = "https://api.x.ai/v1";
}

/// Tool name constants to avoid hardcoding strings throughout the codebase
pub mod tools {
    pub const GREP_SEARCH: &str = "grep_search";
    pub const LIST_FILES: &str = "list_files";
    pub const RUN_TERMINAL_CMD: &str = "run_terminal_cmd";
    pub const READ_FILE: &str = "read_file";
    pub const WRITE_FILE: &str = "write_file";
    pub const EDIT_FILE: &str = "edit_file";
    pub const DELETE_FILE: &str = "delete_file";
    pub const CREATE_FILE: &str = "create_file";
    pub const AST_GREP_SEARCH: &str = "ast_grep_search";
    pub const SIMPLE_SEARCH: &str = "simple_search";
    pub const BASH: &str = "bash";
    pub const APPLY_PATCH: &str = "apply_patch";
    pub const SRGN: &str = "srgn";
    pub const CURL: &str = "curl";
    pub const UPDATE_PLAN: &str = "update_plan";

    // Explorer-specific tools
    pub const FILE_METADATA: &str = "file_metadata";
    pub const PROJECT_OVERVIEW: &str = "project_overview";
    pub const TREE_SITTER_ANALYZE: &str = "tree_sitter_analyze";

    // Special wildcard for full access
    pub const WILDCARD_ALL: &str = "*";
}

pub mod project_doc {
    pub const DEFAULT_MAX_BYTES: usize = 16 * 1024;
}

/// Context window management defaults
pub mod context {
    /// Approximate character count per token when estimating context size
    pub const CHAR_PER_TOKEN_APPROX: usize = 3;

    /// Default maximum context window (in approximate tokens)
    pub const DEFAULT_MAX_TOKENS: usize = 90_000;

    /// Trim target as a percentage of the maximum token budget
    pub const DEFAULT_TRIM_TO_PERCENT: u8 = 80;

    /// Minimum allowed trim percentage (prevents overly aggressive retention)
    pub const MIN_TRIM_RATIO_PERCENT: u8 = 60;

    /// Maximum allowed trim percentage (prevents minimal trimming)
    pub const MAX_TRIM_RATIO_PERCENT: u8 = 90;

    /// Default number of recent turns to preserve verbatim
    pub const DEFAULT_PRESERVE_RECENT_TURNS: usize = 12;

    /// Minimum number of recent turns that must remain after trimming
    pub const MIN_PRESERVE_RECENT_TURNS: usize = 6;

    /// Maximum number of recent turns to keep when aggressively reducing context
    pub const AGGRESSIVE_PRESERVE_RECENT_TURNS: usize = 8;

    /// Maximum number of retry attempts when the provider signals context overflow
    pub const CONTEXT_ERROR_RETRY_LIMIT: usize = 2;
}

/// Chunking constants for large file handling
pub mod chunking {
    /// Maximum lines before triggering chunking for read_file
    pub const MAX_LINES_THRESHOLD: usize = 2_000;

    /// Number of lines to read from start of file when chunking
    pub const CHUNK_START_LINES: usize = 800;

    /// Number of lines to read from end of file when chunking
    pub const CHUNK_END_LINES: usize = 800;

    /// Maximum lines for terminal command output before truncation
    pub const MAX_TERMINAL_OUTPUT_LINES: usize = 3_000;

    /// Number of lines to show from start of terminal output when truncating
    pub const TERMINAL_OUTPUT_START_LINES: usize = 1_000;

    /// Number of lines to show from end of terminal output when truncating
    pub const TERMINAL_OUTPUT_END_LINES: usize = 1_000;

    /// Maximum content size for write_file before chunking (in bytes)
    pub const MAX_WRITE_CONTENT_SIZE: usize = 500_000; // 500KB

    /// Chunk size for write operations (in bytes)
    pub const WRITE_CHUNK_SIZE: usize = 50_000; // 50KB chunks
}
