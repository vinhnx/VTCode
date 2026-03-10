use super::Content;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerateContentResponse {
    pub candidates: Vec<Candidate>,
    #[serde(default, rename = "promptFeedback")]
    pub prompt_feedback: Option<Value>,
    #[serde(default, rename = "usageMetadata")]
    pub usage_metadata: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Candidate {
    pub content: Content,
    #[serde(default, rename = "finishReason")]
    pub finish_reason: Option<String>,
}
