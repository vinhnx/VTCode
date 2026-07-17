use crate::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ResponsesApiTool {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub strict: bool,
    pub parameters: JsonSchema,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FreeformTool {
    pub name: String,
    pub description: String,
    pub format: FreeformToolFormat,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FreeformToolFormat {
    pub lark_grammar: Option<String>,
    pub examples: Vec<String>,
}
