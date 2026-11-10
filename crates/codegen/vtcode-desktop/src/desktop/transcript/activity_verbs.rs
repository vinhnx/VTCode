//! Verb categories for tool-group summaries ("Read 2 files · Ran 1 command").
//!
//! A category is assigned once at projection from the typed event details and
//! never changes afterwards, so group headers fold the rows at render time
//! instead of maintaining incremental counters: unlike streaming status,
//! there is no derived state that could drift.

use std::path::Path;

use vtcode_commons::tool_headlines::{
    describe_grep_file, describe_list_files, search_headline, session_followup_title, strip_mcp_prefix,
    verb_callout_headline,
};
use vtcode_exec_events::{ThreadItemDetails, ToolInvocationItem};

use super::super::strings::display;

/// Coarse verb for one tool row, driving the group summary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ToolCategory {
    Read,
    Edit,
    Run,
    Search,
    Tracker,
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

    pub(super) fn from_tool_name(tool_name: &str, args: Option<&serde_json::Value>) -> Self {
        match strip_mcp_prefix(tool_name) {
            "read_file" | "read_pty_session" | "web_fetch" | "fetch_url" | "defuddle_fetch" | "fetch"
            | "get_errors" | "list_files" => Self::Read,
            "edit_file" | "write_file" | "create_file" | "delete_file" | "apply_patch" | "search_replace" => Self::Edit,
            // Command launches render and merge like `exec_command`, mirroring
            // `tool_intent::is_command_run_tool_call` (its `EXEC_COMMAND` arm needs a
            // command field; these four are launches unconditionally).
            "exec_command" | "run_pty_cmd" | "create_pty_session" | "shell" | "bash" => Self::Run,
            "code_search" | "grep" | "grep_file" => Self::Search,
            // Canonical `task_tracker` (`vtcode-config/src/constants/tools.rs`).
            // Own category (not `Other`) so tracker rows render the inline
            // task block and group summaries name the verb.
            "task_tracker" => Self::Tracker,
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

/// Per-verb totals folded over one group at render time. Plain `usize`
/// counters: groups are bounded by the transcript cap, so incrementing once
/// per row cannot overflow.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ToolCategoryCounts {
    pub read: usize,
    pub edit: usize,
    pub run: usize,
    pub search: usize,
    pub tracker: usize,
    pub other: usize,
}

impl ToolCategoryCounts {
    pub(crate) fn add(&mut self, category: ToolCategory) {
        match category {
            ToolCategory::Read => self.read += 1,
            ToolCategory::Edit => self.edit += 1,
            ToolCategory::Run => self.run += 1,
            ToolCategory::Search => self.search += 1,
            ToolCategory::Tracker => self.tracker += 1,
            ToolCategory::Other => self.other += 1,
        }
    }

    /// Whether any row carries a nameable verb. Verb-less groups keep the
    /// legacy count label instead of rendering an empty phrase.
    pub(crate) fn has_verbs(&self) -> bool {
        self.read > 0 || self.edit > 0 || self.run > 0 || self.search > 0 || self.tracker > 0
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
    if counts.tracker > 0 {
        parts.push(verb_part(counts.tracker, "Updated", "task list", "task lists"));
    }
    if counts.other > 0 {
        parts.push(format!("{} other {}", counts.other, if counts.other == 1 { "action" } else { "actions" }));
    }
    Some(parts.join(display::MIDDOT))
}

fn verb_part(count: usize, verb: &str, singular: &str, plural: &str) -> String {
    format!("{verb} {count} {}", if count == 1 { singular } else { plural })
}

/// Action-aware title for exec-session follow-up calls ("Wait for command session").
///
/// Delegates to the shared builder (`vtcode_commons::tool_headlines`).
/// `None` arguments behave like empty args: a bare `write_stdin` polls, while
/// a bare command-session launch falls through to its argument gist. `Some`
/// also doubles as the session predicate for detail lines: a titled call
/// renders the one-line session row.
pub(super) fn session_tool_title(tool_name: &str, args: Option<&serde_json::Value>) -> Option<String> {
    let empty = serde_json::Value::Null;
    session_followup_title(tool_name, args.unwrap_or(&empty))
}

/// One-line outcome gist for a tool row ("Read Cargo.toml", "Ran cargo check").
///
/// Delegates to the shared verb router (`vtcode_commons::tool_headlines`) so
/// desktop rows, CLI callouts, and completed summaries cannot drift apart.
/// An absent argument object behaves like `Null` and falls back to the
/// prettified tool name, never a raw identifier.
pub(crate) fn tool_row_gist(tool: &ToolInvocationItem, workspace_root: Option<&Path>) -> String {
    let empty = serde_json::Value::Null;
    let args = tool.arguments.as_ref().unwrap_or(&empty);
    verb_callout_headline(&tool.tool_name, args, workspace_root)
}

/// Argument keys the row title consumes beyond its category's gist keys.
///
/// Recomputed from the same shared headline readers the title uses, so a key
/// hides if and only if the title actually phrased it: multi-key calls show
/// the keys the title skipped, and headline-less calls hide nothing.
#[must_use]
pub(crate) fn extra_gist_keys(tool: &ToolInvocationItem) -> &'static [&'static str] {
    let Some(args) = tool.arguments.as_ref() else {
        return &[];
    };
    // Workspace is irrelevant here: consumed keys never depend on display paths.
    match strip_mcp_prefix(&tool.tool_name) {
        "grep_file" => describe_grep_file(args, None).map(|(_, keys)| keys).unwrap_or(&[]),
        "list_files" => describe_list_files(args, None).map(|(_, keys)| keys).unwrap_or(&[]),
        "code_search" | "grep" => search_headline(&tool.tool_name, args).map(|(_, keys)| keys).unwrap_or(&[]),
        _ => &[],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vtcode_exec_events::ToolCallStatus;

    fn gist_with(name: &str, arguments: Option<serde_json::Value>, workspace: Option<&Path>) -> String {
        tool_row_gist(
            &ToolInvocationItem {
                tool_name: name.into(),
                arguments,
                tool_call_id: None,
                status: ToolCallStatus::Completed,
                outcome: None,
            },
            workspace,
        )
    }

    #[test]
    fn gist_fallback_prettifies_raw_tool_names() {
        let gist = |name: &str| gist_with(name, None, None);
        // The screenshot's raw identifier plus its naming-shape siblings.
        for (raw, pretty) in [
            ("DiagramCommand", "Diagram Command"),
            ("diagram_render", "Diagram Render"),
            ("diagramRender", "Diagram Render"),
            ("mcp__server__diagram_render", "Diagram Render"),
            ("mcp::server::read_file", "Read File"),
            ("read_file", "Read File"),
        ] {
            assert_eq!(gist(raw), pretty, "{raw} prettifies");
        }
        // Asymmetric: acronyms and versions keep their casing, empties echo.
        assert_eq!(gist("MCPTool"), "MCPTool");
        assert_eq!(gist("task_tracker_v2"), "Task Tracker V2");
        assert_eq!(gist("___"), "___");
        assert_eq!(gist(""), "");
    }

    #[test]
    fn gist_phrases_list_and_grep_headlines_like_cli() {
        use serde_json::json;
        let gist = |name: &str, arguments: serde_json::Value| gist_with(name, Some(arguments), None);
        // list_files: location first, then the pattern fallbacks.
        assert_eq!(gist("list_files", json!({"path": "checkpoints"})), "List files in checkpoints");
        assert_eq!(gist("list_files", json!({"path": "."})), "List files in workspace root");
        assert_eq!(gist("list_files", json!({"name_pattern": "*.rs"})), "Find files named *.rs");
        assert_eq!(gist("list_files", json!({"content_pattern": "TODO"})), "Search files for TODO");
        // Location wins over patterns when both are present.
        assert_eq!(gist("list_files", json!({"path": "src", "name_pattern": "*.rs"})), "List files in src");
        // grep_file: the pattern is required, the scope joins it when present.
        assert_eq!(gist("grep_file", json!({"pattern": "TODO"})), "Grep TODO");
        assert_eq!(gist("grep_file", json!({"pattern": "TODO", "path": "src/main.rs"})), "Grep TODO in src/main.rs");
        // code_search accepts the CLI's alias keys in precedence order.
        assert_eq!(gist("code_search", json!({"pattern": "Widget"})), "Search code for Widget");
        assert_eq!(gist("code_search", json!({"q": "Widget"})), "Search code for Widget");
        assert_eq!(gist("code_search", json!({"query": "A", "pattern": "B"})), "Search code for A");
        // MCP-wrapped builtins phrase identically.
        assert_eq!(gist("mcp__fs__list_files", json!({"path": "checkpoints"})), "List files in checkpoints");
    }

    #[test]
    fn gist_previews_multiline_commands_like_cli_compact_rows() {
        use serde_json::json;
        let gist = |arguments: serde_json::Value| gist_with("exec_command", Some(arguments), None);
        // The session's 3-line pipeline titles with its first line: separator
        // echoes in later lines never reach a single-line title.
        assert_eq!(
            gist(json!({"command": "sed -n '194,230p' README.md | head -n 40\necho \"---NUM---\"\nnl -ba README.md"})),
            "Ran sed -n '194,230p' README.md | head -n 40"
        );
        // Script-runner prefixes still fold in content instead of dangling.
        assert_eq!(gist(json!({"command": "python3 -c \"\nimport json"})), "Ran python3 -c \"import json");
    }

    #[test]
    fn legacy_mcp_prefix_classifies_and_phrases_like_cli() {
        use serde_json::json;
        assert_eq!(ToolCategory::from_tool_name("mcp_fetch", None), ToolCategory::Read);
        assert_eq!(gist_with("mcp_fetch", Some(json!({"url": "https://x.y/z"})), None), "Fetch https://x.y/z");
        assert_eq!(gist_with("mcp_fetch", None, None), "Fetch");
        // Asymmetric: the qualified form still wins over the legacy fallthrough.
        assert_eq!(ToolCategory::from_tool_name("mcp__fs__read_file", None), ToolCategory::Read);
        assert_eq!(ToolCategory::from_tool_name("mcp_server_tool", None), ToolCategory::Other);
    }

    #[test]
    fn gist_relativizes_headline_paths_against_workspace() {
        use serde_json::json;
        let root = Path::new("/ws");
        let gist_in = |name: &str, arguments: serde_json::Value| gist_with(name, Some(arguments), Some(root));
        assert_eq!(gist_in("list_files", json!({"path": "/ws/checkpoints"})), "List files in checkpoints");
        assert_eq!(
            gist_in("grep_file", json!({"pattern": "TODO", "path": "/ws/src/main.rs"})),
            "Grep TODO in src/main.rs"
        );
        // Asymmetric: outside the workspace the absolute path survives verbatim.
        assert_eq!(gist_with("list_files", Some(json!({"path": "/elsewhere"})), None), "List files in /elsewhere");
    }

    #[test]
    fn extra_gist_keys_follow_search_consumption() {
        use serde_json::json;
        let keys = |name: &str, arguments: serde_json::Value| {
            extra_gist_keys(&ToolInvocationItem {
                tool_name: name.into(),
                arguments: Some(arguments),
                tool_call_id: None,
                status: ToolCallStatus::Completed,
                outcome: None,
            })
        };
        // code_search reports the matched alias only, whatever its siblings —
        // including noise/verbose keys the details would hide anyway.
        assert_eq!(keys("code_search", json!({"query": "A", "pattern": "B"})), &["query"][..]);
        assert_eq!(keys("code_search", json!({"pattern": "A", "q": "B"})), &["pattern"][..]);
        assert_eq!(keys("code_search", json!({"q": "A", "text": "B"})), &["q"][..]);
        // Bare grep reports the first of pattern/query.
        assert_eq!(keys("grep", json!({"pattern": "A", "query": "B"})), &["pattern"][..]);
        assert_eq!(keys("grep", json!({"query": "A"})), &["query"][..]);
        // Headline-less calls and other tools hide nothing.
        let empty: &[&str] = &[];
        assert_eq!(keys("code_search", json!({"limit": 5})), empty);
        assert_eq!(keys("read_file", json!({"path": "a"})), empty);
    }
}
