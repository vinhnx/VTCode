use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use crate::constants::{defaults, tools};

/// Tools configuration
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ToolsConfig {
    /// Default policy for tools not explicitly listed
    #[serde(default = "default_tool_policy")]
    pub default_policy: ToolPolicy,

    /// Specific tool policies
    #[serde(default)]
    #[cfg_attr(
        feature = "schema",
        schemars(with = "std::collections::BTreeMap<String, ToolPolicy>")
    )]
    pub policies: IndexMap<String, ToolPolicy>,

    /// Maximum inner tool-call loops per user turn
    ///
    /// Prevents infinite tool-calling cycles in interactive chat. This limits how
    /// many back-and-forths the agent will perform executing tools and
    /// re-asking the model before returning a final answer.
    ///
    #[serde(default = "default_max_tool_loops")]
    pub max_tool_loops: usize,

    /// Maximum number of times the same tool invocation can be retried with the
    /// identical arguments within a single turn.
    #[serde(default = "default_max_repeated_tool_calls")]
    pub max_repeated_tool_calls: usize,
}

impl Default for ToolsConfig {
    fn default() -> Self {
        let policies = DEFAULT_TOOL_POLICIES
            .iter()
            .map(|(tool, policy)| ((*tool).to_string(), *policy))
            .collect::<IndexMap<_, _>>();
        Self {
            default_policy: default_tool_policy(),
            policies,
            max_tool_loops: default_max_tool_loops(),
            max_repeated_tool_calls: default_max_repeated_tool_calls(),
        }
    }
}

/// Tool execution policy
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ToolPolicy {
    /// Allow execution without confirmation
    Allow,
    /// Prompt user for confirmation
    Prompt,
    /// Deny execution
    Deny,
}

fn default_tool_policy() -> ToolPolicy {
    ToolPolicy::Prompt
}

fn default_max_tool_loops() -> usize {
    defaults::DEFAULT_MAX_TOOL_LOOPS
}

fn default_max_repeated_tool_calls() -> usize {
    defaults::DEFAULT_MAX_REPEATED_TOOL_CALLS
}

const DEFAULT_TOOL_POLICIES: &[(&str, ToolPolicy)] = &[
    (tools::LIST_FILES, ToolPolicy::Allow),
    (tools::GREP_FILE, ToolPolicy::Allow),
    (tools::UPDATE_PLAN, ToolPolicy::Allow),
    (tools::AST_GREP_SEARCH, ToolPolicy::Allow),
    (tools::READ_FILE, ToolPolicy::Allow),
    (tools::WRITE_FILE, ToolPolicy::Allow),
    (tools::EDIT_FILE, ToolPolicy::Allow),
    (tools::CREATE_FILE, ToolPolicy::Allow),
    (tools::APPLY_PATCH, ToolPolicy::Prompt),
    (tools::DELETE_FILE, ToolPolicy::Prompt),
    (tools::CREATE_PTY_SESSION, ToolPolicy::Allow),
    (tools::READ_PTY_SESSION, ToolPolicy::Allow),
    (tools::LIST_PTY_SESSIONS, ToolPolicy::Allow),
    (tools::RESIZE_PTY_SESSION, ToolPolicy::Allow),
    (tools::SEND_PTY_INPUT, ToolPolicy::Prompt),
    (tools::CLOSE_PTY_SESSION, ToolPolicy::Allow),
    (tools::RUN_COMMAND, ToolPolicy::Prompt),
    (tools::WEB_FETCH, ToolPolicy::Prompt),
];
