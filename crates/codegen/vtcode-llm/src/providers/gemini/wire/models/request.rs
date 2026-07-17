use super::{Content, SystemInstruction, Tool, ToolConfig};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerateContentRequest {
    pub contents: Vec<Content>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<Tool>>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "toolConfig")]
    pub tool_config: Option<ToolConfig>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "systemInstruction")]
    pub system_instruction: Option<SystemInstruction>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "generationConfig",
        deserialize_with = "deserialize_boxed_generation_config_opt"
    )]
    pub generation_config: Option<Box<GenerationConfig>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerationConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_k: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "candidateCount")]
    pub candidate_count: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "maxOutputTokens")]
    pub max_output_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "stopSequences")]
    pub stop_sequences: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "responseMimeType")]
    pub response_mime_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "responseSchema")]
    pub response_schema: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "presencePenalty")]
    pub presence_penalty: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "frequencyPenalty")]
    pub frequency_penalty: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "thinkingConfig")]
    pub thinking_config: Option<ThinkingConfig>,
}

impl GenerationConfig {
    fn is_empty(&self) -> bool {
        self.temperature.is_none()
            && self.top_p.is_none()
            && self.top_k.is_none()
            && self.candidate_count.is_none()
            && self.max_output_tokens.is_none()
            && self.stop_sequences.is_none()
            && self.response_mime_type.is_none()
            && self.response_schema.is_none()
            && self.presence_penalty.is_none()
            && self.frequency_penalty.is_none()
            && self.thinking_config.is_none()
    }

    fn into_boxed_if_non_empty(self) -> Option<Box<Self>> {
        (!self.is_empty()).then_some(Box::new(self))
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThinkingConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking_level: Option<String>,
}

fn deserialize_boxed_generation_config_opt<'de, D>(
    deserializer: D,
) -> Result<Option<Box<GenerationConfig>>, D::Error>
where
    D: Deserializer<'de>,
{
    Option::<GenerationConfig>::deserialize(deserializer)
        .map(|value| value.and_then(GenerationConfig::into_boxed_if_non_empty))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_generation_config_deserializes_to_none() {
        let request: GenerateContentRequest = serde_json::from_str(
            r#"{
                "contents": [],
                "generationConfig": {}
            }"#,
        )
        .unwrap();

        assert!(request.generation_config.is_none());
    }

    #[test]
    fn boxed_generation_config_is_smaller_than_inline_option() {
        use std::mem::size_of;

        assert!(size_of::<Option<Box<GenerationConfig>>>() < size_of::<Option<GenerationConfig>>());
    }
}
