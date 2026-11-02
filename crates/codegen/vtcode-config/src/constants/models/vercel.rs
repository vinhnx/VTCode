//! Curated Vercel AI Gateway models exposed by VT Code.
//!
//! Model IDs use the gateway's native `vendor/model` format and are forwarded
//! verbatim to `https://ai-gateway.vercel.sh/v1`.

pub const ANTHROPIC_CLAUDE_SONNET_5: &str = "anthropic/claude-sonnet-5";
pub const ANTHROPIC_CLAUDE_OPUS_5: &str = "anthropic/claude-opus-5";
pub const ANTHROPIC_CLAUDE_OPUS_5_5: &str = "anthropic/claude-opus-5-5";
pub const ANTHROPIC_CLAUDE_HAIKU_4_5: &str = "anthropic/claude-haiku-4.5";
pub const OPENAI_GPT_5_6_SOL: &str = "openai/gpt-5.6-sol";
pub const OPENAI_GPT_6_ASTRA: &str = "openai/gpt-6-astra";
pub const OPENAI_GPT_5_6_LUNA: &str = "openai/gpt-5.6-luna";
pub const OPENAI_GPT_6_SOL: &str = "openai/gpt-6-sol";
pub const OPENAI_GPT_6_LUNA: &str = "openai/gpt-6-luna";
pub const GOOGLE_GEMINI_3_1_PRO_PREVIEW: &str = "google/gemini-3.1-pro-preview";
pub const GOOGLE_GEMINI_3_8_FLASH: &str = "google/gemini-3.8-flash";
pub const DEEPSEEK_FLASH: &str = "deepseek/deepseek-v4.1-flash";
pub const MOONSHOTAI_KIMI_K3: &str = "moonshotai/kimi-k3";
pub const MINIMAX_M3: &str = "minimax/minimax-m3";
pub const SPACEXAI_GROK_4_7: &str = "spacexai/grok-4.7";

pub const DEFAULT_MODEL: &str = ANTHROPIC_CLAUDE_SONNET_5;

/// Curated agent-oriented AI Gateway models. Explicit model IDs outside this
/// list remain valid because the gateway routes hundreds of models.
pub const SUPPORTED_MODELS: &[&str] = &[
    ANTHROPIC_CLAUDE_SONNET_5,
    ANTHROPIC_CLAUDE_OPUS_5,
    ANTHROPIC_CLAUDE_OPUS_5_5,
    ANTHROPIC_CLAUDE_HAIKU_4_5,
    OPENAI_GPT_5_6_SOL,
    OPENAI_GPT_6_ASTRA,
    OPENAI_GPT_5_6_LUNA,
    OPENAI_GPT_6_SOL,
    OPENAI_GPT_6_LUNA,
    GOOGLE_GEMINI_3_1_PRO_PREVIEW,
    GOOGLE_GEMINI_3_8_FLASH,
    DEEPSEEK_FLASH,
    MOONSHOTAI_KIMI_K3,
    MINIMAX_M3,
    SPACEXAI_GROK_4_7,
];

/// Models on the gateway that do not emit reasoning traces.
pub const NON_REASONING_MODELS: &[&str] = &[];
