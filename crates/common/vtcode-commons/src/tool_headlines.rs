//! Shared tool-call headlines and transcript detail lines.
//!
//! Single home for the display phrases both transcript surfaces render — the
//! CLI (`src/agent/runloop/unified/tool_summary*`) and the desktop app
//! (`vtcode-desktop/src/desktop/transcript/`) — so a headline or skip list can
//! never drift between them. String-based by design (same contract as
//! [`crate::tracker_title`]): callers walk their own payload shapes and
//! converge here for the humanization, truncation, and fallback rules.
//!
//! Surface composition stays local: the CLI keeps its `(headline, used)`
//! dispatcher, `MCP ` prefix policy, and TUI renderers; the desktop keeps its
//! verb categories, single-line collapse, and GPUI renderers. Only the pure
//! phrase builders live here.

use std::borrow::Cow;
use std::path::Path;

use serde_json::Value;

use crate::formatting::{collapse_whitespace, truncate_middle, truncate_path_middle};

/// First non-blank string under a single key, trimmed.
#[must_use]
pub fn lookup_string(args: &Value, key: &str) -> Option<String> {
    lookup_first_string(args, &[key])
}

/// First non-blank string under the given keys, trimmed.
///
/// Blank and mistyped values are skipped so a headline never phrases
/// whitespace: trimming here (not at each caller) keeps every describer
/// total over messy agent payloads.
#[must_use]
pub fn lookup_first_string(args: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .filter_map(|key| args.get(key).and_then(Value::as_str))
        .find_map(|text| {
            let trimmed = text.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        })
}

/// Display a path relative to the workspace root when it lies within it.
///
/// Returns the path unchanged when `workspace_root` is `None`, the path is
/// not absolute, or it does not lie within the root (including the
/// degenerate path-equals-root case, kept in its original form).
#[must_use]
pub fn relativize_to_workspace<'a>(path: &'a str, workspace_root: Option<&Path>) -> Cow<'a, str> {
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

/// Keys whose values are file-system paths, shown relative to the workspace.
#[must_use]
pub fn is_path_key(key: &str) -> bool {
    matches!(key, "path" | "file_path" | "filename" | "destination" | "source")
}

/// Humanize a parameter key (`timeout_secs` → `Timeout secs`).
///
/// Only the first letter is capitalized: detail lines read as labels, not
/// titles. Tool *names* take [`humanize_tool_name`] instead, which splits
/// word boundaries per word.
#[must_use]
pub fn humanize_key(key: &str) -> String {
    let replaced = key.replace('_', " ");
    if replaced.is_empty() {
        return replaced;
    }
    let mut chars = replaced.chars();
    let first = chars.next().unwrap_or_default();
    let mut result = first.to_uppercase().collect::<String>();
    result.push_str(&chars.collect::<String>());
    result
}

/// Human fallback for tool names the headline readers cannot phrase.
///
/// Strips MCP prefixes, then splits `snake_case` and `PascalCase`/`camelCase`
/// boundaries into capitalized words (`DiagramCommand` → `Diagram Command`,
/// `diagram_render` → `Diagram Render`). Only the first letter of each word
/// is touched, so acronyms (`MCP`) and versions (`v2`) survive. Never
/// returns blank: a name with no word characters falls back to itself.
#[must_use]
pub fn humanize_tool_name(tool_name: &str) -> String {
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

/// Bare tool name from an MCP-qualified one (`None` for non-MCP names).
///
/// Accepts the `mcp__server__tool`, `mcp::provider::tool`, and legacy
/// `mcp_tool` spellings. Canonical MCP name parser — do not fork the
/// prefix/segment handling per call site, so every surface labels the same
/// tool identically. Surfaces that drop the prefix unconditionally take
/// [`strip_mcp_prefix`] instead.
#[must_use]
pub fn mcp_tool_display_name(tool_name: &str) -> Option<&str> {
    if tool_name.starts_with("mcp__") {
        return Some(tool_name.split("__").last().unwrap_or(tool_name));
    }
    if tool_name.starts_with("mcp::") {
        return Some(tool_name.split("::").last().unwrap_or(tool_name));
    }
    if let Some(rest) = tool_name.strip_prefix("mcp_") {
        return Some(rest);
    }
    None
}

/// MCP prefixes stripped, or the name itself when it carries none.
///
/// Total form of [`mcp_tool_display_name`] for surfaces that never show the
/// prefix. The legacy single-underscore arm runs last, so it cannot misfire
/// on the qualified `mcp__` form.
#[must_use]
pub fn strip_mcp_prefix(tool_name: &str) -> &str {
    mcp_tool_display_name(tool_name).unwrap_or(tool_name)
}

/// Summarize a string list to two items plus an overflow count.
#[must_use]
pub fn summarize_list(items: &[String]) -> String {
    if items.is_empty() {
        return String::new();
    }
    let shown: Vec<String> = items.iter().take(2).map(|s| truncate_middle(s, 60)).collect();
    if items.len() > 2 {
        format!("{} +{} more", shown.join(", "), items.len() - 2)
    } else {
        shown.join(", ")
    }
}

/// Raw content keys too verbose for a summary row (commands, file bodies).
#[must_use]
pub fn is_verbose_key(key: &str) -> bool {
    matches!(
        key,
        "command"
            | "raw_command"
            | "bash_command"
            | "cmd"
            | "old_str"
            | "new_str"
            | "content"
            | "new_content"
            | "text"
            | "patch"
            | "code"
    )
}

/// Whether `text` is an accepted query alias for this tool rather than a body.
///
/// `code_search` phrases `text` as a last-resort query alias, so an
/// unconsumed `text` value shows instead of hiding as verbose content;
/// everywhere else (edits, unknown tools) `text` stays a hidden body.
fn text_is_search_query(tool_name: &str, key: &str) -> bool {
    key == "text" && strip_mcp_prefix(tool_name) == "code_search"
}

/// Parameter keys that are infrastructure/plumbing noise, omitted from the
/// human-facing summary. Session plumbing (`session_id`, `chars`, wait
/// deadlines) is not noise: [`exec_session_param_detail`] folds it into one
/// identity row instead. Search strings (`query`) are content, never
/// plumbing: they hide only when the headline consumed them.
#[must_use]
pub fn is_noise_param(key: &str) -> bool {
    matches!(
        key,
        // Timeouts and size limits
        "timeout_secs"
            | "timeout"
            | "max_bytes"
            | "max_matches"
            // Search plumbing
            | "debug_query"
            | "strictness"
            | "case_sensitive"
            | "literal"
            | "context_lines"
            // Execution plumbing
            | "shell"
            | "login"
            | "tty"
            | "sandbox_permissions"
            | "additional_permissions"
            | "justification"
            | "prefix_rule"
            | "workdir"
            | "cwd"
            | "language"
            | "spool_path"
            // Tool identity / routing
            | "type"
            | "tool_call_id"
            | "call_type"
            // Redundant with summary headline (e.g., "Read file" already implies action=read)
            | "action"
            // Output caps, same plumbing class as the size limits above.
            | "max_output_tokens"
            | "max_tokens"
    )
}

/// Human `Key: value` lines for the arguments a headline does not consume.
///
/// Verbose content keys, plumbing noise, and the caller-supplied `skipped`
/// keys (headline-consumed) stay out; strings truncate to 60 columns, paths
/// relativize against the workspace, lists summarize to two items, and only
/// `true` flags and non-zero numbers render. Key order follows
/// `serde_json::Map` iteration. An empty `skipped` slice shows every
/// displayable key — no separate include-all flag needed. `tool_name` (MCP
/// prefixes accepted) decides the `text` exemption below.
#[must_use]
pub fn collect_param_details(
    tool_name: &str,
    args: &Value,
    skipped: &[&str],
    workspace_root: Option<&Path>,
) -> Vec<String> {
    let mut details = Vec::new();
    let Some(map) = args.as_object() else {
        return details;
    };
    for (key, value) in map {
        let verbose_hidden = is_verbose_key(key) && !text_is_search_query(tool_name, key);
        if is_noise_param(key) || verbose_hidden || skipped.contains(&key.as_str()) {
            continue;
        }
        match value {
            Value::String(s) if !s.is_empty() => {
                // Render file-system path values relative to the workspace root.
                let display: Cow<'_, str> = if is_path_key(key) {
                    relativize_to_workspace(s, workspace_root)
                } else {
                    Cow::Borrowed(s.as_str())
                };
                details.push(format!("{}: {}", humanize_key(key), truncate_middle(&display, 60)))
            }
            Value::Bool(true) => {
                details.push(humanize_key(key));
            }
            Value::Array(items) => {
                let strings: Vec<String> =
                    items.iter().filter_map(|item| item.as_str().map(|s| s.to_string())).collect();
                if !strings.is_empty() {
                    details.push(format!("{}: {}", humanize_key(key), summarize_list(&strings)));
                }
            }
            Value::Number(num) => {
                // Skip zero-valued numbers — they are defaults and add no information.
                if num.as_f64().is_some_and(|n| n == 0.0) {
                    continue;
                }
                details.push(format!("{}: {}", humanize_key(key), num));
            }
            _ => {}
        }
    }
    details
}

/// One row for an exec-session follow-up: session identity plus an explicit
/// wait deadline.
///
/// Output-token caps, yield windows, and the raw stdin payload are
/// deliberately dropped: the model receives the arguments unchanged, and the
/// reader only needs the session at a glance. Returns `None` when the call
/// carries neither, so non-session tools keep their normal detail rows.
#[must_use]
pub fn exec_session_param_detail(args: &Value) -> Option<String> {
    let session = lookup_string(args, "session_id").map(|id| format!("Session {id}"));
    let wait = ["wait_timeout_seconds", "timeout_seconds"]
        .iter()
        .find_map(|key| args.get(key).and_then(Value::as_u64))
        .filter(|secs| *secs > 0)
        .map(|secs| format!("wait up to {secs}s"));

    let parts = [session, wait].into_iter().flatten().collect::<Vec<_>>();
    (!parts.is_empty()).then(|| parts.join(" · "))
}

/// Headline for `list_files`: location first, then the name and content
/// pattern fallbacks. Reports exactly the keys its headline consumed, so
/// detail collection hides if and only if the title phrased them.
#[must_use]
pub fn describe_list_files(args: &Value, workspace_root: Option<&Path>) -> Option<(String, &'static [&'static str])> {
    if let Some(path) = lookup_string(args, "path") {
        let location = if path == "." {
            "workspace root".to_string()
        } else {
            let rel = relativize_to_workspace(&path, workspace_root);
            truncate_path_middle(&rel, 60)
        };
        return Some((format!("List files in {location}"), &["path"]));
    }
    if let Some(pattern) = lookup_string(args, "name_pattern") {
        return Some((format!("Find files named {}", truncate_middle(&pattern, 40)), &["name_pattern"]));
    }
    if let Some(pattern) = lookup_string(args, "content_pattern") {
        return Some((format!("Search files for {}", truncate_middle(&pattern, 40)), &["content_pattern"]));
    }
    None
}

/// Headline for `grep_file`: the pattern is required, the scope joins it
/// when present. Reports exactly the keys its headline consumed.
#[must_use]
pub fn describe_grep_file(args: &Value, workspace_root: Option<&Path>) -> Option<(String, &'static [&'static str])> {
    let pattern = lookup_string(args, "pattern");
    let path = lookup_string(args, "path");
    match (pattern, path) {
        (Some(pat), Some(path)) => Some((
            format!(
                "Grep {} in {}",
                truncate_middle(&pat, 40),
                truncate_path_middle(&relativize_to_workspace(&path, workspace_root), 40)
            ),
            &["pattern", "path"],
        )),
        (Some(pat), None) => Some((format!("Grep {}", truncate_middle(&pat, 40)), &["pattern"])),
        _ => None,
    }
}

/// Headline for `code_search` over the query and its alias keys.
///
/// The schema requires `query`, but common aliases are accepted defensively
/// so a valid search never degrades to a generic header. Records the actual
/// matched key so detail collection and headline highlights stay in sync (a
/// hardcoded "query" would leak e.g. `Pattern: …` as a duplicate line).
#[must_use]
pub fn describe_code_search(args: &Value) -> Option<(String, &'static [&'static str])> {
    for key in ["query", "pattern", "q", "text"] {
        if let Some(query) = lookup_string(args, key) {
            let keys: &'static [&'static str] = match key {
                "query" => &["query"],
                "pattern" => &["pattern"],
                "q" => &["q"],
                _ => &["text"],
            };
            return Some((format!("Search code for {}", truncate_middle(&query, 40)), keys));
        }
    }
    None
}

/// Headline for the fetch tools over the full target URL.
///
/// Approval surfaces must show the full URL (not just the domain) so the
/// reader reviews the exact target. Falls back to a bare verb when no URL
/// is present, consuming no keys.
#[must_use]
pub fn describe_fetch_action(args: &Value) -> (String, &'static [&'static str]) {
    if let Some(url) = lookup_string(args, "url") {
        return (format!("Fetch {url}"), &["url"]);
    }
    ("Use Fetch".to_string(), &[])
}

/// Exact flag tokens that introduce an inline script for a runner.
/// Token-exact (not substring) so `grep -c` / `--code-review` never match.
const SCRIPT_RUNNER_FLAGS: &[&str] = &["-c", "-e", "--code"];

/// Compact single-line preview of a command for `• Ran …` headers.
///
/// Multi-line commands preview their first non-empty line; long commands
/// are head-truncated at a word boundary with a trailing ellipsis. The
/// mid-string ellipsis of `truncate_middle` is avoided on purpose: cutting
/// `checkpoints/turn_1032` into `tur…ool_calls` reads as a rendering bug.
///
/// When the first line is only a script-runner prefix (`python3 -c "`,
/// bare `python3 -c`, or an unclosed quote on a `-c`/`-e`/`--code` runner),
/// the following lines are folded in so the header shows real script content
/// (`python3 -c "import json …`) instead of a dangling `python3 -c "`.
#[must_use]
pub fn preview_command(command: &str, max_len: usize) -> String {
    if max_len == 0 {
        return String::new();
    }
    let lines: Vec<&str> = command.lines().map(str::trim).filter(|line| !line.is_empty()).collect();
    let Some((first, rest)) = lines.split_first() else {
        return String::new();
    };
    let mut first_line = collapse_whitespace(first);
    if is_script_prefix_only(&first_line) && !rest.is_empty() {
        // Bounded: only fold enough continuation lines to fill the preview
        // budget instead of materializing a 100-line script into one String.
        let need = max_len.saturating_sub(first_line.chars().count()).saturating_add(16);
        let mut rest_parts = Vec::new();
        let mut rest_len = 0usize;
        for line in rest {
            let collapsed = collapse_whitespace(line);
            if collapsed.is_empty() {
                continue;
            }
            rest_len += collapsed.chars().count() + 1;
            rest_parts.push(collapsed);
            if rest_len >= need {
                break;
            }
        }
        if !rest_parts.is_empty() {
            let rest = rest_parts.join(" ");
            if first_line.ends_with('"') || first_line.ends_with('\'') {
                // `python3 -c "` + `import json` -> `python3 -c "import json`
                // (no extra space: the newline after the opening quote is
                // insignificant, and `collapse_whitespace` would leave a
                // distracting `" import` gap).
                first_line.push_str(&rest);
            } else {
                first_line.push(' ');
                first_line.push_str(&rest);
            }
        }
    }
    if first_line.chars().count() <= max_len {
        return first_line;
    }

    let budget = max_len.saturating_sub(1);
    let mut head: String = first_line.chars().take(budget).collect();
    if let Some(last_space) = head.rfind(char::is_whitespace) {
        // Only honor the word boundary when it keeps at least half the budget,
        // so a late first space does not collapse the preview to almost nothing.
        if last_space >= budget / 2 {
            head.truncate(last_space);
        }
    }
    format!("{}…", head.trim_end())
}

/// Full-length variant of [`preview_command`] for transcript surfaces.
///
/// Keeps the first-line selection and script-runner prefix folding (so
/// `python3 -c "` still pulls in script content) but applies no length cap:
/// the selected line shows in full, with viewport-aware wrapping owning the
/// overflow instead of a `…` truncation.
#[must_use]
pub fn preview_full_command(command: &str) -> String {
    preview_command(command, usize::MAX)
}

/// Whether `program` names a known inline-script runner (`python3`, `bash`,
/// `node`, …). Compared against the path basename so `/usr/bin/python3` and
/// Windows-style `C:\Python\python.exe` both match, keeping the check
/// token-exact instead of substring-based.
fn is_script_runner_program(program: &str) -> bool {
    let base = program.rsplit(['/', '\\']).next().unwrap_or(program).to_ascii_lowercase();
    base.starts_with("python")
        || matches!(
            base.as_str(),
            "bash" | "sh" | "zsh" | "node" | "ruby" | "perl" | "php" | "deno" | "bun" | "pwsh" | "powershell"
        )
}

/// Whether the first preview line is just a script-runner prefix with no
/// usable script content (e.g. `python3 -c "` or bare `python3 -c`).
///
/// Rendering such a prefix alone reads as broken (`• Ran python3 -c "`).
/// Token-exact on both program and flag so `grep -c`, `grep -e`, and
/// `--code-review` never match. Unclosed quoting is detected with the
/// canonical `shell_words` parser (not raw `'` counting) so contractions
/// inside a balanced script (`python3 -c "don't …"`) do not misfire.
fn is_script_prefix_only(first_line: &str) -> bool {
    let mut tokens = first_line.split_whitespace();
    let Some(program) = tokens.next() else {
        return false;
    };
    if !is_script_runner_program(program) {
        return false;
    }
    if !tokens.clone().any(|token| SCRIPT_RUNNER_FLAGS.contains(&token)) {
        return false;
    }
    // Bare flag with no script yet: `python3 -c`.
    let token_count = 1 + tokens.clone().count();
    if token_count == 2 {
        return true;
    }
    // Unclosed quoting means the script continues on later lines, e.g.
    // `python3 -c "` or `python3 -c "import json`.
    shell_words::split(first_line).is_err()
}

/// Action-aware label for `command_session_internal` follow-up calls.
///
/// Canonical home of the session labels: `vtcode-core` re-exports this, and
/// transcript surfaces call it directly. Launches take `Run command` /
/// `Run code`; callers that phrase launches from the command itself (rather
/// than a generic label) resolve [`crate::tool_intent::command_session_action`]
/// first and skip the `run` action.
#[must_use]
pub fn unified_exec_action_label(args: &Value) -> &'static str {
    let action = crate::tool_intent::command_session_action(args).unwrap_or("run");
    if action.eq_ignore_ascii_case("run") {
        "Run command"
    } else if action.eq_ignore_ascii_case("write") {
        "Send command input"
    } else if action.eq_ignore_ascii_case("poll") {
        "Read command session"
    } else if action.eq_ignore_ascii_case("wait") {
        "Wait for command session"
    } else if action.eq_ignore_ascii_case("continue") {
        "Continue command session"
    } else if action.eq_ignore_ascii_case("inspect") {
        "Inspect command output"
    } else if action.eq_ignore_ascii_case("list") {
        "List command sessions"
    } else if action.eq_ignore_ascii_case("close") {
        "Close command session"
    } else if action.eq_ignore_ascii_case("code") {
        "Run code"
    } else {
        "Exec action"
    }
}

/// Action-aware label for `write_stdin` calls: explicit actions win,
/// otherwise input sniffing decides send vs read so pure waits never
/// misrender as sends.
#[must_use]
pub fn write_stdin_action_label(args: &Value) -> &'static str {
    if let Some(action) = args.get("action").and_then(Value::as_str) {
        if action.eq_ignore_ascii_case("wait") {
            return "Wait for command session";
        }
        if action.eq_ignore_ascii_case("poll") {
            return "Read command session";
        }
        if action.eq_ignore_ascii_case("inspect") {
            return "Inspect command output";
        }
        if action.eq_ignore_ascii_case("close") {
            return "Close command session";
        }
        if action.eq_ignore_ascii_case("terminate") {
            return "Terminate command session";
        }
        if action.eq_ignore_ascii_case("write") {
            // Match `write_stdin_dispatch`: explicit write with empty/missing
            // input polls instead of sending, so label it as a read.
            return if has_session_input(args) {
                "Send command input"
            } else {
                "Read command session"
            };
        }
    }
    if has_session_input(args) {
        "Send command input"
    } else {
        // A session_id-only follow-up polls output; calling it "Send" misleads
        // (screenshot 2026-10-02: pure waits rendered as sends).
        "Read command session"
    }
}

/// Whether the call carries a non-empty stdin payload under the input keys.
#[must_use]
pub fn has_session_input(args: &Value) -> bool {
    ["chars", "input", "text"]
        .iter()
        .any(|key| args.get(key).and_then(Value::as_str).is_some_and(|s| !s.is_empty()))
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
///
/// Conversational surfaces (CLI transcript, desktop transcript) filter
/// through this; forensic surfaces (log viewers, event exports) keep the
/// complete stream.
#[must_use]
pub fn harness_is_user_visible(kind: &vtcode_exec_events::HarnessEventKind) -> bool {
    use vtcode_exec_events::HarnessEventKind::*;
    match kind {
        ToolLatencyRecorded
        | PlanningStarted
        | PlanningCompleted
        | ContinuationStarted
        | ContinuationSkipped
        | EvaluationStarted
        | EvaluationPassed
        | RevisionStarted
        | VerificationStarted
        | VerificationPassed
        | SnapshotCreated
        | DelegatedAgentStatus
        | SessionMessageSent
        | SessionMessageReceived => false,
        SessionMessageExpired
        | TurnBlocked
        | BlockedRecoveryStarted
        | BlockedRecoveryFinished
        | BlockedHandoffWritten
        | BlockedHandoffResolved
        | EvaluationFailed
        | EscalationTriggered
        | EscalationBypassed
        | VerificationFailed
        | ErrorRecovered
        | ToolRetryAttempted
        | SnapshotRestored
        | SessionToolLimitIncreased
        | ToolLoopLimitIncreased
        | BackgroundSubprocessCompleted => true,
    }
}

#[cfg(test)]
mod tests;
