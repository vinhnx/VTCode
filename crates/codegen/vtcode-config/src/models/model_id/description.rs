use std::borrow::Cow;

use super::ModelId;

impl ModelId {
    /// Get a description of the model's characteristics.
    ///
    /// Returns `Cow<'static, str>` because custom user-defined models
    /// carry runtime strings that may not be `'static`.
    pub fn description(&self) -> Cow<'static, str> {
        if let Some(meta) = self.openrouter_metadata() {
            return Cow::Borrowed(meta.description);
        }
        if let Some(description) = self.table_description() {
            return Cow::Borrowed(description);
        }
        match self {
            // OpenRouter models without generated metadata
            ModelId::OpenRouterMoonshotaiKimiK3 => Cow::Borrowed(
                "Kimi K3 2.8T parameter flagship with 1M context, native vision, and always-on deep reasoning via OpenRouter",
            ),
            ModelId::OpenRouterMoonshotaiKimiK26 => Cow::Borrowed(
                "Kimi K2.6 multimodal agentic model for long-horizon coding and design via OpenRouter",
            ),
            ModelId::OpenRouterMoonshotaiKimiK27Code => Cow::Borrowed(
                "Kimi K2.7 Code most capable coding model with long-horizon coding breakthrough via OpenRouter",
            ),
            ModelId::OpenRouterZaiGlm51 => {
                Cow::Borrowed("Z.AI GLM-5.1 next-gen foundation model via OpenRouter")
            }
            ModelId::OpenRouterZaiGlm52 => Cow::Borrowed(
                "Z.AI GLM-5.2 flagship model for long-horizon tasks with 1M context via OpenRouter",
            ),
            ModelId::OpenRouterOpenAIGpt55 => {
                Cow::Borrowed("OpenAI GPT-5.5 model accessed through OpenRouter")
            }
            // Custom user-defined models
            ModelId::Custom(_, _) => Cow::Borrowed("User-defined model"),
            model => Cow::Borrowed(
                model
                    .openrouter_metadata()
                    .expect("generated OpenRouter model should have metadata")
                    .description,
            ),
        }
    }
}
