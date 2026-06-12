// Moonshot.ai models
// Kimi K2.5 - Previous flagship model with enhanced reasoning
// <https://platform.moonshot.ai/docs/guide/kimi-k2-5-quickstart>
// Kimi K2.6 - 1T MoE flagship model (32B active) with MLA attention and MoonViT vision encoder
// <https://platform.moonshot.ai/docs/guide/kimi-k2-6-quickstart>
// Kimi K2.7 Code - Most capable coding model with long-horizon coding breakthrough and 256K context
// <https://platform.kimi.ai/docs/guide/kimi-k2.7-code>
pub const DEFAULT_MODEL: &str = KIMI_K2_7_CODE;
pub const SUPPORTED_MODELS: &[&str] = &[KIMI_K2_7_CODE, KIMI_K2_6, KIMI_K2_5];
pub const REASONING_MODELS: &[&str] = &[KIMI_K2_7_CODE];

pub const KIMI_K2_5: &str = "kimi-k2.5";
pub const KIMI_K2_6: &str = "kimi-k2.6";
pub const KIMI_K2_7_CODE: &str = "kimi-k2.7-code";
