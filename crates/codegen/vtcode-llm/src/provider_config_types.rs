//! Core provider configuration type.

use std::path::PathBuf;
use vtcode_config::TimeoutsConfig;
use vtcode_config::auth::CopilotAuthConfig;
use vtcode_config::auth::OpenAIChatGptAuthHandle;
use vtcode_config::core::{AnthropicConfig, ModelConfig, OpenAIConfig, PromptCachingConfig};

#[derive(Debug, Clone)]
pub struct ProviderConfig {
    pub api_key: Option<String>,
    pub openai_chatgpt_auth: Option<OpenAIChatGptAuthHandle>,
    pub copilot_auth: Option<CopilotAuthConfig>,
    pub base_url: Option<String>,
    pub model: Option<String>,
    pub prompt_cache: Option<PromptCachingConfig>,
    pub timeouts: Option<TimeoutsConfig>,
    pub openai: Option<OpenAIConfig>,
    pub anthropic: Option<AnthropicConfig>,
    pub model_behavior: Option<ModelConfig>,
    pub workspace_root: Option<PathBuf>,
}
