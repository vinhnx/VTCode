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
        ];

        /// Models that require the OpenAI Responses API
        pub const RESPONSES_API_MODELS: &[&str] = &[GPT_5, GPT_5_CODEX, GPT_5_MINI, GPT_5_NANO];

        /// Models that support the OpenAI reasoning parameter payload
        pub const REASONING_MODELS: &[&str] = &[GPT_5, GPT_5_CODEX];

        /// Models that do not expose structured tool calling on the OpenAI platform
        pub const TOOL_UNAVAILABLE_MODELS: &[&str] = &[];

        // Convenience constants for commonly used models
        pub const GPT_5: &str = "gpt-5";
        pub const GPT_5_CODEX: &str = "gpt-5-codex";
        pub const GPT_5_MINI: &str = "gpt-5-mini";
        pub const GPT_5_NANO: &str = "gpt-5-nano";
        pub const CODEX_MINI_LATEST: &str = "codex-mini-latest";
        pub const CODEX_MINI: &str = "codex-mini";
    }

    // Z.AI models (direct API)
    pub mod zai {
        pub const DEFAULT_MODEL: &str = "glm-4.6";
        pub const SUPPORTED_MODELS: &[&str] = &[
            "glm-4.6",
            "glm-4.5",
            "glm-4.5-air",
            "glm-4.5-x",
            "glm-4.5-airx",
            "glm-4.5-flash",
            "glm-4-32b-0414-128k",
        ];

        pub const GLM_4_6: &str = "glm-4.6";
        pub const GLM_4_5: &str = "glm-4.5";
        pub const GLM_4_5_AIR: &str = "glm-4.5-air";
        pub const GLM_4_5_X: &str = "glm-4.5-x";
        pub const GLM_4_5_AIRX: &str = "glm-4.5-airx";
        pub const GLM_4_5_FLASH: &str = "glm-4.5-flash";
        pub const GLM_4_32B_0414_128K: &str = "glm-4-32b-0414-128k";
    }

    // Moonshot.ai models (direct API)
    pub mod moonshot {
        pub const DEFAULT_MODEL: &str = "kimi-k2-turbo-preview";
        pub const SUPPORTED_MODELS: &[&str] = &[
            "kimi-k2-turbo-preview",
            "kimi-k2-0905-preview",
            "kimi-k2-0711-preview",
            "kimi-latest",
            "kimi-latest-8k",
            "kimi-latest-32k",
            "kimi-latest-128k",
        ];

        pub const KIMI_K2_TURBO_PREVIEW: &str = "kimi-k2-turbo-preview";
        pub const KIMI_K2_0905_PREVIEW: &str = "kimi-k2-0905-preview";
        pub const KIMI_K2_0711_PREVIEW: &str = "kimi-k2-0711-preview";
        pub const KIMI_LATEST: &str = "kimi-latest";
        pub const KIMI_LATEST_8K: &str = "kimi-latest-8k";
        pub const KIMI_LATEST_32K: &str = "kimi-latest-32k";
        pub const KIMI_LATEST_128K: &str = "kimi-latest-128k";
    }

    // OpenRouter models (extensible via vtcode.toml)
    pub mod openrouter {
        pub const X_AI_GROK_CODE_FAST_1: &str = "x-ai/grok-code-fast-1";
        pub const X_AI_GROK_4_FAST: &str = "x-ai/grok-4-fast";
        pub const X_AI_GROK_4: &str = "x-ai/grok-4";
        pub const Z_AI_GLM_4_5_AIR_FREE: &str = "z-ai/glm-4.5-air:free";
        pub const Z_AI_GLM_4_6: &str = "z-ai/glm-4.6";
        pub const MOONSHOTAI_KIMI_K2_0905: &str = "moonshotai/kimi-k2-0905";
        pub const QWEN3_MAX: &str = "qwen/qwen3-max";
        pub const QWEN3_235B_A22B: &str = "qwen/qwen3-235b-a22b";
        pub const QWEN3_235B_A22B_FREE: &str = "qwen/qwen3-235b-a22b:free";
        pub const QWEN3_235B_A22B_2507: &str = "qwen/qwen3-235b-a22b-2507";
        pub const QWEN3_235B_A22B_THINKING_2507: &str = "qwen/qwen3-235b-a22b-thinking-2507";
        pub const QWEN3_32B: &str = "qwen/qwen3-32b";
        pub const QWEN3_30B_A3B: &str = "qwen/qwen3-30b-a3b";
        pub const QWEN3_30B_A3B_FREE: &str = "qwen/qwen3-30b-a3b:free";
        pub const QWEN3_30B_A3B_INSTRUCT_2507: &str = "qwen/qwen3-30b-a3b-instruct-2507";
        pub const QWEN3_30B_A3B_THINKING_2507: &str = "qwen/qwen3-30b-a3b-thinking-2507";
        pub const QWEN3_14B: &str = "qwen/qwen3-14b";
        pub const QWEN3_14B_FREE: &str = "qwen/qwen3-14b:free";
        pub const QWEN3_8B: &str = "qwen/qwen3-8b";
        pub const QWEN3_8B_FREE: &str = "qwen/qwen3-8b:free";
        pub const QWEN3_4B_FREE: &str = "qwen/qwen3-4b:free";
        pub const QWEN3_NEXT_80B_A3B_INSTRUCT: &str = "qwen/qwen3-next-80b-a3b-instruct";
        pub const QWEN3_NEXT_80B_A3B_THINKING: &str = "qwen/qwen3-next-80b-a3b-thinking";
        pub const QWEN3_CODER: &str = "qwen/qwen3-coder";
        pub const QWEN3_CODER_FREE: &str = "qwen/qwen3-coder:free";
        pub const QWEN3_CODER_PLUS: &str = "qwen/qwen3-coder-plus";
        pub const QWEN3_CODER_FLASH: &str = "qwen/qwen3-coder-flash";
        pub const QWEN3_CODER_30B_A3B_INSTRUCT: &str = "qwen/qwen3-coder-30b-a3b-instruct";
        pub const DEEPSEEK_DEEPSEEK_V3_2_EXP: &str = "deepseek/deepseek-v3.2-exp";
        pub const DEEPSEEK_DEEPSEEK_CHAT_V3_1: &str = "deepseek/deepseek-chat-v3.1";
        pub const DEEPSEEK_DEEPSEEK_R1: &str = "deepseek/deepseek-r1";
        pub const OPENAI_GPT_OSS_120B: &str = "openai/gpt-oss-120b";
        pub const OPENAI_GPT_OSS_20B: &str = "openai/gpt-oss-20b";
        pub const OPENAI_GPT_OSS_20B_FREE: &str = "openai/gpt-oss-20b:free";
        pub const OPENAI_GPT_5: &str = "openai/gpt-5";
        pub const OPENAI_GPT_5_CODEX: &str = "openai/gpt-5-codex";
        pub const OPENAI_GPT_5_CHAT: &str = "openai/gpt-5-chat";
        pub const OPENAI_GPT_4O_SEARCH_PREVIEW: &str = "openai/gpt-4o-search-preview";
        pub const OPENAI_GPT_4O_MINI_SEARCH_PREVIEW: &str = "openai/gpt-4o-mini-search-preview";
        pub const OPENAI_CHATGPT_4O_LATEST: &str = "openai/chatgpt-4o-latest";
        pub const ANTHROPIC_CLAUDE_SONNET_4_5: &str = "anthropic/claude-sonnet-4.5";
        pub const ANTHROPIC_CLAUDE_OPUS_4_1: &str = "anthropic/claude-opus-4.1";

        pub const DEFAULT_MODEL: &str = X_AI_GROK_CODE_FAST_1;

        pub const SUPPORTED_MODELS: &[&str] = &[
            X_AI_GROK_CODE_FAST_1,
            X_AI_GROK_4_FAST,
            X_AI_GROK_4,
            Z_AI_GLM_4_5_AIR_FREE,
            Z_AI_GLM_4_6,
            MOONSHOTAI_KIMI_K2_0905,
            QWEN3_MAX,
            QWEN3_235B_A22B,
            QWEN3_235B_A22B_FREE,
            QWEN3_235B_A22B_2507,
            QWEN3_235B_A22B_THINKING_2507,
            QWEN3_32B,
            QWEN3_30B_A3B,
            QWEN3_30B_A3B_INSTRUCT_2507,
            QWEN3_30B_A3B_THINKING_2507,
            QWEN3_14B,
            QWEN3_NEXT_80B_A3B_INSTRUCT,
            QWEN3_NEXT_80B_A3B_THINKING,
            QWEN3_CODER,
            QWEN3_CODER_FREE,
            QWEN3_CODER_PLUS,
            QWEN3_CODER_FLASH,
            QWEN3_CODER_30B_A3B_INSTRUCT,
            QWEN3_4B_FREE,
            DEEPSEEK_DEEPSEEK_V3_2_EXP,
            DEEPSEEK_DEEPSEEK_CHAT_V3_1,
            DEEPSEEK_DEEPSEEK_R1,
            OPENAI_GPT_OSS_120B,
            OPENAI_GPT_OSS_20B,
            OPENAI_GPT_5,
            OPENAI_GPT_5_CODEX,
            ANTHROPIC_CLAUDE_SONNET_4_5,
            ANTHROPIC_CLAUDE_OPUS_4_1,
        ];

        /// Models that expose reasoning traces via OpenRouter APIs
        pub const REASONING_MODELS: &[&str] = &[
            X_AI_GROK_CODE_FAST_1,
            X_AI_GROK_4_FAST,
            X_AI_GROK_4,
            Z_AI_GLM_4_6,
            QWEN3_235B_A22B,
            QWEN3_235B_A22B_FREE,
            QWEN3_235B_A22B_THINKING_2507,
            QWEN3_32B,
            QWEN3_30B_A3B,
            QWEN3_30B_A3B_THINKING_2507,
            QWEN3_14B,
            QWEN3_4B_FREE,
            QWEN3_NEXT_80B_A3B_THINKING,
            DEEPSEEK_DEEPSEEK_V3_2_EXP,
            DEEPSEEK_DEEPSEEK_CHAT_V3_1,
            DEEPSEEK_DEEPSEEK_R1,
            OPENAI_GPT_OSS_120B,
            OPENAI_GPT_OSS_20B,
            OPENAI_GPT_5,
            OPENAI_GPT_5_CODEX,
            ANTHROPIC_CLAUDE_SONNET_4_5,
            ANTHROPIC_CLAUDE_OPUS_4_1,
        ];

        /// Models that do not expose function calling via OpenRouter
        pub const TOOL_UNAVAILABLE_MODELS: &[&str] = &[
            Z_AI_GLM_4_5_AIR_FREE,
            QWEN3_30B_A3B_FREE,
            QWEN3_14B_FREE,
            QWEN3_8B,
            QWEN3_8B_FREE,
            OPENAI_GPT_OSS_20B_FREE,
            OPENAI_GPT_5_CHAT,
            OPENAI_GPT_4O_SEARCH_PREVIEW,
            OPENAI_GPT_4O_MINI_SEARCH_PREVIEW,
            OPENAI_CHATGPT_4O_LATEST,
        ];
    }

    pub mod ollama {
        pub const DEFAULT_MODEL: &str = "gpt-oss:20b";
        pub const SUPPORTED_MODELS: &[&str] = &[DEFAULT_MODEL, QWEN3_1_7B];

        pub const GPT_OSS_20B: &str = DEFAULT_MODEL;
        pub const QWEN3_1_7B: &str = "qwen3:1.7b";
    }

    // DeepSeek models (native API)
    pub mod deepseek {
        pub const DEFAULT_MODEL: &str = "deepseek-chat";
        pub const SUPPORTED_MODELS: &[&str] = &["deepseek-chat", "deepseek-reasoner"];

        pub const DEEPSEEK_CHAT: &str = "deepseek-chat";
        pub const DEEPSEEK_REASONER: &str = "deepseek-reasoner";
    }

    // Anthropic models (from docs/models.json) - Updated for tool use best practices
    pub mod anthropic {
        // Standard model for straightforward tools - Sonnet 4 preferred for most use cases
        pub const DEFAULT_MODEL: &str = "claude-sonnet-4-5";
        pub const SUPPORTED_MODELS: &[&str] = &[
            "claude-opus-4-1-20250805", // Latest: Opus 4.1 (2025-08-05)
            "claude-sonnet-4-5",        // Latest: Sonnet 4.5 (2025-10-15)
            "claude-haiku-4-5",         // Latest: Haiku 4.5 (2025-10-15)
            "claude-sonnet-4-20250514", // Previous: Sonnet 4 (2025-05-14)
        ];

        // Convenience constants for commonly used models
        pub const CLAUDE_OPUS_4_1_20250805: &str = "claude-opus-4-1-20250805";
        pub const CLAUDE_SONNET_4_5: &str = "claude-sonnet-4-5";
        pub const CLAUDE_HAIKU_4_5: &str = "claude-haiku-4-5";
        pub const CLAUDE_SONNET_4_20250514: &str = "claude-sonnet-4-20250514";

        /// Models that accept the reasoning effort parameter
        pub const REASONING_MODELS: &[&str] = &[
            CLAUDE_OPUS_4_1_20250805,
            CLAUDE_SONNET_4_5,
            CLAUDE_HAIKU_4_5,
            CLAUDE_SONNET_4_20250514,
        ];
    }

    // xAI models
    pub mod xai {
        pub const DEFAULT_MODEL: &str = "grok-4";
        pub const SUPPORTED_MODELS: &[&str] = &[
            "grok-4",
            "grok-4-mini",
            "grok-4-code",
            "grok-4-code-latest",
            "grok-4-vision",
        ];

        pub const GROK_4: &str = "grok-4";
        pub const GROK_4_MINI: &str = "grok-4-mini";
        pub const GROK_4_CODE: &str = "grok-4-code";
        pub const GROK_4_CODE_LATEST: &str = "grok-4-code-latest";
        pub const GROK_4_VISION: &str = "grok-4-vision";
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
    pub const CLAUDE_SONNET_4_5: &str = anthropic::CLAUDE_SONNET_4_5;
    pub const CLAUDE_HAIKU_4_5: &str = anthropic::CLAUDE_HAIKU_4_5;
    pub const CLAUDE_SONNET_4_20250514: &str = anthropic::CLAUDE_SONNET_4_20250514;
    pub const OPENROUTER_X_AI_GROK_CODE_FAST_1: &str = openrouter::X_AI_GROK_CODE_FAST_1;
    pub const OPENROUTER_X_AI_GROK_4_FAST: &str = openrouter::X_AI_GROK_4_FAST;
    pub const OPENROUTER_X_AI_GROK_4: &str = openrouter::X_AI_GROK_4;
    pub const OPENROUTER_Z_AI_GLM_4_5_AIR_FREE: &str = openrouter::Z_AI_GLM_4_5_AIR_FREE;
    pub const OPENROUTER_Z_AI_GLM_4_6: &str = openrouter::Z_AI_GLM_4_6;
    pub const OPENROUTER_MOONSHOTAI_KIMI_K2_0905: &str = openrouter::MOONSHOTAI_KIMI_K2_0905;
    pub const OPENROUTER_QWEN3_MAX: &str = openrouter::QWEN3_MAX;
    pub const OPENROUTER_QWEN3_235B_A22B: &str = openrouter::QWEN3_235B_A22B;
    pub const OPENROUTER_QWEN3_235B_A22B_FREE: &str = openrouter::QWEN3_235B_A22B_FREE;
    pub const OPENROUTER_QWEN3_235B_A22B_2507: &str = openrouter::QWEN3_235B_A22B_2507;
    pub const OPENROUTER_QWEN3_235B_A22B_THINKING_2507: &str =
        openrouter::QWEN3_235B_A22B_THINKING_2507;
    pub const OPENROUTER_QWEN3_32B: &str = openrouter::QWEN3_32B;
    pub const OPENROUTER_QWEN3_30B_A3B: &str = openrouter::QWEN3_30B_A3B;
    pub const OPENROUTER_QWEN3_30B_A3B_FREE: &str = openrouter::QWEN3_30B_A3B_FREE;
    pub const OPENROUTER_QWEN3_30B_A3B_INSTRUCT_2507: &str =
        openrouter::QWEN3_30B_A3B_INSTRUCT_2507;
    pub const OPENROUTER_QWEN3_30B_A3B_THINKING_2507: &str =
        openrouter::QWEN3_30B_A3B_THINKING_2507;
    pub const OPENROUTER_QWEN3_14B: &str = openrouter::QWEN3_14B;
    pub const OPENROUTER_QWEN3_14B_FREE: &str = openrouter::QWEN3_14B_FREE;
    pub const OPENROUTER_QWEN3_8B: &str = openrouter::QWEN3_8B;
    pub const OPENROUTER_QWEN3_8B_FREE: &str = openrouter::QWEN3_8B_FREE;
    pub const OPENROUTER_QWEN3_4B_FREE: &str = openrouter::QWEN3_4B_FREE;
    pub const OPENROUTER_QWEN3_NEXT_80B_A3B_INSTRUCT: &str =
        openrouter::QWEN3_NEXT_80B_A3B_INSTRUCT;
    pub const OPENROUTER_QWEN3_NEXT_80B_A3B_THINKING: &str =
        openrouter::QWEN3_NEXT_80B_A3B_THINKING;
    pub const OPENROUTER_QWEN3_CODER: &str = openrouter::QWEN3_CODER;
    pub const OPENROUTER_QWEN3_CODER_FREE: &str = openrouter::QWEN3_CODER_FREE;
    pub const OPENROUTER_QWEN3_CODER_PLUS: &str = openrouter::QWEN3_CODER_PLUS;
    pub const OPENROUTER_QWEN3_CODER_FLASH: &str = openrouter::QWEN3_CODER_FLASH;
    pub const OPENROUTER_QWEN3_CODER_30B_A3B_INSTRUCT: &str =
        openrouter::QWEN3_CODER_30B_A3B_INSTRUCT;
    pub const OPENROUTER_DEEPSEEK_V3_2_EXP: &str = openrouter::DEEPSEEK_DEEPSEEK_V3_2_EXP;
    pub const OPENROUTER_DEEPSEEK_CHAT_V3_1: &str = openrouter::DEEPSEEK_DEEPSEEK_CHAT_V3_1;
    pub const OPENROUTER_DEEPSEEK_R1: &str = openrouter::DEEPSEEK_DEEPSEEK_R1;
    pub const OPENROUTER_OPENAI_GPT_OSS_120B: &str = openrouter::OPENAI_GPT_OSS_120B;
    pub const OPENROUTER_OPENAI_GPT_OSS_20B: &str = openrouter::OPENAI_GPT_OSS_20B;
    pub const OPENROUTER_OPENAI_GPT_OSS_20B_FREE: &str = openrouter::OPENAI_GPT_OSS_20B_FREE;
    pub const OPENROUTER_OPENAI_GPT_5: &str = openrouter::OPENAI_GPT_5;
    pub const OPENROUTER_OPENAI_GPT_5_CODEX: &str = openrouter::OPENAI_GPT_5_CODEX;
    pub const OPENROUTER_OPENAI_GPT_5_CHAT: &str = openrouter::OPENAI_GPT_5_CHAT;
    pub const OPENROUTER_OPENAI_GPT_4O_SEARCH_PREVIEW: &str =
        openrouter::OPENAI_GPT_4O_SEARCH_PREVIEW;
    pub const OPENROUTER_OPENAI_GPT_4O_MINI_SEARCH_PREVIEW: &str =
        openrouter::OPENAI_GPT_4O_MINI_SEARCH_PREVIEW;
    pub const OPENROUTER_OPENAI_CHATGPT_4O_LATEST: &str = openrouter::OPENAI_CHATGPT_4O_LATEST;
    pub const OPENROUTER_ANTHROPIC_CLAUDE_SONNET_4_5: &str =
        openrouter::ANTHROPIC_CLAUDE_SONNET_4_5;
    pub const OPENROUTER_ANTHROPIC_CLAUDE_OPUS_4_1: &str = openrouter::ANTHROPIC_CLAUDE_OPUS_4_1;
    pub const MOONSHOT_KIMI_K2_TURBO_PREVIEW: &str = moonshot::KIMI_K2_TURBO_PREVIEW;
    pub const MOONSHOT_KIMI_K2_0905_PREVIEW: &str = moonshot::KIMI_K2_0905_PREVIEW;
    pub const MOONSHOT_KIMI_K2_0711_PREVIEW: &str = moonshot::KIMI_K2_0711_PREVIEW;
    pub const MOONSHOT_KIMI_LATEST: &str = moonshot::KIMI_LATEST;
    pub const MOONSHOT_KIMI_LATEST_8K: &str = moonshot::KIMI_LATEST_8K;
    pub const MOONSHOT_KIMI_LATEST_32K: &str = moonshot::KIMI_LATEST_32K;
    pub const MOONSHOT_KIMI_LATEST_128K: &str = moonshot::KIMI_LATEST_128K;
    pub const XAI_GROK_4: &str = xai::GROK_4;
    pub const XAI_GROK_4_MINI: &str = xai::GROK_4_MINI;
    pub const XAI_GROK_4_CODE: &str = xai::GROK_4_CODE;
    pub const XAI_GROK_4_CODE_LATEST: &str = xai::GROK_4_CODE_LATEST;
    pub const XAI_GROK_4_VISION: &str = xai::GROK_4_VISION;
    pub const DEEPSEEK_CHAT: &str = deepseek::DEEPSEEK_CHAT;
    pub const DEEPSEEK_REASONER: &str = deepseek::DEEPSEEK_REASONER;
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
    pub const ZAI_CACHE_ENABLED: bool = false;
    pub const MOONSHOT_CACHE_ENABLED: bool = true;
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
            "deepseek" => Some(models::deepseek::SUPPORTED_MODELS),
            "openrouter" => Some(models::openrouter::SUPPORTED_MODELS),
            "moonshot" => Some(models::moonshot::SUPPORTED_MODELS),
            "xai" => Some(models::xai::SUPPORTED_MODELS),
            "zai" => Some(models::zai::SUPPORTED_MODELS),
            "ollama" => Some(models::ollama::SUPPORTED_MODELS),
            _ => None,
        }
    }

    /// Get default model for a provider
    pub fn default_for(provider: &str) -> Option<&'static str> {
        match provider {
            "google" | "gemini" => Some(models::google::DEFAULT_MODEL),
            "openai" => Some(models::openai::DEFAULT_MODEL),
            "anthropic" => Some(models::anthropic::DEFAULT_MODEL),
            "deepseek" => Some(models::deepseek::DEFAULT_MODEL),
            "openrouter" => Some(models::openrouter::DEFAULT_MODEL),
            "moonshot" => Some(models::moonshot::DEFAULT_MODEL),
            "xai" => Some(models::xai::DEFAULT_MODEL),
            "zai" => Some(models::zai::DEFAULT_MODEL),
            "ollama" => Some(models::ollama::DEFAULT_MODEL),
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

/// Environment variable names shared across the application.
pub mod env {
    /// Toggle automatic update checks in the onboarding banner.
    pub const UPDATE_CHECK: &str = "VT_UPDATE_CHECK";

    /// Agent Client Protocol specific environment keys
    pub mod acp {
        #[derive(Debug, Clone, Copy)]
        pub enum AgentClientProtocolEnvKey {
            Enabled,
            ZedEnabled,
            ZedToolsReadFileEnabled,
            ZedToolsListFilesEnabled,
            ZedWorkspaceTrust,
        }

        impl AgentClientProtocolEnvKey {
            pub fn as_str(self) -> &'static str {
                match self {
                    Self::Enabled => "VT_ACP_ENABLED",
                    Self::ZedEnabled => "VT_ACP_ZED_ENABLED",
                    Self::ZedToolsReadFileEnabled => "VT_ACP_ZED_TOOLS_READ_FILE_ENABLED",
                    Self::ZedToolsListFilesEnabled => "VT_ACP_ZED_TOOLS_LIST_FILES_ENABLED",
                    Self::ZedWorkspaceTrust => "VT_ACP_ZED_WORKSPACE_TRUST",
                }
            }
        }
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
    pub const DEFAULT_PTY_SCROLLBACK_LINES: usize = 400;
    pub const DEFAULT_TOOL_OUTPUT_MODE: &str = ui::TOOL_OUTPUT_MODE_COMPACT;
}

pub mod ui {
    pub const TOOL_OUTPUT_MODE_COMPACT: &str = "compact";
    pub const TOOL_OUTPUT_MODE_FULL: &str = "full";
    pub const DEFAULT_INLINE_VIEWPORT_ROWS: u16 = 16;
    pub const INLINE_SHOW_TIMELINE_PANE: bool = false;
    pub const SLASH_SUGGESTION_LIMIT: usize = 6;
    pub const SLASH_PALETTE_MIN_WIDTH: u16 = 40;
    pub const SLASH_PALETTE_MIN_HEIGHT: u16 = 9;
    pub const SLASH_PALETTE_HORIZONTAL_MARGIN: u16 = 8;
    pub const SLASH_PALETTE_TOP_OFFSET: u16 = 3;
    pub const SLASH_PALETTE_CONTENT_PADDING: u16 = 6;
    pub const SLASH_PALETTE_HINT_PRIMARY: &str = "Type to filter slash commands.";
    pub const SLASH_PALETTE_HINT_SECONDARY: &str = "Press Enter to apply • Esc to dismiss.";
    pub const MODAL_MIN_WIDTH: u16 = 36;
    pub const MODAL_MIN_HEIGHT: u16 = 9;
    pub const MODAL_LIST_MIN_HEIGHT: u16 = 12;
    pub const MODAL_WIDTH_RATIO: f32 = 0.6;
    pub const MODAL_HEIGHT_RATIO: f32 = 0.6;
    pub const MODAL_MAX_WIDTH_RATIO: f32 = 0.9;
    pub const MODAL_MAX_HEIGHT_RATIO: f32 = 0.8;
    pub const MODAL_CONTENT_HORIZONTAL_PADDING: u16 = 8;
    pub const MODAL_CONTENT_VERTICAL_PADDING: u16 = 6;
    pub const INLINE_HEADER_HEIGHT: u16 = 4;
    pub const INLINE_INPUT_HEIGHT: u16 = 4;
    pub const INLINE_NAVIGATION_PERCENT: u16 = 32;
    pub const INLINE_NAVIGATION_MIN_WIDTH: u16 = 24;
    pub const INLINE_CONTENT_MIN_WIDTH: u16 = 48;
    pub const INLINE_STACKED_NAVIGATION_PERCENT: u16 = INLINE_NAVIGATION_PERCENT;
    pub const INLINE_SCROLLBAR_EDGE_PADDING: u16 = 1;
    pub const INLINE_TRANSCRIPT_BOTTOM_PADDING: u16 = 6;
    pub const INLINE_PREVIEW_MAX_CHARS: usize = 56;
    pub const INLINE_PREVIEW_ELLIPSIS: &str = "…";
    pub const INLINE_AGENT_MESSAGE_LEFT_PADDING: &str = "  ";
    pub const INLINE_AGENT_QUOTE_PREFIX: &str = "";
    pub const INLINE_USER_MESSAGE_DIVIDER_SYMBOL: &str = "─";
    pub const HEADER_VERSION_PROMPT: &str = "> ";
    pub const HEADER_VERSION_PREFIX: &str = "VT Code";
    pub const HEADER_VERSION_LEFT_DELIMITER: &str = "(";
    pub const HEADER_VERSION_RIGHT_DELIMITER: &str = ")";
    pub const HEADER_MODE_INLINE: &str = "Inline session";
    pub const HEADER_MODE_ALTERNATE: &str = "Alternate session";
    pub const HEADER_MODE_AUTO: &str = "Auto session";
    pub const HEADER_MODE_FULL_AUTO_SUFFIX: &str = " (full auto)";
    pub const HEADER_MODE_PRIMARY_SEPARATOR: &str = " | ";
    pub const HEADER_MODE_SECONDARY_SEPARATOR: &str = " | ";
    pub const HEADER_PROVIDER_PREFIX: &str = "Provider: ";
    pub const HEADER_MODEL_PREFIX: &str = "Model: ";
    pub const HEADER_REASONING_PREFIX: &str = "Reasoning: ";
    pub const HEADER_TRUST_PREFIX: &str = "Trust: ";
    pub const HEADER_TOOLS_PREFIX: &str = "Tools: ";
    pub const HEADER_LANGUAGES_PREFIX: &str = "Languages: ";
    pub const HEADER_MCP_PREFIX: &str = "MCP: ";
    pub const HEADER_UNKNOWN_PLACEHOLDER: &str = "unavailable";
    pub const HEADER_STATUS_LABEL: &str = "Status";
    pub const HEADER_STATUS_ACTIVE: &str = "Active";
    pub const HEADER_STATUS_PAUSED: &str = "Paused";
    pub const HEADER_MESSAGES_LABEL: &str = "Messages";
    pub const HEADER_INPUT_LABEL: &str = "Input";
    pub const HEADER_INPUT_ENABLED: &str = "Enabled";
    pub const HEADER_INPUT_DISABLED: &str = "Disabled";
    pub const HEADER_SHORTCUT_HINT: &str =
        "Shortcuts: Ctrl+Enter to submit • Esc to cancel • Ctrl+C to interrupt";
    pub const HEADER_META_SEPARATOR: &str = "   ";
    pub const WELCOME_TEXT_WIDTH: usize = 80;
    pub const WELCOME_SHORTCUT_SECTION_TITLE: &str = "Keyboard Shortcuts";
    pub const WELCOME_SHORTCUT_HINT_PREFIX: &str = "Shortcuts:";
    pub const WELCOME_SHORTCUT_SEPARATOR: &str = "•";
    pub const WELCOME_SHORTCUT_INDENT: &str = "  ";
    pub const WELCOME_SLASH_COMMAND_SECTION_TITLE: &str = "Slash Commands";
    pub const WELCOME_SLASH_COMMAND_LIMIT: usize = 6;
    pub const WELCOME_SLASH_COMMAND_PREFIX: &str = "/";
    pub const WELCOME_SLASH_COMMAND_INTRO: &str =
        "To get started, describe a task or try one of these commands:";
    pub const WELCOME_SLASH_COMMAND_INDENT: &str = "  ";
    pub const NAVIGATION_BLOCK_TITLE: &str = "Timeline";
    pub const NAVIGATION_EMPTY_LABEL: &str = "Waiting for activity";
    pub const NAVIGATION_INDEX_PREFIX: &str = "#";
    pub const NAVIGATION_LABEL_AGENT: &str = "Agent";
    pub const NAVIGATION_LABEL_ERROR: &str = "Error";
    pub const NAVIGATION_LABEL_INFO: &str = "Info";
    pub const NAVIGATION_LABEL_POLICY: &str = "Policy";
    pub const NAVIGATION_LABEL_TOOL: &str = "Tool";
    pub const NAVIGATION_LABEL_USER: &str = "User";
    pub const NAVIGATION_LABEL_PTY: &str = "PTY";
    pub const SUGGESTION_BLOCK_TITLE: &str = "Slash Commands";
}

/// Reasoning effort configuration constants
pub mod reasoning {
    pub const LOW: &str = "low";
    pub const MEDIUM: &str = "medium";
    pub const HIGH: &str = "high";
    pub const ALLOWED_LEVELS: &[&str] = &[LOW, MEDIUM, HIGH];
    pub const LABEL_LOW: &str = "Low";
    pub const LABEL_MEDIUM: &str = "Medium";
    pub const LABEL_HIGH: &str = "High";
    pub const DESCRIPTION_LOW: &str = "Fast responses with lightweight reasoning.";
    pub const DESCRIPTION_MEDIUM: &str = "Balanced depth and speed for general tasks.";
    pub const DESCRIPTION_HIGH: &str = "Maximum reasoning depth for complex problems.";
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
    pub const DEEPSEEK_API_BASE: &str = "https://api.deepseek.com/v1";
    pub const Z_AI_API_BASE: &str = "https://api.z.ai/api";
    pub const MOONSHOT_API_BASE: &str = "https://api.moonshot.cn/v1";
    pub const OLLAMA_API_BASE: &str = "http://localhost:11434";
}

/// Tool name constants to avoid hardcoding strings throughout the codebase
pub mod tools {
    pub const GREP_SEARCH: &str = "grep_search";
    pub const LIST_FILES: &str = "list_files";
    pub const RUN_TERMINAL_CMD: &str = "run_terminal_cmd";
    pub const RUN_PTY_CMD: &str = "run_pty_cmd";
    pub const CREATE_PTY_SESSION: &str = "create_pty_session";
    pub const LIST_PTY_SESSIONS: &str = "list_pty_sessions";
    pub const CLOSE_PTY_SESSION: &str = "close_pty_session";
    pub const SEND_PTY_INPUT: &str = "send_pty_input";
    pub const READ_PTY_SESSION: &str = "read_pty_session";
    pub const RESIZE_PTY_SESSION: &str = "resize_pty_session";
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

pub mod mcp {
    pub const RENDERER_CONTEXT7: &str = "context7";
    pub const RENDERER_SEQUENTIAL_THINKING: &str = "sequential-thinking";
}

pub mod project_doc {
    pub const DEFAULT_MAX_BYTES: usize = 16 * 1024;
}

pub mod instructions {
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

/// Diff preview controls for file operations
pub mod diff {
    /// Maximum number of bytes allowed in diff preview inputs
    pub const MAX_PREVIEW_BYTES: usize = 200_000;

    /// Number of context lines to include around changes in unified diff output
    pub const CONTEXT_RADIUS: usize = 3;

    /// Maximum number of diff lines to keep in preview output before condensation
    pub const MAX_PREVIEW_LINES: usize = 160;

    /// Number of leading diff lines to retain when condensing previews
    pub const HEAD_LINE_COUNT: usize = 96;

    /// Number of trailing diff lines to retain when condensing previews
    pub const TAIL_LINE_COUNT: usize = 32;
}
