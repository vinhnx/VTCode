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
    /// Optional feature list markdown. The planner produces this to enumerate
    /// the project's features with acceptance criteria, so each session can
    /// pick up an incremental unit of work. The evaluator may modify this
    /// during feedback-driven replanning.
    #[serde(default, alias = "feature_list_markdown")]
    pub(super) feature_list_markdown: Option<String>,
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

/// Structured response from the replanner LLM after an evaluator rejection.
///
/// Following the long-running harness pattern: "the evaluator takes on part of
/// the local planner role for feedback-driven replanning." The replanner
/// receives the current artifacts and evaluator feedback, then produces a
/// revised feature list, contract addendum, and new tracker items.
#[derive(Debug, Deserialize)]
pub(super) struct ReplanResponse {
    /// Revised feature list markdown, replacing the previous one entirely.
    #[serde(default, alias = "revised_feature_list")]
    pub(super) revised_feature_list: Option<String>,
    /// Addendum to append to the execution contract.
    #[serde(default, alias = "contract_addendum")]
    pub(super) contract_addendum: Option<String>,
    /// New tracker items to add (e.g. new acceptance criteria discovered
    /// through evaluator testing).
    #[serde(default, alias = "new_tracker_items")]
    pub(super) new_tracker_items: Vec<PlannerItem>,
    /// Why the replanner made these changes.
    #[serde(default)]
    pub(super) rationale: String,
}
