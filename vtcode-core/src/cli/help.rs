use crate::config::constants::models;

/// Returns an informative help snippet listing OpenAI models that use the Responses API.
pub fn openai_responses_models_help() -> String {
    let names = models::openai::RESPONSES_API_MODELS.join(", ");
    format!("OpenAI Responses API models: {}", names)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn help_contains_known_model() {
        let snippet = openai_responses_models_help();
        assert!(snippet.contains(models::openai::GPT_5));
        assert!(snippet.contains(models::openai::GPT_5_2));
    }
}
