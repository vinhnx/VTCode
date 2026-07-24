use super::Content;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerateContentResponse {
    pub(crate) candidates: Vec<Candidate>,
    #[serde(default, rename = "promptFeedback")]
    pub(crate) prompt_feedback: Option<Value>,
    #[serde(default, rename = "usageMetadata")]
    pub(crate) usage_metadata: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Candidate {
    pub(crate) content: Content,
    #[serde(default, rename = "finishReason")]
    pub(crate) finish_reason: Option<String>,
}
