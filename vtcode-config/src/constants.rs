/// Application metadata constants shared across crates
pub mod app {
    pub const DISPLAY_NAME: &str = "VTCode";
}

/// Prompt path constants to avoid hardcoding throughout the codebase
pub mod prompts {
    pub const DEFAULT_SYSTEM_PROMPT_PATH: &str = "prompts/system.md";
    pub const DEFAULT_CUSTOM_PROMPTS_DIR: &str = "~/.vtcode/prompts";
    pub const CUSTOM_PROMPTS_ENV_VAR: &str = "VTCODE_HOME";
    pub const DEFAULT_CUSTOM_PROMPT_MAX_FILE_SIZE_KB: usize = 64;
    pub const CORE_BUILTIN_PROMPTS_DIR: &str = "vtcode-core/prompts/custom";
}

/// Command execution defaults shared across the agent runtime
pub mod commands {
    pub const DEFAULT_EXTRA_PATH_ENTRIES: &[&str] = &[
        "$HOME/.cargo/bin",
        "$HOME/.local/bin",
        "/opt/homebrew/bin",
        "/usr/local/bin",
        "$HOME/.asdf/bin",
        "$HOME/.asdf/shims",
        "$HOME/go/bin",
    ];
}

/// Model ID constants to sync with docs/models.json
pub mod models {
    // Google/Gemini models
    pub mod google {
        /// Default model - using stable version for production reliability
        pub const DEFAULT_MODEL: &str = "gemini-2.5-flash";

        pub const SUPPORTED_MODELS: &[&str] = &[
            "gemini-2.5-pro",
            "gemini-2.5-flash",
            "gemini-2.5-flash-lite",
            "gemini-2.5-flash-preview-05-20",
        ];

        /// Models that support thinking/reasoning capability
        /// Based on: https://ai.google.dev/gemini-api/docs/models
        /// All Gemini 2.5 models support the Thinking capability
        pub const REASONING_MODELS: &[&str] = &[
            "gemini-2.5-pro",
            "gemini-2.5-flash",
            "gemini-2.5-flash-lite",
            "gemini-2.5-flash-preview-05-20",
        ];

        /// Models that support context caching
        /// Context caching reduces costs for repeated API calls with similar contexts
        pub const CACHING_MODELS: &[&str] = &[
            "gemini-2.5-pro",
            "gemini-2.5-flash",
            "gemini-2.5-flash-lite",
            "gemini-2.5-flash-preview-05-20",
        ];

        /// Models that support code execution
        /// Code execution allows models to write and execute Python code
        pub const CODE_EXECUTION_MODELS: &[&str] = &[
            "gemini-2.5-pro",
            "gemini-2.5-flash",
            "gemini-2.5-flash-lite",
            "gemini-2.5-flash-preview-05-20",
        ];

        // Convenience constants for commonly used models
        pub const GEMINI_2_5_PRO: &str = "gemini-2.5-pro";
        pub const GEMINI_2_5_FLASH: &str = "gemini-2.5-flash";
        pub const GEMINI_2_5_FLASH_LITE: &str = "gemini-2.5-flash-lite";
        pub const GEMINI_2_5_FLASH_PREVIEW: &str = "gemini-2.5-flash-preview-05-20";
    }

    // OpenAI models (from docs/models.json)
    pub mod openai {
        pub const DEFAULT_MODEL: &str = "gpt-5";
        pub const SUPPORTED_MODELS: &[&str] = &[
            "gpt-5",
            "gpt-5-codex",
            "gpt-5-mini",
            "gpt-5-nano",
            "gpt-5.1", // Enhanced version of GPT-5 with temperature support and streaming
            "gpt-5.1-codex", // Enhanced version of GPT-5 Codex with temperature support and streaming
            "gpt-5.1-mini",  // Enhanced mini version with temperature support and streaming
            "codex-mini-latest",
            "gpt-oss-20b",
            "gpt-oss-120b",
        ];

        /// Models that require the OpenAI Responses API
        pub const RESPONSES_API_MODELS: &[&str] = &[
            GPT_5,
            GPT_5_CODEX,
            GPT_5_MINI,
            GPT_5_NANO,
            GPT_5_1,
            GPT_5_1_CODEX,
            GPT_5_1_MINI,
        ];

        /// Models that support the OpenAI reasoning parameter payload
        pub const REASONING_MODELS: &[&str] = &[GPT_5, GPT_5_CODEX, GPT_5_1, GPT_5_1_CODEX];

        /// Models that do not expose structured tool calling on the OpenAI platform
        pub const TOOL_UNAVAILABLE_MODELS: &[&str] = &[];

        /// GPT-OSS models that use harmony tokenization
        pub const HARMONY_MODELS: &[&str] = &[GPT_OSS_20B, GPT_OSS_120B];

        // Convenience constants for commonly used models
        pub const GPT_5: &str = "gpt-5";
        pub const GPT_5_CODEX: &str = "gpt-5-codex";
        pub const GPT_5_MINI: &str = "gpt-5-mini";
        pub const GPT_5_NANO: &str = "gpt-5-nano";
        pub const GPT_5_1: &str = "gpt-5.1"; // Enhanced version with temperature support and streaming
        pub const GPT_5_1_CODEX: &str = "gpt-5.1-codex"; // Enhanced version with temperature support and streaming
        pub const GPT_5_1_MINI: &str = "gpt-5.1-mini"; // Enhanced version with temperature support and streaming
        pub const CODEX_MINI_LATEST: &str = "codex-mini-latest";
        pub const GPT_OSS_20B: &str = "gpt-oss-20b";
        pub const GPT_OSS_120B: &str = "gpt-oss-120b";
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
            "kimi-k2-thinking",
            "kimi-k2-thinking-turbo",
            "kimi-k2-0905-preview",
            "kimi-k2-0711-preview",
            "kimi-latest",
            "kimi-latest-8k",
            "kimi-latest-32k",
            "kimi-latest-128k",
        ];

        pub const KIMI_K2_TURBO_PREVIEW: &str = "kimi-k2-turbo-preview";
        pub const KIMI_K2_THINKING: &str = "kimi-k2-thinking";
        pub const KIMI_K2_THINKING_TURBO: &str = "kimi-k2-thinking-turbo";
        pub const KIMI_K2_0905_PREVIEW: &str = "kimi-k2-0905-preview";
        pub const KIMI_K2_0711_PREVIEW: &str = "kimi-k2-0711-preview";
        pub const KIMI_LATEST: &str = "kimi-latest";
        pub const KIMI_LATEST_8K: &str = "kimi-latest-8k";
        pub const KIMI_LATEST_32K: &str = "kimi-latest-32k";
        pub const KIMI_LATEST_128K: &str = "kimi-latest-128k";
    }

    // OpenRouter models (extensible via vtcode.toml)
    #[cfg(not(docsrs))]
    pub mod openrouter {
        include!(concat!(env!("OUT_DIR"), "/openrouter_constants.rs"));
    }

    #[cfg(docsrs)]
    pub mod openrouter {
        pub const SUPPORTED_MODELS: &[&str] = &[];
        pub const REASONING_MODELS: &[&str] = &[];
        pub const TOOL_UNAVAILABLE_MODELS: &[&str] = &[];

        // Define the constants that are referenced elsewhere to avoid compile errors
        pub const X_AI_GROK_CODE_FAST_1: &str = "x-ai/grok-code-fast-1";
        pub const QWEN3_CODER: &str = "qwen/qwen3-coder";
        pub const ANTHROPIC_CLAUDE_SONNET_4_5: &str = "anthropic/claude-4-5-sonnet";

        pub mod vendor {
            pub mod openrouter {
                pub const MODELS: &[&str] = &[];
            }
        }
    }

    // LM Studio models (OpenAI-compatible local server)
    pub mod lmstudio {
        pub const DEFAULT_MODEL: &str = META_LLAMA_31_8B_INSTRUCT;
        pub const SUPPORTED_MODELS: &[&str] = &[
            META_LLAMA_3_8B_INSTRUCT,
            META_LLAMA_31_8B_INSTRUCT,
            QWEN25_7B_INSTRUCT,
            GEMMA_2_2B_IT,
            GEMMA_2_9B_IT,
            PHI_31_MINI_4K_INSTRUCT,
        ];

        pub const META_LLAMA_3_8B_INSTRUCT: &str = "lmstudio-community/meta-llama-3-8b-instruct";
        pub const META_LLAMA_31_8B_INSTRUCT: &str = "lmstudio-community/meta-llama-3.1-8b-instruct";
        pub const QWEN25_7B_INSTRUCT: &str = "lmstudio-community/qwen2.5-7b-instruct";
        pub const GEMMA_2_2B_IT: &str = "lmstudio-community/gemma-2-2b-it";
        pub const GEMMA_2_9B_IT: &str = "lmstudio-community/gemma-2-9b-it";
        pub const PHI_31_MINI_4K_INSTRUCT: &str = "lmstudio-community/phi-3.1-mini-4k-instruct";
    }

    pub mod ollama {
        pub const DEFAULT_LOCAL_MODEL: &str = "gpt-oss:20b";
        pub const DEFAULT_CLOUD_MODEL: &str = "gpt-oss:120b-cloud";
        pub const DEFAULT_MODEL: &str = DEFAULT_LOCAL_MODEL;
        pub const SUPPORTED_MODELS: &[&str] = &[
            DEFAULT_LOCAL_MODEL,
            QWEN3_1_7B,
            DEFAULT_CLOUD_MODEL,
            GPT_OSS_20B_CLOUD,
            DEEPSEEK_V31_671B_CLOUD,
            KIMI_K2_1T_CLOUD,
            QWEN3_CODER_480B_CLOUD,
            GLM_46_CLOUD,
            MINIMAX_M2_CLOUD,
        ];

        /// Models that emit structured reasoning traces when `think` is enabled
        pub const REASONING_MODELS: &[&str] = &[
            GPT_OSS_20B,
            GPT_OSS_20B_CLOUD,
            GPT_OSS_120B_CLOUD,
            QWEN3_1_7B,
            DEEPSEEK_V31_671B_CLOUD,
            KIMI_K2_1T_CLOUD,
            QWEN3_CODER_480B_CLOUD,
            GLM_46_CLOUD,
            MINIMAX_M2_CLOUD,
        ];

        /// Models that require an explicit reasoning effort level instead of boolean toggle
        pub const REASONING_LEVEL_MODELS: &[&str] =
            &[GPT_OSS_20B, GPT_OSS_20B_CLOUD, GPT_OSS_120B_CLOUD];

        pub const GPT_OSS_20B: &str = DEFAULT_LOCAL_MODEL;
        pub const GPT_OSS_20B_CLOUD: &str = "gpt-oss:20b-cloud";
        pub const GPT_OSS_120B_CLOUD: &str = DEFAULT_CLOUD_MODEL;
        pub const QWEN3_1_7B: &str = "qwen3:1.7b";
        pub const DEEPSEEK_V31_671B_CLOUD: &str = "deepseek-v3.1:671b-cloud";
        pub const KIMI_K2_1T_CLOUD: &str = "kimi-k2:1t-cloud";
        pub const QWEN3_CODER_480B_CLOUD: &str = "qwen3-coder:480b-cloud";
        pub const GLM_46_CLOUD: &str = "glm-4.6:cloud";
        pub const MINIMAX_M2_CLOUD: &str = "minimax-m2:cloud";
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

    // MiniMax models (Anthropic-compatible API, standalone provider)
    pub mod minimax {
        pub const DEFAULT_MODEL: &str = MINIMAX_M2;
        pub const SUPPORTED_MODELS: &[&str] = &[MINIMAX_M2];
        pub const MINIMAX_M2: &str = "MiniMax-M2";
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
    pub const GPT_5_1: &str = openai::GPT_5_1;
    pub const GPT_5_1_CODEX: &str = openai::GPT_5_1_CODEX;
    pub const GPT_5_1_MINI: &str = openai::GPT_5_1_MINI;
    pub const CODEX_MINI: &str = openai::CODEX_MINI_LATEST;
    pub const CODEX_MINI_LATEST: &str = openai::CODEX_MINI_LATEST;
    pub const CLAUDE_OPUS_4_1_20250805: &str = anthropic::CLAUDE_OPUS_4_1_20250805;
    pub const CLAUDE_SONNET_4_5: &str = anthropic::CLAUDE_SONNET_4_5;
    pub const CLAUDE_HAIKU_4_5: &str = anthropic::CLAUDE_HAIKU_4_5;
    pub const CLAUDE_SONNET_4_20250514: &str = anthropic::CLAUDE_SONNET_4_20250514;
    pub const MINIMAX_M2: &str = minimax::MINIMAX_M2;
    pub const MOONSHOT_KIMI_K2_TURBO_PREVIEW: &str = moonshot::KIMI_K2_TURBO_PREVIEW;
    pub const MOONSHOT_KIMI_K2_THINKING: &str = moonshot::KIMI_K2_THINKING;
    pub const MOONSHOT_KIMI_K2_THINKING_TURBO: &str = moonshot::KIMI_K2_THINKING_TURBO;
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
    #[cfg(not(docsrs))]
    pub const OPENROUTER_X_AI_GROK_CODE_FAST_1: &str = openrouter::X_AI_GROK_CODE_FAST_1;
    #[cfg(docsrs)]
    pub const OPENROUTER_X_AI_GROK_CODE_FAST_1: &str = "x-ai/grok-code-fast-1";
    #[cfg(not(docsrs))]
    pub const OPENROUTER_QWEN3_CODER: &str = openrouter::QWEN3_CODER;
    #[cfg(docsrs)]
    pub const OPENROUTER_QWEN3_CODER: &str = "qwen/qwen3-coder";
    #[cfg(not(docsrs))]
    pub const OPENROUTER_ANTHROPIC_CLAUDE_SONNET_4_5: &str =
        openrouter::ANTHROPIC_CLAUDE_SONNET_4_5;
    #[cfg(docsrs)]
    pub const OPENROUTER_ANTHROPIC_CLAUDE_SONNET_4_5: &str = "anthropic/claude-4-5-sonnet";
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
            "minimax" => Some(models::minimax::SUPPORTED_MODELS),
            "deepseek" => Some(models::deepseek::SUPPORTED_MODELS),
            #[cfg(not(docsrs))]
            "openrouter" => Some(models::openrouter::SUPPORTED_MODELS),
            #[cfg(docsrs)]
            "openrouter" => Some(&[]),
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
            "minimax" => Some(models::minimax::DEFAULT_MODEL),
            "deepseek" => Some(models::deepseek::DEFAULT_MODEL),
            #[cfg(not(docsrs))]
            "openrouter" => Some(models::openrouter::DEFAULT_MODEL),
            #[cfg(docsrs)]
            "openrouter" => Some("openrouter/auto"), // Fallback for docs.rs build
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
            .map(|list| list.contains(&model))
            .unwrap_or(false)
    }
}

/// Environment variable names shared across the application.
pub mod env {
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

    pub const DEFAULT_MODEL: &str = models::openai::DEFAULT_MODEL;
    pub const DEFAULT_CLI_MODEL: &str = models::openai::DEFAULT_MODEL;
    pub const DEFAULT_PROVIDER: &str = "openai";
    pub const DEFAULT_API_KEY_ENV: &str = "OPENAI_API_KEY";
    pub const DEFAULT_THEME: &str = "ciapre-dark";
    pub const DEFAULT_FULL_AUTO_MAX_TURNS: usize = 30;
    pub const DEFAULT_MAX_TOOL_LOOPS: usize = 100;
    pub const DEFAULT_MAX_REPEATED_TOOL_CALLS: usize = 3;
    pub const ANTHROPIC_DEFAULT_MAX_TOKENS: u32 = 4_096;
    pub const DEFAULT_PTY_STDOUT_TAIL_LINES: usize = 20;
    pub const DEFAULT_PTY_SCROLLBACK_LINES: usize = 400;
    pub const DEFAULT_TOOL_OUTPUT_MODE: &str = ui::TOOL_OUTPUT_MODE_COMPACT;
}

pub mod ui {
    pub const TOOL_OUTPUT_MODE_COMPACT: &str = "compact";
    pub const TOOL_OUTPUT_MODE_FULL: &str = "full";
    pub const DEFAULT_INLINE_VIEWPORT_ROWS: u16 = 16;
    pub const INLINE_SHOW_TIMELINE_PANE: bool = true;
    pub const SLASH_SUGGESTION_LIMIT: usize = 50; // All commands are scrollable
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
    pub const MODAL_INSTRUCTIONS_TITLE: &str = "";
    pub const MODAL_INSTRUCTIONS_BULLET: &str = "•";
    pub const INLINE_HEADER_HEIGHT: u16 = 4;
    pub const INLINE_INPUT_HEIGHT: u16 = 4;
    pub const INLINE_INPUT_MAX_LINES: usize = 10;
    pub const INLINE_NAVIGATION_PERCENT: u16 = 28;
    pub const INLINE_NAVIGATION_MIN_WIDTH: u16 = 24;
    pub const INLINE_CONTENT_MIN_WIDTH: u16 = 48;
    pub const INLINE_STACKED_NAVIGATION_PERCENT: u16 = INLINE_NAVIGATION_PERCENT;
    pub const INLINE_SCROLLBAR_EDGE_PADDING: u16 = 1;
    pub const INLINE_TRANSCRIPT_BOTTOM_PADDING: u16 = 6;
    pub const INLINE_PREVIEW_MAX_CHARS: usize = 56;
    pub const INLINE_PREVIEW_ELLIPSIS: &str = "…";
    pub const HEADER_HIGHLIGHT_PREVIEW_MAX_CHARS: usize = 48;
    pub const INLINE_AGENT_MESSAGE_LEFT_PADDING: &str = "  ";
    pub const INLINE_AGENT_QUOTE_PREFIX: &str = "";
    pub const INLINE_USER_MESSAGE_DIVIDER_SYMBOL: &str = "─";
    pub const INLINE_BLOCK_TOP_LEFT: &str = "╭";
    pub const INLINE_BLOCK_TOP_RIGHT: &str = "╮";
    pub const INLINE_BLOCK_BODY_LEFT: &str = "│";
    pub const INLINE_BLOCK_BODY_RIGHT: &str = "│";
    pub const INLINE_BLOCK_BOTTOM_LEFT: &str = "╰";
    pub const INLINE_BLOCK_BOTTOM_RIGHT: &str = "╯";
    pub const INLINE_BLOCK_HORIZONTAL: &str = "─";
    pub const INLINE_TOOL_HEADER_LABEL: &str = "Tool";
    pub const INLINE_TOOL_ACTION_PREFIX: &str = "→";
    pub const INLINE_TOOL_DETAIL_PREFIX: &str = "↳";
    pub const INLINE_PTY_HEADER_LABEL: &str = "Terminal";
    pub const INLINE_PTY_RUNNING_LABEL: &str = "running";
    pub const INLINE_PTY_STATUS_LIVE: &str = "LIVE";
    pub const INLINE_PTY_STATUS_DONE: &str = "DONE";
    pub const INLINE_PTY_PLACEHOLDER: &str = "Terminal output";
    pub const MODAL_LIST_HIGHLIGHT_SYMBOL: &str = "✦";
    pub const MODAL_LIST_HIGHLIGHT_FULL: &str = "✦ ";
    pub const MODAL_LIST_SUMMARY_FILTER_LABEL: &str = "Filter";
    pub const MODAL_LIST_SUMMARY_SEPARATOR: &str = " • ";
    pub const MODAL_LIST_SUMMARY_MATCHES_LABEL: &str = "Matches";
    pub const MODAL_LIST_SUMMARY_TOTAL_LABEL: &str = "of";
    pub const MODAL_LIST_SUMMARY_NO_MATCHES: &str = "No matches";
    pub const MODAL_LIST_SUMMARY_RESET_HINT: &str = "Press Esc to reset";
    pub const MODAL_LIST_NO_RESULTS_MESSAGE: &str = "No matching options";
    pub const HEADER_VERSION_PROMPT: &str = "> ";
    pub const HEADER_VERSION_PREFIX: &str = "VT Code";
    pub const HEADER_VERSION_LEFT_DELIMITER: &str = "(";
    pub const HEADER_VERSION_RIGHT_DELIMITER: &str = ")";
    pub const HEADER_MODE_INLINE: &str = "Inline session";
    pub const HEADER_MODE_ALTERNATE: &str = "Alternate session";
    pub const HEADER_MODE_AUTO: &str = "Auto session";
    pub const HEADER_MODE_FULL_AUTO_SUFFIX: &str = " (full)";
    pub const HEADER_MODE_PRIMARY_SEPARATOR: &str = " | ";
    pub const HEADER_MODE_SECONDARY_SEPARATOR: &str = " | ";
    pub const HEADER_PROVIDER_PREFIX: &str = "Provider: ";
    pub const HEADER_MODEL_PREFIX: &str = "Model: ";
    pub const HEADER_REASONING_PREFIX: &str = "Reasoning effort: ";
    pub const HEADER_TRUST_PREFIX: &str = "Trust: ";
    pub const HEADER_TOOLS_PREFIX: &str = "Tools: ";
    pub const HEADER_MCP_PREFIX: &str = "MCP: ";
    pub const HEADER_GIT_PREFIX: &str = "git: ";
    pub const HEADER_GIT_CLEAN_SUFFIX: &str = "✓";
    pub const HEADER_GIT_DIRTY_SUFFIX: &str = "*";
    pub const HEADER_UNKNOWN_PLACEHOLDER: &str = "unavailable";
    pub const HEADER_STATUS_LABEL: &str = "Status";
    pub const HEADER_STATUS_ACTIVE: &str = "Active";
    pub const HEADER_STATUS_PAUSED: &str = "Paused";
    pub const HEADER_MESSAGES_LABEL: &str = "Messages";
    pub const HEADER_INPUT_LABEL: &str = "Input";
    pub const HEADER_INPUT_ENABLED: &str = "Enabled";
    pub const HEADER_INPUT_DISABLED: &str = "Disabled";
    pub const INLINE_USER_PREFIX: &str = " ";
    pub const CHAT_INPUT_PLACEHOLDER_BOOTSTRAP: &str = "Task (@files, #prompts, /commands)";
    pub const CHAT_INPUT_PLACEHOLDER_FOLLOW_UP: &str = "Command (@files, #prompts, /commands)";
    pub const HEADER_SHORTCUT_HINT: &str = "Shortcuts: Enter=submit | Shift+Enter=newline | Ctrl/Cmd+Enter=queue | Esc=cancel | Ctrl+C=interrupt | @=file picker | #=custom prompts | /=slash commands";
    pub const HEADER_META_SEPARATOR: &str = "   ";
    pub const WELCOME_TEXT_WIDTH: usize = 80;
    pub const WELCOME_SHORTCUT_SECTION_TITLE: &str = "Keyboard Shortcuts";
    pub const WELCOME_SHORTCUT_HINT_PREFIX: &str = "Shortcuts:";
    pub const WELCOME_SHORTCUT_SEPARATOR: &str = "•";
    pub const WELCOME_SHORTCUT_INDENT: &str = "  ";
    pub const WELCOME_SLASH_COMMAND_SECTION_TITLE: &str = "Slash Commands";
    pub const WELCOME_SLASH_COMMAND_LIMIT: usize = 6;
    pub const WELCOME_SLASH_COMMAND_PREFIX: &str = "/";
    pub const WELCOME_SLASH_COMMAND_INTRO: &str = "";
    pub const WELCOME_SLASH_COMMAND_INDENT: &str = "  ";
    pub const NAVIGATION_BLOCK_TITLE: &str = "Timeline";
    pub const NAVIGATION_BLOCK_SHORTCUT_NOTE: &str = "Ctrl+T";
    pub const NAVIGATION_EMPTY_LABEL: &str = "Waiting for activity";
    pub const NAVIGATION_INDEX_PREFIX: &str = "#";
    pub const NAVIGATION_LABEL_AGENT: &str = "Agent";
    pub const NAVIGATION_LABEL_ERROR: &str = "Error";
    pub const NAVIGATION_LABEL_INFO: &str = "Info";
    pub const NAVIGATION_LABEL_POLICY: &str = "Policy";
    pub const NAVIGATION_LABEL_TOOL: &str = "Tool";
    pub const NAVIGATION_LABEL_USER: &str = "User";
    pub const NAVIGATION_LABEL_PTY: &str = "PTY";
    pub const PLAN_BLOCK_TITLE: &str = "TODOs";
    pub const PLAN_STATUS_EMPTY: &str = "No TODOs";
    pub const PLAN_STATUS_IN_PROGRESS: &str = "In progress";
    pub const PLAN_STATUS_DONE: &str = "Done";
    pub const PLAN_IN_PROGRESS_NOTE: &str = "in progress";
    pub const SUGGESTION_BLOCK_TITLE: &str = "Slash Commands";
    pub const STATUS_LINE_MODE: &str = "auto";
    pub const STATUS_LINE_REFRESH_INTERVAL_MS: u64 = 1000;
    pub const STATUS_LINE_COMMAND_TIMEOUT_MS: u64 = 200;

    // Theme and color constants
    pub const THEME_MIN_CONTRAST_RATIO: f64 = 4.5;
    pub const THEME_FOREGROUND_LIGHTEN_RATIO: f64 = 0.25;
    pub const THEME_SECONDARY_LIGHTEN_RATIO: f64 = 0.2;
    pub const THEME_MIX_RATIO: f64 = 0.35;
    pub const THEME_TOOL_BODY_MIX_RATIO: f64 = 0.35;
    pub const THEME_TOOL_BODY_LIGHTEN_RATIO: f64 = 0.2;
    pub const THEME_RESPONSE_COLOR_LIGHTEN_RATIO: f64 = 0.15;
    pub const THEME_REASONING_COLOR_LIGHTEN_RATIO: f64 = 0.3;
    pub const THEME_USER_COLOR_LIGHTEN_RATIO: f64 = 0.2;
    pub const THEME_SECONDARY_USER_COLOR_LIGHTEN_RATIO: f64 = 0.4;
    pub const THEME_PRIMARY_STATUS_LIGHTEN_RATIO: f64 = 0.35;
    pub const THEME_PRIMARY_STATUS_SECONDARY_LIGHTEN_RATIO: f64 = 0.5;
    pub const THEME_LOGO_ACCENT_BANNER_LIGHTEN_RATIO: f64 = 0.35;
    pub const THEME_LOGO_ACCENT_BANNER_SECONDARY_LIGHTEN_RATIO: f64 = 0.25;

    // UI Color constants
    pub const THEME_COLOR_WHITE_RED: u8 = 0xFF;
    pub const THEME_COLOR_WHITE_GREEN: u8 = 0xFF;
    pub const THEME_COLOR_WHITE_BLUE: u8 = 0xFF;
    pub const THEME_MIX_RATIO_MIN: f64 = 0.0;
    pub const THEME_MIX_RATIO_MAX: f64 = 1.0;
    pub const THEME_BLEND_CLAMP_MIN: f64 = 0.0;
    pub const THEME_BLEND_CLAMP_MAX: f64 = 255.0;

    // WCAG contrast algorithm constants
    pub const THEME_RELATIVE_LUMINANCE_CUTOFF: f64 = 0.03928;
    pub const THEME_RELATIVE_LUMINANCE_LOW_FACTOR: f64 = 12.92;
    pub const THEME_RELATIVE_LUMINANCE_OFFSET: f64 = 0.055;
    pub const THEME_RELATIVE_LUMINANCE_EXPONENT: f64 = 2.4;
    pub const THEME_CONTRAST_RATIO_OFFSET: f64 = 0.05;
    pub const THEME_RED_LUMINANCE_COEFFICIENT: f64 = 0.2126;
    pub const THEME_GREEN_LUMINANCE_COEFFICIENT: f64 = 0.7152;
    pub const THEME_BLUE_LUMINANCE_COEFFICIENT: f64 = 0.0722;
    pub const THEME_LUMINANCE_LIGHTEN_RATIO: f64 = 0.2;
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
    pub const MINIMAX_API_BASE: &str = "https://api.minimax.io/anthropic/v1";
    pub const OPENROUTER_API_BASE: &str = "https://openrouter.ai/api/v1";
    pub const XAI_API_BASE: &str = "https://api.x.ai/v1";
    pub const DEEPSEEK_API_BASE: &str = "https://api.deepseek.com/v1";
    pub const Z_AI_API_BASE: &str = "https://api.z.ai/api";
    pub const MOONSHOT_API_BASE: &str = "https://api.moonshot.cn/v1";
    pub const LMSTUDIO_API_BASE: &str = "http://localhost:1234/v1";
    pub const OLLAMA_API_BASE: &str = "http://localhost:11434";
    pub const OLLAMA_CLOUD_API_BASE: &str = "https://ollama.com";
}

/// Environment variable names for overriding provider base URLs
pub mod env_vars {
    pub const GEMINI_BASE_URL: &str = "GEMINI_BASE_URL";
    pub const OPENAI_BASE_URL: &str = "OPENAI_BASE_URL";
    pub const ANTHROPIC_BASE_URL: &str = "ANTHROPIC_BASE_URL";
    pub const OPENROUTER_BASE_URL: &str = "OPENROUTER_BASE_URL";
    pub const XAI_BASE_URL: &str = "XAI_BASE_URL";
    pub const DEEPSEEK_BASE_URL: &str = "DEEPSEEK_BASE_URL";
    pub const Z_AI_BASE_URL: &str = "ZAI_BASE_URL";
    pub const MOONSHOT_BASE_URL: &str = "MOONSHOT_BASE_URL";
    pub const LMSTUDIO_BASE_URL: &str = "LMSTUDIO_BASE_URL";
    pub const OLLAMA_BASE_URL: &str = "OLLAMA_BASE_URL";
    pub const MINIMAX_BASE_URL: &str = "MINIMAX_BASE_URL";
}

/// HTTP header constants for provider integrations
pub mod headers {
    pub const ACCEPT_LANGUAGE: &str = "Accept-Language";
    pub const ACCEPT_LANGUAGE_DEFAULT: &str = "en-US,en";
}

/// Tool name constants to avoid hardcoding strings throughout the codebase
pub mod tools {
    /// Sole content-search tool (ripgrep-backed)
    pub const GREP_FILE: &str = "grep_file";
    pub const LIST_FILES: &str = "list_files";
    pub const RUN_COMMAND: &str = "run_terminal_cmd";
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
    pub const APPLY_PATCH: &str = "apply_patch";
    pub const UPDATE_PLAN: &str = "update_plan";
    pub const WEB_FETCH: &str = "web_fetch";
    pub const SEARCH_TOOLS: &str = "search_tools";
    pub const EXECUTE_CODE: &str = "execute_code";
    /// Returns recent errors and suggested fixes gathered from session snapshots and tool history
    pub const GET_ERRORS: &str = "get_errors";
    pub const DEBUG_AGENT: &str = "debug_agent";
    pub const ANALYZE_AGENT: &str = "analyze_agent";

    // Special wildcard for full access
    pub const WILDCARD_ALL: &str = "*";
}

/// Bash tool security validation constants
pub mod bash {
    /// Commands that are always blocked for security reasons
    pub const ALWAYS_BLOCKED_COMMANDS: &[&str] = &[
        "rm",
        "rmdir",
        "del",
        "format",
        "fdisk",
        "mkfs",
        "dd",
        "shred",
        "wipe",
        "srm",
        "unlink",
        "chmod",
        "chown",
        "passwd",
        "usermod",
        "userdel",
        "systemctl",
        "service",
        "kill",
        "killall",
        "pkill",
        "reboot",
        "shutdown",
        "halt",
        "poweroff",
        "sudo",
        "su",
        "doas",
        "runas",
        "mount",
        "umount",
        "fsck",
        "tune2fs", // Filesystem operations
        "iptables",
        "ufw",
        "firewalld", // Firewall
        "crontab",
        "at", // Scheduling
        "podman",
        "kubectl", // Container/orchestration
    ];

    /// Network commands that require sandbox to be enabled
    pub const NETWORK_COMMANDS: &[&str] = &[
        "wget", "ftp", "scp", "rsync", "ssh", "telnet", "nc", "ncat", "socat",
    ];

    /// Commands that are always allowed (safe development tools)
    pub const ALLOWED_COMMANDS: &[&str] = &[
        // File system and basic utilities
        "ls",
        "pwd",
        "cat",
        "head",
        "tail",
        "grep",
        "find",
        "wc",
        "sort",
        "uniq",
        "cut",
        "awk",
        "sed",
        "echo",
        "printf",
        "seq",
        "basename",
        "dirname",
        "date",
        "cal",
        "bc",
        "expr",
        "test",
        "[",
        "]",
        "true",
        "false",
        "sleep",
        "which",
        "type",
        "file",
        "stat",
        "du",
        "df",
        "ps",
        "top",
        "htop",
        "tree",
        "less",
        "more",
        "tac",
        "rev",
        "tr",
        "fold",
        "paste",
        "join",
        "comm",
        "diff",
        "patch",
        "gzip",
        "gunzip",
        "bzip2",
        "bunzip2",
        "xz",
        "unxz",
        "tar",
        "zip",
        "unzip",
        "shasum",
        "md5sum",
        "sha256sum",
        "sha512sum", // Hashing tools
        // Version control
        "git",
        "hg",
        "svn",
        "git-lfs",
        // Build systems and tools
        "make",
        "cmake",
        "ninja",
        "meson",
        "bazel",
        "buck2",
        "scons",
        "waf",
        "xcodebuild",
        // Rust/Cargo ecosystem
        "cargo",
        "rustc",
        "rustfmt",
        "rustup",
        "clippy",
        "cargo-clippy",
        "cargo-fmt",
        "cargo-build",
        "cargo-test",
        "cargo-run",
        "cargo-check",
        "cargo-doc",
        // Node.js/npm ecosystem
        "npm",
        "yarn",
        "pnpm",
        "bun",
        "npx",
        "node",
        "yarnpkg",
        "npm-run",
        "npm-test",
        "npm-start",
        "npm-build",
        "npm-lint",
        "npm-install",
        "yarn-test",
        "yarn-start",
        "yarn-build",
        "yarn-lint",
        "yarn-install",
        "pnpm-test",
        "pnpm-start",
        "pnpm-build",
        "pnpm-lint",
        "pnpm-install",
        "bun-test",
        "bun-start",
        "bun-build",
        "bun-lint",
        "bun-install",
        "bun-run",
        // Python ecosystem
        "python",
        "python3",
        "pip",
        "pip3",
        "virtualenv",
        "venv",
        "conda",
        "pytest",
        "python-m-pytest",
        "python3-m-pytest",
        "python-m-pip",
        "python3-m-pip",
        "python-m-venv",
        "python3-m-venv",
        "black",
        "flake8",
        "mypy",
        "pylint",
        "isort",
        "ruff",
        "bandit",
        // Java ecosystem
        "java",
        "javac",
        "jar",
        "jarsigner",
        "javadoc",
        "jmap",
        "jstack",
        "jstat",
        "jinfo",
        "mvn",
        "gradle",
        "gradlew",
        "./gradlew",
        "mvnw",
        "./mvnw",
        "mvn-test",
        "mvn-compile",
        "mvn-package",
        "mvn-install",
        "mvn-clean",
        "gradle-test",
        "gradle-build",
        "gradle-check",
        "gradle-run",
        "gradle-clean",
        // Go ecosystem
        "go",
        "gofmt",
        "goimports",
        "golint",
        "go-test",
        "go-build",
        "go-run",
        "go-mod",
        "golangci-lint",
        "go-doc",
        "go-vet",
        "go-install",
        "go-clean",
        // C/C++ ecosystem
        "gcc",
        "g++",
        "clang",
        "clang++",
        "clang-cl",
        "cpp",
        "cc",
        "c++",
        "gcc-ar",
        "gcc-nm",
        "gcc-ranlib",
        "ld",
        "lld",
        "gold",
        "bfdld",
        "make",
        "cmake",
        "ninja",
        "autotools",
        "autoconf",
        "automake",
        "libtool",
        "pkg-config",
        "pkgconfig",
        // Testing frameworks and tools
        "pytest",
        "jest",
        "mocha",
        "jasmine",
        "karma",
        "chai",
        "sinon",
        "vitest",
        "cypress",
        "selenium",
        "playwright",
        "testcafe",
        "tape",
        "ava",
        "qunit",
        "junit",
        "googletest",
        "catch2",
        "benchmark",
        "hyperfine",
        // Linting and formatting tools
        "eslint",
        "prettier",
        "tslint",
        "jshint",
        "jscs",
        "stylelint",
        "htmlhint",
        "jsonlint",
        "yamllint",
        "toml-check",
        "markdownlint",
        "remark-cli",
        "shellcheck",
        "hadolint",
        "rustfmt",
        "gofmt",
        "black",
        "isort",
        "ruff",
        "clang-format",
        "clang-tidy",
        // Documentation tools
        "doxygen",
        "sphinx",
        "mkdocs",
        "hugo",
        "jekyll",
        "gatsby",
        "next",
        "nuxt",
        "vuepress",
        "docusaurus",
        "storybook",
        "gitbook",
        "readthedocs",
        "pandoc",
        "mdbook",
        "mdBook",
        // Container tools (safe operations only)
        "docker",
        "docker-compose",
        "docker-buildx",
        "podman",
        "buildah",
        "docker-build",
        "docker-run",
        "docker-ps",
        "docker-images",
        "docker-inspect",
        "docker-exec",
        "docker-logs",
        "docker-stats",
        "docker-system",
        "docker-network",
        // Database tools (development usage)
        "sqlite3",
        "mysql",
        "psql",
        "mongosh",
        "redis-cli",
        "redis-server",
        // Cloud and deployment tools
        "aws",
        "gcloud",
        "az",
        "kubectl",
        "helm",
        "terraform",
        "tf",
        "terragrunt",
        "serverless",
        "sls",
        "pulumi",
        "cdk",
        "sam",
        "localstack",
        "minikube",
        // Security and analysis tools
        "gitleaks",
        "trivy",
        "snyk",
        "npm-audit",
        "pip-audit",
        "cargo-audit",
        "bandit",
        "safety",
        "pipenv",
        "poetry",
        // Performance profiling tools
        "perf",
        "strace",
        "ltrace",
        "valgrind",
        "gdb",
        "lldb",
        "sar",
        "iostat",
        "vmstat",
        "htop",
        "iotop",
        "nethogs",
        "iftop",
        "speedtest-cli",
        "ab",
        "wrk",
        "hey",
        // CI/CD tools
        "gh",
        "gitlab-ci",
        "bitbucket",
        "azure-pipelines",
        "circleci",
        "jenkins",
        "drone",
        "buildkite",
        "travis",
        "appveyor",
        // Package managers for various languages
        "composer",
        "pear",
        "gem",
        "rbenv",
        "rvm",
        "nvm",
        "nodenv",
        "pyenv",
        "rbenv",
        "sdkman",
        "jenv",
        "lein",
        "boot",
        "mix",
        "rebar3",
        "erl",
        "elixir",
        // Web development tools
        "webpack",
        "rollup",
        "vite",
        "parcel",
        "esbuild",
        "snowpack",
        "turbo",
        "swc",
        "babel",
        "postcss",
        "sass",
        "scss",
        "less",
        "stylus",
        "tailwindcss",
        // Mobile development tools
        "xcodebuild",
        "fastlane",
        "gradle",
        "./gradlew",
        "cordova",
        "ionic",
        "react-native",
        "flutter",
        "expo",
        "capacitor",
    ];
}

pub mod mcp {
    pub const RENDERER_CONTEXT7: &str = "context7";
    pub const RENDERER_SEQUENTIAL_THINKING: &str = "sequential-thinking";

    /// Default startup timeout for MCP servers in milliseconds (60 seconds)
    /// Can be overridden via config: mcp.startup_timeout_seconds
    pub const DEFAULT_STARTUP_TIMEOUT_MS: u64 = 60_000;
}

pub mod project_doc {
    pub const DEFAULT_MAX_BYTES: usize = 16 * 1024;
}

pub mod instructions {
    pub const DEFAULT_MAX_BYTES: usize = 16 * 1024;
}

/// LLM generation parameters
pub mod llm_generation {
    /// Default temperature for main LLM responses (0.0-1.0)
    /// Controls randomness/creativity: 0=deterministic, 1=random
    /// 0.7 provides balanced creativity and consistency
    pub const DEFAULT_TEMPERATURE: f32 = 0.7;

    /// Default maximum tokens for main LLM generation responses
    pub const DEFAULT_MAX_TOKENS: u32 = 2_000;

    /// Default temperature for prompt refinement (0.0-1.0)
    /// Lower than main temperature for more deterministic refinement
    pub const DEFAULT_REFINE_TEMPERATURE: f32 = 0.3;

    /// Default maximum tokens for prompt refinement
    /// Prompts are shorter, so 800 tokens is typically sufficient
    pub const DEFAULT_REFINE_MAX_TOKENS: u32 = 800;

    /// Maximum tokens recommended for models with 256k context window
    /// Leaves room for input context and token overhead
    pub const MAX_TOKENS_256K_CONTEXT: u32 = 32_768;

    /// Maximum tokens recommended for models with 128k context window
    pub const MAX_TOKENS_128K_CONTEXT: u32 = 16_384;
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

    /// Enable semantic-aware compression heuristics by default
    pub const DEFAULT_SEMANTIC_COMPRESSION_ENABLED: bool = false;

    /// Enable tool-aware retention heuristics by default
    pub const DEFAULT_TOOL_AWARE_RETENTION_ENABLED: bool = false;

    /// Default maximum structural depth to preserve during semantic pruning
    pub const DEFAULT_MAX_STRUCTURAL_DEPTH: usize = 3;

    /// Default number of recent tool results to preserve when tool-aware retention is enabled
    pub const DEFAULT_PRESERVE_RECENT_TOOLS: usize = 5;

    /// Minimum structural depth allowed for semantic pruning
    pub const MIN_STRUCTURAL_DEPTH: usize = 1;

    /// Maximum structural depth allowed for semantic pruning to prevent runaway retention
    pub const MAX_STRUCTURAL_DEPTH: usize = 12;

    /// Minimum number of tool outputs to preserve when tool-aware retention is enabled
    pub const MIN_PRESERVE_RECENT_TOOLS: usize = 1;

    /// Maximum number of tool outputs to preserve when tool-aware retention is enabled
    pub const MAX_PRESERVE_RECENT_TOOLS: usize = 24;

    /// Maximum number of retry attempts when the provider signals context overflow
    pub const CONTEXT_ERROR_RETRY_LIMIT: usize = 2;

    /// Default semantic score for cached values (0-255 scale, typically)
    pub const DEFAULT_SEMANTIC_CACHE_SCORE: u8 = 128;

    /// Default semantic score for non-system messages
    pub const DEFAULT_SEMANTIC_SCORE: u32 = 500;

    /// Default token count estimate for message parts with multiple components
    pub const DEFAULT_TOKENS_FOR_PARTS: usize = 256;

    /// Approximate number of characters per token used for token estimation
    pub const CHAR_PER_TOKEN_APPROXIMATION: usize = 4;

    /// Default semantic score for system messages
    pub const SYSTEM_MESSAGE_SEMANTIC_SCORE: u32 = 950;

    /// Default semantic score for user messages
    pub const USER_MESSAGE_SEMANTIC_SCORE: u32 = 850;

    /// Scaling factor for semantic scores (typically scales from 0-255 to 0-1000 range)
    pub const SEMANTIC_SCORE_SCALING_FACTOR: u32 = 4;

    /// Conversion factor for percentage calculations (100.0)
    pub const PERCENTAGE_CONVERSION_FACTOR: f64 = 100.0;

    /// Decimal precision for context utilization percentage display
    pub const CONTEXT_UTILIZATION_PRECISION: usize = 1;

    /// Decimal precision for semantic value per token display
    pub const SEMANTIC_VALUE_PRECISION: usize = 2;

    /// Minimum token count to prevent division by zero
    pub const MIN_TOKEN_COUNT: usize = 1;
}

/// Chunking constants for large file handling
pub mod chunking {
    /// Maximum lines before triggering chunking for read_file
    pub const MAX_LINES_THRESHOLD: usize = 2_000;

    /// Number of lines to read from start of file when chunking
    pub const CHUNK_START_LINES: usize = 800;

    /// Number of lines to read from end of file when chunking
    pub const CHUNK_END_LINES: usize = 800;

    // DEPRECATED: Terminal output truncation now uses token-based limits instead of line limits
    // See: src/agent/runloop/tool_output/streams.rs (MAX_TOOL_RESPONSE_TOKENS: 25_000)
    // These constants are no longer used and can be safely removed when cleaning up

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
