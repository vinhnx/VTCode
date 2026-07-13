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
    Gemini31ProPreview {
        provider: Gemini,
        id: models::GEMINI_3_1_PRO_PREVIEW,
        parse: [models::GEMINI_3_1_PRO_PREVIEW],
        display: "Gemini 3.1 Pro Preview",
        description: "Latest Gemini 3.1 Pro flagship model with improved thinking, efficiency, and factual consistency",
    },
    Gemini31ProPreviewCustomTools {
        provider: Gemini,
        id: models::GEMINI_3_1_PRO_PREVIEW_CUSTOMTOOLS,
        parse: [models::GEMINI_3_1_PRO_PREVIEW_CUSTOMTOOLS],
        display: "Gemini 3.1 Pro Preview (Custom Tools)",
        description: "Gemini 3.1 Pro variant optimized for agentic workflows using custom tools and bash",
    },
    Gemini35Flash {
        provider: Gemini,
        id: models::GEMINI_3_5_FLASH,
        parse: [models::GEMINI_3_5_FLASH, models::GEMINI_3_FLASH_PREVIEW],
        display: "Gemini 3.5 Flash",
        description: "High-efficiency frontier model for fast inference with excellent quality-to-speed balance",
    },
    // OpenAI models
    GPT56Sol {
        provider: OpenAI,
        id: models::openai::GPT_5_6_SOL,
        parse: [models::openai::GPT_5_6_SOL, models::openai::GPT_5_6],
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
    GPT55 {
        provider: OpenAI,
        id: models::openai::GPT_5_5,
        parse: [models::openai::GPT_5_5, models::openai::GPT_5_5_DATED],
        display: "GPT-5.5",
        description: "Next-gen OpenAI model with frontier reasoning and long context (2026-04-23 dated release)",
    },
    GPT54 {
        provider: OpenAI,
        id: models::GPT_5_4,
        parse: [models::GPT_5_4, models::GPT],
        display: "GPT-5.4",
        description: "Mainline frontier GPT model for general-purpose work, coding, long context, and multi-step agents",
    },
    GPT54Pro {
        provider: OpenAI,
        id: models::GPT_5_4_PRO,
        parse: [models::GPT_5_4_PRO],
        display: "GPT-5.4 Pro",
        description: "Higher-compute GPT-5.4 variant for tougher problems with deeper reasoning",
    },
    GPT54Nano {
        provider: OpenAI,
        id: models::openai::GPT_5_4_NANO,
        parse: [models::openai::GPT_5_4_NANO],
        display: "GPT-5.4 Nano",
        description: "Lightweight GPT-5.4 variant optimized for speed and cost-efficiency",
    },
    GPT54Mini {
        provider: OpenAI,
        id: models::openai::GPT_5_4_MINI,
        parse: [models::openai::GPT_5_4_MINI],
        display: "GPT-5.4 Mini",
        description: "Compact GPT-5.4 variant for cost-effective tasks with reduced reasoning overhead",
    },
    GPT53Codex {
        provider: OpenAI,
        id: models::openai::GPT_5_3_CODEX,
        parse: [models::openai::GPT_5_3_CODEX],
        display: "GPT-5.3 Codex",
        description: "GPT-5.3 variant optimized for agentic coding tasks with reasoning effort support (low, medium, high, xhigh)",
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
    ClaudeFable5 {
        provider: Anthropic,
        id: models::CLAUDE_FABLE_5,
        parse: [models::CLAUDE_FABLE_5],
        display: "Claude Fable 5",
        description: "Anthropic's most capable widely released model, for the most demanding reasoning and long-horizon agentic work",
    },
    ClaudeMythos5 {
        provider: Anthropic,
        id: models::CLAUDE_MYTHOS_5,
        parse: [models::CLAUDE_MYTHOS_5],
        display: "Claude Mythos 5",
        description: "Shares Claude Fable 5's capabilities without safety classifiers. Limited availability through Project Glasswing",
    },
    ClaudeOpus48 {
        provider: Anthropic,
        id: models::CLAUDE_OPUS_4_8,
        parse: [models::CLAUDE_OPUS_4_8],
        display: "Claude Opus 4.8",
        description: "Anthropic's most capable model for complex reasoning, long-horizon agentic coding, and high-autonomy work",
    },
    ClaudeSonnet46 {
        provider: Anthropic,
        id: models::CLAUDE_SONNET_4_6,
        parse: [models::CLAUDE_SONNET_4_6],
        display: "Claude Sonnet 4.6",
        description: "Balanced flagship model for coding with budgeted thinking in VT Code's current Anthropic rollout",
    },
    ClaudeHaiku45 {
        provider: Anthropic,
        id: models::CLAUDE_HAIKU_4_5,
        parse: [models::CLAUDE_HAIKU_4_5, models::CLAUDE_HAIKU_4_5_20251001],
        display: "Claude Haiku 4.5",
        description: "Latest efficient Anthropic model optimized for low-latency agent workflows",
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
        id: models::copilot::GPT_5_2_CODEX,
        parse: [models::copilot::GPT_5_2_CODEX],
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
        id: models::copilot::GPT_5_4,
        parse: [models::copilot::GPT_5_4],
        display: "GitHub Copilot GPT-5.4",
        description: "GitHub Copilot GPT-5.4 option for complex professional work and long context",
    },
    CopilotGPT54Mini {
        provider: Copilot,
        id: models::copilot::GPT_5_4_MINI,
        parse: [models::copilot::GPT_5_4_MINI],
        display: "GitHub Copilot GPT-5.4 Mini",
        description: "GitHub Copilot GPT-5.4 Mini option for faster, lighter-weight tasks",
    },
    CopilotClaudeSonnet46 {
        provider: Copilot,
        id: models::copilot::CLAUDE_SONNET_4_6,
        parse: [models::copilot::CLAUDE_SONNET_4_6],
        display: "GitHub Copilot Claude Sonnet 4.6",
        description: "GitHub Copilot Claude Sonnet 4.6 option for balanced coding and reasoning work",
    },
    // DeepSeek models
    DeepSeekV4Pro {
        provider: DeepSeek,
        id: models::deepseek::DEEPSEEK_V4_PRO,
        parse: [models::deepseek::DEEPSEEK_V4_PRO],
        display: "DeepSeek V4 Pro",
        description: "High-performance reasoning model with advanced thinking capabilities",
    },
    DeepSeekV4Flash {
        provider: DeepSeek,
        id: models::deepseek::DEEPSEEK_V4_FLASH,
        parse: [models::deepseek::DEEPSEEK_V4_FLASH],
        display: "DeepSeek V4 Flash",
        description: "Fast inference model for cost-effective reasoning",
    },
    // Mistral models
    MistralLarge3 {
        provider: Mistral,
        id: models::mistral::MISTRAL_LARGE_3,
        parse: [models::mistral::MISTRAL_LARGE_3],
        display: "Mistral Large 3",
        description: "State-of-the-art open-weight general-purpose multimodal model with Mixture-of-Experts architecture",
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
    HuggingFaceGlm51ZaiOrg {
        provider: HuggingFace,
        id: models::huggingface::ZAI_GLM_5_1_ZAI_ORG,
        parse: [models::huggingface::ZAI_GLM_5_1_ZAI_ORG],
        display: "GLM-5.1 (zai-org)",
        description: "Z.ai GLM-5.1 model via zai-org inference provider on HuggingFace router.",
    },
    HuggingFaceGlm52Novita {
        provider: HuggingFace,
        id: models::huggingface::ZAI_GLM_5_2_NOVITA,
        parse: [models::huggingface::ZAI_GLM_5_2_NOVITA],
        display: "GLM-5.2 (Novita)",
        description: "Z.ai GLM-5.2 flagship model via Novita inference provider on HuggingFace router. 1M context for long-horizon tasks.",
    },
    HuggingFaceKimiK26Novita {
        provider: HuggingFace,
        id: models::huggingface::KIMI_K2_6_NOVITA,
        parse: [models::huggingface::KIMI_K2_6_NOVITA],
        display: "Kimi K2.6 (Novita)",
        description: "Kimi K2.6 via Novita inference provider on HuggingFace router.",
    },
    HuggingFaceDeepseekV4FlashNovita {
        provider: HuggingFace,
        id: models::huggingface::DEEPSEEK_V4_FLASH_NOVITA,
        parse: [models::huggingface::DEEPSEEK_V4_FLASH_NOVITA],
        display: "DeepSeek V4 Flash (Novita)",
        description: "DeepSeek V4 Flash via Novita inference provider on HuggingFace router. Fast inference model for cost-effective reasoning (1M context, 158B params).",
    },
    HuggingFaceDeepseekV4ProTogether {
        provider: HuggingFace,
        id: models::huggingface::DEEPSEEK_V4_PRO_TOGETHER,
        parse: [models::huggingface::DEEPSEEK_V4_PRO_TOGETHER],
        display: "DeepSeek V4 Pro (Together)",
        description: "DeepSeek V4 Pro via Together inference provider on HuggingFace router. High-performance reasoning model with advanced thinking capabilities (1M context, 1.6T params).",
    },
    HuggingFaceStep35Flash {
        provider: HuggingFace,
        id: models::huggingface::STEP_3_5_FLASH,
        parse: [
            models::huggingface::STEP_3_5_FLASH,
            models::huggingface::STEP_3_5_FLASH_BASE,
            models::huggingface::STEP_3_5_FLASH_LEGACY_FASTEST,
        ],
        display: "Step 3.5 Flash (HF)",
        description: "Step 3.5 Flash flagship model via HuggingFace router (featherless-ai provider). Supports streaming and fast inference.",
    },
    HuggingFaceGlm51Deepinfra {
        provider: HuggingFace,
        id: models::huggingface::ZAI_GLM_5_1_DEEPINFRA,
        parse: [models::huggingface::ZAI_GLM_5_1_DEEPINFRA],
        display: "GLM-5.1 (DeepInfra)",
        description: "Z.ai GLM-5.1 model via DeepInfra inference provider on HuggingFace router.",
    },
    HuggingFaceMinimaxM27Novita {
        provider: HuggingFace,
        id: models::huggingface::MINIMAX_M2_7_NOVITA,
        parse: [models::huggingface::MINIMAX_M2_7_NOVITA],
        display: "MiniMax-M2.7 (Novita)",
        description: "MiniMax-M2.7 model via Novita inference provider on HuggingFace router. Recursive self-improvement with enhanced reasoning.",
    },
    HuggingFaceMinimaxM3Novita {
        provider: HuggingFace,
        id: models::huggingface::MINIMAX_M3_NOVITA,
        parse: [models::huggingface::MINIMAX_M3_NOVITA],
        display: "MiniMax-M3 (Novita)",
        description: "MiniMax-M3 model via Novita inference provider on HuggingFace router. Frontier multimodal coding model with 1M context window.",
    },
    HuggingFaceDeepseekV4ProNovita {
        provider: HuggingFace,
        id: models::huggingface::DEEPSEEK_V4_PRO_NOVITA,
        parse: [models::huggingface::DEEPSEEK_V4_PRO_NOVITA],
        display: "DeepSeek V4 Pro (Novita)",
        description: "DeepSeek V4 Pro via Novita inference provider on HuggingFace router. High-performance reasoning model with advanced thinking capabilities (1M context, 1.6T params).",
    },
    // StepFun models
    StepFun37Flash {
        provider: StepFun,
        id: models::stepfun::STEP_3_7_FLASH,
        parse: [models::stepfun::STEP_3_7_FLASH],
        display: "Step 3.7 Flash",
        description: "StepFun's flagship multimodal reasoning model with 256K context, native image/video input, and tool calling.",
    },
    // Evolink gateway models (namespaced; the provider strips the `evolink/` prefix)
    EvolinkGpt52 {
        provider: Evolink,
        id: "evolink/gpt-5.2",
        parse: ["evolink/gpt-5.2"],
        display: "GPT-5.2 (Evolink)",
        description: "GPT-5.2 served through the Evolink OpenAI-compatible gateway (direct.evolink.ai).",
    },
    EvolinkGpt55 {
        provider: Evolink,
        id: "evolink/gpt-5.5",
        parse: ["evolink/gpt-5.5"],
        display: "GPT-5.5 (Evolink)",
        description: "GPT-5.5 flagship model served through the Evolink OpenAI-compatible gateway (direct.evolink.ai).",
    },
    EvolinkDeepseekV4Pro {
        provider: Evolink,
        id: "evolink/deepseek-v4-pro",
        parse: ["evolink/deepseek-v4-pro"],
        display: "DeepSeek V4 Pro (Evolink)",
        description: "DeepSeek V4 Pro reasoning model served through the Evolink gateway (direct.evolink.ai).",
    },
    EvolinkDeepseekV4Flash {
        provider: Evolink,
        id: "evolink/deepseek-v4-flash",
        parse: ["evolink/deepseek-v4-flash"],
        display: "DeepSeek V4 Flash (Evolink)",
        description: "DeepSeek V4 Flash fast inference model served through the Evolink gateway (direct.evolink.ai).",
    },
    EvolinkDoubaoSeed20Pro {
        provider: Evolink,
        id: "evolink/doubao-seed-2.0-pro",
        parse: ["evolink/doubao-seed-2.0-pro"],
        display: "Doubao Seed 2.0 Pro (Evolink)",
        description: "Doubao Seed 2.0 Pro served through the Evolink gateway (direct.evolink.ai).",
    },
    EvolinkGemini31Pro {
        provider: Evolink,
        id: "evolink/gemini-3.1-pro-preview",
        parse: ["evolink/gemini-3.1-pro-preview"],
        display: "Gemini 3.1 Pro (Evolink)",
        description: "Gemini 3.1 Pro served through the Evolink gateway via OpenAI SDK format (direct.evolink.ai).",
    },
    EvolinkGemini35Flash {
        provider: Evolink,
        id: "evolink/gemini-3.5-flash",
        parse: ["evolink/gemini-3.5-flash"],
        display: "Gemini 3.5 Flash (Evolink)",
        description: "Gemini 3.5 Flash served through the Evolink gateway via OpenAI SDK format (direct.evolink.ai).",
    },
    EvolinkMinimaxM3 {
        provider: Evolink,
        id: "evolink/MiniMax-M3",
        parse: ["evolink/MiniMax-M3"],
        display: "MiniMax-M3 (Evolink)",
        description: "MiniMax-M3 frontier multimodal model served through the Evolink gateway (direct.evolink.ai).",
    },
    EvolinkClaudeSonnet46 {
        provider: Evolink,
        id: "evolink/claude-sonnet-4-6",
        parse: ["evolink/claude-sonnet-4-6"],
        display: "Claude Sonnet 4.6 (Evolink)",
        description: "Claude Sonnet 4.6 served through the Evolink gateway via Anthropic Messages API.",
    },
    EvolinkClaudeOpus48 {
        provider: Evolink,
        id: "evolink/claude-opus-4-8",
        parse: ["evolink/claude-opus-4-8"],
        display: "Claude Opus 4.8 (Evolink)",
        description: "Claude Opus 4.8 served through the Evolink gateway via Anthropic Messages API.",
    },
    EvolinkClaudeHaiku45 {
        provider: Evolink,
        id: "evolink/claude-haiku-4-5-20251001",
        parse: ["evolink/claude-haiku-4-5-20251001"],
        display: "Claude Haiku 4.5 (Evolink)",
        description: "Claude Haiku 4.5 served through the Evolink gateway via Anthropic Messages API.",
    },
    // Z.AI models
    ZaiGlm52 {
        provider: ZAI,
        id: models::zai::GLM_5_2,
        parse: [models::zai::GLM_5_2],
        display: "GLM 5.2",
        description: "Z.ai flagship model for long-horizon tasks with truly usable 1M-token context",
    },
    ZaiGlm51 {
        provider: ZAI,
        id: models::zai::GLM_5_1,
        parse: [models::zai::GLM_5_1],
        display: "GLM 5.1",
        description: "Z.ai next-gen GLM-5.1 foundation model with improved reasoning and agent capabilities",
    },
    // MiMo models
    MiMoV25Pro {
        provider: MiMo,
        id: models::mimo::MIMO_V2_5_PRO,
        parse: [models::mimo::MIMO_V2_5_PRO],
        display: "MiMo V2.5 Pro",
        description: "Xiaomi's flagship reasoning model with advanced capabilities (1M context)",
    },
    MiMoV25 {
        provider: MiMo,
        id: models::mimo::MIMO_V2_5,
        parse: [models::mimo::MIMO_V2_5],
        display: "MiMo V2.5",
        description: "Xiaomi's omni-modal model with full-modal understanding and 1M context",
    },
    // Moonshot models
    MoonshotKimiK27Code {
        provider: Moonshot,
        id: models::moonshot::KIMI_K2_7_CODE,
        parse: [models::moonshot::KIMI_K2_7_CODE],
        display: "Kimi K2.7 Code (Moonshot)",
        description: "Kimi K2.7 Code - Moonshot.ai's most capable coding model with long-horizon coding breakthrough, 256K context, and strong reasoning",
    },
    MoonshotKimiK26 {
        provider: Moonshot,
        id: models::moonshot::KIMI_K2_6,
        parse: [models::moonshot::KIMI_K2_6],
        display: "Kimi K2.6 (Moonshot)",
        description: "Kimi K2.6 - Moonshot.ai's 1T MoE flagship with 32B active parameters, MLA attention, and MoonViT vision",
    },
    // OpenCode Zen models (parse only via the `opencode/`/`opencode-zen/` prefix)
    OpenCodeZenGPT54 {
        provider: OpenCodeZen,
        id: models::opencode_zen::GPT_5_4,
        parse: [],
        display: "GPT-5.4 (OpenCode Zen)",
        description: "OpenCode Zen flagship GPT-5.4 route using OpenCode's curated pay-as-you-go gateway",
    },
    OpenCodeZenGPT54Mini {
        provider: OpenCodeZen,
        id: models::opencode_zen::GPT_5_4_MINI,
        parse: [],
        display: "GPT-5.4 Mini (OpenCode Zen)",
        description: "Lower-cost OpenCode Zen GPT-5.4 Mini option for faster and cheaper tasks",
    },
    OpenCodeZenClaudeSonnet46 {
        provider: OpenCodeZen,
        id: models::opencode_zen::CLAUDE_SONNET_4_6,
        parse: [],
        display: "Claude Sonnet 4.6 (OpenCode Zen)",
        description: "Claude Sonnet 4.6 served through OpenCode Zen's curated Anthropic endpoint",
    },
    OpenCodeZenGlm51 {
        provider: OpenCodeZen,
        id: models::opencode_zen::GLM_5_1,
        parse: [],
        display: "GLM-5.1 (OpenCode Zen)",
        description: "GLM-5.1 served through OpenCode Zen for lower-cost reasoning and coding work",
    },
    // OpenCode Go models (parse only via the `opencode-go/` prefix)
    OpenCodeGoGlm52 {
        provider: OpenCodeGo,
        id: models::opencode_go::GLM_5_2,
        parse: [],
        display: "GLM-5.2 (OpenCode Go)",
        description: "GLM-5.2 included with the OpenCode Go subscription for flagship open-model coding",
    },
    OpenCodeGoGlm51 {
        provider: OpenCodeGo,
        id: models::opencode_go::GLM_5_1,
        parse: [],
        display: "GLM-5.1 (OpenCode Go)",
        description: "GLM-5.1 included with the OpenCode Go subscription for open-model coding workflows",
    },
    OpenCodeGoKimiK27Code {
        provider: OpenCodeGo,
        id: models::opencode_go::KIMI_K2_7_CODE,
        parse: [],
        display: "Kimi K2.7 Code (OpenCode Go)",
        description: "Kimi K2.7 Code included with the OpenCode Go subscription for long-horizon coding",
    },
    OpenCodeGoKimiK26 {
        provider: OpenCodeGo,
        id: models::opencode_go::KIMI_K2_6,
        parse: [],
        display: "Kimi K2.6 (OpenCode Go)",
        description: "Kimi K2.6 included with the OpenCode Go subscription for general agentic coding",
    },
    OpenCodeGoMimoV25 {
        provider: OpenCodeGo,
        id: models::opencode_go::MIMO_V2_5,
        parse: [],
        display: "MiMo-V2.5 (OpenCode Go)",
        description: "MiMo-V2.5 included with the OpenCode Go subscription for high-volume coding workloads",
    },
    OpenCodeGoMimoV25Pro {
        provider: OpenCodeGo,
        id: models::opencode_go::MIMO_V2_5_PRO,
        parse: [],
        display: "MiMo-V2.5-Pro (OpenCode Go)",
        description: "MiMo-V2.5-Pro included with the OpenCode Go subscription for complex agentic tasks",
    },
    OpenCodeGoMinimaxM3 {
        provider: OpenCodeGo,
        id: models::opencode_go::MINIMAX_M3,
        parse: [],
        display: "MiniMax-M3 (OpenCode Go)",
        description: "MiniMax-M3 included with the OpenCode Go subscription for frontier agentic coding",
    },
    OpenCodeGoMinimaxM27 {
        provider: OpenCodeGo,
        id: models::opencode_go::MINIMAX_M2_7,
        parse: [],
        display: "MiniMax-M2.7 (OpenCode Go)",
        description: "MiniMax-M2.7 included with the OpenCode Go subscription for stronger agentic coding",
    },
    OpenCodeGoQwen37Max {
        provider: OpenCodeGo,
        id: models::opencode_go::QWEN_3_7_MAX,
        parse: [],
        display: "Qwen3.7 Max (OpenCode Go)",
        description: "Qwen3.7 Max included with the OpenCode Go subscription for the highest capability tier",
    },
    OpenCodeGoQwen37Plus {
        provider: OpenCodeGo,
        id: models::opencode_go::QWEN_3_7_PLUS,
        parse: [],
        display: "Qwen3.7 Plus (OpenCode Go)",
        description: "Qwen3.7 Plus included with the OpenCode Go subscription as a balanced coding model",
    },
    OpenCodeGoQwen36Plus {
        provider: OpenCodeGo,
        id: models::opencode_go::QWEN_3_6_PLUS,
        parse: [],
        display: "Qwen3.6 Plus (OpenCode Go)",
        description: "Qwen3.6 Plus included with the OpenCode Go subscription for cost-effective coding",
    },
    OpenCodeGoDeepseekV4Pro {
        provider: OpenCodeGo,
        id: models::opencode_go::DEEPSEEK_V4_PRO,
        parse: [],
        display: "DeepSeek V4 Pro (OpenCode Go)",
        description: "DeepSeek V4 Pro included with the OpenCode Go subscription for high-quality reasoning",
    },
    OpenCodeGoDeepseekV4Flash {
        provider: OpenCodeGo,
        id: models::opencode_go::DEEPSEEK_V4_FLASH,
        parse: [],
        display: "DeepSeek V4 Flash (OpenCode Go)",
        description: "DeepSeek V4 Flash included with the OpenCode Go subscription for fast, low-cost coding",
    },
    // Qwen models (picker-only; their ids resolve to the native variants above)
    QwenDeepSeekV4Flash {
        provider: Qwen,
        id: models::qwen::DEEPSEEK_V4_FLASH,
        parse: [],
        display: "DeepSeek V4 Flash (Qwen)",
        description: "DeepSeek V4 Flash fast inference model served through Qwen Cloud API (1M context)",
    },
    QwenDeepSeekV4Pro {
        provider: Qwen,
        id: models::qwen::DEEPSEEK_V4_PRO,
        parse: [],
        display: "DeepSeek V4 Pro (Qwen)",
        description: "DeepSeek V4 Pro high-performance reasoning model served through Qwen Cloud API (1M context)",
    },
    QwenGlm51 {
        provider: Qwen,
        id: models::qwen::GLM_5_1,
        parse: [],
        display: "GLM-5.1 (Qwen)",
        description: "Z.AI GLM-5.1 next-gen foundation model served through Qwen Cloud API",
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
        provider: Ollama,
        id: models::ollama::GPT_OSS_20B_CLOUD,
        parse: [models::ollama::GPT_OSS_20B_CLOUD],
        display: "GPT-OSS 20B (cloud)",
        description: "Cloud-hosted GPT-OSS 20B accessed through Ollama Cloud for efficient reasoning tasks",
    },
    OllamaGptOss120bCloud {
        provider: Ollama,
        id: models::ollama::GPT_OSS_120B_CLOUD,
        parse: [models::ollama::GPT_OSS_120B_CLOUD],
        display: "GPT-OSS 120B (cloud)",
        description: "Cloud-hosted GPT-OSS 120B accessed through Ollama Cloud for larger reasoning tasks",
    },
    OllamaDeepseekV4FlashCloud {
        provider: Ollama,
        id: models::ollama::DEEPSEEK_V4_FLASH_CLOUD,
        parse: [models::ollama::DEEPSEEK_V4_FLASH_CLOUD],
        display: "DeepSeek V4 Flash (cloud)",
        description: "DeepSeek V4 Flash cloud deployment via Ollama with fast inference and efficient reasoning",
    },
    OllamaDeepseekV4ProCloud {
        provider: Ollama,
        id: models::ollama::DEEPSEEK_V4_PRO_CLOUD,
        parse: [models::ollama::DEEPSEEK_V4_PRO_CLOUD],
        display: "DeepSeek V4 Pro (cloud)",
        description: "DeepSeek V4 Pro cloud deployment via Ollama with advanced thinking and strong reasoning",
    },
    OllamaMinimaxM27Cloud {
        provider: Ollama,
        id: models::ollama::MINIMAX_M27_CLOUD,
        parse: [models::ollama::MINIMAX_M27_CLOUD],
        display: "MiniMax-M2.7 (cloud)",
        description: "Cloud-hosted MiniMax-M2.7 model accessed through Ollama Cloud for reasoning tasks",
    },
    OllamaMinimaxM3Cloud {
        provider: Ollama,
        id: models::ollama::MINIMAX_M3_CLOUD,
        parse: [models::ollama::MINIMAX_M3_CLOUD],
        display: "MiniMax-M3 (cloud)",
        description: "Cloud-hosted MiniMax-M3 model served via Ollama Cloud",
    },
    OllamaGlm51Cloud {
        provider: Ollama,
        id: models::ollama::GLM_5_1_CLOUD,
        parse: [models::ollama::GLM_5_1_CLOUD],
        display: "GLM-5.1 (cloud)",
        description: "Cloud-hosted GLM-5.1 model served via Ollama Cloud",
    },
    OllamaGlm52Cloud {
        provider: Ollama,
        id: models::ollama::GLM_5_2_CLOUD,
        parse: [models::ollama::GLM_5_2_CLOUD],
        display: "GLM-5.2 (cloud)",
        description: "Cloud-hosted GLM-5.2 flagship model for long-horizon tasks with 1M context via Ollama Cloud",
    },
    OllamaKimiK26Cloud {
        provider: Ollama,
        id: models::ollama::KIMI_K2_6_CLOUD,
        parse: [models::ollama::KIMI_K2_6_CLOUD],
        display: "Kimi-K2.6 (cloud)",
        description: "Cloud-hosted Kimi K2.6 model served via Ollama Cloud",
    },
    OllamaKimiK27CodeCloud {
        provider: Ollama,
        id: models::ollama::KIMI_K2_7_CODE_CLOUD,
        parse: [models::ollama::KIMI_K2_7_CODE_CLOUD],
        display: "Kimi-K2.7-Code (cloud)",
        description: "Cloud-hosted Kimi K2.7 Code model served via Ollama Cloud",
    },
    OllamaGemma4 {
        provider: Ollama,
        id: models::ollama::GEMMA_4,
        parse: [models::ollama::GEMMA_4],
        display: "Gemma 4",
        description: "Google Gemma 4 model designed for frontier-level reasoning, agentic workflows, coding, and multimodal understanding (128K context).",
    },
    OllamaLagunaXs2 {
        provider: Ollama,
        id: models::ollama::LAGUNA_XS_2,
        parse: [models::ollama::LAGUNA_XS_2],
        display: "Laguna XS.2 (local)",
        description: "Poolside's 33B MoE model with 3B activated parameters, optimized for agentic coding with sliding window attention and native reasoning support (128K context)",
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
    LlamaCppStep35Flash {
        provider: LlamaCpp,
        id: models::llamacpp::STEP_3_5_FLASH,
        parse: [models::llamacpp::STEP_3_5_FLASH],
        display: "Step 3.5 Flash (llama.cpp)",
        description: "StepFun's efficient reasoning model served locally through llama.cpp",
    },
    // MiniMax models
    MinimaxM3 {
        provider: Minimax,
        id: models::minimax::MINIMAX_M3,
        parse: [models::minimax::MINIMAX_M3],
        display: "MiniMax-M3",
        description: "Frontier multimodal coding model with 1M context window",
    },
    MinimaxM27 {
        provider: Minimax,
        id: models::minimax::MINIMAX_M2_7,
        parse: [models::minimax::MINIMAX_M2_7],
        display: "MiniMax-M2.7",
        description: "Beginning the journey of recursive self-improvement with 204.8K context and strong reasoning/coding performance",
    },
    // Poolside models
    PoolsideLagunaM1 {
        provider: Poolside,
        id: models::poolside::LAGUNA_M1,
        parse: [models::poolside::LAGUNA_M1],
        display: "Laguna M.1",
        description: "Poolside's flagship MoE coding agent model with 128K context, optimized for multi-step agentic tasks, tool use, and validation",
    },
    PoolsideLagunaXs2 {
        provider: Poolside,
        id: models::poolside::LAGUNA_XS2,
        parse: [models::poolside::LAGUNA_XS2],
        display: "Laguna XS.2",
        description: "Poolside's efficient MoE coding agent model with 128K context, optimized for fast agentic coding with lower resource requirements",
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
        matches!(
            model.provider(),
            Provider::OpenCodeZen | Provider::OpenCodeGo | Provider::Qwen
        )
        // "gpt-oss-20b" is shared with OpenAI and parses to OpenAIGptOss20b
        // (legacy behavior preserved by table row order).
        || *model == ModelId::LlamaCppGptOss20b
    }

    #[test]
    fn all_models_round_trip_through_from_str() {
        for model in ModelId::all_models() {
            if round_trip_exempt(&model) {
                continue;
            }
            let id = model.as_str();
            let parsed = ModelId::from_str(&id)
                .unwrap_or_else(|err| panic!("failed to parse {id} back into {model:?}: {err}"));
            assert_eq!(parsed, model, "round-trip mismatch for id {id}");
        }
    }

    #[test]
    fn every_non_openrouter_model_is_in_the_table() {
        for model in ModelId::all_models() {
            if model.provider() == Provider::OpenRouter {
                continue;
            }
            assert!(
                model.table_id().is_some(),
                "{model:?} is missing from the model_id_table! invocation"
            );
        }
    }

    #[test]
    fn parse_aliases_resolve_to_canonical_variants() {
        let cases: &[(&str, ModelId)] = &[
            (models::GPT, ModelId::GPT54),
            (models::openai::GPT_5_5_DATED, ModelId::GPT55),
            (models::GEMINI_3_FLASH_PREVIEW, ModelId::Gemini35Flash),
            (models::CLAUDE_HAIKU_4_5_20251001, ModelId::ClaudeHaiku45),
            (
                models::huggingface::STEP_3_5_FLASH_BASE,
                ModelId::HuggingFaceStep35Flash,
            ),
            (
                models::huggingface::STEP_3_5_FLASH_LEGACY_FASTEST,
                ModelId::HuggingFaceStep35Flash,
            ),
        ];
        for (alias, expected) in cases {
            let parsed = ModelId::from_str(alias)
                .unwrap_or_else(|err| panic!("alias {alias} failed to parse: {err}"));
            assert_eq!(&parsed, expected, "alias {alias} resolved incorrectly");
        }
    }

    #[test]
    fn shared_gpt_oss_20b_id_resolves_to_openai() {
        assert_eq!(models::openai::GPT_OSS_20B, models::llamacpp::GPT_OSS_20B);
        assert_eq!(
            ModelId::from_str(models::llamacpp::GPT_OSS_20B).unwrap(),
            ModelId::OpenAIGptOss20b
        );
    }
}
