//! Verb categories for tool-group summaries ("Read 2 files · Ran 1 command").
//!
//! A category is assigned once at projection from the typed event details and
//! never changes afterwards, so group headers fold the rows at render time
//! instead of maintaining incremental counters: unlike streaming status,
//! there is no derived state that could drift.

use vtcode_exec_events::{ThreadItemDetails, ToolInvocationItem};

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

/// Action-aware title for exec-session follow-up calls ("Wait for command session").
///
/// Mirrors the service-side session labels (`write_stdin_action_label` and
/// `unified_exec_action_label` in `vtcode-core/src/tools/registry/labels.rs`) plus the
/// fixed follow-up labels. Returns `None` for launches and non-session tools so the
/// caller falls through to the argument gist, which is more specific than a generic
/// label. `Some` also doubles as the session predicate for detail lines: a titled call
/// renders the one-line session row. English literals, same rule as the group verbs.
pub(super) fn session_tool_title(tool_name: &str, args: Option<&serde_json::Value>) -> Option<String> {
    match strip_mcp_prefix(tool_name) {
        "send_pty_input" => Some("Send command input".to_string()),
        "read_pty_session" => Some("Read command session".to_string()),
        "write_stdin" => Some(write_stdin_title(args)),
        "command_session_internal" => unified_exec_title(args),
        _ => None,
    }
}

/// Mirror of `write_stdin_action_label`: explicit actions win, otherwise input
/// sniffing decides send vs read so pure waits never misrender as sends.
fn write_stdin_title(args: Option<&serde_json::Value>) -> String {
    if let Some(action) = args.and_then(|args| unified_action(Some(args))) {
        if action.eq_ignore_ascii_case("wait") {
            return "Wait for command session".to_string();
        }
        if action.eq_ignore_ascii_case("poll") {
            return "Read command session".to_string();
        }
        if action.eq_ignore_ascii_case("inspect") {
            return "Inspect command output".to_string();
        }
        if action.eq_ignore_ascii_case("close") {
            return "Close command session".to_string();
        }
        if action.eq_ignore_ascii_case("terminate") {
            return "Terminate command session".to_string();
        }
        if action.eq_ignore_ascii_case("write") {
            // Explicit write with empty/missing input polls instead of sending.
            return if has_session_input(args) {
                "Send command input".to_string()
            } else {
                "Read command session".to_string()
            };
        }
    }
    if has_session_input(args) {
        "Send command input".to_string()
    } else {
        "Read command session".to_string()
    }
}

/// Mirror of `unified_exec_action_label`, except launches return `None` so the gist
/// keeps rendering the bare command.
fn unified_exec_title(args: Option<&serde_json::Value>) -> Option<String> {
    let action = session_action(args).unwrap_or("run");
    if action.eq_ignore_ascii_case("run") {
        None
    } else if action.eq_ignore_ascii_case("write") {
        Some("Send command input".to_string())
    } else if action.eq_ignore_ascii_case("poll") {
        Some("Read command session".to_string())
    } else if action.eq_ignore_ascii_case("wait") {
        Some("Wait for command session".to_string())
    } else if action.eq_ignore_ascii_case("continue") {
        Some("Continue command session".to_string())
    } else if action.eq_ignore_ascii_case("inspect") {
        Some("Inspect command output".to_string())
    } else if action.eq_ignore_ascii_case("list") {
        Some("List command sessions".to_string())
    } else if action.eq_ignore_ascii_case("close") {
        Some("Close command session".to_string())
    } else if action.eq_ignore_ascii_case("code") {
        Some("Run code".to_string())
    } else {
        Some("Exec action".to_string())
    }
}

/// Mirror of `tool_intent::command_session_action` for display routing. Omits the
/// indexed-command-parts probe (`command_args` lives in vtcode-core, which the desktop
/// app must not depend on): indexed launches without an explicit action or a standard
/// command field fall through to the raw-name gist.
fn session_action(args: Option<&serde_json::Value>) -> Option<&str> {
    let args = args?;
    if let Some(action) = unified_action(Some(args)) {
        return Some(action);
    }
    if args.get("command").is_some() || args.get("cmd").is_some() || args.get("raw_command").is_some() {
        return Some("run");
    }
    if args.get("code").is_some() {
        return Some("code");
    }
    if args.get("input").is_some() || args.get("chars").is_some() || args.get("text").is_some() {
        return Some("write");
    }
    if args.get("spool_path").is_some()
        || args.get("query").is_some()
        || args.get("head_lines").is_some()
        || args.get("tail_lines").is_some()
        || args.get("max_matches").is_some()
        || args.get("literal").is_some()
    {
        return Some("inspect");
    }
    None
}

/// Mirror of the service `has_session_input`: non-empty payload under the input keys.
fn has_session_input(args: Option<&serde_json::Value>) -> bool {
    ["chars", "input", "text"].iter().any(|key| {
        args.and_then(|args| args.get(key))
            .and_then(serde_json::Value::as_str)
            .is_some_and(|s| !s.is_empty())
    })
}

/// One-line outcome gist for a tool row ("Read Cargo.toml", "cargo check").
///
/// Session follow-ups take their action-aware title first. Read/Edit rows take the
/// shared header verbs plus the argument's basename — a bare filename cannot tell
/// read from write, so only these two categories take a verb. Run/Search rows render
/// the command or query bare, matching their `CommandExecution`/`WebSearch` siblings.
/// Anything without a usable object (missing, empty, or mistyped args, `Other` tools)
/// keeps the raw tool name, never worse than today.
pub(crate) fn tool_row_gist(tool: &ToolInvocationItem) -> String {
    let args = tool.arguments.as_ref();
    if let Some(title) = session_tool_title(&tool.tool_name, args) {
        return title;
    }
    let category = ToolCategory::from_tool_name(&tool.tool_name, args);
    let gist = match category {
        ToolCategory::Read => args.and_then(read_object).map(|object| format!("Read {object}")),
        ToolCategory::Edit => args.and_then(file_basename).map(|base| format!("Edited {base}")),
        ToolCategory::Run => args.and_then(command_text),
        ToolCategory::Search => args.and_then(|args| first_string(args, &["query", "pattern"])),
        ToolCategory::Other => None,
    };
    gist.unwrap_or_else(|| tool.tool_name.clone())
}

/// File basename, else the bare URL fetch tools carry. URLs keep the host:
/// a basename would mislead ("page" from two different sites).
fn read_object(args: &serde_json::Value) -> Option<String> {
    file_basename(args).or_else(|| first_string(args, &["url"]))
}

/// Basename of the path argument, scanning keys in service order
/// (`primary_path_arg` in `vtcode-core/src/tools/safety_gateway/file_targets.rs`).
fn file_basename(args: &serde_json::Value) -> Option<String> {
    first_string(args, &["path", "file_path", "filepath", "target_path"])
        .as_deref()
        .and_then(basename)
        .map(str::to_string)
}

/// Command text in service display order (`DISPLAY_COMMAND_KEYS` in
/// `vtcode-core/src/tools/command_args/mod.rs`), plus the `command` array
/// form joined with spaces.
fn command_text(args: &serde_json::Value) -> Option<String> {
    if let Some(text) = first_string(args, &["command", "cmd", "raw_command", "bash_command"]) {
        return Some(text);
    }
    let parts: Vec<&str> = args
        .get("command")?
        .as_array()?
        .iter()
        .filter_map(serde_json::Value::as_str)
        .collect();
    let joined = parts.join(" ");
    (!joined.trim().is_empty()).then(|| joined.trim().to_string())
}

/// First non-blank string under the given keys, trimmed.
fn first_string(args: &serde_json::Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .filter_map(|key| args.get(key).and_then(serde_json::Value::as_str))
        .find_map(|text| {
            let trimmed = text.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        })
}

/// Last non-empty `/`- or `\`-separated segment. Plain splitting, not
/// `Path`: arg values are display strings, and `Path` separators are
/// platform-bound.
fn basename(path: &str) -> Option<&str> {
    path.split(['/', '\\']).rfind(|segment| !segment.is_empty())
}
