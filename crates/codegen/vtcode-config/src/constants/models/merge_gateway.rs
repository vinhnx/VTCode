//! Curated Merge Gateway model routes exposed by VT Code.

pub const DEFAULT_ROUTING: &str = "default_routing";
pub const OPENAI_GPT_5_5: &str = "openai/gpt-5.5";
pub const ANTHROPIC_CLAUDE_OPUS_5: &str = "anthropic/claude-opus-5";
pub const ANTHROPIC_CLAUDE_OPUS_5_5: &str = "anthropic/claude-opus-5-5";
pub const ANTHROPIC_CLAUDE_SONNET_5: &str = "anthropic/claude-sonnet-5";
pub const ANTHROPIC_CLAUDE_SONNET_5_5: &str = "anthropic/claude-sonnet-5-5";
pub const GOOGLE_GEMINI_3_6_FLASH: &str = "google/gemini-3.6-flash";
pub const GOOGLE_GEMINI_3_7_FLASH: &str = "google/gemini-3.7-flash";
pub const XAI_GROK_4_6: &str = "xai/grok-4.6";
pub const XAI_GROK_4_7: &str = "xai/grok-4.7";
pub const MINIMAX_H3: &str = "minimax/minimax-h3";
pub const MOONSHOT_KIMI_K3: &str = "moonshot/kimi-k3";
pub const THINKINGMACHINES_INKLING: &str = "thinkingmachines/inkling";
pub const ZAI_GLM_5_3_FLASH: &str = "zai/glm-5.3-flash";
pub const ZAI_GLM_5_3_FLASHX: &str = "zai/glm-5.3-flashx";
pub const OPENAI_GPT_5_6_LUNA: &str = "openai/gpt-5.6-luna";
pub const OPENAI_GPT_5_6_SOL: &str = "openai/gpt-5.6-sol";
pub const OPENAI_GPT_5_6_TERRA: &str = "openai/gpt-5.6-terra";
pub const OPENAI_GPT_6_ASTRA: &str = "openai/gpt-6-astra";
pub const OPENAI_GPT_6_SOL: &str = "openai/gpt-6-sol";
pub const OPENAI_GPT_6_1_SOL: &str = "openai/gpt-6.1-sol";
pub const OPENAI_GPT_6_LUNA: &str = "openai/gpt-6-luna";
pub const GOOGLE_GEMINI_3_8_FLASH: &str = "google/gemini-3.8-flash";
pub const ANTHROPIC_CLAUDE_HAIKU_4_5_20251001: &str = "anthropic/claude-haiku-4-5-20251001";
pub const ANTHROPIC_CLAUDE_HAIKU_5_5: &str = "anthropic/claude-haiku-5-5";
pub const ANTHROPIC_CLAUDE_FABLE_5_1: &str = "anthropic/claude-fable-5-1";
pub const DEEPSEEK_FLASH: &str = "deepseek/deepseek-v4.1-flash";
pub const XIAOMIMIMO_MIMO_V2_6_PRO: &str = "xiaomimimo/mimo-v2.6-pro";
pub const XIAOMIMIMO_MIMO_V2_6_FLASH: &str = "xiaomimimo/mimo-v2.6-flash";
pub const MISTRAL_LARGE_4: &str = "mistral/mistral-large-4-0";

pub const DEFAULT_MODEL: &str = DEFAULT_ROUTING;

/// Curated routes shown in VT Code's model picker. Merge Gateway also accepts
/// other valid `provider/model` identifiers through explicit configuration.
pub const SUPPORTED_MODELS: &[&str] = &[
    DEFAULT_ROUTING,
    OPENAI_GPT_5_5,
    ANTHROPIC_CLAUDE_OPUS_5,
    ANTHROPIC_CLAUDE_OPUS_5_5,
    ANTHROPIC_CLAUDE_SONNET_5,
    ANTHROPIC_CLAUDE_SONNET_5_5,
    ANTHROPIC_CLAUDE_HAIKU_4_5_20251001,
    ANTHROPIC_CLAUDE_HAIKU_5_5,
    ANTHROPIC_CLAUDE_FABLE_5_1,
    GOOGLE_GEMINI_3_6_FLASH,
    GOOGLE_GEMINI_3_7_FLASH,
    GOOGLE_GEMINI_3_8_FLASH,
    DEEPSEEK_FLASH,
    XAI_GROK_4_6,
    XAI_GROK_4_7,
    MINIMAX_H3,
    MOONSHOT_KIMI_K3,
    THINKINGMACHINES_INKLING,
    ZAI_GLM_5_3_FLASH,
    ZAI_GLM_5_3_FLASHX,
    OPENAI_GPT_5_6_LUNA,
    OPENAI_GPT_5_6_SOL,
    OPENAI_GPT_5_6_TERRA,
    OPENAI_GPT_6_ASTRA,
    OPENAI_GPT_6_SOL,
    OPENAI_GPT_6_1_SOL,
    OPENAI_GPT_6_LUNA,
    XIAOMIMIMO_MIMO_V2_6_PRO,
    XIAOMIMIMO_MIMO_V2_6_FLASH,
    MISTRAL_LARGE_4,
];

/// Routes that advertise provider-native `reasoning_effort` controls through
/// Merge's `/v1/models` catalog.
pub const REASONING_EFFORT_ROUTES: &[&str] = &[
    OPENAI_GPT_5_5,
    XAI_GROK_4_6,
    XAI_GROK_4_7,
    MOONSHOT_KIMI_K3,
    ZAI_GLM_5_3_FLASH,
    ZAI_GLM_5_3_FLASHX,
    OPENAI_GPT_5_6_LUNA,
    OPENAI_GPT_5_6_SOL,
    OPENAI_GPT_5_6_TERRA,
    OPENAI_GPT_6_ASTRA,
    OPENAI_GPT_6_SOL,
    OPENAI_GPT_6_1_SOL,
    OPENAI_GPT_6_LUNA,
];

/// Routes that advertise Gateway-controlled `thinking.budget_tokens` controls.
///
/// `xiaomimimo/` routes and `anthropic/claude-haiku-5-5` are intentionally
/// absent: Merge Gateway currently has no vendor serving reasoning jointly
/// with tools for them (a `thinking` payload turns every agentic request
/// into a `capability_unavailable` rejection), so they stay conservative
/// until the route gains one.
pub const THINKING_BUDGET_ROUTES: &[&str] = &[
    ANTHROPIC_CLAUDE_OPUS_5,
    ANTHROPIC_CLAUDE_OPUS_5_5,
    ANTHROPIC_CLAUDE_SONNET_5,
    ANTHROPIC_CLAUDE_SONNET_5_5,
    ANTHROPIC_CLAUDE_FABLE_5_1,
    GOOGLE_GEMINI_3_6_FLASH,
    GOOGLE_GEMINI_3_7_FLASH,
    GOOGLE_GEMINI_3_8_FLASH,
    DEEPSEEK_FLASH,
    MINIMAX_H3,
    THINKINGMACHINES_INKLING,
];

/// Curated Merge Gateway routes that support reasoning. Reasoning is controlled
/// per route: either a provider-native `reasoning_effort` or a Gateway-managed
/// thinking budget. `anthropic/claude-haiku-5-5` is intentionally absent: the
/// gateway has no vendor serving reasoning jointly with tools for it yet.
pub const REASONING_MODELS: &[&str] = &[
    ANTHROPIC_CLAUDE_OPUS_5,
    ANTHROPIC_CLAUDE_OPUS_5_5,
    ANTHROPIC_CLAUDE_SONNET_5,
    ANTHROPIC_CLAUDE_SONNET_5_5,
    ANTHROPIC_CLAUDE_FABLE_5_1,
    GOOGLE_GEMINI_3_8_FLASH,
    DEEPSEEK_FLASH,
    XAI_GROK_4_6,
    XAI_GROK_4_7,
    MINIMAX_H3,
    MOONSHOT_KIMI_K3,
    THINKINGMACHINES_INKLING,
    ZAI_GLM_5_3_FLASH,
    ZAI_GLM_5_3_FLASHX,
    OPENAI_GPT_5_6_LUNA,
    OPENAI_GPT_5_6_SOL,
    OPENAI_GPT_5_6_TERRA,
    OPENAI_GPT_6_ASTRA,
    OPENAI_GPT_6_SOL,
    OPENAI_GPT_6_1_SOL,
    OPENAI_GPT_6_LUNA,
];

/// Returns true when the route exposes a provider-native `reasoning_effort`
/// control through Merge Gateway. Explicit `provider/model` route identifiers
/// follow the same prefix convention as the curated routes.
pub fn route_uses_reasoning_effort(model: &str) -> bool {
    let model = model.trim();
    model.starts_with("openai/")
        || model.starts_with("xai/")
        || model.starts_with("moonshot/")
        || model.starts_with("zai/")
}

/// Returns true when the route exposes a Gateway-managed `thinking.budget_tokens`
/// control. Explicit `provider/model` route identifiers follow the same prefix
/// convention as the curated routes.
///
/// `anthropic/claude-haiku-5-5` is excluded: the gateway has no vendor serving
/// reasoning jointly with tools for it yet (see `THINKING_BUDGET_ROUTES` and
/// `merge_reasoning_control_for_model`).
pub fn route_uses_thinking_budget(model: &str) -> bool {
    let model = model.trim();
    if model == ANTHROPIC_CLAUDE_HAIKU_5_5 {
        return false;
    }
    model.starts_with("anthropic/")
        || model.starts_with("google/gemini-")
        || model.starts_with("deepseek/")
        || model.starts_with("minimax/")
        || model.starts_with("thinkingmachines/")
}

/// Returns true when the route supports configurable reasoning through Merge
/// Gateway. Unclassified routes (including `default_routing`) stay conservative.
pub fn route_supports_reasoning(model: &str) -> bool {
    route_uses_reasoning_effort(model) || route_uses_thinking_budget(model)
}
