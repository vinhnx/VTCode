//! Shell activity classification for progress accounting and output previews.
//!
//! Mutation safety remains owned by [`super::classify_tool_intent`]. This
//! module adds the narrower distinction between repository inspection and
//! verification without duplicating that safety decision in binary consumers.

use std::path::Path;

use serde_json::Value;
use vtcode_commons::validation::NonEmptySlice;

use crate::tools::command_args::shell_command_text_with_args;

use super::readonly::{
    command_words_are_readonly, static_shell_command_words, static_shell_command_words_with_output_plumbing,
};

mod verifier;
pub(crate) use verifier::pipefail_verifier_pipeline;
pub use verifier::{
    GENERIC_VERIFIER_DESCRIPTION, VERIFIER_SHELL_FORM_NOTE, default_verifier_for_workspace,
    rewrite_truncation_only_verifier, shell_command_contains_verifier, shell_command_is_admitted_verification_attempt,
    shell_command_is_output_search, shell_command_is_standalone_grep_search, verification_recovery_directive,
    verifier_reference,
};
use verifier::{
    contains_verification_invocation, has_logical_sequencing, is_verification_invocation, rewrite_verifier_pipeline,
};

/// Progress semantics for a command invocation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShellActivity {
    /// Read-only repository or environment inspection.
    Inspection,
    /// A build, test, lint, or compile command that verifies work.
    Verification,
    /// A command that may mutate state and is not primarily verification.
    Mutation,
}

/// Shell-call arguments as the execution kernel runs them.
///
/// The kernel applies `rewrite_verifier_pipeline` to every
/// command-run call before execution, so the process that runs (and whose
/// exit status the outcome reports) is the standalone verifier, not the typed
/// pipeline. Static filtering pipelines instead enable pipefail. Gate
/// bookkeeping must classify that same command; classifying the
/// typed pipeline would call a truthful `cargo check 2>&1 | tail -5` success a
/// mutation and leave the gate pending. Arguments the kernel runs unchanged
/// (already-normalized arguments included, since the rewrite is idempotent)
/// are borrowed as-is.
pub fn shell_args_as_executed<'a>(tool_name: &str, args: &'a Value) -> std::borrow::Cow<'a, Value> {
    if !super::is_command_run_tool_call(tool_name, args) {
        return std::borrow::Cow::Borrowed(args);
    }
    let Some(rewritten) = rewrite_verifier_pipeline(args) else {
        return std::borrow::Cow::Borrowed(args);
    };
    let mut executed = args.clone();
    match executed.as_object_mut() {
        Some(payload) => {
            if payload.get("raw_command").is_some_and(Value::is_string) {
                payload.insert("raw_command".to_string(), Value::String(rewritten.clone()));
            }
            // The rewrite already includes the suffix (or elides its truncator).
            // Keeping args would append it a second time at launch.
            payload.remove("args");
            payload.insert("command".to_string(), Value::String(rewritten));
            std::borrow::Cow::Owned(executed)
        }
        None => std::borrow::Cow::Borrowed(args),
    }
}

fn is_known_inspection(words: &[String]) -> bool {
    if words
        .iter()
        .any(|word| matches!(word.as_str(), ">" | ">>" | "|" | "&&" | ";" | "||"))
    {
        return false;
    }
    command_words_are_readonly(words)
}

fn classify_provable_shell_sequence(command: &str) -> Option<ShellActivity> {
    let (segments, has_output_plumbing) = if let Some(segments) = static_shell_command_words(command) {
        (segments, false)
    } else {
        (static_shell_command_words_with_output_plumbing(command)?, true)
    };
    if has_output_plumbing && segments.len() != 1 {
        return None;
    }
    let has_multiple_segments = segments.len() > 1;
    let mut saw_verification = false;

    for words in segments {
        if is_verification_invocation(&words) {
            saw_verification = true;
        } else if !command_words_are_readonly(&words) {
            return None;
        }
    }

    if has_output_plumbing && !saw_verification {
        return None;
    }

    // Shell execution does not guarantee that a pipeline or logical chain's
    // final status reflects every verification stage. Do not let a successful
    // downstream command clear the anti-blind checkpoint after an earlier
    // verifier failed.
    //
    // Exception: a pure `&&` chain of verification (or readonly) segments
    // short-circuits on first failure, so its exit status does represent every
    // stage. `cargo fmt --check && cargo check --locked && cargo nextest run`
    // must clear the gate; `;`, `||`, `|`, and background `&` still mask
    // failures and stay `Mutation`.
    if saw_verification && has_multiple_segments {
        if shell_uses_only_and_chaining(command) {
            return Some(ShellActivity::Verification);
        }
        return Some(ShellActivity::Mutation);
    }

    Some(if saw_verification {
        ShellActivity::Verification
    } else {
        ShellActivity::Inspection
    })
}

fn has_shell_sequence(command: &str) -> bool {
    static_shell_command_words(command).is_none_or(|segments| segments.len() > 1)
}

/// Quote-aware check that a shell command chains segments only with `&&`.
///
/// Returns `false` when an unquoted `;`, newline, `||`, single `&`
/// (background), or `|` (pipeline) is present, since those operators let a
/// downstream success mask an earlier verifier failure. `&&` short-circuits,
/// so a pure `&&` chain of verifiers has a faithful aggregate exit status and
/// may clear the anti-blind-editing gate. Backslash escapes outside single
/// quotes are honored, so a shell-literal `\"` cannot open a phantom quote
/// state that hides a later live operator.
fn shell_uses_only_and_chaining(command: &str) -> bool {
    let chars: Vec<char> = command.chars().collect();
    let mut index = 0;
    let mut in_single_quote = false;
    let mut in_double_quote = false;

    while index < chars.len() {
        let character = chars[index];
        // Outside single quotes a backslash escapes the next character for the
        // shell: `\"` is a literal quote (no quote-state change) and `\;` is an
        // inert character, not an operator. Skip the pair so the scanner stays
        // aligned with the shell and fails closed.
        if character == '\\' && !in_single_quote && index + 1 < chars.len() {
            index += 2;
            continue;
        }
        if character == '\'' && !in_double_quote {
            in_single_quote = !in_single_quote;
            index += 1;
            continue;
        }
        if character == '"' && !in_single_quote {
            in_double_quote = !in_double_quote;
            index += 1;
            continue;
        }
        if in_single_quote || in_double_quote {
            index += 1;
            continue;
        }
        match character {
            // Token extraction walks nested commands and drops negation;
            // neither shape proves each check actually returned success.
            ';' | '\n' | '|' | '!' | '(' | ')' => return false,
            '&' => {
                let next_is_and = chars.get(index + 1) == Some(&'&');
                if !next_is_and {
                    return false;
                }
                // A `||`-style `|` was already rejected above; `&&` consumes
                // both characters. `|||`, `&&&`, and similar malformed
                // sequences fail closed.
                let third = chars.get(index + 2);
                if third == Some(&'&') || third == Some(&'|') {
                    return false;
                }
                index += 2;
                continue;
            }
            _ => {}
        }
        index += 1;
    }

    true
}

/// Match required checks within a statically proven, status-preserving chain.
/// This is evidence accounting only; execution and permission gates still apply.
pub(crate) fn shell_command_covers_required_check(command: &str, required: &str) -> bool {
    if !shell_uses_only_and_chaining(command) || !shell_uses_only_and_chaining(required) {
        return false;
    }
    let Some(segments) = static_shell_command_words(command) else {
        return false;
    };
    let Some(required_segments) = static_shell_command_words(required) else {
        return false;
    };
    if segments.iter().any(|words| {
        words.first().is_some_and(|word| word == "cd")
            || (!is_verification_invocation(words) && !command_words_are_readonly(words))
    }) {
        return false;
    }
    segments
        .windows(required_segments.len())
        .any(|window| window == required_segments)
}

/// Classify a shell call without weakening the authoritative mutation guard.
///
/// Standalone output plumbing such as `2>&1` or `> build.log` does not turn a
/// primary verification command into a mutation for progress accounting.
/// Pipelines and `;`/`||` chains remain mutations because their final
/// status does not reliably represent every verification stage. Pure `&&`
/// chains of verification-or-readonly segments are `Verification` since `&&`
/// short-circuits on first failure.
#[must_use]
pub fn classify_shell_activity(tool_name: &str, args: &Value) -> ShellActivity {
    let command = shell_command_text_with_args(args);
    if command.is_none() && crate::tools::command_args::raw_command_text(args).is_some() {
        return ShellActivity::Mutation;
    }
    if command.as_deref().and_then(pipefail_verifier_pipeline).is_some() {
        return ShellActivity::Verification;
    }
    let words = crate::tools::command_args::command_words(args).ok().flatten();
    let has_unclassified_shell_sequence = command.as_deref().is_some_and(has_shell_sequence);

    if let Some(activity) = command.as_deref().and_then(classify_provable_shell_sequence) {
        return activity;
    }
    // A failed sequence proof cannot fall back to the original tool intent:
    // appended arguments may make an otherwise read-only tail mutate state.
    if has_unclassified_shell_sequence {
        return ShellActivity::Mutation;
    }

    let intent = super::classify_tool_intent(tool_name, args);

    if !has_unclassified_shell_sequence && words.as_deref().is_some_and(is_known_inspection) {
        return ShellActivity::Inspection;
    }

    let starts_with_verification = words.as_deref().is_some_and(is_verification_invocation);
    let contains_verification =
        starts_with_verification || command.as_deref().is_some_and(contains_verification_invocation);
    if !intent.mutating {
        return if contains_verification {
            ShellActivity::Verification
        } else {
            ShellActivity::Inspection
        };
    }

    if starts_with_verification
        && !has_unclassified_shell_sequence
        && !words.as_deref().is_some_and(has_logical_sequencing)
    {
        ShellActivity::Verification
    } else {
        ShellActivity::Mutation
    }
}

fn is_literal_timer(words: &[String]) -> bool {
    let seconds = match words {
        [program, seconds] if program == "sleep" => Some(seconds.as_str()),
        [program, flag, code] if matches!(program.as_str(), "python" | "python3") && flag == "-c" => code
            .strip_prefix("import time; time.sleep(")
            .and_then(|code| code.strip_suffix(')')),
        _ => None,
    };
    seconds.is_some_and(|seconds| {
        !seconds.is_empty()
            && seconds.bytes().all(|byte| byte.is_ascii_digit() || byte == b'.')
            && seconds
                .parse::<f64>()
                .is_ok_and(|seconds| seconds.is_finite() && seconds >= 0.0)
    })
}

/// Whether an exec result creates workspace mutation debt. This is accounting
/// only: permission and admission still use the stricter static classifiers.
/// Unknown syntax or commands retain debt; changed-file evidence always wins.
#[must_use]
pub fn exec_mutation_should_incur_debt(command: &str, output: &Value) -> bool {
    if exec_output_has_changes(output) {
        return true;
    }
    let Ok(tree) = crate::command_safety::shell_parser::BashTree::parse(command) else {
        return true;
    };
    let root = tree.root();
    tree.has_error() || root.named_children().is_empty() || !nonmutating_shell_node(root)
}

pub(crate) fn exec_output_has_changes(output: &Value) -> bool {
    output
        .get("modified_files")
        .and_then(Value::as_array)
        .is_some_and(|files| !files.is_empty())
        || crate::tools::file_ops::diff_output_has_effective_change(output) == Some(true)
}

/// Path- and dot-prefixed globs cannot expand into options. Keep this narrower than
/// general expansion: variables, substitutions, bare globs and dynamic heads
/// remain unproven, and the parent command still checks program/options.
fn is_literal_path_glob(word: &str) -> bool {
    let prefix = match word.split_once('/') {
        Some((prefix, _)) => prefix,
        None if word.starts_with('.') => ".",
        None => return false,
    };
    !prefix.starts_with('-')
        && !prefix.contains(['*', '?', '[', ']'])
        && word.contains(['*', '?'])
        && !word.contains(['[', ']'])
        && !word
            .chars()
            .any(|character| character.is_whitespace() || "\"'$`\\{}()<>&;|~!".contains(character))
}

/// Inline grep filters remain one option even when the literal pattern has globs.
/// This is accounting proof only; shell admission is unchanged.
fn is_literal_grep_filter(word: &str) -> bool {
    let Some((option, pattern)) = word.split_once('=') else {
        return false;
    };
    matches!(option, "--include" | "--exclude" | "--exclude-dir")
        && !pattern.is_empty()
        && !pattern.chars().any(|c| c.is_whitespace() || "\"'$`\\{}()<>&;|~!".contains(c))
}

/// Recognize the shell's file-existence test on one quoted variable. Other
/// test expressions can contain substitutions or arithmetic with effects.
fn is_quoted_variable_file_test(text: &str) -> bool {
    let Some(expression) = text.strip_prefix('[').and_then(|s| s.strip_suffix(']')) else {
        return false;
    };
    let Some(operand) = expression.trim().strip_prefix("-f ") else {
        return false;
    };
    let Some(variable) = operand
        .trim()
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .and_then(|s| s.strip_prefix('$'))
    else {
        return false;
    };
    let mut chars = variable.chars();
    chars.next().is_some_and(|ch| ch == '_' || ch.is_ascii_alphabetic())
        && chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}

/// Walk every supported node, including commands nested in substitutions.
/// Unlike command extraction, this proof must never discard unknown syntax.
fn nonmutating_shell_node(node: crate::command_safety::shell_parser::BashNode<'_>) -> bool {
    use crate::command_safety::shell_parser::contains_dynamic_shell_syntax;
    let text = node.text();
    match node.kind() {
        "test_command" => return is_quoted_variable_file_test(text),
        "command" => {
            let Some(name) = node.field("name") else {
                return false;
            };
            let name_text = name.text();
            if contains_dynamic_shell_syntax(name_text) {
                return false;
            }
            let mut words = Vec::new();
            for child in node.named_children() {
                let word = child.text();
                if child.kind() == "variable_assignment" {
                    return false;
                }
                if child.start_byte() != name.start_byte()
                    && child.kind() == "word"
                    && (is_literal_path_glob(word)
                        || (matches!(name_text, "grep" | "egrep" | "fgrep" | "rg") && is_literal_grep_filter(word)))
                    && matches!(name_text, "rg" | "grep" | "egrep" | "fgrep" | "ls" | "cat" | "head" | "tail" | "wc")
                {
                    // Classification sees a path operand; expansion cannot
                    // produce flags, while static mutating options stay visible.
                    words.push(word.to_owned());
                } else if contains_dynamic_shell_syntax(word) {
                    // Only these builtins cannot turn a dynamic argument into
                    // an option that writes or executes code. Keep arguments
                    // quoted so expansion cannot alter their dimensionality.
                    if !matches!(name_text, "echo" | "test") || !word.starts_with('"') {
                        return false;
                    }
                    words.push("dynamic-argument".to_owned());
                } else {
                    let Ok(parts) = shell_words::split(word) else {
                        return false;
                    };
                    words.extend(parts);
                }
            }
            if crate::tools::command_args::has_unsafe_readonly_options(&words)
                || (!is_verification_invocation(&words)
                    && !command_words_are_readonly(&words)
                    && !is_literal_timer(&words))
            {
                return false;
            }
        }
        "file_redirect" => {
            // Exact AST spans, never substring replacement: a filename such
            // as /dev/null.backup must not be mistaken for descriptor plumbing.
            return matches!(text, "2>&1" | "1>&2") || (cfg!(unix) && text == "2>/dev/null");
        }
        "word" | "regex" => {
            return !contains_dynamic_shell_syntax(text) || is_literal_path_glob(text) || is_literal_grep_filter(text);
        }
        "raw_string" | "string_content" | "comment" | "number" | "variable_name" | "special_variable_name" => {
            return true;
        }
        "simple_expansion" => return true,
        "expansion" => {
            // Support the literal prefix/suffix removal used by link probes;
            // other parameter operators and arithmetic remain unproven.
            if text.contains([':', '=', '+', '?', '!']) {
                return false;
            }
        }
        "program"
        | "list"
        | "pipeline"
        | "for_statement"
        | "do_group"
        | "if_statement"
        | "elif_clause"
        | "else_clause"
        | "while_statement"
        | "command_name"
        | "string"
        | "concatenation"
        | "command_substitution"
        | "redirected_statement" => {}
        _ if !node.is_named() => {
            return matches!(
                text,
                "&&" | "||"
                    | ";"
                    | "|"
                    | "for"
                    | "in"
                    | "do"
                    | "done"
                    | "if"
                    | "then"
                    | "elif"
                    | "else"
                    | "fi"
                    | "while"
                    | "until"
                    | "\""
                    | "'"
                    | "$("
                    | ")"
                    | "`"
                    | "$"
                    | "${"
                    | "}"
                    | "#"
                    | "##"
                    | "%"
                    | "%%"
            );
        }
        _ => return false,
    }
    node.children().iter().all(|child| nonmutating_shell_node(*child))
}

#[cfg(test)]
mod tests;
