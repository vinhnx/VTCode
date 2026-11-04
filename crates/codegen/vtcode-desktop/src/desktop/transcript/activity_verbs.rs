//! Verb categories for tool-group summaries ("Read 2 files · Ran 1 command").
//!
//! A category is assigned once at projection from the typed event details and
//! never changes afterwards, so group headers fold the rows at render time
//! instead of maintaining incremental counters: unlike streaming status,
//! there is no derived state that could drift.

use vtcode_exec_events::ThreadItemDetails;

use super::super::strings::display;

/// Coarse verb for one tool row, driving the group summary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ToolCategory {
    Read,
    Edit,
    Run,
    Search,
    Other,
}

impl ToolCategory {
    /// Classify a tool item from its event details. Returns `None` for rows
    /// that carry no classifiable action of their own: follow-up output
    /// (which shares its invocation's id and keeps that row's category) and
    /// non-tool variants.
    ///
    /// The tool-name mapping mirrors the service-side `tool_action_label`
    /// (`vtcode-core/src/tools/registry/labels.rs`) at summary granularity.
    /// Canonical names are matched literally against
    /// `vtcode-config/src/constants/tools.rs`; unknown names — including
    /// future tools — fall through to `Other`, so a registry addition can
    /// never break the summary.
    pub(crate) fn from_details(details: &ThreadItemDetails) -> Option<Self> {
        let category = match details {
            ThreadItemDetails::CommandExecution(_) => Self::Run,
            ThreadItemDetails::WebSearch(_) => Self::Search,
            ThreadItemDetails::McpToolCall(_) | ThreadItemDetails::Harness(_) | ThreadItemDetails::Error(_) => {
                Self::Other
            }
            ThreadItemDetails::ToolInvocation(tool) => Self::from_tool_name(&tool.tool_name, tool.arguments.as_ref()),
            ThreadItemDetails::ToolOutput(_)
            | ThreadItemDetails::AgentMessage(_)
            | ThreadItemDetails::Plan(_)
            | ThreadItemDetails::Reasoning(_)
            | ThreadItemDetails::Decision(_)
            | ThreadItemDetails::FileChange(_) => return None,
        };
        Some(category)
    }

    fn from_tool_name(tool_name: &str, args: Option<&serde_json::Value>) -> Self {
        match strip_mcp_prefix(tool_name) {
            "read_file" | "read_pty_session" | "web_fetch" | "fetch_url" | "defuddle_fetch" | "fetch"
            | "get_errors" | "list_files" => Self::Read,
            "edit_file" | "write_file" | "create_file" | "delete_file" | "apply_patch" | "search_replace" => Self::Edit,
            "exec_command" => Self::Run,
            "code_search" | "grep" | "grep_file" => Self::Search,
            // Internal unified tools dispatch on their action arg; a missing
            // action reads (file) or runs (exec), matching `tool_action_label`.
            "file_operation_internal" => match unified_action(args).unwrap_or("read") {
                op if op.eq_ignore_ascii_case("read") => Self::Read,
                op if ["write", "edit", "patch", "delete", "move", "copy"]
                    .iter()
                    .any(|known| op.eq_ignore_ascii_case(known)) =>
                {
                    Self::Edit
                }
                _ => Self::Other,
            },
            "command_session_internal" => match unified_action(args).unwrap_or("run") {
                op if op.eq_ignore_ascii_case("run") || op.eq_ignore_ascii_case("code") => Self::Run,
                op if op.eq_ignore_ascii_case("poll") => Self::Read,
                _ => Self::Other,
            },
            _ => Self::Other,
        }
    }
}

/// Action arg shared by the internal unified tools.
fn unified_action(args: Option<&serde_json::Value>) -> Option<&str> {
    args?.get("action")?.as_str()
}

/// Strip MCP prefixes the way `tool_action_label` normalizes names, so a
/// suffixed builtin keeps the service's own display semantics.
fn strip_mcp_prefix(tool_name: &str) -> &str {
    if let Some(rest) = tool_name.strip_prefix("mcp__") {
        return rest.rsplit("__").next().unwrap_or(rest);
    }
    if let Some(rest) = tool_name.strip_prefix("mcp::") {
        return rest.rsplit("::").next().unwrap_or(rest);
    }
    tool_name
}

/// Per-verb totals folded over one group at render time. Plain `usize`
/// counters: groups are bounded by the transcript cap, so incrementing once
/// per row cannot overflow.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ToolCategoryCounts {
    pub read: usize,
    pub edit: usize,
    pub run: usize,
    pub search: usize,
    pub other: usize,
}

impl ToolCategoryCounts {
    pub(crate) fn add(&mut self, category: ToolCategory) {
        match category {
            ToolCategory::Read => self.read += 1,
            ToolCategory::Edit => self.edit += 1,
            ToolCategory::Run => self.run += 1,
            ToolCategory::Search => self.search += 1,
            ToolCategory::Other => self.other += 1,
        }
    }

    /// Whether any row carries a nameable verb. Verb-less groups keep the
    /// legacy count label instead of rendering an empty phrase.
    pub(crate) fn has_verbs(&self) -> bool {
        self.read > 0 || self.edit > 0 || self.run > 0 || self.search > 0
    }
}

/// Verb-phrase summary in fixed category order ("Read 2 files · Ran 1
/// command"). English literals: new user-visible strings stay untranslated
/// unless full native-quality translations ship for every locale, and no
/// existing key covers verbs. Returns `None` when the group carries no
/// nameable verbs, in which case the caller keeps the legacy count label.
pub(crate) fn describe_activity_verbs(counts: &ToolCategoryCounts) -> Option<String> {
    if !counts.has_verbs() {
        return None;
    }
    let mut parts = Vec::new();
    if counts.read > 0 {
        parts.push(verb_part(counts.read, "Read", "file", "files"));
    }
    if counts.edit > 0 {
        parts.push(verb_part(counts.edit, "Edited", "file", "files"));
    }
    if counts.run > 0 {
        parts.push(verb_part(counts.run, "Ran", "command", "commands"));
    }
    if counts.search > 0 {
        parts.push(verb_part(counts.search, "Searched", "query", "queries"));
    }
    if counts.other > 0 {
        parts.push(format!("{} other {}", counts.other, if counts.other == 1 { "action" } else { "actions" }));
    }
    Some(parts.join(display::MIDDOT))
}

fn verb_part(count: usize, verb: &str, singular: &str, plural: &str) -> String {
    format!("{verb} {count} {}", if count == 1 { singular } else { plural })
}
