//! Single source of truth for the hand-written (non-OpenRouter) model catalog.
//!
//! One `model_id_table!` invocation defines, per variant: provider, canonical id
//! string, the set of strings that parse to the variant, display name, and
//! description. The accessor modules (`as_str`, `display`, `description`,
//! `provider`, `parse`) delegate here instead of each repeating a ~100-arm match.
//!
//! Deliberately NOT in the table:
//! - Generated OpenRouter variants (served by `openrouter_metadata()`).
//! - `Custom(provider, model)` (runtime strings).
//! - Parse preamble rules (ZAI shadow guards, `opencode*/` prefix routing,
//!   OpenRouter fallback) — those stay order-sensitive in `parse.rs`.
//!
//! Rows with `parse: []` are picker-only or prefix-routed:
//! - OpenCode Zen/Go variants parse only via their `opencode*/` prefixes; their
//!   bare ids intentionally resolve to the native variants.
//! - Qwen variants are picker-only; their ids resolve to the native
//!   DeepSeek/ZAI variants.
//!
//! Row order matters only where two rows share a parseable string; rows are kept
//! in enum declaration order, which preserves the legacy resolution (e.g.
//! `gpt-oss-20b` resolves to `OpenAIGptOss20b`, not `LlamaCppGptOss20b`).

use crate::constants::models;
use crate::models::Provider;

use super::ModelId;

macro_rules! model_id_table {
    (
        $(
            $variant:ident {
                provider: $provider:ident,
                id: $id:expr,
                parse: [ $( $parse:expr ),* $(,)? ],
                display: $display:expr,
                description: $description:expr $(,)?
            }
        ),* $(,)?
    ) => {
        impl ModelId {
            /// Canonical id string for table-backed variants; `None` for
            /// OpenRouter and `Custom` variants.
            pub(super) fn table_id(&self) -> Option<&'static str> {
                match self {
                    $( ModelId::$variant => Some($id), )*
                    _ => None,
                }
            }

            /// Display name for table-backed variants.
            pub(super) fn table_display(&self) -> Option<&'static str> {
                match self {
                    $( ModelId::$variant => Some($display), )*
                    _ => None,
                }
            }

            /// Description for table-backed variants.
            pub(super) fn table_description(&self) -> Option<&'static str> {
                match self {
                    $( ModelId::$variant => Some($description), )*
                    _ => None,
                }
            }

            /// Provider for table-backed variants.
            pub(super) fn table_provider(&self) -> Option<Provider> {
                match self {
                    $( ModelId::$variant => Some(Provider::$provider), )*
                    _ => None,
                }
            }

            /// Resolve a bare model string to a table-backed variant.
            /// Checked in row order so shared strings keep legacy resolution.
            pub(super) fn parse_table(s: &str) -> Option<ModelId> {
                $( $( if s == $parse { return Some(ModelId::$variant); } )* )*
                None
            }
        }
    };
}

model_id_table! {
    // Gemini models
    Gemini38Flash {
        provider: Gemini,
        id: models::GEMINI_3_8_FLASH,
        parse: [models::GEMINI_3_8_FLASH],
        display: "Gemini 3.8 Flash",
        description: "Most intelligent Flash for long-horizon SWE, autonomous agents, and complex enterprise workflows with 1M context and tunable thinking (low, medium, high)",
    },
    // OpenAI models
    GPT6Astra {
        provider: OpenAI,
        id: models::openai::GPT_6_ASTRA,
        parse: [models::openai::GPT_6_ASTRA],
        display: "GPT-6 Astra",
        description: "Most capable model for hardest end-to-end work with complex reasoning, coding, computer use, research, and document creation",
    },
    GPT6Sol {
        provider: OpenAI,
        id: models::openai::GPT_6_SOL,
        parse: [models::openai::GPT_6_SOL],
        display: "GPT-6 Sol",
        description: "Cost-efficient high-end model in the GPT-6 series for demanding professional work",
    },
    GPT61Sol {
        provider: OpenAI,
        id: models::openai::GPT_6_1_SOL,
        parse: [models::openai::GPT_6_1_SOL],
        display: "GPT-6.1 Sol",
        description: "Near-Astra performance for complex coding, computer use, and professional work at a lower cost",
    },
    GPT6Luna {
        provider: OpenAI,
        id: models::openai::GPT_6_LUNA,
        parse: [models::openai::GPT_6_LUNA],
        display: "GPT-6 Luna",
        description: "Fast cost-efficient model in the GPT-6 series for high-volume latency-sensitive workloads",
    },
    GPT56Sol {
        provider: OpenAI,
        id: models::openai::GPT_5_6_SOL,
        parse: [models::openai::GPT_5_6_SOL, models::openai::GPT_5_6, models::GPT],
        display: "GPT-5.6 Sol",
        description: "Frontier model for complex professional work in the GPT-5.6 family",
    },
    GPT56Terra {
        provider: OpenAI,
        id: models::openai::GPT_5_6_TERRA,
        parse: [models::openai::GPT_5_6_TERRA],
        display: "GPT-5.6 Terra",
        description: "GPT-5.6 model that balances intelligence and cost",
    },
    GPT56Luna {
        provider: OpenAI,
        id: models::openai::GPT_5_6_LUNA,
        parse: [models::openai::GPT_5_6_LUNA],
        display: "GPT-5.6 Luna",
        description: "GPT-5.6 model optimized for cost-sensitive workloads",
    },
    OpenAIGptOss20b {
        provider: OpenAI,
        id: models::openai::GPT_OSS_20B,
        parse: [models::openai::GPT_OSS_20B],
        display: "GPT-OSS 20B",
        description: "OpenAI's open-source 20B parameter GPT-OSS model using harmony tokenization",
    },
    OpenAIGptOss120b {
        provider: OpenAI,
        id: models::openai::GPT_OSS_120B,
        parse: [models::openai::GPT_OSS_120B],
        display: "GPT-OSS 120B",
        description: "OpenAI's open-source 120B parameter GPT-OSS model using harmony tokenization",
    },
    // Anthropic models
    ClaudeSonnet5 {
        provider: Anthropic,
        id: models::CLAUDE_SONNET_5,
        parse: [models::CLAUDE_SONNET_5],
        display: "Claude Sonnet 5",
        description: "Anthropic's best combination of speed and intelligence with adaptive thinking on by default, 1M context, and new tokenizer",
    },
    ClaudeSonnet55 {
        provider: Anthropic,
        id: models::CLAUDE_SONNET_5_5,
        parse: [models::CLAUDE_SONNET_5_5],
        display: "Claude Sonnet 5.5",
        description: "Latest Anthropic Sonnet with the best combination of speed and intelligence, adaptive thinking on by default, 1M context, 128K output, and `between_tools` as the lowest thinking setting",
    },
    ClaudeHaiku55 {
        provider: Anthropic,
        id: models::CLAUDE_HAIKU_5_5,
        parse: [models::CLAUDE_HAIKU_5_5],
        display: "Claude Haiku 5.5",
        description: "High-volume latency-sensitive work for classification, routing, extraction, and subagents with adaptive thinking, 1M context, 128K output, and default effort medium",
    },
    ClaudeFable5 {
        provider: Anthropic,
        id: models::CLAUDE_FABLE_5,
        parse: [models::CLAUDE_FABLE_5],
        display: "Claude Fable 5",
        description: "Anthropic's most capable widely released model, for the most demanding reasoning and long-horizon agentic work",
    },
    ClaudeFable51 {
        provider: Anthropic,
        id: models::CLAUDE_FABLE_5_1,
        parse: [models::CLAUDE_FABLE_5_1],
        display: "Claude Fable 5.1",
        description: "Successor to Claude Fable 5 for demanding reasoning and long-horizon agentic work, adaptive thinking always on, 1M context, 128K output, cache reads at 1/4 cost",
    },
    ClaudeOpus5 {
        provider: Anthropic,
        id: models::CLAUDE_OPUS_5,
        parse: [models::CLAUDE_OPUS_5],
        display: "Claude Opus 5",
        description: "Anthropic's newest Opus-tier model with 1M context, thinking on by default, and full effort ladder support",
    },
    ClaudeOpus55 {
        provider: Anthropic,
        id: models::CLAUDE_OPUS_5_5,
        parse: [models::CLAUDE_OPUS_5_5],
        display: "Claude Opus 5.5",
        description: "Opus-tier successor for long-running agentic coding and knowledge work, adaptive thinking always on, 1M context, 128K output, default effort medium",
    },
    // GitHub Copilot models
    CopilotAuto {
        provider: Copilot,
        id: models::copilot::AUTO,
        parse: [models::copilot::AUTO],
        display: "GitHub Copilot Auto",
        description: "GitHub Copilot preview provider with automatic model selection via the official Copilot CLI",
    },
    CopilotGPT52Codex {
        provider: Copilot,
        id: models::copilot::GPT_5_CODEX,
        parse: [models::copilot::GPT_5_CODEX],
        display: "GitHub Copilot GPT-5.2 Codex",
        description: "GitHub Copilot GPT-5.2 Codex option for agentic software engineering workflows",
    },
    CopilotGPT51CodexMax {
        provider: Copilot,
        id: models::copilot::GPT_5_1_CODEX_MAX,
        parse: [models::copilot::GPT_5_1_CODEX_MAX],
        display: "GitHub Copilot GPT-5.1 Codex Max",
        description: "GitHub Copilot GPT-5.1 Codex Max option for longer-running engineering tasks",
    },
    CopilotGPT54 {
        provider: Copilot,
        id: models::copilot::GPT_5_6_SOL,
        parse: [models::copilot::GPT_5_6_SOL],
        display: "GitHub Copilot GPT-5.4",
        description: "GitHub Copilot GPT-5.4 option for complex professional work and long context",
    },
    CopilotGPT54Mini {
        provider: Copilot,
        id: models::copilot::GPT_5_6_LUNA,
        parse: [models::copilot::GPT_5_6_LUNA],
        display: "GitHub Copilot GPT-5.4 Mini",
        description: "GitHub Copilot GPT-5.4 Mini option for faster, lighter-weight tasks",
    },
    // DeepSeek models
    DeepSeekFlash {
        provider: DeepSeek,
        id: models::deepseek::DEEPSEEK_FLASH,
        parse: [models::deepseek::DEEPSEEK_FLASH],
        display: "DeepSeek V4.1 Flash",
        description: "Latest flash model with improved reasoning, efficiency, and agent capabilities",
    },
    // Official Meta AI models
    MetaMuseSpark11 {
        provider: Meta,
        id: models::meta::MUSE_SPARK_1_1,
        parse: [models::meta::MUSE_SPARK_1_1],
        display: "Muse Spark 1.1 (Meta AI)",
        description: "Official Meta AI Muse Spark 1.1 Standard-tier model with always-on reasoning and long context",
    },
    MetaMuseSpark13 {
        provider: Meta,
        id: models::meta::MUSE_SPARK_1_3,
        parse: [models::meta::MUSE_SPARK_1_3],
        display: "Muse Spark 1.3 (Meta AI)",
        description: "Official Meta AI Muse Spark 1.3 Standard-tier flagship tuned for agentic workflows with always-on reasoning and long context",
    },
    MetaMuseSpark13Contributor {
        provider: Meta,
        id: models::meta::MUSE_SPARK_1_3_CONTRIBUTOR,
        parse: [models::meta::MUSE_SPARK_1_3_CONTRIBUTOR],
        display: "Muse Spark 1.3 Contributor (Meta AI)",
        description: "Official Meta AI Muse Spark 1.3 Contributor-tier variant with always-on reasoning and long context",
    },
    // NVIDIA NIM models
    NvidiaNemotron3Ultra550bA55b {
        provider: NVIDIA,
        id: models::nvidia::NEMOTRON_3_ULTRA_550B_A55B,
        parse: [models::nvidia::NEMOTRON_3_ULTRA_550B_A55B],
        display: "Nemotron 3 Ultra (NVIDIA)",
        description: "NVIDIA's flagship Nemotron 3 Ultra model for long-context agentic reasoning, coding, planning, and tool use via NVIDIA NIM",
    },
    NvidiaNemotron3Super120bA12b {
        provider: NVIDIA,
        id: models::nvidia::NEMOTRON_3_SUPER_120B_A12B,
        parse: [models::nvidia::NEMOTRON_3_SUPER_120B_A12B],
        display: "Nemotron 3 Super (NVIDIA)",
        description: "Efficient NVIDIA Nemotron 3 Super model for long-context reasoning, agentic workflows, and tool use via NVIDIA NIM",
    },
    NvidiaNemotron3Nano30bA3b {
        provider: NVIDIA,
        id: models::nvidia::NEMOTRON_3_NANO_30B_A3B,
        parse: [models::nvidia::NEMOTRON_3_NANO_30B_A3B],
        display: "Nemotron 3 Nano (NVIDIA)",
        description: "Efficient NVIDIA Nemotron 3 Nano model for coding, reasoning, instruction following, and tool use via NVIDIA NIM",
    },
    // Merge Gateway routes
    MergeGatewayDefaultRouting {
        provider: MergeGateway,
        id: models::merge_gateway::DEFAULT_ROUTING,
        parse: [models::merge_gateway::DEFAULT_ROUTING],
        display: "Default Routing (Merge Gateway)",
        description: "Merge Gateway's automatic route selection across configured model vendors",
    },
    MergeGatewayAnthropicClaudeOpus5 {
        provider: MergeGateway,
        id: models::merge_gateway::ANTHROPIC_CLAUDE_OPUS_5,
        parse: [models::merge_gateway::ANTHROPIC_CLAUDE_OPUS_5],
        display: "Claude Opus 5 (Merge Gateway)",
        description: "Anthropic Claude Opus 5 accessed through Merge Gateway's OpenAI-compatible endpoint",
    },
    MergeGatewayAnthropicClaudeOpus55 {
        provider: MergeGateway,
        id: models::merge_gateway::ANTHROPIC_CLAUDE_OPUS_5_5,
        parse: [models::merge_gateway::ANTHROPIC_CLAUDE_OPUS_5_5],
        display: "Claude Opus 5.5 (Merge Gateway)",
        description: "Anthropic Claude Opus 5.5 accessed through Merge Gateway's OpenAI-compatible endpoint",
    },
    MergeGatewayAnthropicClaudeSonnet5 {
        provider: MergeGateway,
        id: models::merge_gateway::ANTHROPIC_CLAUDE_SONNET_5,
        parse: [models::merge_gateway::ANTHROPIC_CLAUDE_SONNET_5],
        display: "Claude Sonnet 5 (Merge Gateway)",
        description: "Anthropic Claude Sonnet 5 accessed through Merge Gateway's OpenAI-compatible endpoint",
    },
    MergeGatewayAnthropicClaudeSonnet55 {
        provider: MergeGateway,
        id: models::merge_gateway::ANTHROPIC_CLAUDE_SONNET_5_5,
        parse: [models::merge_gateway::ANTHROPIC_CLAUDE_SONNET_5_5],
        display: "Claude Sonnet 5.5 (Merge Gateway)",
        description: "Anthropic Claude Sonnet 5.5 accessed through Merge Gateway's OpenAI-compatible endpoint",
    },
    MergeGatewayDeepseekFlash {
        provider: MergeGateway,
        id: models::merge_gateway::DEEPSEEK_FLASH,
        parse: [models::merge_gateway::DEEPSEEK_FLASH],
        display: "DeepSeek V4.1 Flash (Merge Gateway)",
        description: "DeepSeek V4.1 Flash accessed through Merge Gateway's OpenAI-compatible endpoint",
    },
    MergeGatewayXaiGrok46 {
        provider: MergeGateway,
        id: models::merge_gateway::XAI_GROK_4_6,
        parse: [models::merge_gateway::XAI_GROK_4_6],
        display: "Grok 4.6 (Merge Gateway)",
        description: "xAI Grok 4.6 accessed through Merge Gateway's OpenAI-compatible endpoint",
    },
    MergeGatewayXaiGrok47 {
        provider: MergeGateway,
        id: models::merge_gateway::XAI_GROK_4_7,
        parse: [models::merge_gateway::XAI_GROK_4_7],
        display: "Grok 4.7 (Merge Gateway)",
        description: "xAI Grok 4.7 accessed through Merge Gateway's OpenAI-compatible endpoint",
    },
    MergeGatewayMinimaxH3 {
        provider: MergeGateway,
        id: models::merge_gateway::MINIMAX_H3,
        parse: [models::merge_gateway::MINIMAX_H3],
        display: "MiniMax H3 (Merge Gateway)",
        description: "MiniMax H3 accessed through Merge Gateway's OpenAI-compatible endpoint",
    },
    MergeGatewayMoonshotKimiK3 {
        provider: MergeGateway,
        id: models::merge_gateway::MOONSHOT_KIMI_K3,
        parse: [models::merge_gateway::MOONSHOT_KIMI_K3],
        display: "Kimi K3 (Merge Gateway)",
        description: "Moonshot Kimi K3 accessed through Merge Gateway's OpenAI-compatible endpoint",
    },
    MergeGatewayThinkingMachinesInkling {
        provider: MergeGateway,
        id: models::merge_gateway::THINKINGMACHINES_INKLING,
        parse: [models::merge_gateway::THINKINGMACHINES_INKLING],
        display: "Inkling (Merge Gateway)",
        description: "Thinking Machines Inkling accessed through Merge Gateway's OpenAI-compatible endpoint",
    },
    MergeGatewayZaiGlm53Flash {
        provider: MergeGateway,
        id: models::merge_gateway::ZAI_GLM_5_3_FLASH,
        parse: [models::merge_gateway::ZAI_GLM_5_3_FLASH],
        display: "GLM-5.3 Flash (Merge Gateway)",
        description: "Z.AI GLM-5.3 Flash efficient multimodal model via Merge Gateway",
    },
    MergeGatewayZaiGlm53Flashx {
        provider: MergeGateway,
        id: models::merge_gateway::ZAI_GLM_5_3_FLASHX,
        parse: [models::merge_gateway::ZAI_GLM_5_3_FLASHX],
        display: "GLM-5.3 FlashX (Merge Gateway)",
        description: "Z.AI GLM-5.3 FlashX high-speed efficient multimodal model via Merge Gateway",
    },
    MergeGatewayOpenAIGpt56Luna {
        provider: MergeGateway,
        id: models::merge_gateway::OPENAI_GPT_5_6_LUNA,
        parse: [models::merge_gateway::OPENAI_GPT_5_6_LUNA],
        display: "GPT-5.6 Luna (Merge Gateway)",
        description: "OpenAI GPT-5.6 Luna accessed through Merge Gateway's OpenAI-compatible endpoint",
    },
    MergeGatewayOpenAIGpt56Sol {
        provider: MergeGateway,
        id: models::merge_gateway::OPENAI_GPT_5_6_SOL,
        parse: [models::merge_gateway::OPENAI_GPT_5_6_SOL],
        display: "GPT-5.6 Sol (Merge Gateway)",
        description: "OpenAI GPT-5.6 Sol accessed through Merge Gateway's OpenAI-compatible endpoint",
    },
    MergeGatewayOpenAIGpt56Terra {
        provider: MergeGateway,
        id: models::merge_gateway::OPENAI_GPT_5_6_TERRA,
        parse: [models::merge_gateway::OPENAI_GPT_5_6_TERRA],
        display: "GPT-5.6 Terra (Merge Gateway)",
        description: "OpenAI GPT-5.6 Terra accessed through Merge Gateway's OpenAI-compatible endpoint",
    },
    MergeGatewayOpenAIGpt6Astra {
        provider: MergeGateway,
        id: models::merge_gateway::OPENAI_GPT_6_ASTRA,
        parse: [models::merge_gateway::OPENAI_GPT_6_ASTRA],
        display: "GPT-6 Astra (Merge Gateway)",
        description: "OpenAI GPT-6 Astra accessed through Merge Gateway's OpenAI-compatible endpoint",
    },
    MergeGatewayOpenAIGpt6Sol {
        provider: MergeGateway,
        id: models::merge_gateway::OPENAI_GPT_6_SOL,
        parse: [models::merge_gateway::OPENAI_GPT_6_SOL],
        display: "GPT-6 Sol (Merge Gateway)",
        description: "OpenAI GPT-6 Sol accessed through Merge Gateway's OpenAI-compatible endpoint",
    },
    MergeGatewayOpenAIGpt61Sol {
        provider: MergeGateway,
        id: models::merge_gateway::OPENAI_GPT_6_1_SOL,
        parse: [models::merge_gateway::OPENAI_GPT_6_1_SOL],
        display: "GPT-6.1 Sol (Merge Gateway)",
        description: "OpenAI GPT-6.1 Sol accessed through Merge Gateway's OpenAI-compatible endpoint",
    },
    MergeGatewayOpenAIGpt6Luna {
        provider: MergeGateway,
        id: models::merge_gateway::OPENAI_GPT_6_LUNA,
        parse: [models::merge_gateway::OPENAI_GPT_6_LUNA],
        display: "GPT-6 Luna (Merge Gateway)",
        description: "OpenAI GPT-6 Luna accessed through Merge Gateway's OpenAI-compatible endpoint",
    },
    MergeGatewayGoogleGemini38Flash {
        provider: MergeGateway,
        id: models::merge_gateway::GOOGLE_GEMINI_3_8_FLASH,
        parse: [models::merge_gateway::GOOGLE_GEMINI_3_8_FLASH],
        display: "Gemini 3.8 Flash (Merge Gateway)",
        description: "Google Gemini 3.8 Flash accessed through Merge Gateway's OpenAI-compatible endpoint",
    },
    MergeGatewayAnthropicClaudeHaiku4520251001 {
        provider: MergeGateway,
        id: models::merge_gateway::ANTHROPIC_CLAUDE_HAIKU_4_5_20251001,
        parse: [models::merge_gateway::ANTHROPIC_CLAUDE_HAIKU_4_5_20251001],
        display: "Claude Haiku 4.5 (Merge Gateway)",
        description: "Anthropic Claude Haiku 4.5 fast, cost-efficient model accessed through Merge Gateway",
    },
    MergeGatewayAnthropicClaudeHaiku55 {
        provider: MergeGateway,
        id: models::merge_gateway::ANTHROPIC_CLAUDE_HAIKU_5_5,
        parse: [models::merge_gateway::ANTHROPIC_CLAUDE_HAIKU_5_5],
        display: "Claude Haiku 5.5 (Merge Gateway)",
        description: "Anthropic Claude Haiku 5.5 fast, cost-efficient model accessed through Merge Gateway (reasoning not forwarded: no joint reasoning+tools vendor yet)",
    },
    MergeGatewayAnthropicClaudeFable51 {
        provider: MergeGateway,
        id: models::merge_gateway::ANTHROPIC_CLAUDE_FABLE_5_1,
        parse: [models::merge_gateway::ANTHROPIC_CLAUDE_FABLE_5_1],
        display: "Claude Fable 5.1 (Merge Gateway)",
        description: "Anthropic Claude Fable 5.1 accessed through Merge Gateway's OpenAI-compatible endpoint",
    },
    MergeGatewayXiaomimimoMimoV26Pro {
        provider: MergeGateway,
        id: models::merge_gateway::XIAOMIMIMO_MIMO_V2_6_PRO,
        parse: [models::merge_gateway::XIAOMIMIMO_MIMO_V2_6_PRO],
        display: "MiMo V2.6 Pro (Merge Gateway)",
        description: "Xiaomi MiMo V2.6 Pro flagship reasoning model accessed through Merge Gateway",
    },
    MergeGatewayXiaomimimoMimoV26Flash {
        provider: MergeGateway,
        id: models::merge_gateway::XIAOMIMIMO_MIMO_V2_6_FLASH,
        parse: [models::merge_gateway::XIAOMIMIMO_MIMO_V2_6_FLASH],
        display: "MiMo V2.6 Flash (Merge Gateway)",
        description: "Xiaomi MiMo V2.6 Flash efficient reasoning model accessed through Merge Gateway",
    },
    MergeGatewayMistralLarge4 {
        provider: MergeGateway,
        id: models::merge_gateway::MISTRAL_LARGE_4,
        parse: [models::merge_gateway::MISTRAL_LARGE_4],
        display: "Mistral Large 4 (Merge Gateway)",
        description: "Mistral Large 4 open-weight MoE flagship with 1M context accessed through Merge Gateway",
    },
    // Mistral models
    MistralLarge3 {
        provider: Mistral,
        id: models::mistral::MISTRAL_LARGE_3,
        parse: [models::mistral::MISTRAL_LARGE_3],
        display: "Mistral Large 3",
        description: "State-of-the-art open-weight general-purpose multimodal model with Mixture-of-Experts architecture",
    },
    MistralLarge4 {
        provider: Mistral,
        id: models::mistral::MISTRAL_LARGE_4,
        parse: [models::mistral::MISTRAL_LARGE_4, "mistral-large-2610", "mistral-large-4-0"],
        display: "Mistral Large 4",
        description: "Open-weight MoE flagship (49B active / 1.05T total) with 1M context and multimodal support",
    },
    // Hugging Face models
    HuggingFaceOpenAIGptOss20b {
        provider: HuggingFace,
        id: models::huggingface::OPENAI_GPT_OSS_20B,
        parse: [models::huggingface::OPENAI_GPT_OSS_20B],
        display: "GPT-OSS 20B (HF)",
        description: "OpenAI GPT-OSS 20B via Hugging Face router",
    },
    HuggingFaceOpenAIGptOss120b {
        provider: HuggingFace,
        id: models::huggingface::OPENAI_GPT_OSS_120B,
        parse: [models::huggingface::OPENAI_GPT_OSS_120B],
        display: "GPT-OSS 120B (HF)",
        description: "OpenAI GPT-OSS 120B via Hugging Face router",
    },
    HuggingFaceGlm53FlashTogether {
        provider: HuggingFace,
        id: models::huggingface::ZAI_GLM_5_3_FLASH_TOGETHER,
        parse: [models::huggingface::ZAI_GLM_5_3_FLASH_TOGETHER],
        display: "GLM-5.3 Flash (Together)",
        description: "Z.ai GLM-5.3 Flash via Together inference provider on HuggingFace router. Efficient multimodal model with hybrid sparse+linear attention (320B/18B, 1M context, native vision).",
    },
    HuggingFaceGlm53Together {
        provider: HuggingFace,
        id: models::huggingface::ZAI_GLM_5_3_TOGETHER,
        parse: [models::huggingface::ZAI_GLM_5_3_TOGETHER],
        display: "GLM-5.3 (Together)",
        description: "Z.ai GLM-5.3 via Together inference provider on HuggingFace router. Frontier coding model with 1M context.",
    },
    HuggingFaceKimiK3Together {
        provider: HuggingFace,
        id: models::huggingface::KIMI_K3_TOGETHER,
        parse: [models::huggingface::KIMI_K3_TOGETHER],
        display: "Kimi K3 (Together)",
        description: "Kimi K3 2.8T flagship with 1M context and native vision via Together inference provider on HuggingFace router.",
    },
    HuggingFaceMinimaxM3Novita {
        provider: HuggingFace,
        id: models::huggingface::MINIMAX_M3_NOVITA,
        parse: [models::huggingface::MINIMAX_M3_NOVITA],
        display: "MiniMax-M3 (Novita)",
        description: "MiniMax-M3 model via Novita inference provider on HuggingFace router. Frontier multimodal coding model with 1M context window.",
    },
    // StepFun models
    StepFun37Flash {
        provider: StepFun,
        id: models::stepfun::STEP_3_7_FLASH,
        parse: [models::stepfun::STEP_3_7_FLASH],
        display: "Step 3.7 Flash",
        description: "StepFun's flagship multimodal reasoning model with 256K context, native image/video input, and tool calling.",
    },
    StepFun5Preview {
        provider: StepFun,
        id: models::stepfun::STEP_5_PREVIEW,
        parse: [models::stepfun::STEP_5_PREVIEW],
        display: "Step 5 Preview",
        description: "StepFun's frontier model for production-scale Agent applications with 1M context, native image/video input, and tool calling.",
    },
    // Evolink gateway models (namespaced; the provider strips the `evolink/` prefix)
    EvolinkGemini31Pro {
         provider: Evolink,
         id: "evolink/gemini-3.1-pro-preview",
         parse: ["evolink/gemini-3.1-pro-preview"],
         display: "Gemini 3.1 Pro (Evolink)",
         description: "Gemini 3.1 Pro served through the Evolink gateway via OpenAI SDK format (direct.evolink.ai).",
     },
     EvolinkMinimaxM3 {
        provider: Evolink,
        id: "evolink/MiniMax-M3",
        parse: ["evolink/MiniMax-M3"],
        display: "MiniMax-M3 (Evolink)",
        description: "MiniMax-M3 frontier multimodal model served through the Evolink gateway (direct.evolink.ai).",
    },
    EvolinkClaudeHaiku45 {
        provider: Evolink,
        id: "evolink/claude-haiku-4-5-20251001",
        parse: ["evolink/claude-haiku-4-5-20251001"],
        display: "Claude Haiku 4.5 (Evolink)",
        description: "Claude Haiku 4.5 served through the Evolink gateway via Anthropic Messages API.",
    },
    // Z.AI models
    ZaiGlm53 {
        provider: ZAI,
        id: models::zai::GLM_5_3,
        parse: [models::zai::GLM_5_3],
        display: "GLM 5.3",
        description: "Z.ai flagship coding model with frontier long-horizon agentic performance and 1M-token context",
    },
    ZaiGlm53Flash {
        provider: ZAI,
        id: models::zai::GLM_5_3_FLASH,
        parse: [models::zai::GLM_5_3_FLASH],
        display: "GLM 5.3 Flash",
        description: "Z.ai efficient multimodal model with hybrid sparse+linear attention, 320B total / 18B active, 1M context and native vision",
    },
    ZaiGlm53Flashx {
        provider: ZAI,
        id: models::zai::GLM_5_3_FLASHX,
        parse: [models::zai::GLM_5_3_FLASHX],
        display: "GLM 5.3 FlashX",
        description: "Z.ai high-speed Flash variant with faster inference (up to 200 tok/s), 320B total / 18B active, 1M context and native vision",
    },
    // MiMo models
    MiMoV26Pro {
        provider: MiMo,
        id: models::mimo::MIMO_V2_6_PRO,
        parse: [models::mimo::MIMO_V2_6_PRO],
        display: "MiMo V2.6 Pro",
        description: "Xiaomi's flagship reasoning model with advanced capabilities (1M context)",
    },
    MiMoV26Flash {
        provider: MiMo,
        id: models::mimo::MIMO_V2_6_FLASH,
        parse: [models::mimo::MIMO_V2_6_FLASH],
        display: "MiMo V2.6 Flash",
        description: "Xiaomi's efficient high-volume model for professional office scenarios (1M context)",
    },
    MiMoV26ProUltraspeed {
        provider: MiMo,
        id: models::mimo::MIMO_V2_6_PRO_ULTRASPEED,
        parse: [models::mimo::MIMO_V2_6_PRO_ULTRASPEED],
        display: "MiMo V2.6 Pro UltraSpeed",
        description: "Xiaomi's fastest flagship variant for real-time production scenarios (1M context)",
    },
    // Moonshot models
    MoonshotKimiK3 {
        provider: Moonshot,
        id: models::moonshot::KIMI_K3,
        parse: [models::moonshot::KIMI_K3],
        display: "Kimi K3 (Moonshot)",
        description: "Kimi K3 - Moonshot.ai's 2.8T parameter flagship with Delta Attention, native vision, 1M context, and always-on deep reasoning",
    },
    // OpenCode Zen models (parse only via the `opencode/`/`opencode-zen/` prefix)
    // OpenCode Go models (parse only via the `opencode-go/` prefix)
    OpenCodeGoGlm53 {
        provider: OpenCodeGo,
        id: models::opencode_go::GLM_5_3,
        parse: [],
        display: "GLM-5.3 (OpenCode Go)",
        description: "GLM-5.3 included with the OpenCode Go subscription for frontier long-horizon coding",
    },
    OpenCodeGoGpt56Luna {
        provider: OpenCodeGo,
        id: models::opencode_go::GPT_5_6_LUNA,
        parse: [],
        display: "GPT-5.6 Luna (OpenCode Go)",
        description: "GPT-5.6 Luna included with the OpenCode Go subscription for cost-sensitive workloads",
    },
    OpenCodeGoKimiK3 {
        provider: OpenCodeGo,
        id: models::opencode_go::KIMI_K3,
        parse: [],
        display: "Kimi K3 (OpenCode Go)",
        description: "Kimi K3 included with the OpenCode Go subscription for frontier agentic coding",
    },
    OpenCodeGoMinimaxM3 {
        provider: OpenCodeGo,
        id: models::opencode_go::MINIMAX_M3,
        parse: [],
        display: "MiniMax-M3 (OpenCode Go)",
        description: "MiniMax-M3 included with the OpenCode Go subscription for frontier agentic coding",
    },
    // Ollama models
    OllamaGptOss20b {
        provider: Ollama,
        id: models::ollama::GPT_OSS_20B,
        parse: [models::ollama::GPT_OSS_20B],
        display: "GPT-OSS 20B (local)",
        description: "Local GPT-OSS 20B deployment served via Ollama with no external API dependency",
    },
    OllamaGptOss20bCloud {
        provider: OllamaCloud,
        id: models::ollama::GPT_OSS_20B_CLOUD,
        parse: [models::ollama::GPT_OSS_20B_CLOUD],
        display: "GPT-OSS 20B (cloud)",
        description: "Cloud-hosted GPT-OSS 20B accessed through Ollama Cloud for efficient reasoning tasks",
    },
    OllamaGptOss120bCloud {
        provider: OllamaCloud,
        id: models::ollama::GPT_OSS_120B_CLOUD,
        parse: [models::ollama::GPT_OSS_120B_CLOUD],
        display: "GPT-OSS 120B (cloud)",
        description: "Cloud-hosted GPT-OSS 120B accessed through Ollama Cloud for larger reasoning tasks",
    },
    OllamaMinimaxM3Cloud {
        provider: OllamaCloud,
        id: models::ollama::MINIMAX_M3_CLOUD,
        parse: [models::ollama::MINIMAX_M3_CLOUD],
        display: "MiniMax-M3 (cloud)",
        description: "Cloud-hosted MiniMax-M3 model served via Ollama Cloud",
    },
    OllamaGlm53Cloud {
        provider: OllamaCloud,
        id: models::ollama::GLM_5_3_CLOUD,
        parse: [models::ollama::GLM_5_3_CLOUD],
        display: "GLM-5.3 (cloud)",
        description: "Cloud-hosted GLM-5.3 flagship model for long-horizon tasks with 1M context via Ollama Cloud",
    },
    OllamaKimiK3Cloud {
        provider: OllamaCloud,
        id: models::ollama::KIMI_K3_CLOUD,
        parse: [models::ollama::KIMI_K3_CLOUD],
        display: "Kimi-K3 (cloud)",
        description: "Cloud-hosted Kimi K3 flagship model with 1M context and native vision via Ollama Cloud",
    },
    OllamaGemma4 {
        provider: Ollama,
        id: models::ollama::GEMMA_4,
        parse: [models::ollama::GEMMA_4],
        display: "Gemma 4",
        description: "Google Gemma 4 model designed for frontier-level reasoning, agentic workflows, coding, and multimodal understanding (128K context).",
    },
    // llama.cpp models
    LlamaCppGemma426bA4b {
        provider: LlamaCpp,
        id: models::llamacpp::GEMMA_4_26B_A4B,
        parse: [models::llamacpp::GEMMA_4_26B_A4B],
        display: "Gemma 4 26B A4B (llama.cpp)",
        description: "Gemma 4 desktop MoE model served through llama.cpp with strong reasoning and fast local inference",
    },
    LlamaCppGemma4E4b {
        provider: LlamaCpp,
        id: models::llamacpp::GEMMA_4_E4B,
        parse: [models::llamacpp::GEMMA_4_E4B],
        display: "Gemma 4 E4B (llama.cpp)",
        description: "Tiny-footprint Gemma 4 local model served through llama.cpp for phones and low-end laptops",
    },
    LlamaCppGptOss20b {
        provider: LlamaCpp,
        id: models::llamacpp::GPT_OSS_20B,
        parse: [models::llamacpp::GPT_OSS_20B],
        display: "GPT-OSS 20B (llama.cpp)",
        description: "OpenAI's open-weight GPT-OSS 20B model served locally through llama.cpp",
    },
    // MiniMax models
    MinimaxM3 {
        provider: Minimax,
        id: models::minimax::MINIMAX_M3,
        parse: [models::minimax::MINIMAX_M3],
        display: "MiniMax-M3",
        description: "Frontier multimodal coding model with 1M context window",
    },
    // xAI models
    XaiGrok46 {
        provider: XAI,
        id: models::xai::GROK_4_6,
        parse: [models::xai::GROK_4_6],
        display: "Grok 4.6",
        description: "xAI's flagship reasoning model with reasoning_effort support (500k context)",
    },
    XaiGrok47 {
        provider: XAI,
        id: models::xai::GROK_4_7,
        parse: [models::xai::GROK_4_7],
        display: "Grok 4.7",
        description: "xAI's flagship reasoning model with reasoning_effort support (500k context)",
    },
    // Vercel AI Gateway models (ids use the gateway's native `vendor/model` format)
    VercelAnthropicClaudeSonnet5 {
        provider: Vercel,
        id: models::vercel::ANTHROPIC_CLAUDE_SONNET_5,
        parse: [models::vercel::ANTHROPIC_CLAUDE_SONNET_5],
        display: "Claude Sonnet 5 (Vercel AI Gateway)",
        description: "Anthropic Claude Sonnet 5 served through the Vercel AI Gateway (ai-gateway.vercel.sh)",
    },
    VercelAnthropicClaudeOpus5 {
        provider: Vercel,
        id: models::vercel::ANTHROPIC_CLAUDE_OPUS_5,
        parse: [models::vercel::ANTHROPIC_CLAUDE_OPUS_5],
        display: "Claude Opus 5 (Vercel AI Gateway)",
        description: "Anthropic Claude Opus 5 flagship model served through the Vercel AI Gateway (ai-gateway.vercel.sh)",
    },
    VercelAnthropicClaudeOpus55 {
        provider: Vercel,
        id: models::vercel::ANTHROPIC_CLAUDE_OPUS_5_5,
        parse: [models::vercel::ANTHROPIC_CLAUDE_OPUS_5_5],
        display: "Claude Opus 5.5 (Vercel AI Gateway)",
        description: "Anthropic Claude Opus 5.5 flagship model served through the Vercel AI Gateway (ai-gateway.vercel.sh)",
    },
    VercelAnthropicClaudeHaiku45 {
        provider: Vercel,
        id: models::vercel::ANTHROPIC_CLAUDE_HAIKU_4_5,
        parse: [models::vercel::ANTHROPIC_CLAUDE_HAIKU_4_5],
        display: "Claude Haiku 4.5 (Vercel AI Gateway)",
        description: "Anthropic Claude Haiku 4.5 fast, cost-efficient model served through the Vercel AI Gateway",
    },
    VercelOpenAiGpt56Sol {
        provider: Vercel,
        id: models::vercel::OPENAI_GPT_5_6_SOL,
        parse: [models::vercel::OPENAI_GPT_5_6_SOL],
        display: "GPT-5.6 Sol (Vercel AI Gateway)",
        description: "OpenAI GPT-5.6 Sol flagship reasoning model served through the Vercel AI Gateway",
    },
    VercelOpenAiGpt6Astra {
        provider: Vercel,
        id: models::vercel::OPENAI_GPT_6_ASTRA,
        parse: [models::vercel::OPENAI_GPT_6_ASTRA],
        display: "GPT-6 Astra (Vercel AI Gateway)",
        description: "OpenAI GPT-6 Astra flagship reasoning model served through the Vercel AI Gateway",
    },
    VercelOpenAiGpt56Luna {
        provider: Vercel,
        id: models::vercel::OPENAI_GPT_5_6_LUNA,
        parse: [models::vercel::OPENAI_GPT_5_6_LUNA],
        display: "GPT-5.6 Luna (Vercel AI Gateway)",
        description: "OpenAI GPT-5.6 Luna cost-efficient reasoning model served through the Vercel AI Gateway",
    },
    VercelGoogleGemini38Flash {
        provider: Vercel,
        id: models::vercel::GOOGLE_GEMINI_3_8_FLASH,
        parse: [models::vercel::GOOGLE_GEMINI_3_8_FLASH],
        display: "Gemini 3.8 Flash (Vercel AI Gateway)",
        description: "Google Gemini 3.8 Flash fast, cost-efficient model served through the Vercel AI Gateway",
    },
    VercelDeepseekFlash {
        provider: Vercel,
        id: models::vercel::DEEPSEEK_FLASH,
        parse: [models::vercel::DEEPSEEK_FLASH],
        display: "DeepSeek V4.1 Flash (Vercel AI Gateway)",
        description: "DeepSeek V4.1 Flash latest flash model served through the Vercel AI Gateway",
    },
    VercelMoonshotaiKimiK3 {
        provider: Vercel,
        id: models::vercel::MOONSHOTAI_KIMI_K3,
        parse: [models::vercel::MOONSHOTAI_KIMI_K3],
        display: "Kimi K3 (Vercel AI Gateway)",
        description: "Moonshot AI Kimi K3 flagship reasoning model served through the Vercel AI Gateway",
    },
    VercelMinimaxM3 {
        provider: Vercel,
        id: models::vercel::MINIMAX_M3,
        parse: [models::vercel::MINIMAX_M3],
        display: "MiniMax M3 (Vercel AI Gateway)",
        description: "MiniMax M3 model served through the Vercel AI Gateway",
    },
    VercelSpacexaiGrok47 {
        provider: Vercel,
        id: models::vercel::SPACEXAI_GROK_4_7,
        parse: [models::vercel::SPACEXAI_GROK_4_7],
        display: "Grok 4.7 (Vercel AI Gateway)",
        description: "xAI Grok 4.7 reasoning model served through the Vercel AI Gateway",
    },
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use crate::constants::models;
    use crate::models::Provider;

    use super::ModelId;

    /// Providers whose model ids intentionally do not round-trip through
    /// `from_str`: their `as_str()` values are bare ids that collide with (and
    /// resolve to) native provider variants. They are reachable only through
    /// picker selection or provider prefixes (`opencode*/`).
    fn round_trip_exempt(model: &ModelId) -> bool {
        matches!(model.provider(), Provider::OpenCodeZen | Provider::OpenCodeGo | Provider::Qwen)
            || *model == ModelId::LlamaCppGptOss20b
            || matches!(
                model,
                ModelId::MergeGatewayAnthropicClaudeOpus5
                    | ModelId::MergeGatewayAnthropicClaudeOpus55
                    | ModelId::MergeGatewayAnthropicClaudeSonnet5
                    | ModelId::MergeGatewayAnthropicClaudeSonnet55
                    | ModelId::MergeGatewayGoogleGemini38Flash
                    | ModelId::MergeGatewayOpenAIGpt6Astra
                    | ModelId::MergeGatewayOpenAIGpt6Sol
                    | ModelId::MergeGatewayOpenAIGpt61Sol
                    | ModelId::MergeGatewayOpenAIGpt6Luna
                    | ModelId::MergeGatewayDeepseekFlash
            )
            || matches!(
                model,
                ModelId::VercelAnthropicClaudeSonnet5
                    | ModelId::VercelAnthropicClaudeOpus5
                    | ModelId::VercelAnthropicClaudeOpus55
                    | ModelId::VercelOpenAiGpt6Astra
                    | ModelId::VercelOpenAiGpt56Sol
                    | ModelId::VercelOpenAiGpt56Luna
                    | ModelId::VercelGoogleGemini38Flash
                    | ModelId::VercelDeepseekFlash
                    | ModelId::VercelMoonshotaiKimiK3
            )
    }

    #[test]
    fn all_models_round_trip_through_from_str() {
        for model in ModelId::all_models() {
            if round_trip_exempt(&model) {
                continue;
            }
            let id = model.as_str();
            let parsed =
                ModelId::from_str(&id).unwrap_or_else(|err| panic!("failed to parse {id} back into {model:?}: {err}"));
            assert_eq!(parsed, model, "round-trip mismatch for id {id}");
        }
    }

    #[test]
    fn every_non_openrouter_model_is_in_the_table() {
        for model in ModelId::all_models() {
            if model.provider() == Provider::OpenRouter {
                continue;
            }
            assert!(model.table_id().is_some(), "{model:?} is missing from the model_id_table! invocation");
        }
    }

    #[test]
    fn parse_aliases_resolve_to_canonical_variants() {
        let cases: &[(&str, ModelId)] = &[
            (models::GPT, ModelId::GPT56Sol),
            (models::openai::GPT_5_6_SOL, ModelId::GPT56Sol),
            (models::CLAUDE_SONNET_5, ModelId::ClaudeSonnet5),
            (models::mistral::MISTRAL_LARGE_4, ModelId::MistralLarge4),
            ("mistral-large-2610", ModelId::MistralLarge4),
            ("mistral-large-4-0", ModelId::MistralLarge4),
        ];
        for (alias, expected) in cases {
            let parsed = ModelId::from_str(alias).unwrap_or_else(|err| panic!("alias {alias} failed to parse: {err}"));
            assert_eq!(&parsed, expected, "alias {alias} resolved incorrectly");
        }
    }

    #[test]
    fn shared_gpt_oss_20b_id_resolves_to_openai() {
        assert_eq!(models::openai::GPT_OSS_20B, models::llamacpp::GPT_OSS_20B);
        assert_eq!(ModelId::from_str(models::llamacpp::GPT_OSS_20B).unwrap(), ModelId::OpenAIGptOss20b);
    }
}
