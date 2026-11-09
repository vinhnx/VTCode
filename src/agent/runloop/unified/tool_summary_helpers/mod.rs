use hashbrown::HashSet;

use serde_json::Value;
use std::borrow::Cow;
use std::path::Path;
use vtcode_commons::formatting::{collapse_whitespace, truncate_middle, truncate_path_middle};
use vtcode_commons::tool_headlines::{is_path_key, lookup_string, preview_command, relativize_to_workspace};
use vtcode_core::tools::command_args;

pub(crate) use super::tool_pipeline::is_exec_session_call;

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

#[cfg(test)]
mod tests;
