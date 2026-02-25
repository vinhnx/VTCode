pub mod anthropic;
pub mod deepseek;
pub mod google;
pub mod huggingface;
pub mod lmstudio;
pub mod minimax;
pub mod moonshot;
pub mod ollama;
pub mod openai;
pub mod openresponses;
pub mod openrouter;
pub mod xai;
pub mod zai;

// Backwards compatibility - keep old constants working
pub const GEMINI_3_1_PRO_PREVIEW: &str = google::GEMINI_3_1_PRO_PREVIEW;
pub const GEMINI_3_1_PRO_PREVIEW_CUSTOMTOOLS: &str = google::GEMINI_3_1_PRO_PREVIEW_CUSTOMTOOLS;
pub const GEMINI_3_PRO_PREVIEW: &str = google::GEMINI_3_PRO_PREVIEW;
pub const GEMINI_3_FLASH_PREVIEW: &str = google::GEMINI_3_FLASH_PREVIEW;
pub const GPT_5: &str = openai::GPT_5;
pub const GPT_5_MINI: &str = openai::GPT_5_MINI;
pub const GPT_5_NANO: &str = openai::GPT_5_NANO;
pub const GPT_5_2: &str = openai::GPT_5_2;
pub const GPT_5_2_ALIAS: &str = openai::GPT_5_2_ALIAS;
pub const CLAUDE_OPUS_4_1_20250805: &str = anthropic::CLAUDE_OPUS_4_1_20250805;
pub const CLAUDE_OPUS_4_5_20251101: &str = anthropic::CLAUDE_OPUS_4_5_20251101;
pub const CLAUDE_OPUS_4_20250514: &str = anthropic::CLAUDE_OPUS_4_20250514;
pub const CLAUDE_SONNET_4_20250514: &str = anthropic::CLAUDE_SONNET_4_20250514;
pub const CLAUDE_3_7_SONNET_20250219: &str = anthropic::CLAUDE_3_7_SONNET_20250219;
pub const CLAUDE_3_5_SONNET_20241022: &str = anthropic::CLAUDE_3_5_SONNET_20241022;
pub const CLAUDE_SONNET_4_5: &str = anthropic::CLAUDE_SONNET_4_5;
pub const CLAUDE_SONNET_4_6: &str = anthropic::CLAUDE_SONNET_4_6;
pub const CLAUDE_HAIKU_4_5: &str = anthropic::CLAUDE_HAIKU_4_5;
pub const CLAUDE_SONNET_4_5_20250929: &str = anthropic::CLAUDE_SONNET_4_5_20250929;
pub const CLAUDE_HAIKU_4_5_20251001: &str = anthropic::CLAUDE_HAIKU_4_5_20251001;
pub const CLAUDE_OPUS_4_1: &str = anthropic::CLAUDE_OPUS_4_1;
pub const CLAUDE_OPUS_4_5: &str = anthropic::CLAUDE_OPUS_4_5;
pub const CLAUDE_OPUS_4_6: &str = anthropic::CLAUDE_OPUS_4_6;
pub const CLAUDE_SONNET_4_0: &str = anthropic::CLAUDE_SONNET_4_0;
pub const CLAUDE_OPUS_4_0: &str = anthropic::CLAUDE_OPUS_4_0;
pub const CLAUDE_3_7_SONNET_LATEST: &str = anthropic::CLAUDE_3_7_SONNET_LATEST;
pub const CLAUDE_3_5_SONNET_LATEST: &str = anthropic::CLAUDE_3_5_SONNET_LATEST;
pub const CLAUDE_3_5_HAIKU_20241022: &str = anthropic::CLAUDE_3_5_HAIKU_20241022;
pub const CLAUDE_3_5_HAIKU_LATEST: &str = anthropic::CLAUDE_3_5_HAIKU_LATEST;
pub const MINIMAX_M2: &str = minimax::MINIMAX_M2;
pub const MINIMAX_M2_5: &str = minimax::MINIMAX_M2_5;
pub const GLM_5: &str = zai::GLM_5;
pub const XAI_GROK_4: &str = xai::GROK_4;
pub const XAI_GROK_4_MINI: &str = xai::GROK_4_MINI;
pub const XAI_GROK_4_CODE: &str = xai::GROK_4_CODE;
pub const XAI_GROK_4_CODE_LATEST: &str = xai::GROK_4_CODE_LATEST;
pub const XAI_GROK_4_VISION: &str = xai::GROK_4_VISION;
pub const XAI_GROK_3: &str = xai::GROK_3;
pub const XAI_GROK_2: &str = xai::GROK_2_1212;
pub const XAI_GROK_BETA: &str = xai::GROK_BETA;
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
pub const OPENROUTER_ANTHROPIC_CLAUDE_SONNET_4_5: &str = openrouter::ANTHROPIC_CLAUDE_SONNET_4_5;
#[cfg(docsrs)]
pub const OPENROUTER_ANTHROPIC_CLAUDE_SONNET_4_5: &str = "anthropic/claude-sonnet-4.5";
