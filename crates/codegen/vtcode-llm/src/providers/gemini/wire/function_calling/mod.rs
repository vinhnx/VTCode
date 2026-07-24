use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionCall {
    pub name: String,
    pub args: Value,
    #[serde(default)]
    pub id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionResponse {
    pub name: String,
    pub response: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionCallingConfig {
    pub(crate) mode: String,
    #[serde(skip_serializing_if = "Option::is_none", rename = "allowedFunctionNames")]
    pub(crate) allowed_function_names: Option<Vec<String>>,
}

impl FunctionCallingConfig {
    pub(crate) fn auto() -> Self {
        Self {
            mode: "AUTO".to_owned(),
            allowed_function_names: None,
        }
    }

    pub(crate) fn validated() -> Self {
        Self {
            mode: "VALIDATED".to_owned(),
            allowed_function_names: None,
        }
    }

    pub(crate) fn none() -> Self {
        Self {
            mode: "NONE".to_owned(),
            allowed_function_names: None,
        }
    }

    pub(crate) fn any() -> Self {
        Self {
            mode: "ANY".to_owned(),
            allowed_function_names: None,
        }
    }
}
