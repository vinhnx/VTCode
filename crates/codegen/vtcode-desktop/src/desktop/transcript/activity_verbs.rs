//! Verb categories for tool-group summaries ("Read 2 files · Ran 1 command").
//!
//! A category is assigned once at projection from the typed event details and
//! never changes afterwards, so group headers fold the rows at render time
//! instead of maintaining incremental counters: unlike streaming status,
//! there is no derived state that could drift.

use std::borrow::Cow;
use std::path::Path;

use vtcode_commons::formatting::{truncate_middle, truncate_path_middle};
use vtcode_exec_events::{HarnessEventKind, ThreadItemDetails, ToolInvocationItem};

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

/// Display a path relative to the workspace root when it lies within it. Mirrors the
/// CLI `relativize_to_workspace`. Shared by the title gist readers and the
/// detail lines so both surfaces relativize identically.
pub(crate) fn relativize_to_workspace<'a>(path: &'a str, workspace_root: Option<&Path>) -> Cow<'a, str> {
    let Some(root) = workspace_root else {
        return Cow::Borrowed(path);
    };
    let candidate = Path::new(path);
    if candidate.is_absolute() {
        if let Ok(rel) = candidate.strip_prefix(root) {
            if !rel.as_os_str().is_empty() {
                return Cow::Owned(rel.to_string_lossy().into_owned());
            }
        }
    }
    Cow::Borrowed(path)
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
    // Legacy single-underscore form (`is_legacy_mcp_tool_name`); the qualified
    // arms above already excluded `mcp__`, so this cannot misfire on them.
    if let Some(rest) = tool_name.strip_prefix("mcp_") {
        return rest;
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

/// Whether a harness event belongs in the user-visible transcript.
///
/// Routine telemetry — latency records, lifecycle starts and passes,
/// per-turn snapshots, delegation observations, mailbox plumbing — stays out:
/// the transcript is a conversation surface, and the canonical event still
/// carries the complete evidence for forensics. Failures, blocks, losses
/// (including an expired undelivered mailbox message), recoveries, rewinds,
/// background completions, and limit grants stay: the user can act on them.
/// Exhaustive on purpose — a new `HarnessEventKind` must make a conscious
/// display choice here at compile time instead of silently inheriting
/// visibility.
#[must_use]
pub(crate) fn harness_is_user_visible(kind: &HarnessEventKind) -> bool {
    match kind {
        HarnessEventKind::ToolLatencyRecorded
        | HarnessEventKind::PlanningStarted
        | HarnessEventKind::PlanningCompleted
        | HarnessEventKind::ContinuationStarted
        | HarnessEventKind::ContinuationSkipped
        | HarnessEventKind::EvaluationStarted
        | HarnessEventKind::EvaluationPassed
        | HarnessEventKind::RevisionStarted
        | HarnessEventKind::VerificationStarted
        | HarnessEventKind::VerificationPassed
        | HarnessEventKind::SnapshotCreated
        | HarnessEventKind::DelegatedAgentStatus
        | HarnessEventKind::SessionMessageSent
        | HarnessEventKind::SessionMessageReceived => false,
        HarnessEventKind::SessionMessageExpired
        | HarnessEventKind::TurnBlocked
        | HarnessEventKind::BlockedRecoveryStarted
        | HarnessEventKind::BlockedRecoveryFinished
        | HarnessEventKind::BlockedHandoffWritten
        | HarnessEventKind::BlockedHandoffResolved
        | HarnessEventKind::EvaluationFailed
        | HarnessEventKind::EscalationTriggered
        | HarnessEventKind::EscalationBypassed
        | HarnessEventKind::VerificationFailed
        | HarnessEventKind::ErrorRecovered
        | HarnessEventKind::ToolRetryAttempted
        | HarnessEventKind::SnapshotRestored
        | HarnessEventKind::SessionToolLimitIncreased
        | HarnessEventKind::ToolLoopLimitIncreased
        | HarnessEventKind::BackgroundSubprocessCompleted => true,
    }
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
/// keeps rendering the `Ran` command headline.
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

/// One-line outcome gist for a tool row ("Read Cargo.toml", "Ran cargo check").
///
/// Session follow-ups take their action-aware title first. Tool-specific
/// headlines mirror the CLI describers (`tool_summary_helpers`): `list_files`
/// phrases its location or pattern, `grep_file` phrases pattern and scope,
/// runs lead with `Ran`, and searches phrase their query — so a bare command
/// or filename can never be mistaken for its verb. Read/Edit rows keep the
/// basename form, matching the reference transcript. Anything without a
/// usable object (missing, empty, or mistyped args, `Other` tools) takes the
/// prettified tool name, never a raw identifier.
pub(crate) fn tool_row_gist(tool: &ToolInvocationItem, workspace_root: Option<&Path>) -> String {
    let args = tool.arguments.as_ref();
    if let Some(title) = session_tool_title(&tool.tool_name, args) {
        return title;
    }
    if let Some(args) = args {
        let name = strip_mcp_prefix(&tool.tool_name);
        if name == "list_files" {
            if let Some((title, _)) = list_files_headline(args, workspace_root) {
                return title;
            }
        } else if name == "grep_file" {
            if let Some((title, _)) = grep_file_headline(args, workspace_root) {
                return title;
            }
        }
    }
    let category = ToolCategory::from_tool_name(&tool.tool_name, args);
    let gist = match category {
        ToolCategory::Read => args.and_then(read_object).map(|object| format!("Read {object}")),
        ToolCategory::Edit => args.and_then(file_basename).map(|base| format!("Edited {base}")),
        ToolCategory::Run => args.and_then(command_text).map(|command| format!("Ran {command}")),
        ToolCategory::Search => args.and_then(|args| search_gist(&tool.tool_name, args)),
        // Tracker rows keep the tool name; the inline task block below
        // the title carries the checklist detail.
        ToolCategory::Tracker | ToolCategory::Other => None,
    };
    gist.unwrap_or_else(|| pretty_tool_name(&tool.tool_name))
}

/// Argument keys the row title consumes beyond its category's gist keys.
///
/// Recomputed from the same headline readers the title uses (mirroring the
/// CLI describers' `(headline, used)` shape), so a key hides if and only if
/// the title actually phrased it: multi-key calls show the keys the title
/// skipped, and headline-less calls hide nothing.
#[must_use]
pub(crate) fn extra_gist_keys(tool: &ToolInvocationItem) -> &'static [&'static str] {
    let Some(args) = tool.arguments.as_ref() else {
        return &[];
    };
    // Workspace is irrelevant here: consumed keys never depend on display paths.
    match strip_mcp_prefix(&tool.tool_name) {
        "grep_file" => grep_file_headline(args, None).map(|(_, keys)| keys).unwrap_or(&[]),
        "list_files" => list_files_headline(args, None).map(|(_, keys)| keys).unwrap_or(&[]),
        _ => &[],
    }
}

/// Mirror of the CLI `describe_list_files`: location first, then the name and
/// content pattern fallbacks. Truncation budgets (60/40) match the CLI. Each
/// arm reports exactly the keys its headline consumed.
fn list_files_headline(
    args: &serde_json::Value,
    workspace_root: Option<&Path>,
) -> Option<(String, &'static [&'static str])> {
    if let Some(path) = first_string(args, &["path"]) {
        let location = if path == "." {
            "workspace root".to_string()
        } else {
            truncate_path_middle(&relativize_to_workspace(&path, workspace_root), 60)
        };
        return Some((format!("List files in {location}"), &["path"]));
    }
    if let Some(pattern) = first_string(args, &["name_pattern"]) {
        return Some((format!("Find files named {}", truncate_middle(&pattern, 40)), &["name_pattern"]));
    }
    first_string(args, &["content_pattern"]).map(|pattern| {
        let keys: &'static [&'static str] = &["content_pattern"];
        (format!("Search files for {}", truncate_middle(&pattern, 40)), keys)
    })
}

/// Mirror of the CLI `describe_grep_file`: the pattern is required, the scope
/// joins it when present. Truncation budgets (40/40) match the CLI. Reports
/// exactly the keys its headline consumed.
fn grep_file_headline(
    args: &serde_json::Value,
    workspace_root: Option<&Path>,
) -> Option<(String, &'static [&'static str])> {
    let pattern = first_string(args, &["pattern"])?;
    let path = first_string(args, &["path"]);
    let mut title = format!("Grep {}", truncate_middle(&pattern, 40));
    let keys: &'static [&'static str] = match path {
        Some(path) => {
            title.push_str(&format!(
                " in {}",
                truncate_path_middle(&relativize_to_workspace(&path, workspace_root), 40)
            ));
            &["pattern", "path"]
        }
        None => &["pattern"],
    };
    Some((title, keys))
}

/// Search headline per tool. `code_search` mirrors the CLI
/// `describe_code_search` (same key precedence); anything else in the Search
/// category phrases a file search over the pattern.
fn search_gist(tool_name: &str, args: &serde_json::Value) -> Option<String> {
    if strip_mcp_prefix(tool_name) == "code_search" {
        return first_string(args, &["query", "pattern", "q", "text"])
            .map(|query| format!("Search code for {}", truncate_middle(&query, 40)));
    }
    first_string(args, &["pattern", "query"])
        .map(|pattern| format!("Search files for {}", truncate_middle(&pattern, 40)))
}

/// Human fallback for tool names the gist readers cannot phrase.
///
/// Strips MCP prefixes, then splits `snake_case` and `PascalCase`/`camelCase`
/// boundaries into capitalized words (`DiagramCommand` → `Diagram Command`,
/// `diagram_render` → `Diagram Render`). Only the first letter of each word
/// is touched, so acronyms (`MCP`) and versions (`v2`) survive. Never
/// returns blank: a name with no word characters falls back to itself.
fn pretty_tool_name(tool_name: &str) -> String {
    let bare = strip_mcp_prefix(tool_name);
    let mut words: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut previous: Option<char> = None;
    // Single char walk, no slicing: `_` always breaks, and a new word starts
    // before an uppercase letter that follows a lowercase letter or digit.
    // Acronym runs (`MCPTool`) stay whole rather than guessing.
    for c in bare.chars() {
        if c == '_' {
            if !current.is_empty() {
                words.push(std::mem::take(&mut current));
            }
        } else {
            let boundary = previous.is_some_and(|prev| c.is_uppercase() && (prev.is_lowercase() || prev.is_numeric()));
            if boundary && !current.is_empty() {
                words.push(std::mem::take(&mut current));
            }
            current.push(c);
        }
        previous = Some(c);
    }
    if !current.is_empty() {
        words.push(current);
    }
    if words.is_empty() {
        return tool_name.to_string();
    }
    for word in &mut words {
        // Only the first letter is touched, so acronyms (`MCP`) and versions
        // (`v2`) survive; the rest keeps its original casing.
        let mut chars = word.chars();
        if let Some(first) = chars.next() {
            let mut pretty = first.to_uppercase().collect::<String>();
            pretty.push_str(chars.as_str());
            *word = pretty;
        }
    }
    words.join(" ")
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

#[cfg(test)]
mod tests {
    use super::*;
    use vtcode_exec_events::{HarnessEventKind::*, ToolCallStatus};

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
    fn legacy_mcp_prefix_classifies_and_phrases_like_cli() {
        use serde_json::json;
        assert_eq!(ToolCategory::from_tool_name("mcp_fetch", None), ToolCategory::Read);
        assert_eq!(gist_with("mcp_fetch", Some(json!({"url": "https://x.y/z"})), None), "Read https://x.y/z");
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
    fn harness_visibility_pins_every_kind() {
        // Routine telemetry stays out of the transcript …
        for kind in [
            ToolLatencyRecorded,
            PlanningStarted,
            PlanningCompleted,
            ContinuationStarted,
            ContinuationSkipped,
            EvaluationStarted,
            EvaluationPassed,
            RevisionStarted,
            VerificationStarted,
            VerificationPassed,
            SnapshotCreated,
            DelegatedAgentStatus,
            SessionMessageSent,
            SessionMessageReceived,
        ] {
            assert!(!harness_is_user_visible(&kind), "{kind:?} stays out of the transcript");
        }
        // … while anything the user can act on stays visible.
        for kind in [
            SessionMessageExpired,
            TurnBlocked,
            BlockedRecoveryStarted,
            BlockedRecoveryFinished,
            BlockedHandoffWritten,
            BlockedHandoffResolved,
            EvaluationFailed,
            EscalationTriggered,
            EscalationBypassed,
            VerificationFailed,
            ErrorRecovered,
            ToolRetryAttempted,
            SnapshotRestored,
            SessionToolLimitIncreased,
            ToolLoopLimitIncreased,
            BackgroundSubprocessCompleted,
        ] {
            assert!(harness_is_user_visible(&kind), "{kind:?} stays visible");
        }
    }
}
