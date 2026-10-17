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

/// Wire mode the API assumes when `functionCallingConfig` is omitted.
const MODE_AUTO: &str = "AUTO";

impl FunctionCallingConfig {
    pub(crate) fn auto() -> Self {
        Self {
            mode: MODE_AUTO.to_owned(),
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

    /// Whether this is the wire default (`AUTO` with no allowed-name filter),
    /// which a request may omit without changing provider behaviour.
    pub(crate) fn is_default_auto(&self) -> bool {
        self.mode == MODE_AUTO && self.allowed_function_names.is_none()
    }
}
