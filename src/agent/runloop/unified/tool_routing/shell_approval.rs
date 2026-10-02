use serde_json::Value;
use url::Url;

use crate::agent::runloop::unified::tool_summary::{describe_tool_action, humanize_tool_name};

use super::permission_prompt::{
    extract_shell_approval_command_prefix_words, extract_shell_approval_command_words, extract_shell_command_text,
    extract_shell_permission_scope_signature, extract_shell_persistent_approval_prefix_rule,
    extract_shell_raw_command_text, render_shell_approval_command_words, render_shell_persistent_approval_prefix_entry,
    split_command_words_on_operators,
};

/// Secondary learning key for shell-command "families" (e.g. all safe
/// `find <subdir> ...` invocations share one key) so the auto-approve
/// classifier promotes equivalent-pattern calls after the user has approved a
/// few variants. Only attached for command shapes that are demonstrably safe
/// regardless of remaining flags — see [`learned_shell_pattern`].
#[derive(Debug, Clone)]
pub(super) struct LearnedPattern {
    pub key: String,
    pub label: String,
}

#[derive(Debug, Clone)]
pub(super) struct ApprovalLearningTarget {
    pub approval_key: String,
    pub display_label: String,
    pub pattern: Option<LearnedPattern>,
}

impl ApprovalLearningTarget {
    pub fn new(approval_key: String, display_label: String) -> Self {
        Self { approval_key, display_label, pattern: None }
    }

    pub fn with_pattern(mut self, pattern: Option<LearnedPattern>) -> Self {
        self.pattern = pattern;
        self
    }

    /// Iterate over every (key, label) pair this target contributes to
    /// learning: the exact invocation first, then the optional family pattern.
    ///
    /// Skips the pattern key if it matches the approval key to avoid
    /// double-counting approvals in [`record_approval_blocking`] (the
    /// auto-approve classifier) and duplicate session cache entries.
    pub fn iter_keys(&self) -> impl Iterator<Item = (&str, &str)> {
        let approval_key = self.approval_key.as_str();
        let display_label = self.display_label.as_str();
        std::iter::once((approval_key, display_label)).chain(
            self.pattern
                .iter()
                .filter(move |p| p.key != approval_key)
                .map(|p| (p.key.as_str(), p.label.as_str())),
        )
    }
}

#[derive(Debug, Clone)]
pub(super) struct ToolDisplayLabels {
    pub prompt_label: String,
    pub learning_label: String,
}

#[derive(Debug, Clone)]
pub(super) enum PersistentApprovalTarget {
    ToolLevel,
    ExactInvocation {
        display_label: String,
    },
    PrefixRule {
        prefix_rule: Vec<String>,
        display_label: String,
    },
}

fn exact_shell_learning_target(
    tool_name: &str,
    tool_args: Option<&Value>,
    default_learning_label: &str,
) -> Option<ApprovalLearningTarget> {
    let scope_signature = extract_shell_permission_scope_signature(tool_name, tool_args)?;

    if let Some(command_words) = extract_shell_approval_command_words(tool_name, tool_args) {
        let raw_command_text = extract_shell_raw_command_text(tool_name, tool_args);
        if let Some(target) =
            segmented_shell_learning_target(&command_words, &scope_signature, raw_command_text.as_deref())
        {
            return Some(target);
        }

        let rendered_command = render_shell_approval_command_words(&command_words);
        return Some(ApprovalLearningTarget::new(
            format!("{rendered_command}|{scope_signature}"),
            format!("command `{rendered_command}`"),
        ));
    }

    if let Some(command_text) = extract_shell_command_text(tool_name, tool_args) {
        return Some(ApprovalLearningTarget::new(
            format!("{command_text}|{scope_signature}"),
            format!("command `{command_text}`"),
        ));
    }

    let fallback_key = tool_args.map(Value::to_string).unwrap_or_else(|| tool_name.to_string());
    Some(ApprovalLearningTarget::new(
        format!("{fallback_key}|{scope_signature}"),
        default_learning_label.to_string(),
    ))
}

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

fn segmented_shell_learning_target(
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

/// Extract the domain from a `web_fetch` / `fetch_url` URL argument.
///
/// Returns `Some("example.com")` for `https://example.com/path`. The domain is
/// normalised to lowercase so that `https://Example.COM/` and `https://example.com/`
/// share one cache entry.
fn web_fetch_domain(tool_args: Option<&Value>) -> Option<String> {
    let url = tool_args?.as_object()?.get("url")?.as_str()?;
    let parsed = Url::parse(url).ok()?;
    let host = parsed.host_str()?;
    if host.is_empty() {
        return None;
    }
    Some(host.to_ascii_lowercase())
}

pub(super) fn approval_learning_target(
    tool_name: &str,
    tool_args: Option<&Value>,
    default_learning_label: &str,
) -> ApprovalLearningTarget {
    use vtcode_core::config::constants::tools::{FETCH_URL, WEB_FETCH};

    // For web_fetch / fetch_url, key by domain so that permanent approval is
    // scoped to the specific domain rather than the entire tool.
    if (tool_name == WEB_FETCH || tool_name == FETCH_URL)
        && let Some(domain) = web_fetch_domain(tool_args)
    {
        let approval_key = format!("{tool_name}:{domain}");
        let display_label = format!("fetch from {domain}");
        return ApprovalLearningTarget::new(approval_key, display_label);
    }

    let pattern = learned_shell_pattern(tool_name, tool_args);

    if let Some(scope_signature) = extract_shell_permission_scope_signature(tool_name, tool_args) {
        if let Some(prefix_rule) = extract_shell_persistent_approval_prefix_rule(tool_name, tool_args)
            && let Some(rendered_rule) =
                render_shell_persistent_approval_prefix_entry(tool_name, tool_args, &prefix_rule)
        {
            let rendered_prefix = render_shell_approval_command_words(&prefix_rule);
            return ApprovalLearningTarget::new(rendered_rule, format!("commands starting with `{rendered_prefix}`"))
                .with_pattern(pattern);
        }

        return exact_shell_learning_target(tool_name, tool_args, default_learning_label)
            .unwrap_or_else(|| {
                ApprovalLearningTarget::new(
                    format!("{tool_name}|{scope_signature}"),
                    default_learning_label.to_string(),
                )
            })
            .with_pattern(pattern);
    }

    ApprovalLearningTarget::new(
        vtcode_core::tools::names::canonical_tool_name(tool_name).to_owned(),
        default_learning_label.to_string(),
    )
}

pub(super) fn exact_shell_approval_target(
    tool_name: &str,
    tool_args: Option<&Value>,
    default_learning_label: &str,
) -> Option<ApprovalLearningTarget> {
    use vtcode_core::config::constants::tools::{FETCH_URL, WEB_FETCH};

    // For web_fetch / fetch_url, return the domain-scoped target so that
    // persisted approval lookups match the domain-specific key.
    if (tool_name == WEB_FETCH || tool_name == FETCH_URL)
        && let Some(domain) = web_fetch_domain(tool_args)
    {
        let approval_key = format!("{tool_name}:{domain}");
        let display_label = format!("fetch from {domain}");
        return Some(ApprovalLearningTarget::new(approval_key, display_label));
    }

    // Exact persistent cache entries intentionally omit any broader pattern:
    // "always approve this exact invocation" must not silently widen its scope.
    exact_shell_learning_target(tool_name, tool_args, default_learning_label)
}

pub(super) fn persistent_approval_target(
    tool_name: &str,
    tool_args: Option<&Value>,
    default_learning_label: &str,
) -> PersistentApprovalTarget {
    use vtcode_core::config::constants::tools::{FETCH_URL, WEB_FETCH};

    if let Some(prefix_rule) = extract_shell_persistent_approval_prefix_rule(tool_name, tool_args) {
        let rendered_prefix = render_shell_approval_command_words(&prefix_rule);
        return PersistentApprovalTarget::PrefixRule {
            prefix_rule,
            display_label: format!("commands starting with `{rendered_prefix}`"),
        };
    }

    // For web_fetch / fetch_url, always offer domain-scoped permanent approval.
    if (tool_name == WEB_FETCH || tool_name == FETCH_URL)
        && let Some(domain) = web_fetch_domain(tool_args)
    {
        return PersistentApprovalTarget::ExactInvocation { display_label: format!("fetch from {domain}") };
    }

    if extract_shell_permission_scope_signature(tool_name, tool_args).is_some() {
        let learning = approval_learning_target(tool_name, tool_args, default_learning_label);
        return PersistentApprovalTarget::ExactInvocation { display_label: learning.display_label };
    }

    PersistentApprovalTarget::ToolLevel
}

pub(super) fn tool_display_labels(tool_name: &str, tool_args: Option<&Value>) -> ToolDisplayLabels {
    let learning_label = humanize_tool_name(tool_name);
    let prompt_label = tool_args
        .map(|args| describe_tool_action(tool_name, args, None).0)
        .filter(|headline| !headline.is_empty())
        .unwrap_or_else(|| learning_label.clone());

    ToolDisplayLabels { prompt_label, learning_label }
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
fn learned_shell_pattern(tool_name: &str, tool_args: Option<&Value>) -> Option<LearnedPattern> {
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
    if command_words.len() < 2 || !command_words[1..].iter().any(|word| is_probable_readonly_path_arg(word)) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn pattern_for(command: &str) -> Option<LearnedPattern> {
        let args = json!({ "action": "run", "command": command });
        learned_shell_pattern("exec_command", Some(&args))
    }

    #[test]
    fn find_under_subdir_yields_pattern_key() {
        let pattern = pattern_for("find src -type f -name '*.rs'").expect("pattern");
        assert!(pattern.key.starts_with("shell-pattern:find src|sandbox_permissions="));
        assert_eq!(pattern.label, "safe `find src` commands");
    }

    #[test]
    fn find_root_directory_does_not_get_pattern() {
        assert!(pattern_for("find . -type f").is_none());
        assert!(pattern_for("find / -type f").is_none());
        assert!(pattern_for("find ./ -type f").is_none());
    }

    #[test]
    fn find_with_destructive_flags_does_not_get_pattern() {
        assert!(pattern_for("find src -delete").is_none());
        assert!(pattern_for("find src -exec rm {} +").is_none());
        assert!(pattern_for("find src -name foo -ok rm {} \\;").is_none());
    }

    #[test]
    fn find_with_spliced_destructive_flag_does_not_get_pattern() {
        for command in [
            "find src -maxdepth 0 -exe$''c touch /tmp/VT_BYPASS_POC {} +",
            "find src -maxdepth 0 -exe$@c touch /tmp/VT_BYPASS_POC {} +",
            "find src -maxdepth 0 -exe$*c touch /tmp/VT_BYPASS_POC {} +",
            "find src -maxdepth 0 -ex{e,}c touch /tmp/VT_BYPASS_POC {} +",
            "find src -maxdepth 0 -ex* touch /tmp/VT_BYPASS_POC {} +",
        ] {
            assert!(pattern_for(command).is_none(), "dynamic find syntax must not learn: {command}");
        }
    }

    #[test]
    fn path_qualified_programs_stay_exact_only() {
        // A path-qualified executable (`./find`, `/usr/bin/find`, `bin/find`)
        // can be an agent-created binary, so it must not inherit the bare
        // `find` family approval, and must not fall through to a generic
        // `shell-pattern:/usr/bin/find` family either.
        for command in [
            "/usr/bin/find src -type f -name '*.rs'",
            "./find src -type f",
            "bin/find src -type f",
            "/usr/bin/FIND src -type f",
            "/usr/bin/find src -delete",
            "/usr/bin/find src -maxdepth 0 -exe$''c touch /tmp/VT_BYPASS_POC {} +",
            "/usr/bin/find src -maxdepth 0 -ex'ec' touch /tmp/VT_BYPASS_POC {} +",
            "/bin/find src -del'ete'",
        ] {
            assert!(pattern_for(command).is_none(), "path-qualified program must stay exact-only: {command}");
        }
        // The bare program still learns.
        assert!(pattern_for("find src -type f -name '*.rs'").is_some());
    }

    #[test]
    fn wrapper_and_env_prefixes_do_not_learn_families() {
        // Wrappers and environment/assignment prefixes can reselect the
        // executable or change the effective directory, so they never train
        // (or inherit) a family key — including the CodeRabbit-reported
        // `env PATH=./bin find` and `env -C <dir> sed` shapes.
        for command in [
            "env find src -type f",
            "FOO=bar find src -type f",
            "PATH=./bin find src -type f",
            "env PATH=./bin find src -type f",
            "env -C /tmp sed -n '1p' src/file.rs",
            "env find src -maxdepth 0 -exe$''c touch /tmp/VT_BYPASS_POC {} +",
            "FOO=bar find src -delete",
            "sudo find src -type f",
            "nice find src -type f",
            "env sudo find src -type f",
            "FOO=bar grep -r foo src",
            "FOO=bar rm -rf target",
            "sudo ls src",
        ] {
            assert!(pattern_for(command).is_none(), "wrapper/env must stay exact-only: {command}");
        }
    }

    #[test]
    fn compound_with_unsafe_segment_does_not_learn() {
        // A safe segment must not supply a family key that would let
        // `prompt_tool_permission` auto-approve a sibling unsafe segment.
        for command in [
            "ls src && rm foo.txt",
            "ls src; python3 mutate.py",
            "ls src && PATH=./bin find src -type f",
            "cat docs/a.md && sed -i 's/a/b/' src/lib.rs",
        ] {
            let args = json!({ "action": "run", "command": command });
            let key = exact_shell_learning_target("exec_command", Some(&args), "Run Command")
                .expect("exact target")
                .approval_key;
            assert!(
                !key.starts_with("shell-pattern:"),
                "compound with an unsafe segment must not learn a family key: {command} -> {key}"
            );
        }
    }

    #[test]
    fn generic_pattern_rejects_dynamic_shell_syntax() {
        assert!(pattern_for("ls src").is_some());
        for command in ["ls src/$FOO", "grep -r foo src/*.rs", "wc -l src/file.txt; echo hi"] {
            // `;` is already rejected by the prefix gate; `$`/glob shapes must
            // fail closed via the new dynamic-syntax gate.
            if command.contains(';') {
                assert!(pattern_for(command).is_none());
            } else {
                assert!(pattern_for(command).is_none(), "dynamic generic must not learn: {command}");
            }
        }
    }

    #[test]
    fn mixed_case_and_spliced_programs_stay_exact_only() {
        // Only the bare lowercase program trains a family; uppercase or
        // quoted-spliced spellings stay exact-only (and cannot dodge the
        // find-specific rules via the generic fallback).
        assert!(pattern_for("FIND src -type f").is_none());
        assert!(pattern_for("/usr/bin/FIND src -delete").is_none());
        assert!(pattern_for("find src -del'ete'").is_none());
        assert!(pattern_for("/usr/bin/find src -del'ete'").is_none());
    }

    #[test]
    fn compound_shell_commands_do_not_get_pattern() {
        assert!(pattern_for("find src -type f ; rm -rf target").is_none());
        assert!(pattern_for("find src -type f && rm -rf target").is_none());
        assert!(pattern_for("find src -type f || true").is_none());
        assert!(pattern_for("find src -type f | xargs rm").is_none());
        assert!(pattern_for("bash -c 'find src -type f'").is_none());
        assert!(pattern_for("sh -lc \"find src -type f\"").is_none());
    }

    #[test]
    fn absolute_and_traversal_roots_do_not_get_pattern() {
        assert!(pattern_for("find /tmp -type f").is_none());
        assert!(pattern_for("find /Users/me/project -type f").is_none());
        assert!(pattern_for("find ../other -type f").is_none());
        assert!(pattern_for("find src/../other -type f").is_none());
        assert!(pattern_for("find ~/src -type f").is_none());
        assert!(pattern_for("find ~ -type f").is_none());
        assert!(pattern_for("find / -type f").is_none());
    }

    #[test]
    fn mutating_commands_have_no_pattern() {
        assert!(pattern_for("rm -rf target").is_none());
        assert!(pattern_for("cp src/lib.rs /tmp/lib.rs").is_none());
        assert!(pattern_for("mkdir build").is_none());
    }

    #[test]
    fn readonly_path_commands_get_generic_pattern() {
        let pattern = pattern_for("grep -r foo src").expect("pattern");
        assert!(pattern.key.starts_with("shell-pattern:grep|"));

        let pattern = pattern_for("ls src").expect("pattern");
        assert!(pattern.key.starts_with("shell-pattern:ls|"));

        let pattern = pattern_for("wc -l src/main.rs").expect("pattern");
        assert!(pattern.key.starts_with("shell-pattern:wc|"));
    }

    #[test]
    fn grep_command_has_no_pattern() {
        let pattern = pattern_for("grep -r foo src").expect("pattern");
        assert!(pattern.key.starts_with("shell-pattern:grep|"));
    }

    #[test]
    fn sed_print_under_workspace_path_yields_pattern_key() {
        let pattern =
            pattern_for("sed -n '87,140p' crates/codegen/vtcode-core/src/core/agent/features.rs").expect("pattern");

        assert!(
            pattern
                .key
                .starts_with("shell-pattern:sed -n <range> crates|sandbox_permissions=")
        );
        assert_eq!(pattern.label, "safe `sed -n` reads under `crates`");
    }

    #[test]
    fn sed_without_print_range_has_no_pattern() {
        assert!(pattern_for("sed -i 's/a/b/' src/lib.rs").is_none());
        assert!(pattern_for("sed -n '1,10d' src/lib.rs").is_none());
        assert!(pattern_for("sed -n '1,10p' ../src/lib.rs").is_none());
    }

    #[test]
    fn awk_range_print_under_workspace_path_yields_pattern_key() {
        let pattern = pattern_for("awk 'NR>=895 && NR<=935 {print NR\": \"$0}' src/agent/runloop/orchestration.rs")
            .expect("pattern");

        assert!(pattern.key.starts_with("shell-pattern:awk src|sandbox_permissions="));
        assert_eq!(pattern.label, "safe `awk` reads under `src`");
    }

    #[test]
    fn awk_with_data_options_yields_same_family_key() {
        let plain = pattern_for("awk 'NR>=40 && NR<=140' README.md").expect("pattern");
        let field_sep = pattern_for("awk -F: '{print $1}' README.md").expect("pattern");
        let var_assign = pattern_for("awk -v limit=10 'NR<=limit' README.md").expect("pattern");

        for pattern in [&plain, &field_sep, &var_assign] {
            assert!(pattern.key.starts_with("shell-pattern:awk README.md|sandbox_permissions="));
        }
        assert_eq!(plain.key.split('|').next(), field_sep.key.split('|').next());
        assert_eq!(plain.key.split('|').next(), var_assign.key.split('|').next());
    }

    #[test]
    fn awk_read_with_quoted_pipe_yields_family_pattern() {
        // The reported shape: the quoted `"|"` argument to `index()` previously
        // tripped the naive control-operator gate, so no family key was
        // attached and every new `NR` range re-prompted. It must now learn.
        let pattern = pattern_for(r#"awk 'NR>=208 && NR<=212 {n=index(rest,"|"); print n}' README.md"#)
            .expect("quoted-pipe awk read must yield a family pattern");
        assert!(pattern.key.starts_with("shell-pattern:awk README.md|sandbox_permissions="));
        assert_eq!(pattern.label, "safe `awk` reads under `README.md`");

        // Exact reported multi-line program (single-quoted, so `$0`/`\$` are
        // literal awk text, not shell expansion) must also learn.
        let reported = r#"awk 'NR>=208 && NR<=212 {line=$0; body=substr(line,1,length(line)-1); n=0; while (body ~ / \$/) { body=substr(body,1,length(body)-1); n++ }} # find guide start after label cell
rest=substr(line,3); g=index(rest,"|")+1; guide=substr(rest,g+2); gp=0; gg=guide; while (gg ~ / \$/) { gg=substr(gg,1,length(gg)-1); gp++ } print "%d: linelen=%d labelcell=%s pad_before_final_pipe=%d guide_pad=%d\n", NR, length(line), substr(line,3,20), n, gp }' README.md"#;
        let reported_pattern = pattern_for(reported).expect("reported multi-line awk read must yield a family pattern");
        assert!(
            reported_pattern
                .key
                .starts_with("shell-pattern:awk README.md|sandbox_permissions=")
        );
    }

    #[test]
    fn awk_mutating_shapes_have_no_pattern() {
        for command in [
            "awk '{print > \"out.txt\"}' README.md",
            "awk '{print >> \"out.txt\"}' README.md",
            "awk '{print | \"sort\"}' README.md",
            "awk 'BEGIN{system(\"touch out\")}' README.md",
            "awk -i inplace '{print}' README.md",
            "awk -f program.awk README.md",
            "awk '@include \"x.awk\"' README.md",
            "awk '@load \"ext\"' README.md",
            "awk -v f=system 'BEGIN{@f(\"id\")}' README.md",
            "awk 'BEGIN{@s(\"id\")}' README.md",
            "awk -l injail '{print}' README.md",
            "awk '$3>100' README.md",
            "awk '/error|warning/' README.md",
            "awk 'NR>=1' README.md > out.txt",
            "awk -F:",
            "awk",
        ] {
            assert!(pattern_for(command).is_none(), "mutating awk must not learn: {command}");
        }
    }

    #[test]
    fn awk_dynamic_syntax_has_no_pattern() {
        for command in [
            "awk 'NR>=1' src/file.txt $(whoami)",
            "awk 'NR>=1' src/file.txt `whoami`",
            "awk 'NR>=1' src/*.rs",
            "awk 'NR>=1' src/file.txt; echo hi",
        ] {
            assert!(pattern_for(command).is_none(), "dynamic awk must not learn: {command}");
        }
    }

    #[test]
    fn awk_multi_family_and_stdin_have_no_pattern() {
        assert!(pattern_for("awk 'NR>=1' src/a.rs docs/b.md").is_none());
        assert!(pattern_for("awk 'NR>=1' src/a.rs src/../other/b.rs").is_none());
        assert!(pattern_for("awk '{print $1}' -").is_none());
        assert!(pattern_for("awk 'NR>=1'").is_none());
    }

    #[test]
    fn awk_absolute_and_traversal_paths_have_no_pattern() {
        assert!(pattern_for("awk 'NR>=1 && NR<=5' /tmp/file.txt").is_none());
        assert!(pattern_for("awk 'NR>=1 && NR<=5' ../src/lib.rs").is_none());
        assert!(pattern_for("awk 'NR>=1 && NR<=5' src/../other/file.txt").is_none());
    }

    #[test]
    fn awk_compound_commands_have_no_pattern() {
        assert!(pattern_for("awk 'NR>=1' src/file.txt && rm -rf target").is_none());
        assert!(pattern_for("awk 'NR>=1' src/file.txt | xargs rm").is_none());
    }

    #[test]
    fn ls_multiple_absolute_paths_yields_compact_pattern_key() {
        let pattern = pattern_for(
            "ls /Users/me/project/.claude/agents/ /Users/me/project/.codex/agents/ /Users/me/project/.opencode/agents/",
        )
        .expect("pattern");

        assert!(pattern.key.starts_with("shell-pattern:ls|sandbox_permissions="));
        assert_eq!(pattern.label, "safe `ls` path reads");
    }

    #[test]
    fn compound_ls_commands_use_compact_segmented_target() {
        let args = json!({
            "action": "run",
            "command": "ls /Users/me/project/.agents/ 2>/dev/null; ls /Users/me/project/docs/ 2>/dev/null"
        });
        let target = exact_shell_learning_target("exec_command", Some(&args), "Run Command").expect("target");

        assert_eq!(
            target.approval_key,
            "shell-pattern:ls|sandbox_permissions=\"use_default\"|additional_permissions=null"
        );
        assert_eq!(target.display_label, "safe `ls` path reads");
    }

    #[test]
    fn compound_with_a_patternless_segment_stays_exact_only() {
        // CodeRabbit: a safe segment must not supply a lone key for a
        // path-qualified `./find` segment (agent-created binary), a bare
        // find/sed/awk segment, or any unsafe segment.
        for command in [
            "ls src && ./find src -type f",
            "ls src && find src -type f",
            "ls src; python3 mutate.py",
        ] {
            let args = json!({ "action": "run", "command": command });
            let key = exact_shell_learning_target("exec_command", Some(&args), "Run Command")
                .expect("exact target")
                .approval_key;
            assert!(
                !key.starts_with("shell-pattern:"),
                "compound with a pattern-less/unsafe segment must stay exact-only: {command} -> {key}"
            );
        }
    }

    #[test]
    fn unknown_non_mutating_path_command_gets_compact_pattern() {
        let pattern = pattern_for("wc -l src/lib.rs README.md").expect("pattern");
        assert!(pattern.key.starts_with("shell-pattern:wc|"));
        assert_eq!(pattern.label, "safe `wc` path reads");
    }

    #[test]
    fn mutating_path_commands_do_not_get_generic_pattern() {
        assert!(pattern_for("rm src/lib.rs").is_none());
        assert!(pattern_for("cp src/lib.rs /tmp/lib.rs").is_none());
        assert!(pattern_for("perl -i -pe 's/a/b/' src/lib.rs").is_none());
    }

    #[test]
    fn find_subdir_path_collapses_to_first_segment() {
        let pattern = pattern_for("find src/agent/runloop -type f").expect("pattern");
        assert!(pattern.key.starts_with("shell-pattern:find src|sandbox_permissions="));
    }

    #[test]
    fn iter_keys_yields_only_exact_when_no_pattern() {
        let target = ApprovalLearningTarget::new("key".into(), "label".into());
        let keys: Vec<_> = target.iter_keys().collect();
        assert_eq!(keys, vec![("key", "label")]);
    }

    #[test]
    fn iter_keys_yields_pattern_after_exact_when_present() {
        let target =
            ApprovalLearningTarget::new("exact".into(), "exact-label".into()).with_pattern(Some(LearnedPattern {
                key: "pattern".into(),
                label: "pattern-label".into(),
            }));
        let keys: Vec<_> = target.iter_keys().collect();
        assert_eq!(keys, vec![("exact", "exact-label"), ("pattern", "pattern-label")]);
    }

    #[tokio::test]
    async fn record_blocking_records_both_exact_and_pattern_keys() {
        use vtcode_core::tools::ApprovalRecorder;

        let temp_dir = std::env::temp_dir().join(format!(
            "vtcode_record_blocking_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or_default()
        ));
        let _ = std::fs::remove_dir_all(&temp_dir);

        let recorder = ApprovalRecorder::new(temp_dir.clone());
        let target = approval_learning_target(
            "exec_command",
            Some(&json!({"action":"run","command":"find src -type f"})),
            "default",
        );
        let pattern = target.pattern.as_ref().expect("pattern attached");

        super::super::approval_cache::record_approval_blocking(&recorder, &target, true).await;

        assert_eq!(recorder.get_approval_count(&target.approval_key).await, 1);
        assert_eq!(recorder.get_approval_count(&pattern.key).await, 1);

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[tokio::test]
    async fn denial_propagates_to_pattern_key() {
        use vtcode_core::tools::ApprovalRecorder;

        let temp_dir = std::env::temp_dir().join(format!(
            "vtcode_pattern_denial_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or_default()
        ));
        let _ = std::fs::remove_dir_all(&temp_dir);

        let recorder = ApprovalRecorder::new(temp_dir.clone());
        let target = approval_learning_target(
            "exec_command",
            Some(&json!({"action":"run","command":"find src -type f"})),
            "default",
        );
        let pattern = target.pattern.as_ref().expect("pattern attached");

        super::super::approval_cache::record_approval_blocking(&recorder, &target, false).await;

        assert_eq!(recorder.get_approval_count(&target.approval_key).await, 0);
        assert_eq!(recorder.get_approval_count(&pattern.key).await, 0);
        // ...but the pattern key's deny_count is bumped, so a future burst of
        // approvals is tempered when computing approval rate.
        let stored = recorder.get_pattern(&pattern.key).await.expect("stored");
        assert_eq!(stored.deny_count, 1);

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[tokio::test]
    async fn three_safe_find_invocations_promote_pattern_to_auto_approve() {
        use vtcode_core::tools::ApprovalRecorder;

        let temp_dir = std::env::temp_dir().join(format!(
            "vtcode_pattern_promote_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or_default()
        ));
        let _ = std::fs::remove_dir_all(&temp_dir);

        let recorder = ApprovalRecorder::new(temp_dir.clone());

        // Three different (but equally safe) `find src ...` approvals,
        // simulating the user manually approving each variant.
        for command in [
            "find src -type f -name '*.rs'",
            "find src -type d",
            "find src -name foo",
        ] {
            let target =
                approval_learning_target("exec_command", Some(&json!({"action":"run","command":command})), "default");
            super::super::approval_cache::record_approval_blocking(&recorder, &target, true).await;
        }

        // A *new* safe `find src ...` invocation should auto-approve via the
        // pattern key even though its exact form has never been seen before.
        let new_target = approval_learning_target(
            "exec_command",
            Some(&json!({"action":"run","command":"find src -path '*runloop*'"})),
            "default",
        );
        let pattern = new_target.pattern.as_ref().expect("pattern attached");
        assert!(recorder.should_auto_approve(&pattern.key).await);
        assert_eq!(recorder.get_approval_count(&new_target.approval_key).await, 0);

        // Destructive `find src -delete` MUST NOT inherit the pattern.
        let destructive = approval_learning_target(
            "exec_command",
            Some(&json!({"action":"run","command":"find src -delete"})),
            "default",
        );
        assert!(destructive.pattern.is_none(), "destructive find must not carry pattern");

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[tokio::test]
    async fn three_safe_awk_invocations_promote_pattern_to_auto_approve() {
        use vtcode_core::tools::ApprovalRecorder;

        let temp_dir = std::env::temp_dir().join(format!(
            "vtcode_awk_pattern_promote_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or_default()
        ));
        let _ = std::fs::remove_dir_all(&temp_dir);

        let recorder = ApprovalRecorder::new(temp_dir.clone());

        // Three different (but equally safe) `awk ... src/...` approvals with
        // distinct `NR` ranges — the exact shape that previously re-prompted.
        for command in [
            "awk 'NR>=895 && NR<=935 {print NR\": \"$0}' src/agent/runloop/orchestration.rs",
            "awk 'NR>=40 && NR<=140' src/lib.rs",
            "awk -F: '{print $1}' src/main.rs",
        ] {
            let target =
                approval_learning_target("exec_command", Some(&json!({"action":"run","command":command})), "default");
            super::super::approval_cache::record_approval_blocking(&recorder, &target, true).await;
        }

        // A *new* safe `awk ... src/...` range should auto-approve via the
        // family key even though its exact form was never seen before.
        let new_target = approval_learning_target(
            "exec_command",
            Some(&json!({"action":"run","command":"awk 'NR>=1 && NR<=5' src/other.rs"})),
            "default",
        );
        let pattern = new_target.pattern.as_ref().expect("pattern attached");
        assert!(recorder.should_auto_approve(&pattern.key).await);
        assert_eq!(recorder.get_approval_count(&new_target.approval_key).await, 0);

        // Mutating `awk` MUST NOT inherit the family pattern.
        let destructive = approval_learning_target(
            "exec_command",
            Some(&json!({"action":"run","command":"awk '{print > \"out.txt\"}' src/other.rs"})),
            "default",
        );
        assert!(destructive.pattern.is_none(), "destructive awk must not carry pattern");

        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
