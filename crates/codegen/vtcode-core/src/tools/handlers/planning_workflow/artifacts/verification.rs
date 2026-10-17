//! Concrete target and verification classification.

use super::sections::find_placeholder_tokens;
use super::steps::{PLAN_TARGET_LABELS, marker_value, parse_bracket_list};
use std::path::Path;

fn is_concrete_value(value: &str) -> bool {
    let value = value.trim();
    !value.is_empty() && value != "[]" && find_placeholder_tokens(value).is_empty()
}

pub(super) fn is_concrete_target(value: &str) -> bool {
    let value = value.trim();
    if !is_concrete_value(value) {
        return false;
    }

    let target = marker_value(value, PLAN_TARGET_LABELS).unwrap_or(value).trim();
    if !is_concrete_value(target) {
        return false;
    }

    let lower = target.to_ascii_lowercase();
    if lower.starts_with('[') && lower.ends_with(']') {
        let items = parse_bracket_list(target);
        return !items.is_empty() && items.iter().any(|item| is_concrete_target(item));
    }

    let has_structural_reference = target.split_whitespace().any(|token| {
        let token = token.trim_matches(|ch: char| ch.is_ascii_punctuation() && ch != '_' && ch != '/');
        token.contains('/')
            || token.contains('\\')
            || token.contains("::")
            || token.contains('_')
            || token.chars().skip(1).any(char::is_uppercase)
            || token.rsplit_once('.').is_some_and(|(_, suffix)| !suffix.is_empty())
    });
    if has_structural_reference {
        return true;
    }

    const GENERIC_TARGETS: &[&str] = &[
        "file",
        "files",
        "path",
        "paths",
        "symbol",
        "symbols",
        "files/symbols",
        "files or symbols",
        "file/symbol",
        "file or symbol",
        "behavior",
        "behaviour",
        "code",
        "codebase",
        "implementation",
        "feature",
        "workflow",
        "module",
        "modules",
        "component",
        "components",
        "target",
        "relevant files",
        "relevant code",
        "relevant modules",
        "relevant symbols",
        "appropriate files",
        "appropriate code",
        "appropriate modules",
        "affected files",
        "affected code",
        "affected modules",
        "the file",
        "the files",
        "the path",
        "the symbol",
        "the symbols",
        "the behavior",
        "the behaviour",
        "the code",
        "the codebase",
        "the implementation",
        "the feature",
        "the workflow",
        "the module",
        "the modules",
        "the component",
        "the components",
        "the relevant files",
        "the relevant code",
        "the relevant modules",
        "the relevant symbols",
        "the affected files",
        "the affected code",
        "the affected modules",
        "existing code",
        "existing files",
        "existing modules",
        "changed code",
        "changed files",
        "changed modules",
        "all relevant files",
        "all relevant code",
        "all relevant modules",
    ];
    if GENERIC_TARGETS.iter().any(|generic| lower == *generic) {
        return false;
    }

    let generic_prefixes = [
        "relevant ",
        "appropriate ",
        "affected ",
        "the relevant ",
        "the affected ",
        "existing ",
        "changed ",
        "all relevant ",
        "the ",
        "a ",
        "an ",
        "some ",
        "any ",
    ];
    if generic_prefixes.iter().any(|prefix| lower.starts_with(prefix)) {
        return false;
    }

    // A behavior target may be prose, but it still needs two recognizable
    // domain terms. This rejects arbitrary filler such as `foo bar` or
    // `implementation details` while allowing concrete behavior names such
    // as `approval handoff`, `startup latency`, and `cache invalidation`.
    const CONCRETE_BEHAVIOR_WORDS: &[&str] = &[
        "agent",
        "assertion",
        "approval",
        "artifact",
        "benchmark",
        "benchmarks",
        "bootstrap",
        "budget",
        "cache",
        "check",
        "command",
        "configuration",
        "confirmation",
        "context",
        "defer",
        "deferred",
        "duration",
        "durations",
        "error",
        "event",
        "execution",
        "fallback",
        "flow",
        "handoff",
        "initialization",
        "initialize",
        "input",
        "instrumentation",
        "interview",
        "latency",
        "launch",
        "launches",
        "lifecycle",
        "logic",
        "markup",
        "measure",
        "measurement",
        "measurements",
        "memory",
        "milestone",
        "milestones",
        "output",
        "parser",
        "parsing",
        "path",
        "performance",
        "permission",
        "persistence",
        "phase",
        "phases",
        "plan",
        "planning",
        "policy",
        "prompt",
        "question",
        "read",
        "recovery",
        "refresh",
        "report",
        "reports",
        "request",
        "response",
        "runtime",
        "state",
        "startup",
        "step",
        "stream",
        "symbol",
        "task",
        "test",
        "timeout",
        "timing",
        "tracker",
        "trace",
        "traces",
        "transition",
        "tool",
        "ui",
        "validation",
        "workflow",
        "write",
    ];
    let concrete_word_count = lower
        .split_whitespace()
        .map(|word| word.trim_matches(|character: char| character.is_ascii_punctuation()))
        .filter(|word| CONCRETE_BEHAVIOR_WORDS.contains(word))
        .count();
    lower.split_whitespace().count() >= 2 && concrete_word_count >= 2
}

fn verification_words(value: &str) -> Vec<&str> {
    value
        .split_whitespace()
        .map(|word| {
            // Preserve flag-shaped tokens (`-n`, `--locked`): verification
            // validation uses them as command-invocation evidence, and
            // stripping the leading hyphen would turn `-l` into prose `l`.
            if word.len() > 1 && word.starts_with('-') {
                return word;
            }
            word.trim_matches(|character: char| {
                character.is_ascii_punctuation()
                    && !matches!(
                        character,
                        '_' | '/'
                            | '.'
                            | ';'
                            | '&'
                            | '|'
                            | '<'
                            | '>'
                            | '$'
                            | '('
                            | ')'
                            | '{'
                            | '}'
                            | '['
                            | ']'
                            | '!'
                            | '*'
                            | '?'
                            | '~'
                            | '\\'
                    )
            })
        })
        .filter(|word| !word.is_empty())
        .collect()
}

fn is_invocation_cue(word: &str) -> bool {
    word.eq_ignore_ascii_case("run")
        || word.eq_ignore_ascii_case("running")
        || word.eq_ignore_ascii_case("execute")
        || word.eq_ignore_ascii_case("executing")
        || word.eq_ignore_ascii_case("invoke")
        || word.eq_ignore_ascii_case("invoking")
        || word.eq_ignore_ascii_case("use")
        || word.eq_ignore_ascii_case("using")
        || word.eq_ignore_ascii_case("with")
        || word.eq_ignore_ascii_case("by")
        || word.eq_ignore_ascii_case("via")
        || word.eq_ignore_ascii_case("through")
        || word.eq_ignore_ascii_case("then")
        || word.eq_ignore_ascii_case("plus")
        || word.eq_ignore_ascii_case("after")
        || word.eq_ignore_ascii_case("rerun")
}

fn is_verification_wrapper(word: &str) -> bool {
    ["command", "execute", "invoke", "run", "use"]
        .iter()
        .any(|wrapper| word.eq_ignore_ascii_case(wrapper))
}

fn is_actual_command_token(raw_word: &str) -> bool {
    let word = raw_word.trim_matches(|character: char| matches!(character, '`' | '"' | '\''));
    let bare_word = word.trim_matches(|character: char| character.is_ascii_punctuation());
    const COMMAND_NAMES: &[&str] = &[
        "awk",
        "bun",
        "cargo",
        "cat",
        "cmake",
        "clippy",
        "cut",
        "deno",
        "diff",
        "dotnet",
        "egrep",
        "eslint",
        "fgrep",
        "file",
        "find",
        "go",
        "gradle",
        "grep",
        "head",
        "jq",
        "just",
        "ls",
        "make",
        "meson",
        "mvn",
        "mypy",
        "ninja",
        "nextest",
        "npm",
        "npx",
        "pnpm",
        "python",
        "python3",
        "pytest",
        "rg",
        "ruff",
        "rustfmt",
        "sed",
        "shellcheck",
        "sort",
        "stat",
        "swiftlint",
        "tail",
        "tr",
        "tsc",
        "uniq",
        "wc",
        "xcodebuild",
        "yarn",
    ];
    (!word.contains('/') && COMMAND_NAMES.iter().any(|candidate| bare_word.eq_ignore_ascii_case(candidate)))
        || word.starts_with('/')
        || is_safe_workspace_relative_command_token(word)
        || (!word.contains('/') && is_script_command_token(word))
}

/// Command heads that are also common English words. Expanding `COMMAND_NAMES`
/// with inspection tools introduced false accepts such as `file changes` or
/// `sort order` — multi-word phrases that look like commands only because the
/// first token is allowlisted. Those heads now require a flag or path-like
/// later token; tooling names like `cargo`/`rg` keep the multi-token rule.
const AMBIGUOUS_COMMAND_HEADS: &[&str] = &[
    "cat", "cut", "diff", "file", "find", "head", "just", "ls", "make", "sort", "stat", "tr", "uniq", "wc",
];

fn is_flag_like_token(raw_word: &str) -> bool {
    let word = raw_word.trim_matches(|character: char| matches!(character, '`' | '"' | '\''));
    word.len() > 1 && word.starts_with('-')
}

/// Filename-shaped evidence for ambiguous command heads: `README.md`,
/// `notes.txt`, `Cargo.toml`. `is_pathlike_command_token` only accepts
/// slash/`./`/absolute shapes, so slash-less inspection args would otherwise
/// false-reject (`wc README.md`).
fn is_filename_like_token(raw_word: &str) -> bool {
    let word = raw_word.trim_matches(|character: char| matches!(character, '`' | '"' | '\''));
    if word.is_empty() || word.starts_with('-') {
        return false;
    }
    match word.rsplit_once('.') {
        Some((stem, suffix)) => {
            !stem.is_empty()
                && !suffix.is_empty()
                && suffix.len() <= 12
                && suffix.chars().all(|character| character.is_ascii_alphanumeric())
                && stem
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.'))
        }
        None => false,
    }
}

/// True when a command-head phrase still looks like a real invocation after
/// the allowlist match. Ambiguous English heads need flag/path evidence;
/// unambiguous heads keep existing semantics.
fn command_head_has_invocation_shape(words: &[&str]) -> bool {
    let Some(head) = words.first() else {
        return false;
    };
    let bare = head.trim_matches(|character: char| {
        character.is_ascii_punctuation() && !matches!(character, '_' | '/') || matches!(character, '`' | '"' | '\'')
    });
    if !AMBIGUOUS_COMMAND_HEADS
        .iter()
        .any(|candidate| bare.eq_ignore_ascii_case(candidate))
    {
        return true;
    }
    words.iter().skip(1).any(|word| {
        is_flag_like_token(word)
            || is_pathlike_command_token(word)
            || word.contains('/')
            || is_filename_like_token(word)
    })
}

fn is_safe_workspace_relative_command_token(raw_word: &str) -> bool {
    let word = raw_word.trim_matches(|character: char| matches!(character, '`' | '"' | '\''));
    let (word, dot_relative) = match word.strip_prefix("./") {
        Some(relative_word) => (relative_word, true),
        None => (word, false),
    };

    if has_url_scheme(word) || word.chars().any(is_shell_metacharacter) {
        return false;
    }

    if word.contains('/') {
        return word.split('/').all(is_safe_workspace_path_component);
    }

    dot_relative && is_safe_workspace_path_component(word)
}

fn is_safe_workspace_path_component(component: &str) -> bool {
    !component.is_empty()
        && component != "."
        && component != ".."
        && component
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-'))
}

fn has_url_scheme(word: &str) -> bool {
    let Some((scheme, _)) = word.split_once(':') else {
        return false;
    };
    let mut scheme_characters = scheme.chars();
    matches!(scheme_characters.next(), Some(first) if first.is_ascii_alphabetic())
        && scheme_characters.all(|character| character.is_ascii_alphanumeric() || matches!(character, '+' | '-' | '.'))
}

fn is_shell_metacharacter(character: char) -> bool {
    matches!(
        character,
        ';' | '&' | '|' | '<' | '>' | '$' | '(' | ')' | '{' | '}' | '[' | ']' | '!' | '*' | '?' | '~' | '\\'
    )
}

fn is_script_command_token(word: &str) -> bool {
    word.ends_with(".sh") || word.ends_with(".cmd") || word.ends_with(".ps1") || word.ends_with(".bat")
}

fn is_shell_assignment_token(raw_word: &str) -> bool {
    let word = raw_word.trim_matches(|character: char| matches!(character, '`' | '"' | '\''));
    let Some((name, value)) = word.split_once('=') else {
        return false;
    };

    if name.is_empty() || value.is_empty() {
        return false;
    }

    let mut name_chars = name.chars();
    match name_chars.next() {
        Some(first) if first == '_' || first.is_ascii_alphabetic() => {}
        _ => return false,
    }

    name_chars.all(|character| character == '_' || character.is_ascii_alphanumeric())
}

fn is_pathlike_command_token(raw_word: &str) -> bool {
    let word = raw_word.trim_matches(|character: char| matches!(character, '`' | '"' | '\''));
    word.starts_with('/')
        || is_safe_workspace_relative_command_token(word)
        || (!word.contains('/') && is_script_command_token(word))
}

fn contains_actual_command_invocation(value: &str) -> bool {
    let words = verification_words(value);
    if words.len() < 2 {
        return false;
    }

    let mut assignment_prefix_end = 0;
    while words
        .get(assignment_prefix_end)
        .is_some_and(|word| is_shell_assignment_token(word))
    {
        assignment_prefix_end += 1;
    }
    if assignment_prefix_end > 0
        && words
            .get(assignment_prefix_end)
            .is_some_and(|word| is_actual_command_token(word))
        && (is_pathlike_command_token(words[assignment_prefix_end]) || words.len() > assignment_prefix_end + 1)
        && command_head_has_invocation_shape(&words[assignment_prefix_end..])
    {
        return true;
    }

    for index in 0..words.len() {
        let word = words[index];
        if !is_actual_command_token(word) {
            continue;
        }

        if index == 0 {
            return (words.len() > 1 || is_pathlike_command_token(word))
                && command_head_has_invocation_shape(words.as_slice());
        }

        if (is_pathlike_command_token(word) || words.get(index + 1).is_some())
            && words[..index].iter().rev().take(3).any(|previous| is_invocation_cue(previous))
            && command_head_has_invocation_shape(&words[index..])
        {
            return true;
        }
    }

    value
        .split('`')
        .skip(1)
        .step_by(2)
        .any(|span| contains_actual_command_invocation(span.trim()))
}

/// History inspection can verify a review step when it selects actual evidence.
/// Keep this narrower than the shell read-only policy: a bare Git command or
/// whitespace check alone does not establish the step's outcome.
fn is_git_command_token(raw_word: &str) -> bool {
    let word = raw_word.trim_matches(|character: char| matches!(character, '`' | '"' | '\''));
    word.rsplit([';', '&', '|'])
        .next()
        .unwrap_or(word)
        .rsplit(['/', '\\'])
        .next()
        .is_some_and(|name| matches!(name, "git" | "git.exe"))
}

fn contains_shell_metacharacter(value: &str) -> bool {
    value
        .chars()
        .any(|character| matches!(character, ';' | '&' | '|' | '$' | '`' | '<' | '>' | '\\'))
}

fn git_verification_command_start(words: &[&str]) -> Option<usize> {
    let mut assignment_prefix_end = 0;
    while words
        .get(assignment_prefix_end)
        .is_some_and(|word| is_shell_assignment_token(word))
    {
        assignment_prefix_end += 1;
    }

    if words.get(assignment_prefix_end).is_some_and(|word| is_git_command_token(word)) {
        return Some(assignment_prefix_end);
    }
    if is_verification_wrapper(words.get(assignment_prefix_end)?)
        && words
            .get(assignment_prefix_end + 1)
            .is_some_and(|word| is_git_command_token(word))
    {
        return Some(assignment_prefix_end + 1);
    }

    words.iter().enumerate().find_map(|(index, word)| {
        (is_git_command_token(word)
            && (contains_shell_metacharacter(word)
                || words[..index].iter().any(|previous| contains_shell_metacharacter(previous))
                || words[..index]
                    .iter()
                    .rev()
                    .take(3)
                    .any(|previous| is_invocation_cue(previous) || is_verification_wrapper(previous))))
        .then_some(index)
    })
}

fn is_concrete_git_verification(value: &str) -> bool {
    if value.contains(['\n', '\r']) {
        return false;
    }
    let words = verification_words(value);
    let Some(command_start) = git_verification_command_start(&words) else {
        return false;
    };
    if words[..command_start].iter().any(|prefix| contains_shell_metacharacter(prefix)) {
        return false;
    }
    let Some((command, words)) = words[command_start..].split_first() else {
        return false;
    };
    if !is_git_command_token(command) || contains_shell_metacharacter(command) {
        return false;
    }
    let Some((subcommand, args)) = words.split_first() else {
        return false;
    };
    if !matches!(*subcommand, "log" | "show" | "diff" | "blame")
        || args.is_empty()
        || args.iter().any(|arg| {
            ["--check", "--output", "--ext-diff", "--exec", "--format=%x"]
                .iter()
                .any(|unsafe_arg| arg.contains(unsafe_arg))
                || contains_shell_metacharacter(arg)
        })
    {
        return false;
    }
    args.iter().enumerate().any(|(index, arg)| {
        let arg = arg.trim_matches(|ch| matches!(ch, '\'' | '"'));
        (arg == "--" && args.get(index + 1).is_some())
            || (arg.starts_with('-')
                && (["--since=", "--until=", "--author=", "--grep=", "--max-count="]
                    .iter()
                    .any(|prefix| arg.strip_prefix(prefix).is_some_and(|value| !value.is_empty()))
                    || arg
                        .strip_prefix('-')
                        .is_some_and(|count| !count.is_empty() && count.chars().all(|ch| ch.is_ascii_digit()))))
            || (!arg.starts_with('-') && (index == 0 || args[index - 1] != "--"))
    })
}

/// Luu agentic-testing: fresh-context independent re-derivation counts as verification.
/// Accepts `independent-rederive <target> (fresh context, no helper reuse)`
/// so high-risk steps can require an oracle independent of production helpers.
pub(super) fn is_independent_rederivation(value: &str) -> bool {
    let lowered = value.to_ascii_lowercase();
    let fresh = lowered.contains("fresh") || lowered.contains("independent");
    let rederive = lowered.contains("re-derive") || lowered.contains("rederive") || lowered.contains("re derive");
    let no_reuse = lowered.contains("without reusing")
        || lowered.contains("no helper reuse")
        || lowered.contains("no reuse")
        || lowered.contains("fresh context");
    if !(fresh && rederive && no_reuse) {
        return false;
    }
    // Require a concrete target beyond the keywords so vacuous
    // `independent rederive fresh context` cannot pass as verification.
    verification_words(value).len() >= 4
}

fn is_observable_manual_verification(value: &str) -> bool {
    let words = verification_words(value);
    if words.len() < 2 {
        return false;
    }

    const CUES: &[&str] = &[
        "benchmark",
        "benchmarks",
        "check",
        "checks",
        "compare",
        "compares",
        "confirm",
        "confirms",
        "instrument",
        "instrumented",
        "launch",
        "launches",
        "measure",
        "measures",
        "record",
        "records",
        "review",
        "reviews",
        "observe",
        "observes",
        "inspect",
        "inspects",
        "profile",
        "profiles",
        "run",
        "runs",
        "test",
        "tests",
        "validate",
        "validates",
        "verify",
        "verifies",
    ];
    const EVIDENCE: &[&str] = &[
        "after",
        "baseline",
        "before",
        "cold",
        "debug",
        "duration",
        "durations",
        "faster",
        "fewer",
        "improve",
        "improved",
        "improvement",
        "improves",
        "latency",
        "log",
        "logs",
        "metric",
        "metrics",
        "output",
        "outputs",
        "phase",
        "phases",
        "read",
        "reads",
        "reported",
        "result",
        "results",
        "speed",
        "startup",
        "time",
        "times",
        "timing",
        "unchanged",
        "warm",
        "launch",
        "launches",
        "prompt",
    ];

    let cue_count = words
        .iter()
        .filter(|word| CUES.iter().any(|cue| word.eq_ignore_ascii_case(cue)))
        .count();
    if cue_count == 0 {
        return false;
    }

    let evidence_count = words
        .iter()
        .filter(|word| {
            word.chars().any(|character| character.is_ascii_digit())
                || EVIDENCE.iter().any(|evidence| word.eq_ignore_ascii_case(evidence))
        })
        .count();
    let has_non_temporal_evidence = words.iter().any(|word| {
        !matches!(*word, "after" | "before") && EVIDENCE.iter().any(|evidence| word.eq_ignore_ascii_case(evidence))
    });
    let legacy_observable_marker = [
        "assert",
        "available",
        "completes",
        "contains",
        "deferred",
        "emits",
        "expected",
        "fails",
        "finishes",
        "holds",
        "includes",
        "matches",
        "manual",
        "measure",
        "never",
        "observable",
        "outputs",
        "persists",
        "preserves",
        "remains",
        "renders",
        "reports",
        "returns",
        "shows",
        "starts",
        "stays",
        "survives",
        "updates",
        "visible",
        "waits",
    ]
    .iter()
    .any(|marker| words.iter().any(|word| word.eq_ignore_ascii_case(marker)));
    let tests_pass = words
        .iter()
        .any(|word| word.eq_ignore_ascii_case("tests") || word.eq_ignore_ascii_case("checks"))
        && words.iter().any(|word| word.eq_ignore_ascii_case("pass"));

    (evidence_count >= 2 && has_non_temporal_evidence) || legacy_observable_marker || tests_pass
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum VerificationValidationError {
    NotConcrete,
    InvalidItem { ordinal: usize },
}

pub(super) fn is_optional_markdown_verification(value: &str) -> bool {
    let value = value.trim();
    if value.starts_with('[') && value.ends_with(']') {
        let items = parse_bracket_list(value);
        return !items.is_empty() && items.iter().all(|item| is_optional_markdown_verification(item));
    }
    value.eq_ignore_ascii_case("skip Markdown lint if unavailable")
        || value.eq_ignore_ascii_case("skip Markdown validation if unavailable")
}

pub(super) fn is_markdown_target(value: &str) -> bool {
    let value = marker_value(value, PLAN_TARGET_LABELS).unwrap_or(value).trim();
    if value.starts_with('[') && value.ends_with(']') {
        let items = parse_bracket_list(value);
        return !items.is_empty() && items.iter().all(|item| is_markdown_target(item));
    }
    let path = value.trim_matches(['`', '\'', '"']);
    let path = path.split('#').next().unwrap_or(path);
    let path = path
        .rsplit_once(':')
        .filter(|(prefix, _)| Path::new(prefix).extension().is_some())
        .map_or(path, |(prefix, _)| prefix);
    let extension = Path::new(path).extension().and_then(|extension| extension.to_str());
    extension
        .is_some_and(|extension| extension.eq_ignore_ascii_case("md") || extension.eq_ignore_ascii_case("markdown"))
}

pub(super) fn validate_concrete_verification(value: &str) -> Result<(), VerificationValidationError> {
    let value = value.trim();
    if !is_concrete_value(value) {
        return Err(VerificationValidationError::NotConcrete);
    }

    if value.starts_with('[') && value.ends_with(']') {
        let items = parse_bracket_list(value);
        if items.is_empty() {
            return Err(VerificationValidationError::NotConcrete);
        }
        for (index, item) in items.iter().enumerate() {
            if validate_concrete_verification(item).is_err() {
                return Err(VerificationValidationError::InvalidItem { ordinal: index + 1 });
            }
        }
        return Ok(());
    }

    // Optional Markdown tooling must not block a documentation plan. Keep
    // the conditional exception explicit; ordinary checks remain required.
    if is_optional_markdown_verification(value) {
        return Ok(());
    }

    let words = verification_words(value);
    if git_verification_command_start(&words).is_some()
        || (value.contains(['\n', '\r']) && words.iter().any(|word| is_git_command_token(word)))
    {
        return is_concrete_git_verification(value)
            .then_some(())
            .ok_or(VerificationValidationError::NotConcrete);
    }
    let leading_wrapper = words.first().is_some_and(|word| is_verification_wrapper(word));
    if (words.first().is_some_and(|word| is_actual_command_token(word))
        && (words.len() > 1 || words.first().is_some_and(|word| is_pathlike_command_token(word)))
        && command_head_has_invocation_shape(words.as_slice()))
        || (leading_wrapper
            && words.get(1).is_some_and(|word| is_actual_command_token(word))
            && (words.len() > 2 || words.get(1).is_some_and(|word| is_pathlike_command_token(word)))
            && command_head_has_invocation_shape(&words[1..]))
        || contains_actual_command_invocation(value)
    {
        return Ok(());
    }

    if is_independent_rederivation(value) {
        return Ok(());
    }

    if is_observable_manual_verification(value) {
        Ok(())
    } else {
        Err(VerificationValidationError::NotConcrete)
    }
}
