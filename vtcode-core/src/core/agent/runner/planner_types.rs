//! Planner response types for the plan-build-evaluate harness.
//!
//! These types represent the structured JSON response from the LLM planner
//! that expands a task into an execution spec, contract, and tracker items.

use serde::Deserialize;
use serde::Deserializer;

/// Structured response from the planner LLM.
#[derive(Debug, Deserialize)]
pub(super) struct PlannerResponse {
    #[serde(
        alias = "execution_spec",
        deserialize_with = "deserialize_string_or_object"
    )]
    pub(super) spec_markdown: Option<String>,
    #[serde(
        default,
        alias = "execution_contract",
        deserialize_with = "deserialize_string_or_object"
    )]
    pub(super) contract_markdown: Option<String>,
    #[serde(default, alias = "task_title")]
    pub(super) task_title: Option<String>,
    #[serde(default, alias = "tracker_items")]
    pub(super) items: Vec<PlannerItem>,
}

/// A single item in the planner's tracker.
#[derive(Debug, Deserialize)]
pub(super) struct PlannerItem {
    #[serde(default)]
    pub(super) description: String,
    #[serde(default)]
    pub(super) files: Vec<String>,
    #[serde(default)]
    pub(super) outcome: String,
    #[serde(
        default,
        alias = "verification_command",
        deserialize_with = "deserialize_string_or_vec"
    )]
    pub(super) verify: Vec<String>,
}

pub(super) fn deserialize_string_or_vec<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    match value {
        serde_json::Value::String(s) => Ok(vec![s]),
        serde_json::Value::Array(arr) => {
            let mut result = Vec::new();
            for item in arr {
                if let serde_json::Value::String(s) = item {
                    result.push(s);
                }
            }
            Ok(result)
        }
        _ => Ok(Vec::new()),
    }
}

pub(super) fn deserialize_string_or_object<'de, D>(
    deserializer: D,
) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    match value {
        serde_json::Value::String(s) => Ok(Some(s)),
        serde_json::Value::Object(_) => Ok(Some(value.to_string())),
        serde_json::Value::Null => Ok(None),
        _ => Ok(None),
    }
}
