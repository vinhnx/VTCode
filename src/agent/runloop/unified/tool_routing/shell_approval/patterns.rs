use serde_json::Value;

use super::super::permission_prompt::{
    extract_shell_approval_command_prefix_words, extract_shell_approval_command_words,
    extract_shell_permission_scope_signature, extract_shell_raw_command_text, split_command_words_on_operators,
};
use super::{ApprovalLearningTarget, LearnedPattern};

fn segment_readonly_pattern(segment: &[String], scope_signature: &str) -> Option<LearnedPattern> {
    let program = segment.first().map(String::as_str);
    let basename = program.map(shell_program_basename);
    // Commands with specific pattern rules that rejected this segment get no
    // generic pattern.  This prevents e.g. `find /tmp` from creating a broad
    // `shell-pattern:find` family key when the specific find-pattern rejected
    // the absolute-path argument. Match by basename so `/usr/bin/find`,
    // `./find`, and similar invocations cannot fall through to the generic
    // path-read family.
    if matches!(basename.as_deref(), Some("find" | "sed" | "awk")) {
        return None;
    }
    if program.is_some_and(is_wrapper_program) || has_environment_prefix(segment) {
        return None;
    }
    learned_readonly_path_pattern(segment, scope_signature)
}

/// Whether a command begins with an `env` wrapper or leading `KEY=value`
/// assignments. Such prefixes change executable resolution (`PATH=./bin`),
/// the effective working directory (`env -C`), or the process environment, so
/// a family key built from the remaining words would let a different program
/// (or a different directory) inherit a trusted approval. Keep them exact-only.
fn has_environment_prefix(words: &[String]) -> bool {
    vtcode_core::tools::command_args::command_words_after_environment_prefix(words).len() != words.len()
}

pub(super) fn segmented_shell_learning_target(
    command_words: &[String],
    scope_signature: &str,
    raw_command_text: Option<&str>,
) -> Option<ApprovalLearningTarget> {
    // The word-level splitter can miss operators glued to a token
    // (`ls src; rm foo` tokenizes as `src;`), so run the authoritative
    // whole-command read-only check. A command the parser sees as compound or
    // unsafe stays exact-only rather than leaking a safe sibling's family key.
    let raw = raw_command_text?;
    let args = serde_json::json!({ "action": "run", "command": raw });
    if !vtcode_core::tools::tool_intent::is_readonly_command_session_command(&args) {
        return None;
    }

    // Segment with the shell grammar so glued operators are split; fall back to
    // the word-level splitter only when the grammar cannot produce a list.
    let segments = vtcode_core::command_safety::shell_parser::parse_shell_commands(raw)
        .ok()
        .filter(|segments| !segments.is_empty())
        .or_else(|| split_command_words_on_operators(command_words))?;

    // EVERY segment must yield its own family pattern. Dropping a pattern-less
    // segment and keeping a sibling's key (e.g. `ls src && ./find src -type f`
    // -> only `shell-pattern:ls`) would let prior `ls` approvals auto-approve
    // the whole invocation, including an agent-created `./find`. Otherwise the
    // compound stays exact-only. The whole-command read-only check above
    // already proves every segment is independently read-only.
    let mut patterns = segments
        .iter()
        .map(|segment| segment_readonly_pattern(segment, scope_signature))
        .collect::<Option<Vec<_>>>()?;

    patterns.sort_by(|left, right| left.key.cmp(&right.key));
    patterns.dedup_by(|left, right| left.key == right.key);
    if patterns.len() == 1 {
        let pattern = patterns.remove(0);
        return Some(ApprovalLearningTarget::new(pattern.key, pattern.label));
    }

    let key = patterns
        .iter()
        .map(|pattern| pattern.key.as_str())
        .collect::<Vec<_>>()
        .join("&&");
    let label = patterns
        .iter()
        .map(|pattern| pattern.label.as_str())
        .collect::<Vec<_>>()
        .join(" and ");
    Some(ApprovalLearningTarget::new(key, label))
}

/// Build a conservative family/pattern learning key for safe shell commands.
///
/// Currently matches safe read-only command families such as `find <subdir>`,
/// `sed -n <range> <path>`, and write-free `awk <program> <path>` invocations
/// that:
/// - contain no destructive options,
/// - are a single simple command (no `&&`, `||`, `;`, `|`, nested shells, etc.
///   — `find`/`sed`/generic via [`extract_shell_approval_command_prefix_words`];
///   `awk` via quote-aware `split_command_words_on_operators` + tree-sitter
///   `parse_shell_commands` because `NR>=a && NR<=b` carries `&&` inside quotes),
/// - target a non-absolute, non-traversal, workspace-relative path.
///
/// Scope (sandbox + additional permissions) is baked into the key so a
/// pattern approved under default permissions does not promote escalated runs.
pub(super) fn learned_shell_pattern(tool_name: &str, tool_args: Option<&Value>) -> Option<LearnedPattern> {
    let scope_signature = extract_shell_permission_scope_signature(tool_name, tool_args)?;
    // Use the *prefix* extractor which already rejects compound commands and
    // nested shell invocations — a broader pattern key must never be trained
    // by commands like `find src && rm -rf target` or `bash -c '...'`.
    let prefix_words = extract_shell_approval_command_prefix_words(tool_name, tool_args);
    // A wrapper (`sudo`, `nice`, `env`, …) or an environment/assignment prefix
    // (`PATH=./bin`, `env -C /tmp`, …) can reselect the executable or change the
    // effective working directory. A family key built from the stripped words
    // would let that command inherit a trusted approval, so keep it exact-only.
    if let Some(words) = prefix_words.as_ref()
        && (has_environment_prefix(words) || words.first().is_some_and(|program| is_wrapper_program(program)))
    {
        return None;
    }

    // Specific command patterns first: find, sed, awk.
    // `find`/`sed` use prefix-gated words; `awk` uses its own quote-aware
    // extraction below. All have tighter path-validation rules (e.g. reject
    // absolute paths, directory traversal, and destructive flags).
    let raw_command_text = extract_shell_raw_command_text(tool_name, tool_args);
    if let Some(command_words) = prefix_words.as_ref() {
        if let Some(pattern) = learned_find_pattern(command_words, &scope_signature, raw_command_text.as_deref()) {
            return Some(pattern);
        }
        if let Some(pattern) = learned_sed_print_pattern(command_words, &scope_signature, raw_command_text.as_deref()) {
            return Some(pattern);
        }
    }
    // `awk 'NR>=a && NR<=b {...}'` carries `&&` inside single quotes, which the
    // naive substring gate in the prefix extractor misreads as a compound
    // command. Fetch awk words via the non-gating extractor and prove
    // single-command shape with the tree-sitter parser inside the pattern fn.
    if let Some(words) = extract_shell_approval_command_words(tool_name, tool_args)
        && let Some(pattern) = learned_awk_read_pattern(&words, &scope_signature, raw_command_text.as_deref())
    {
        return Some(pattern);
    }
    // Generic read-only path-read pattern as fallback for commands without
    // specific pattern rules (e.g. ls, grep, wc).  If find/sed/awk had specific
    // rules that rejected this invocation, no generic pattern is attached.
    // Fail closed on dynamic shell syntax: `shell_words` normalisation strips
    // quotes (so `-ex'ec'` becomes `-exec` and is caught above) but leaves
    // `$''`/`$@`/`{..}` splices intact (e.g. `-exe$''c` stays `-exe$c`).
    // Without this gate `/usr/bin/find src -exe$''c …` would inherit a generic
    // `shell-pattern:/usr/bin/find` family key (GHSA-r249-hpfx-x2w7).
    let command_words = prefix_words?;
    if let Some(raw) = raw_command_text.as_deref()
        && vtcode_core::tools::command_args::contains_dynamic_shell_syntax(raw)
    {
        return None;
    }
    segment_readonly_pattern(&command_words, &scope_signature)
}

fn learned_readonly_path_pattern(command_words: &[String], scope_signature: &str) -> Option<LearnedPattern> {
    let program = command_words.first()?.as_str();
    if !command_looks_like_readonly_path_query(program, command_words) {
        return None;
    }

    let base_rendered = program.to_string();

    Some(LearnedPattern {
        key: format!("shell-pattern:{base_rendered}|{scope_signature}"),
        label: format!("safe `{base_rendered}` path reads"),
    })
}

/// Lowercased basename of a shell program word (`/usr/bin/FIND` → `find`).
///
/// Used by the generic path-read rules and the wrapper deny-list so an
/// absolute path, a `./` prefix, or a mixed-case spelling (the same binary on
/// case-insensitive filesystems) cannot dodge them. The specific
/// `find`/`sed`/`awk` family rules deliberately require the bare program name
/// so a path-qualified executable stays exact-only.
fn shell_program_basename(program: &str) -> String {
    std::path::Path::new(program)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(program)
        .to_ascii_lowercase()
}

/// Wrapper prefixes that must never train a family key. `env`/`sudo`/`nice`
/// strip to the real program at execution time, so `env find src -exec …`
/// would otherwise learn a `shell-pattern:env` key that auto-approves the
/// destructive shape.
fn is_wrapper_program(program: &str) -> bool {
    matches!(
        shell_program_basename(program).as_str(),
        "env"
            | "sudo"
            | "su"
            | "doas"
            | "runas"
            | "nice"
            | "timeout"
            | "stdbuf"
            | "nohup"
            | "command"
            | "builtin"
            | "time"
    )
}

fn command_looks_like_readonly_path_query(program: &str, words: &[String]) -> bool {
    const KNOWN_MUTATING_COMMANDS: &[&str] = &[
        "awk", "cargo", "chmod", "chown", "cp", "curl", "dd", "find", "install", "ln", "mkdir", "mv", "perl", "python",
        "python3", "rm", "rmdir", "rsync", "ruby", "sed", "sh", "bash", "zsh", "tee", "touch", "truncate", "wget",
    ];
    const MUTATING_OPTION_HINTS: &[&str] = &[
        "--delete",
        "--exec",
        "--in-place",
        "--output",
        "--remove",
        "--write",
        "-delete",
        "-exec",
        "-execdir",
        "-i",
        "-o",
    ];

    !program.is_empty()
        && !KNOWN_MUTATING_COMMANDS.contains(&shell_program_basename(program).as_str())
        && !words.iter().skip(1).any(|word| MUTATING_OPTION_HINTS.contains(&word.as_str()))
        && words.iter().skip(1).any(|word| is_probable_readonly_path_arg(word))
}

fn is_probable_readonly_path_arg(word: &str) -> bool {
    if word.is_empty() || word.starts_with('-') || word.starts_with('~') || word == "." {
        return false;
    }
    let trimmed = word.trim_end_matches('/');
    if trimmed.is_empty() {
        return false;
    }
    let parts = if trimmed.starts_with('/') {
        trimmed.split('/').skip(1).collect::<Vec<_>>()
    } else {
        trimmed.split('/').collect::<Vec<_>>()
    };

    !parts.is_empty()
        && parts
            .iter()
            .all(|part| !part.is_empty() && *part != "." && *part != ".." && !part.contains('\0'))
}

fn learned_find_pattern(
    command_words: &[String],
    scope_signature: &str,
    raw_command_text: Option<&str>,
) -> Option<LearnedPattern> {
    let program = command_words.first().map(String::as_str)?;
    if program != "find" {
        return None;
    }

    // A family approval is only safe for a static shell command. Expansion
    // syntax can splice a destructive option together after tokenization
    // (for example, `-exe$''c` becomes `-exec` in bash).
    let raw_command_text = raw_command_text?;
    if vtcode_core::tools::command_args::contains_dynamic_shell_syntax(raw_command_text) {
        return None;
    }

    if command_words.iter().any(|word| is_destructive_find_option(word)) {
        return None;
    }

    let root = command_words.get(1)?;
    if root.starts_with('-') {
        return None;
    }
    let normalized_root = normalize_find_root(root)?;

    Some(LearnedPattern {
        key: format!("shell-pattern:find {normalized_root}|{scope_signature}"),
        label: format!("safe `find {normalized_root}` commands"),
    })
}

fn is_destructive_find_option(word: &str) -> bool {
    matches!(
        word,
        "-delete" | "-exec" | "-execdir" | "-ok" | "-okdir" | "-fls" | "-fprint" | "-fprint0" | "-fprintf"
    )
}

fn learned_sed_print_pattern(
    command_words: &[String],
    scope_signature: &str,
    raw_command_text: Option<&str>,
) -> Option<LearnedPattern> {
    let program = command_words.first().map(String::as_str)?;
    if program != "sed" {
        return None;
    }
    // Fail closed on expansion syntax for the same reason as find/awk.
    if let Some(raw) = raw_command_text
        && vtcode_core::tools::command_args::contains_dynamic_shell_syntax(raw)
    {
        return None;
    }
    let [_, flag, range, path] = command_words else {
        return None;
    };
    if flag != "-n" || !is_sed_print_range(range) {
        return None;
    }
    let family = normalize_workspace_file_family(path)?;

    Some(LearnedPattern {
        key: format!("shell-pattern:sed -n <range> {family}|{scope_signature}"),
        label: format!("safe `sed -n` reads under `{family}`"),
    })
}

/// Family key for write-free `awk <program> <path>` reads (e.g.
/// `awk 'NR>=895 && NR<=935 {print NR": "$0}' src/file.rs`).
///
/// `awk` stays in the generic mutating-command denylist because its program
/// text can write (`print > file`), pipe (`print | "cmd"`), execute
/// (`system()`), indirect-call (`@func()`), or load code (`@include`/`@load`),
/// and its options can edit in place (`-i`) or load programs (`-f`/`-l`).
/// This pattern is only attached when the authoritative read-only classifier
/// (`vtcode_core::tools::tool_intent::is_readonly_command_session_command`,
/// backed by `command_args::has_unsafe_awk_options`) proves the invocation
/// write-free, so the family key (which intentionally ignores the exact `NR`
/// range/program text, mirroring `sed -n <range>`) can never promote a
/// mutating `awk` shape. The `-v`/`-F`/`--assign`/`--field-separator` skipping
/// below mirrors `has_unsafe_awk_options`; keep them in sync. All file args
/// must share one workspace-relative top-level family;
/// absolute/traversal/multi-family reads get no pattern and stay exact-only.
fn learned_awk_read_pattern(
    command_words: &[String],
    scope_signature: &str,
    raw_command_text: Option<&str>,
) -> Option<LearnedPattern> {
    let program = command_words.first().map(String::as_str)?;
    if program != "awk" {
        return None;
    }
    let raw = raw_command_text?;
    if vtcode_core::tools::command_args::contains_dynamic_shell_syntax(raw) {
        return None;
    }
    {
        let args = serde_json::json!({"action": "run", "command": raw});
        if !vtcode_core::tools::tool_intent::is_readonly_command_session_command(&args) {
            return None;
        }
    }
    // Quote-aware single-command gate: `&&`/`||`/`|`/`;` inside single quotes
    // (the common `NR>=a && NR<=b` shape) must not count as a compound.
    // `split_command_words_on_operators` only splits standalone operator words
    // produced by quote-respecting `shell_words::split`, and the tree-sitter
    // parser proves the raw string is one simple command.
    {
        let segments = split_command_words_on_operators(command_words)?;
        if segments.len() != 1 {
            return None;
        }
    }
    if let Ok(parsed) = vtcode_core::command_safety::shell_parser::parse_shell_commands(raw) {
        if parsed.len() != 1 {
            return None;
        }
    } else {
        return None;
    }

    let mut index = 1;
    let mut options_ended = false;
    while index < command_words.len() {
        let word = command_words[index].as_str();
        if !options_ended && word == "--" {
            options_ended = true;
            index += 1;
            continue;
        }
        if !options_ended && word.starts_with('-') && word.len() > 1 {
            if word == "-v" || word == "--assign" || word == "-F" || word == "--field-separator" {
                index += 2;
                continue;
            }
            if word.starts_with("-v")
                || word.starts_with("--assign=")
                || word.starts_with("-F")
                || word.starts_with("--field-separator=")
            {
                index += 1;
                continue;
            }
            return None;
        }
        break;
    }

    let _program = command_words.get(index)?;
    let files = command_words.get(index + 1..)?;
    if files.is_empty() {
        return None;
    }

    let mut family: Option<String> = None;
    for file in files {
        let current = normalize_workspace_file_family(file)?;
        if let Some(existing) = &family {
            if existing != &current {
                return None;
            }
        } else {
            family = Some(current);
        }
    }
    let family = family?;

    Some(LearnedPattern {
        key: format!("shell-pattern:awk {family}|{scope_signature}"),
        label: format!("safe `awk` reads under `{family}`"),
    })
}

fn is_sed_print_range(range: &str) -> bool {
    let Some(range) = range.strip_suffix('p') else {
        return false;
    };

    let Some((start, end)) = range.split_once(',') else {
        return range.parse::<usize>().is_ok();
    };

    start.parse::<usize>().is_ok() && end.parse::<usize>().is_ok()
}

/// Reduce a `find <root>` argument to a stable, safe, workspace-relative
/// top-level segment. Rejects anything that would escape the workspace
/// (absolute paths, `..` traversal, `~` home expansion, empty segments) so the
/// resulting pattern key can never accidentally span filesystems or escalate.
fn normalize_find_root(root: &str) -> Option<String> {
    let trimmed = root.trim();
    if trimmed.is_empty() {
        return None;
    }
    let stripped = trimmed.strip_prefix("./").unwrap_or(trimmed).trim_end_matches('/');

    if stripped.is_empty()
        || stripped == "."
        || stripped == "/"
        || stripped.starts_with('/')
        || stripped.starts_with('~')
        || stripped.split('/').any(|part| part.is_empty() || part == "." || part == "..")
    {
        return None;
    }

    // Collapse `src/foo/bar` to `src` so all safe finds under the same
    // top-level workspace subdirectory share a single family key.
    stripped.split('/').next().map(str::to_owned)
}

fn normalize_workspace_file_family(path: &str) -> Option<String> {
    let trimmed = path.trim();
    if trimmed.is_empty() || trimmed.starts_with('/') || trimmed.starts_with('~') || trimmed.starts_with('-') {
        return None;
    }
    let stripped = trimmed.strip_prefix("./").unwrap_or(trimmed);
    let mut parts = stripped.split('/');
    let first = parts.next()?;
    if first.is_empty() || first == "." || first == ".." || first.contains('\0') {
        return None;
    }
    if parts.any(|part| part.is_empty() || part == "." || part == ".." || part.contains('\0')) {
        return None;
    }

    Some(first.to_string())
}
