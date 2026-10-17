use hashbrown::HashSet;

use serde_json::Value;
use std::borrow::Cow;
use std::path::Path;
pub(super) use vtcode_commons::formatting::truncate_path_middle;
use vtcode_commons::formatting::{collapse_whitespace, truncate_middle};
use vtcode_core::tools::command_args;

pub(crate) use super::tool_pipeline::is_exec_session_call;

pub(super) fn humanize_tool_name(name: &str) -> String {
    humanize_key(name)
}

pub(super) fn describe_fetch_action(args: &Value) -> (String, HashSet<String>) {
    if let Some(url) = args
        .as_object()
        .and_then(|map| map.get("url"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|url| !url.is_empty())
    {
        // Approval modals must show the full URL (not just the domain) so the
        // user can review the exact target before allowing the fetch.
        let mut used = HashSet::new();
        used.insert("url".to_string());
        return (format!("Fetch {url}"), used);
    }
    ("Use Fetch".into(), HashSet::new())
}

/// Extract a shell command string from the common command argument keys.
///
/// Thin binary wrapper over [`command_args::extract_command_text_with_key`]:
/// spool preview generation belongs to `vtcode-core`, the binary only keeps
/// the typed `(text, key)` reference for highlight tracking.
fn extract_command(args: &Value) -> Option<(String, &'static str)> {
    command_args::extract_command_text_with_key(args)
}

/// Shared preview budgets so compact previews stay in sync. Expanded `• Ran`
/// headlines and live PTY headers show the command in full (TUI reflow owns
/// viewport-aware wrapping); only compact/collapsed surfaces (`$` detail
/// lines, viewer headers, `• Ran N commands` rows) head-truncate here:
/// 70 chars for expanded non-run summaries, 120 chars for compact previews.
pub(super) const SUMMARY_PREVIEW_LEN: usize = 70;
pub(super) const COMPACT_PREVIEW_LEN: usize = 120;

/// Shared `• Ran` header wrap widths so expanded summaries and live PTY headers
/// stay in sync. Canonical values live in `vtcode_commons::formatting` so
/// every surface (expanded summaries, live PTY headers, compact rows,
/// plain-text fallback) wraps identically; TUI reflow owns any residual
/// viewport overflow.
pub(super) use vtcode_commons::formatting::{RAN_COMMAND_CONTINUATION_WIDTH, RAN_COMMAND_FIRST_WIDTH};

/// Exact flag tokens that introduce an inline script for a runner.
/// Token-exact (not substring) so `grep -c` / `--code-review` never match.
const SCRIPT_RUNNER_FLAGS: &[&str] = &["-c", "-e", "--code"];

pub(super) fn describe_shell_command(args: &Value) -> Option<(String, HashSet<String>)> {
    let (command, key) = extract_command(args)?;
    let mut used = HashSet::new();
    used.insert(key.to_string());
    Some((preview_command(&command, SUMMARY_PREVIEW_LEN), used))
}

/// Join command words for display, quoting only words that contain whitespace.
///
/// Unlike `shell_words::join`, shell metacharacters (`|`, `>`, `;`) are left
/// bare: this string is rendered, never executed, and quoting every operator
/// made the `• Ran` headers read as broken shell.
///
/// Each word is collapsed to a single line first so a multi-line `python3 -c`
/// script does not leak raw newlines (with an unclosed quote) into the
/// header. When quoting is needed the outer quote is chosen to avoid nesting:
/// words containing `'` use `"`, and vice versa, so
/// `open('.vtcode/x.json')` does not render as nested `'...open('...')...'`.
fn display_join_words(words: &[String]) -> String {
    words
        .iter()
        .map(|word| collapse_whitespace(word))
        .filter(|collapsed| !collapsed.is_empty())
        .map(|collapsed| {
            if collapsed.chars().any(char::is_whitespace) {
                if !collapsed.contains('\'') {
                    format!("'{collapsed}'")
                } else if !collapsed.contains('"') {
                    format!("\"{collapsed}\"")
                } else {
                    // Contains both quote styles (e.g. `print("it's")`):
                    // display-only, so keep single-quote wrapping rather than
                    // emitting escaped `\'\'` noise.
                    format!("'{collapsed}'")
                }
            } else {
                collapsed
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Readable one-line display text for a command's arguments.
///
/// Words come from the `command` array/string (plus the `args` array when
/// present) and are joined with [`display_join_words`]. Falls back to the raw
/// string for keys like `bash_command` that `command_words` does not cover so
/// those headers do not degrade to a bare tool name.
pub(super) fn display_command_text(args: &Value) -> Option<String> {
    if let Some(words) = command_args::command_words(args).ok().flatten() {
        let joined = display_join_words(&words);
        if !joined.is_empty() {
            return Some(joined);
        }
    }
    // Fallback delegates to `extract_command` (single canonical key order) so
    // display precedence can never drift from `describe_shell_command`.
    // `command_words` fails on unbalanced quotes while the raw string is still
    // readable; `args[]` extras are appended to match the primary path.
    let (raw, _) = extract_command(args)?;
    let mut base = collapse_whitespace(&raw);
    if base.is_empty() {
        return None;
    }
    if let Some(extra) = args.get("args").and_then(Value::as_array) {
        let extra_words: Vec<String> = extra
            .iter()
            .filter_map(Value::as_str)
            .map(collapse_whitespace)
            .filter(|word| !word.is_empty())
            .collect();
        if !extra_words.is_empty() {
            base.push(' ');
            base.push_str(&display_join_words(&extra_words));
        }
    }
    Some(base)
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
pub(super) fn preview_command(command: &str, max_len: usize) -> String {
    if max_len == 0 {
        return String::new();
    }
    let lines: Vec<&str> = command.lines().map(str::trim).filter(|line| !line.is_empty()).collect();
    if lines.is_empty() {
        return String::new();
    }
    let mut first_line = collapse_whitespace(lines[0]);
    if is_script_prefix_only(&first_line) && lines.len() > 1 {
        // Bounded: only fold enough continuation lines to fill the preview
        // budget instead of materializing a 100-line script into one String.
        let need = max_len.saturating_sub(first_line.chars().count()).saturating_add(16);
        let mut rest_parts = Vec::new();
        let mut rest_len = 0usize;
        for line in &lines[1..] {
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

/// Full-command variant of [`preview_command`] for transcript surfaces.
///
/// Keeps the script-runner prefix folding (so `python3 -c "` still pulls in
/// script content) but applies no length cap: expanded `• Ran` headlines and
/// live PTY headers must show the command in full, with viewport-aware
/// wrapping (TUI reflow) owning the overflow instead of a `…` truncation.
pub(super) fn preview_full_command(command: &str) -> String {
    preview_command(command, usize::MAX)
}

pub(super) fn describe_list_files(args: &Value, workspace_root: Option<&Path>) -> Option<(String, HashSet<String>)> {
    if let Some(path) = lookup_string(args, "path") {
        let mut used = HashSet::new();
        used.insert("path".to_string());
        let location = if path == "." {
            "workspace root".to_string()
        } else {
            let rel = relativize_to_workspace(&path, workspace_root);
            truncate_path_middle(&rel, 60)
        };
        return Some((format!("List files in {location}"), used));
    }
    if let Some(pattern) = lookup_string(args, "name_pattern") {
        let mut used = HashSet::new();
        used.insert("name_pattern".to_string());
        return Some((format!("Find files named {}", truncate_middle(&pattern, 40)), used));
    }
    if let Some(pattern) = lookup_string(args, "content_pattern") {
        let mut used = HashSet::new();
        used.insert("content_pattern".to_string());
        return Some((format!("Search files for {}", truncate_middle(&pattern, 40)), used));
    }
    None
}

pub(super) fn describe_grep_file(args: &Value, workspace_root: Option<&Path>) -> Option<(String, HashSet<String>)> {
    let pattern = lookup_string(args, "pattern");
    let path = lookup_string(args, "path");
    match (pattern, path) {
        (Some(pat), Some(path)) => {
            let mut used = HashSet::new();
            used.insert("pattern".to_string());
            used.insert("path".to_string());
            Some((
                format!(
                    "Grep {} in {}",
                    truncate_middle(&pat, 40),
                    truncate_path_middle(&relativize_to_workspace(&path, workspace_root), 40)
                ),
                used,
            ))
        }
        (Some(pat), None) => {
            let mut used = HashSet::new();
            used.insert("pattern".to_string());
            Some((format!("Grep {}", truncate_middle(&pat, 40)), used))
        }
        _ => None,
    }
}

pub(super) fn describe_code_search(args: &Value) -> Option<(String, HashSet<String>)> {
    // The schema requires `query`, but accept common aliases defensively so a
    // valid search never degrades to a generic "Search code" header with the
    // query hidden. Record the actual matched key so detail collection and
    // headline highlights stay in sync (a hardcoded "query" would leak e.g.
    // `Pattern: …` as a duplicate detail line).
    for key in ["query", "pattern", "q", "text"] {
        if let Some(query) = lookup_string(args, key) {
            let mut used = HashSet::new();
            used.insert(key.to_string());
            return Some((format!("Search code for {}", truncate_middle(&query, 40)), used));
        }
    }
    None
}

pub(super) fn describe_path_action(
    args: &Value,
    verb: &str,
    keys: &[&str],
    workspace_root: Option<&Path>,
) -> Option<(String, HashSet<String>)> {
    for key in keys {
        if let Some(value) = lookup_string(args, key) {
            let mut used = HashSet::new();
            used.insert((*key).to_string());
            let rel = relativize_to_workspace(&value, workspace_root);
            let summary = truncate_path_middle(&rel, 60);
            let annotated_summary = annotate_skill_doc_summary(rel.as_ref(), summary);
            return Some((format!("{verb} {annotated_summary}"), used));
        }
    }
    None
}

fn annotate_skill_doc_summary(raw_path: &str, summary: String) -> String {
    let path = Path::new(raw_path.trim());
    let is_skill_doc = path
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.eq_ignore_ascii_case("SKILL.md"));
    if !is_skill_doc {
        return summary;
    }

    let Some(skill_name) = path
        .parent()
        .and_then(Path::file_name)
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
    else {
        return summary;
    };

    format!("{summary} ({skill_name} skill)")
}

pub(super) fn lookup_string(args: &Value, key: &str) -> Option<String> {
    args.as_object()
        .and_then(|map| map.get(key))
        .and_then(|value| value.as_str())
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty())
}

/// Keys whose values are file-system paths and should be displayed relative to
/// the workspace root when possible.
fn is_path_key(key: &str) -> bool {
    matches!(key, "path" | "file_path" | "filename" | "destination" | "source")
}

/// Relativize an absolute `path` against the `workspace_root` for compact display.
///
/// Returns the path unchanged when `workspace_root` is `None`, the path is not
/// absolute, or it does not lie within the workspace root.
pub(super) fn relativize_to_workspace<'a>(path: &'a str, workspace_root: Option<&Path>) -> Cow<'a, str> {
    let Some(root) = workspace_root else {
        return Cow::Borrowed(path);
    };
    let p = Path::new(path);
    if p.is_absolute() {
        if let Ok(rel) = p.strip_prefix(root) {
            // `rel` is empty only when the path equals the root itself; keep the
            // original form in that degenerate case for clarity.
            if !rel.as_os_str().is_empty() {
                return Cow::Owned(rel.to_string_lossy().into_owned());
            }
        }
    }
    Cow::Borrowed(path)
}

/// Relativize absolute paths within a command string for compact display.
///
/// Each whitespace-delimited token that is an absolute path within the
/// workspace root gets rewritten to a relative path. Tokens outside the
/// workspace (e.g. system paths like /dev/null, /tmp) are left unchanged.
pub(super) fn relativize_command_paths(command: &str, workspace_root: Option<&Path>) -> String {
    let Some(root) = workspace_root else {
        return command.to_owned();
    };
    command
        .split(' ')
        .filter(|word| !word.is_empty())
        .map(|word| {
            let p = Path::new(word);
            if p.is_absolute() {
                if let Ok(rel) = p.strip_prefix(root) {
                    if !rel.as_os_str().is_empty() {
                        return rel.to_string_lossy().into_owned();
                    }
                }
            }
            word.to_owned()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

pub(super) fn humanize_key(key: &str) -> String {
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

pub(super) fn collect_param_details(
    args: &Value,
    keys: &HashSet<String>,
    workspace_root: Option<&Path>,
) -> Vec<String> {
    let mut details = Vec::new();
    let Some(map) = args.as_object() else {
        return details;
    };
    let include_all = keys.is_empty();
    for (key, value) in map {
        // Skip command-related and raw content keys (too verbose in summaries)
        if matches!(
            key.as_str(),
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
        ) {
            continue;
        }
        // Skip infrastructure/plumbing parameters that are implementation details
        if is_noise_param(key) {
            continue;
        }
        if !include_all && keys.contains(key) {
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
                    details.push(format!("{}: {}", humanize_key(key), summarize_list(&strings, 2, 60)));
                }
            }
            Value::Number(num) => {
                // Skip zero-valued numbers — they are defaults and add no information
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

/// Returns `true` for parameter keys that are infrastructure/plumbing noise
/// and should be omitted from the human-facing transcript summary.
fn is_noise_param(key: &str) -> bool {
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
            | "query"
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

/// One row summarizing the plumbing of an exec-session call.
///
/// Keeps the session identity (so a reader can follow which command a poll or
/// wait belongs to) plus the only knob that changes observable behavior — an
/// explicit wait deadline. Output-token caps, yield windows, and the raw stdin
/// payload are deliberately dropped from the transcript: the model receives the
/// arguments unchanged, and the user only needs the session at a glance.
///
/// Returns `None` when the call carries neither, so non-session tools keep
/// their normal detail rows.
pub(super) fn exec_session_param_detail(args: &Value) -> Option<String> {
    let session = lookup_string(args, "session_id").map(|id| format!("Session {id}"));
    let wait = ["wait_timeout_seconds", "timeout_seconds"]
        .iter()
        .find_map(|key| args.get(key).and_then(Value::as_u64))
        .filter(|secs| *secs > 0)
        .map(|secs| format!("wait up to {secs}s"));

    let parts = [session, wait].into_iter().flatten().collect::<Vec<_>>();
    (!parts.is_empty()).then(|| parts.join(" · "))
}

pub(super) fn should_render_command_line(highlights: &HashSet<String>) -> bool {
    highlights.is_empty()
        || (!highlights.contains("command")
            && !highlights.contains("raw_command")
            && !highlights.contains("bash_command")
            && !highlights.contains("cmd"))
}

pub(super) fn command_line_for_args(args: &Value) -> Option<String> {
    let (command, _) = extract_command(args)?;
    let trimmed = command.trim();
    if trimmed.is_empty() {
        return None;
    }
    // Head-truncate at a word boundary with a trailing ellipsis instead of
    // `truncate_middle`. Splitting a middle token reads as a rendering bug —
    // `git diff --stat crates/…onfig/config` looks malformed, and the command
    // verb plus the first meaningful args are what the reader needs. Shares
    // `preview_command` with the `describe_shell_command` headline so both
    // surfaces truncate identically.
    Some(preview_command(trimmed, COMPACT_PREVIEW_LEN))
}

pub(super) fn highlight_texts_for_summary(
    args: &Value,
    highlights: &HashSet<String>,
    workspace_root: Option<&Path>,
) -> Vec<String> {
    let mut values = Vec::new();
    for key in highlights {
        if let Some(value) = lookup_string(args, key) {
            let limit = match key.as_str() {
                "pattern" | "name_pattern" | "content_pattern" => 40,
                "command" | "raw_command" | "bash_command" => 70,
                _ => 60,
            };
            // Render file-system path values relative to the workspace root.
            let display: Cow<'_, str> = if is_path_key(key) {
                relativize_to_workspace(&value, workspace_root)
            } else {
                Cow::Borrowed(&value)
            };
            values.push(truncate_middle(&display, limit));
        }
    }
    values
}

pub(super) fn summarize_list(items: &[String], max_items: usize, max_len: usize) -> String {
    if items.is_empty() {
        return String::new();
    }
    let shown: Vec<String> = items.iter().take(max_items).map(|s| truncate_middle(s, max_len)).collect();
    if items.len() > max_items {
        format!("{} +{} more", shown.join(", "), items.len() - max_items)
    } else {
        shown.join(", ")
    }
}

/// Extract the bare tool name from an MCP-qualified tool name.
///
/// Accepts the `mcp__server__tool`, `mcp::provider::tool`, and `mcp_tool`
/// spellings; returns `None` for non-MCP names. This is the canonical MCP
/// name parser — do not fork the prefix/segment handling per call site, so
/// every TUI surface labels the same tool identically.
pub(crate) fn mcp_tool_display_name(tool_name: &str) -> Option<&str> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_describe_shell_command_new_format() {
        let args = json!({
            "command": ["bash", "-lc", "ls -R"]
        });

        let result = describe_shell_command(&args);
        assert!(result.is_some());

        let (description, _used) = result.unwrap();
        assert_eq!(description, "bash -lc ls -R");
    }

    #[test]
    fn test_describe_shell_command_bash_command_format() {
        let args = json!({
            "bash_command": "pwd"
        });

        let result = describe_shell_command(&args);
        assert!(result.is_some());

        let (description, _used) = result.unwrap();
        assert_eq!(description, "pwd");
    }

    #[test]
    fn test_describe_shell_command_truncation() {
        let long_command = "a".repeat(100);
        let args = json!({
            "command": [long_command]
        });

        let result = describe_shell_command(&args);
        assert!(result.is_some());

        let (description, _used) = result.unwrap();
        assert!(description.contains("…"));
    }

    #[test]
    fn test_describe_shell_command_string_format() {
        let args = json!({
            "command": "cargo check -p vtcode"
        });

        let result = describe_shell_command(&args);
        assert!(result.is_some());

        let (description, _used) = result.unwrap();
        assert_eq!(description, "cargo check -p vtcode");
    }

    #[test]
    fn test_describe_shell_command_raw_command_fallback() {
        let args = json!({
            "raw_command": "cargo test -- --nocapture"
        });

        let result = describe_shell_command(&args);
        assert!(result.is_some());

        let (description, _used) = result.unwrap();
        assert_eq!(description, "cargo test -- --nocapture");
    }

    #[test]
    fn collect_param_details_skips_noise_params() {
        let args = json!({
            "action": "grep",
            "pattern": "agent loop",
            "strictness": "relaxed",
            "debug_query": "pattern",
            "detail_level": "full",
            "max_results": 20,
            "context_lines": 2,
            "scope": "repo",
            "max_bytes": 6000,
            "timeout_secs": 120
        });
        let mut keys = HashSet::new();
        keys.insert("pattern".to_string());
        let details = collect_param_details(&args, &keys, None);
        // Only detail_level, max_results, and scope should remain;
        // pattern is in keys (highlighted), noise params (including action) are skipped.
        for detail in &details {
            assert!(
                !detail.contains("Timeout")
                    && !detail.contains("Max bytes")
                    && !detail.contains("Debug query")
                    && !detail.contains("Strictness")
                    && !detail.contains("Context lines")
                    && !detail.contains("Action"),
                "Noise param leaked through: {detail}"
            );
        }
    }

    #[test]
    fn collect_param_details_skips_zero_numbers() {
        let args = json!({
            "action": "read",
            "path": "src/main.rs",
            "start_line": 1,
            "end_line": 200,
            "offset": 0,
            "limit": 0
        });
        let mut keys = HashSet::new();
        keys.insert("path".to_string());
        let details = collect_param_details(&args, &keys, None);
        for detail in &details {
            assert!(
                !detail.contains("Offset") && !detail.contains("Limit"),
                "Zero-valued param leaked through: {detail}"
            );
        }
        assert!(details.iter().any(|d| d.contains("Start line: 1")));
        assert!(details.iter().any(|d| d.contains("End line: 200")));
    }

    #[test]
    fn is_noise_param_matches_expected_keys() {
        assert!(is_noise_param("timeout_secs"));
        assert!(is_noise_param("max_bytes"));
        assert!(is_noise_param("debug_query"));
        assert!(is_noise_param("strictness"));
        assert!(is_noise_param("case_sensitive"));
        assert!(is_noise_param("context_lines"));
        assert!(is_noise_param("shell"));
        assert!(is_noise_param("sandbox_permissions"));
        assert!(is_noise_param("action")); // Redundant with summary headline
        // Output caps are plumbing like the size limits above.
        assert!(is_noise_param("max_output_tokens"));
        assert!(is_noise_param("max_tokens"));
        // Session plumbing is folded by `exec_session_param_detail` instead.
        assert!(!is_noise_param("session_id"));
        assert!(!is_noise_param("chars"));
        assert!(!is_noise_param("wait_timeout_seconds"));
        // Read file params should pass through (not noise)
        assert!(!is_noise_param("offset"));
        assert!(!is_noise_param("limit"));
        assert!(!is_noise_param("head_lines"));
        assert!(!is_noise_param("tail_lines"));
        assert!(!is_noise_param("start_line"));
        assert!(!is_noise_param("end_line"));
        // Meaningful params should pass through
        assert!(!is_noise_param("pattern"));
        assert!(!is_noise_param("path"));
        assert!(!is_noise_param("mode"));
    }

    #[test]
    fn exec_session_param_detail_keeps_session_and_wait_only() {
        let args = json!({
            "session_id": "run-2d5752f2",
            "chars": "y\n",
            "yield_time_ms": 1000,
            "wait_timeout_seconds": 600,
            "max_output_tokens": 4000,
            "max_tokens": 4000
        });
        // Token caps, the yield window, and the raw stdin payload must not each
        // become their own tree row.
        assert_eq!(exec_session_param_detail(&args).as_deref(), Some("Session run-2d5752f2 · wait up to 600s"));
    }

    #[test]
    fn exec_session_param_detail_accepts_timeout_alias() {
        let args = json!({ "session_id": "run-abc", "timeout_seconds": 30 });
        assert_eq!(exec_session_param_detail(&args).as_deref(), Some("Session run-abc · wait up to 30s"));
    }

    #[test]
    fn exec_session_param_detail_skips_zero_and_missing_wait() {
        let args = json!({ "session_id": "run-abc", "wait_timeout_seconds": 0 });
        assert_eq!(exec_session_param_detail(&args).as_deref(), Some("Session run-abc"));
    }

    #[test]
    fn exec_session_param_detail_none_without_session_or_wait() {
        assert!(exec_session_param_detail(&json!({ "chars": "y\n", "max_output_tokens": 4000 })).is_none());
        assert!(exec_session_param_detail(&json!({ "path": "AGENTS.md" })).is_none());
    }

    #[test]
    fn command_line_for_args_avoids_mid_string_ellipsis() {
        // Long chained commands must not cut a path in half: `preview_command`
        // head-truncates at a word boundary with a trailing ellipsis, while the
        // old `truncate_middle` produced `crates/…onfig/…`-style artifacts in
        // `• Ran` summaries (screenshot 2026-09-24 11.40.47).
        let long = "cargo nextest run -p vtcode --bin vtcode && cargo fmt --all -- --check && \
                    git status --short && git diff --stat && git diff --cached --stat && \
                    ./scripts/check-dev.sh --workspace && echo done";
        assert!(long.chars().count() > 120, "fixture must actually overflow the cap");
        let args = json!({ "cmd": long });
        let command = command_line_for_args(&args).expect("command line");

        assert!(command.ends_with('\u{2026}'), "tail ellipsis expected, got: {command:?}");
        // The head survives intact: no path is cut in half by a middle cut.
        assert!(command.starts_with("cargo nextest run -p vtcode"), "got: {command:?}");
        // A middle cut would splice the middle of a later token into the preview.
        let body = command.trim_end_matches('\u{2026}');
        assert!(!body.contains('\u{2026}'), "mid-string ellipsis leaked: {command:?}");
        // Exactly one ellipsis, terminating the command.
        assert_eq!(command.matches('\u{2026}').count(), 1, "got: {command:?}");
    }

    #[test]
    fn command_line_for_args_keeps_short_commands_unchanged() {
        let args = json!({ "cmd": "git status --short" });
        assert_eq!(command_line_for_args(&args).as_deref(), Some("git status --short"));
    }

    #[test]
    fn truncate_path_middle_breaks_at_separator() {
        let path = "/Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/hello/src/main.rs";
        let truncated = truncate_path_middle(path, 40);
        // Should break at a '/' not in the middle of a word
        assert!(truncated.contains("…"));
        // The character after '…' should be a '/' or start of a path component
        if let Some(char_idx) = truncated.char_indices().find(|(_, c)| *c == '…') {
            let after: String = truncated[char_idx.0 + '…'.len_utf8()..].chars().collect();
            assert!(
                after.starts_with('/') || after.starts_with('h') || after.starts_with('s'),
                "Expected path break after ellipsis, got: {after}"
            );
        }
    }

    #[test]
    fn truncate_path_middle_short_path_not_truncated() {
        let path = "src/main.rs";
        let truncated = truncate_path_middle(path, 40);
        assert_eq!(truncated, "src/main.rs");
    }

    #[test]
    fn preview_command_multiline_script_folds_in_content() {
        // Screenshot 2026-09-02: `• Ran python3 -c "` with `tur…ool_calls`
        // continuations. A prefix-only first line must fold in script lines so
        // the header shows real content instead of a dangling quote.
        let command = "python3 -c \"\nimport json\nwith open('.vtcode/checkpoints/turn_1032.json') as f:\n    pass\n\"";
        let preview = preview_command(command, 70);
        assert!(
            preview.starts_with("python3 -c \"import json"),
            "prefix-only line should fold in script, got: {preview}"
        );
        assert!(!preview.contains("tur…ool"), "mid-string ellipsis leaked: {preview}");
        assert_ne!(preview, "python3 -c \"");
    }

    #[test]
    fn preview_command_unclosed_first_line_appends_continuation() {
        // `python3 -c "import json` (unclosed, partial content) must also pull
        // the next line instead of rendering a dangling open quote.
        let command = "python3 -c \"import json\nwith open('a.json') as f: pass\"";
        let preview = preview_command(command, 70);
        assert!(preview.contains("import json"), "got: {preview}");
        assert!(preview.contains("with open"), "got: {preview}");
    }

    #[test]
    fn preview_command_windows_style_runner_folds_in_content() {
        // Windows-style separators must still be recognized as script runners
        // so the header folds in the script instead of a dangling
        // `python.exe -c "` first line.
        let command = "C:\\Python\\python.exe -c \"\nimport json\nprint('hi')\n\"";
        let preview = preview_command(command, 70);
        assert!(
            preview.starts_with("C:\\Python\\python.exe -c \"import json"),
            "windows runner should fold in script, got: {preview}"
        );
    }

    #[test]
    fn preview_command_plain_multiline_keeps_first_line() {
        // Ordinary multi-line shell still previews only the first line.
        assert_eq!(preview_command("ls -R\npwd", 70), "ls -R");
    }

    #[test]
    fn preview_command_ignores_contractions_and_non_runners() {
        // `echo don't` has an apostrophe but no runner flag: never fold.
        assert_eq!(preview_command("echo don't\nnext-line", 70), "echo don't");
        // `grep -c` / `--code-review` are substring traps, not script runners.
        assert_eq!(preview_command("grep -c 'pat\nnext-line", 70), "grep -c 'pat");
        assert_eq!(preview_command("tool --code-review\nnext-line", 70), "tool --code-review");
        // Balanced runner script with a contraction inside stays single-line.
        assert_eq!(preview_command("python3 -c \"print('hi') # don't\"", 70), "python3 -c \"print('hi') # don't\"");
    }

    #[test]
    fn preview_command_long_command_head_truncates_at_word_boundary() {
        // 30-char budget: the 29-char cut lands inside "epsilon", but the last
        // space (index 27) is past half the budget, so it is honored.
        let command = "echo alpha beta gamma delta epsilon zeta eta theta iota";
        assert_eq!(preview_command(command, 30), "echo alpha beta gamma delta…");
    }

    #[test]
    fn preview_full_command_never_truncates() {
        // Transcript surfaces (expanded `• Ran`, live PTY headers) show the
        // command in full; only compact previews truncate. Exact screenshot
        // bytes (`||`, `\.backup`) round-trip unchanged.
        let command = "grep -rn \"@vinhnx/vtcode|npm install -g||npx @vinhnx\" docs | grep -v node_modules | grep -v package-lock | grep -v \"\\.backup\"";
        let full = preview_full_command(command);
        assert_eq!(full, command);
        assert!(!full.contains('…'));
    }

    #[test]
    fn preview_command_keeps_half_budget_when_first_space_is_early() {
        // Cut lands inside "epsilon" with the only space at index 4, well under
        // half the 29-char budget: fall back to a hard cut instead of collapsing
        // the preview to "echo…".
        let command = "echo alpha-beta-gamma-delta-epsilon zeta";
        assert_eq!(preview_command(command, 30), "echo alpha-beta-gamma-delta-e…");
    }

    #[test]
    fn preview_command_short_command_unchanged() {
        assert_eq!(preview_command("git status --short", 70), "git status --short");
        assert_eq!(preview_command("   ", 70), "");
        assert_eq!(preview_command("echo hi", 0), "");
    }

    #[test]
    fn display_command_text_leaves_operators_unquoted() {
        let args = json!({
            "command": ["cat", "docs/guides/agent-loop-contract.md", "2>/dev/null", "|", "head", "-120", ";", "echo", "---"]
        });
        let display = display_command_text(&args).expect("command display text");
        assert_eq!(display, "cat docs/guides/agent-loop-contract.md 2>/dev/null | head -120 ; echo ---");
    }

    #[test]
    fn display_command_text_quotes_only_whitespace_words() {
        let args = json!({ "command": ["echo", "hello world", "|", "tr", "a-z", "A-Z"] });
        let display = display_command_text(&args).expect("command display text");
        assert_eq!(display, "echo 'hello world' | tr a-z A-Z");
    }

    #[test]
    fn describe_shell_command_no_mid_string_ellipsis() {
        let args = json!({
            "command": "python3 -c \"\nimport json\nwith open('.vtcode/checkpoints/turn_1032.json') as f: d = json.load(f)\""
        });
        let (summary, used) = describe_shell_command(&args).expect("shell command summary");
        assert_eq!(used.iter().collect::<Vec<_>>(), ["command"]);
        assert!(!summary.contains("tur…ool"), "mid-string ellipsis leaked: {summary}");
        assert!(summary.starts_with("python3"));
        assert!(summary.contains("import json"), "script content folded in: {summary}");
        assert_ne!(summary, "python3 -c \"");
    }

    #[test]
    fn display_command_text_collapses_script_newlines_without_nesting() {
        // `python3 -c "a\nb"` splits into a script word with newlines and `'`.
        // The header must stay single-line and prefer `"` outer quotes so
        // `open('…')` does not render as nested `'...open('...')...'`.
        let args = json!({
            "command": "python3 -c \"import json\nwith open('.vtcode/x.json') as f: pass\""
        });
        let display = display_command_text(&args).expect("command display text");
        assert!(!display.contains('\n'), "newlines leaked: {display:?}");
        assert!(display.starts_with("python3 -c "), "got: {display}");
        assert!(display.contains("import json"), "got: {display}");
        assert!(!display.contains("'import json"), "nested single quotes: {display}");
    }

    #[test]
    fn display_command_text_falls_back_to_bash_command() {
        let args = json!({ "bash_command": "cargo check -p vtcode" });
        assert_eq!(display_command_text(&args).as_deref(), Some("cargo check -p vtcode"));
        // Both present: fallback follows `extract_command` order
        // (`raw_command` before `bash_command`) so display and describe agree.
        let args = json!({ "raw_command": "cargo check -p vtcode", "bash_command": "cargo test" });
        assert_eq!(display_command_text(&args).as_deref(), Some("cargo check -p vtcode"));
    }

    #[test]
    fn display_command_text_skips_blank_words_and_keeps_args() {
        // Whitespace-only array elements collapse away instead of leaving a
        // trailing space (`echo '  '` -> `echo`).
        let args = json!({ "command": ["echo", "  "] });
        assert_eq!(display_command_text(&args).as_deref(), Some("echo"));
        // Unbalanced `command` still yields a header via the raw fallback and
        // preserves `args[]` extras like the primary path does.
        let args = json!({ "command": "echo 'unclosed", "args": ["extra"] });
        assert_eq!(display_command_text(&args).as_deref(), Some("echo 'unclosed extra"));
    }

    #[test]
    fn describe_code_search_marks_query_used() {
        let args = json!({ "query": "agent loop implementation", "max_results": 15 });
        let (summary, used) = describe_code_search(&args).expect("code search summary");
        assert_eq!(summary, "Search code for agent loop implementation");
        assert!(used.contains("query"));
        assert!(!used.contains("max_results"));
    }

    #[test]
    fn describe_code_search_accepts_query_aliases() {
        // A valid search must never degrade to a generic header when the
        // query arrives under an alias key. The used set must record the
        // actual key so the value is not duplicated as a detail line.
        for key in ["pattern", "q", "text"] {
            let args = json!({ key: "core agent loop", "file_types": ["rs"] });
            let (summary, used) = describe_code_search(&args).expect("alias query should describe");
            assert_eq!(summary, "Search code for core agent loop");
            assert!(used.contains(key), "used should record the matched key, got {used:?}");
        }
    }

    #[test]
    fn mcp_tool_display_name_handles_all_spellings() {
        assert_eq!(mcp_tool_display_name("mcp__server__tool"), Some("tool"));
        assert_eq!(mcp_tool_display_name("mcp__tool"), Some("tool"));
        assert_eq!(mcp_tool_display_name("mcp::provider::tool"), Some("tool"));
        assert_eq!(mcp_tool_display_name("mcp_fetch"), Some("fetch"));
        assert_eq!(mcp_tool_display_name("fetch"), None);
    }
}
