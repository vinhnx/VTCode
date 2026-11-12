use crate::tools::command_args::{
    command_words_after_environment_prefix, has_unsafe_readonly_options, shell_command_text_with_args,
};
use crate::tools::output_spooler::SpooledOutputReference;
use serde_json::Value;
use std::path::Path;

/// Conservative allow-list of read-only inspection commands used by
/// `command_session`. Any command that could write, move, or delete must be
/// rejected so it is not cached or parallelized as read-only.
///
/// `cd` is included because changing the working directory mutates nothing;
/// models habitually prefix exploration with `cd <workspace> && …` and plan
/// mode must not reject that pattern (checkpoint turn_810).
///
/// `ast-grep`/`sg` are deliberately absent. VT Code no longer ships the
/// ast-grep binary, and `sg` also names the shadow-utils group-switch
/// command, which runs an arbitrary command (`sg <group> -c '<command>'`).
/// Allow-listing it would classify such an invocation as a read-only
/// inspection and skip the approval gate.
///
/// Programs that stay on the list but can still write or execute need an
/// option-level guard in `command_args::has_unsafe_readonly_options` — for
/// example `bat --pager <command>`, which spawns the named program.
const READONLY_UNIFIED_EXEC_COMMANDS: &[&str] = &[
    "rg", "ls", "cat", "diff", "find", "wc", "grep", "egrep", "fgrep", "head", "tail", "sort", "uniq", "bat", "sed",
    "awk", "cut", "tr", "echo", "pwd", "printf", "true", "false", "test", "cd", "fd", "tree", "which", "stat", "file",
    "du", "df", "realpath", "basename", "dirname", "nl", "column", "jq", "date", "whoami", "uname",
];

pub fn is_readonly_base_command(command: &str) -> bool {
    READONLY_UNIFIED_EXEC_COMMANDS.contains(&command)
}

/// Read-only subcommand allow-list for multi-word tools whose base command is
/// not inherently safe (`git`, `cargo`, package managers). Only inspection
/// subcommands are listed; anything that can mutate the worktree, index,
/// refs, or lockfiles must stay out.
///
/// Unconditionally read-only: every form of these subcommands is an
/// inspection, so this list is also safe for the flag-skipping parallel-safe
/// extractor. Flag-shaped subcommands whose bare form reads but whose operand
/// forms mutate live in [`is_readonly_flag_shaped_git_subcommand`].
fn is_readonly_subcommand(first: &str, second: Option<&str>, third: Option<&str>) -> bool {
    match first {
        "git" => matches!(
            second,
            Some(
                "status"
                    | "log"
                    | "diff"
                    | "show"
                    | "blame"
                    | "ls-files"
                    | "rev-parse"
                    | "describe"
                    | "shortlog"
                    | "grep"
                    | "rev-list"
                    | "ls-tree"
                    | "cat-file"
                    | "diff-tree"
                    | "merge-base"
                    | "range-diff"
                    | "whatchanged"
                    | "count-objects"
                    | "var"
                    | "version"
                    | "help"
            )
        ),
        "cargo" => match second {
            Some("check" | "test" | "metadata" | "tree" | "clippy") => true,
            Some("nextest") => matches!(third, Some("run" | "list")),
            _ => false,
        },
        "npm" | "pnpm" | "yarn" => match second {
            Some("test") => true,
            Some("run") => matches!(third, Some("test")),
            _ => false,
        },
        _ => false,
    }
}

/// Flag-shaped git subcommands whose bare or flag-only form is a pure read
/// (`git tag`, `git branch -a`, `git remote -v`, `git reflog`, `git stash
/// list`, `git worktree list`), while the same name with an operand mutates
/// (`tag v1`, `branch -D x`, `reflog expire`, `stash pop`, `remote add`,
/// `worktree add`). The mutating operand survives as the third word, so this
/// classification is position-safe only on the read-only path, which keeps
/// operands; the flag-skipping parallel-safe extractor must never consult it
/// (there `git tag -f x` would look identical to `git tag`).
///
/// Two hardening rules keep the bare/flag-only forms honest:
/// - bare `stash` is excluded: bare `git stash` is `git stash push`, which
///   stashes uncommitted work, and flag-only push forms (`-k`, `-u`, `-a`,
///   `-m=msg`) carry no operand to betray them;
/// - `branch` carries a mutating-flag denylist, because inline-value flags
///   (`--set-upstream-to=origin/x`, `--unset-upstream`, `--edit-description`)
///   and glued short mutators (`-m<name>`, `-c<name>`, `-u<upstream>`) start
///   with `-` and are invisible to the operand finder.
fn is_readonly_flag_shaped_git_subcommand(
    subcommand: Option<&str>,
    operand: Option<&str>,
    command_words: &[String],
    sub_index: usize,
) -> bool {
    if subcommand == Some("branch") {
        let mutates = command_words.iter().skip(sub_index + 1).any(|word| {
            matches!(word.as_str(), "--set-upstream" | "--unset-upstream" | "--edit-description")
                || word.starts_with("--set-upstream-to")
                || word.starts_with("-u")
                || word.starts_with("-m")
                || word.starts_with("-M")
                || word.starts_with("-c")
                || word.starts_with("-C")
        });
        if mutates {
            return false;
        }
    }

    matches!(
        (subcommand, operand),
        (Some("tag") | Some("branch"), None)
            | (Some("reflog"), None | Some("show"))
            | (Some("remote"), None | Some("show") | Some("get-url"))
            | (Some("stash"), Some("list") | Some("show"))
            | (Some("worktree"), Some("list"))
    )
}

/// A version or help probe prints to stdout and mutates nothing for any
/// program, including subcommand-based tools (`cargo --version`, `node
/// --version`, `python3 -V`) that the subcommand allow-list cannot classify
/// because every word after the program is a flag. At least one probe flag
/// is required (a bare program name like `tsc` still compiles) and ALL
/// trailing words must be probe flags, so `python3 script.py` or `git commit
/// -v` stay outside. Lowercase `-v` is deliberately excluded: it means
/// verbose rather than version for most tools, and for shell interpreters
/// (`bash -v`) it reads stdin.
fn is_version_or_help_probe(command_words: &[String]) -> bool {
    fn is_probe_flag(word: &str) -> bool {
        matches!(word, "-h" | "--help" | "-V" | "--version")
    }

    let mut flags = command_words.iter().skip(1);
    flags.next().is_some_and(|word| is_probe_flag(word)) && flags.all(|word| is_probe_flag(word))
}

/// Parse a shell script into simple command words after proving that it has no
/// dynamic expansion, file redirection, or unsupported background operator.
///
/// The tree-sitter parser is the single shell-grammar boundary used by the
/// safety subsystem. Callers receive already-tokenized command words and must
/// still apply their own command-policy predicate; an unknown command never
/// becomes read-only merely because parsing succeeded.
pub(crate) fn static_shell_command_words(command: &str) -> Option<Vec<Vec<String>>> {
    let sanitized = sanitize_static_shell_command(command)?;
    parse_shell_command_words(&sanitized)
}

/// Parse a command whose only shell operators outside quotes are output
/// redirections. This is intentionally separate from the read-only parser:
/// writing a build log is still a mutating command for the authoritative tool
/// intent, but it should not hide the verification nature of `cargo check` in
/// progress accounting.
pub(crate) fn static_shell_command_words_with_output_plumbing(command: &str) -> Option<Vec<Vec<String>>> {
    if !crate::command_safety::shell_parser::has_only_output_redirections(command) {
        return None;
    }
    parse_shell_command_words(command)
}

/// Tokenize shell invocations without proving static execution or safety.
/// Permission and activity predicates must sanitize before using these words;
/// diagnostic-only consumers may inspect dynamic shell text without grants.
pub(super) fn parse_shell_command_words(command: &str) -> Option<Vec<Vec<String>>> {
    let commands = crate::command_safety::shell_parser::parse_shell_commands_tree_sitter(command).ok()?;
    if commands.is_empty() {
        return None;
    }

    commands
        .into_iter()
        .map(|command| {
            let mut words = Vec::new();
            for word in command {
                let tokens = shell_words::split(&word).ok()?;
                words.extend(tokens);
            }
            (!words.is_empty()).then_some(words)
        })
        .collect()
}

/// Shared head of both read-only classifiers: strip environment prefixes,
/// resolve the lowercased program name, and apply the option-level guard.
/// Returns the stripped words plus the program name, or `None` when the
/// command cannot be classified (fail closed). The parallel-safe classifier
/// shares this head; its stricter tail (no builds/tests, git allow-list
/// only) stays separate on purpose.
///
/// The guard reads the original `words` (not the stripped view) so
/// environment-injected options stay visible; keep that argument as-is.
fn readonly_command_head(words: &[String]) -> Option<(&[String], String)> {
    let command_words = command_words_after_environment_prefix(words);
    let first = command_words
        .first()
        .and_then(|word| Path::new(word).file_name())
        .and_then(|name| name.to_str())
        .map(str::to_ascii_lowercase)?;
    if has_unsafe_readonly_options(words) {
        return None;
    }
    Some((command_words, first))
}

/// Returns whether one parsed command is an allow-listed read-only command.
/// Option-level guards are shared with raw argument validation so activity
/// accounting cannot accidentally become less strict than tool intent.
pub(crate) fn command_words_are_readonly(words: &[String]) -> bool {
    let Some((command_words, first)) = readonly_command_head(words) else {
        return false;
    };
    if first == "command" {
        return matches!(command_words.get(1).map(String::as_str), Some("-v" | "-V"))
            && command_words.len() > 2
            && command_words
                .iter()
                .skip(2)
                .all(|word| !word.starts_with('-') && !word.is_empty());
    }

    if is_readonly_base_command(&first)
        || is_version_or_help_probe(command_words)
        || is_known_readonly_dry_run(command_words, &first)
    {
        return true;
    }

    let lowered = command_words
        .iter()
        .filter(|word| !word.starts_with('-') && !word.contains('='))
        .take(3)
        .map(|word| word.to_ascii_lowercase())
        .collect::<Vec<_>>();
    if first != "git" {
        let (second, third) = (lowered.get(1).map(String::as_str), lowered.get(2).map(String::as_str));
        return is_readonly_subcommand(&first, second, third);
    }

    let Some((sub_index, operand)) = git_subcommand_and_operand(command_words) else {
        return false;
    };
    let subcommand = command_words.get(sub_index).map(String::as_str);
    is_readonly_subcommand("git", subcommand, None)
        || is_readonly_flag_shaped_git_subcommand(subcommand, operand, command_words, sub_index)
}

/// Locate the git subcommand and the first operand after it, skipping global
/// options — including their separate values (`-C <dir>`, `-c <config>`,
/// `--git-dir <path>`, `--shallow-file <info>`), whose inline forms
/// (`-C<dir>`, `--git-dir=<path>`) carry no extra word. Without this, the
/// option value occupies the subcommand slot and worktree-style exploration
/// (`git -C <dir> log`) is misread as an unknown subcommand.
///
/// Position-based on purpose: the operand of a flag-shaped subcommand stays
/// visible as the second tuple field, which is what keeps `git tag v1`
/// distinguishable from `git tag -l`. Fails closed on any unknown
/// global-shaped word: an unrecognized option may consume a separate value,
/// which would otherwise smuggle the real (possibly mutating) subcommand past
/// the allow-list (`git --shallow-file log commit -m x`).
///
/// Returns the subcommand's index into `command_words` (callers re-read it —
/// real git subcommands are case-sensitive, so no lowercasing happens here)
/// plus the first non-flag operand after it, or `None` when the command must
/// stay denied.
fn git_subcommand_and_operand(command_words: &[String]) -> Option<(usize, Option<&str>)> {
    const GLOBAL_OPTIONS_WITH_VALUE: &[&str] = &[
        "-C",
        "-c",
        "--git-dir",
        "--work-tree",
        "--namespace",
        "--super-prefix",
        "--exec-path",
        "--config-env",
        "--shallow-file",
        "--attr-source",
    ];
    const GLOBAL_OPTIONS_FLAG_ONLY: &[&str] = &[
        "-p",
        "--paginate",
        "--no-pager",
        "--bare",
        "--literal-pathspecs",
        "--no-replace-objects",
        "--no-optional-locks",
        "--end-of-options",
        "--version",
        "--help",
        "--html-path",
    ];

    let mut index = 1;
    while let Some(word) = command_words.get(index) {
        if !word.starts_with('-') {
            let operand = command_words
                .iter()
                .skip(index + 1)
                .find(|candidate| !candidate.starts_with('-'))
                .map(String::as_str);
            return Some((index, operand));
        }
        if GLOBAL_OPTIONS_WITH_VALUE.contains(&word.as_str()) {
            index += 2; // skip the option and its separate value
            continue;
        }
        if GLOBAL_OPTIONS_FLAG_ONLY.contains(&word.as_str()) {
            index += 1;
            continue;
        }
        // Unknown global-shaped word: an option this allow-list does not know
        // may take a separate value, so the next word could be that value
        // rather than the subcommand. Fail closed.
        return None;
    }
    None
}

/// Return whether one command is a side-effect-free inspection suitable for
/// concurrent execution. Build and test commands remain read-only for policy
/// purposes, but are excluded here because they write caches, lockfiles, or
/// build artifacts and contend on shared package-manager state.
fn command_words_are_parallel_safe(words: &[String]) -> bool {
    let Some((command_words, first)) = readonly_command_head(words) else {
        return false;
    };

    if is_readonly_base_command(&first) {
        return true;
    }
    // Version/help probes are side-effect-free for any program.
    if is_version_or_help_probe(command_words) {
        return true;
    }
    if first != "git" {
        return false;
    }

    let subcommand = command_words
        .iter()
        .skip(1)
        .find(|word| !word.starts_with('-') && !word.contains('='))
        .map(|word| word.to_ascii_lowercase());
    is_readonly_subcommand("git", subcommand.as_deref(), None)
}

fn is_known_readonly_dry_run(words: &[String], program: &str) -> bool {
    if !matches!(program, "npm" | "pnpm" | "yarn") {
        return false;
    }

    let Some(subcommand) = words.iter().skip(1).find(|word| !word.starts_with('-') && !word.contains('=')) else {
        return false;
    };

    subcommand == "install" && words.iter().any(|word| word == "--dry-run")
}

fn matches_at(chars: &[char], index: usize, pattern: &[char]) -> bool {
    chars
        .get(index..index.saturating_add(pattern.len()))
        .is_some_and(|candidate| candidate == pattern)
}

/// Remove only stderr plumbing that cannot write workspace state and reject
/// all other redirection/background syntax. Operators inside quoted arguments
/// remain ordinary data and are never rewritten.
fn sanitize_static_shell_command(command: &str) -> Option<String> {
    if crate::command_safety::shell_parser::contains_dynamic_shell_syntax(command) {
        return None;
    }

    let chars = command.chars().collect::<Vec<_>>();
    let mut sanitized = String::with_capacity(command.len());
    let mut index = 0;
    let mut in_single_quote = false;
    let mut in_double_quote = false;

    while index < chars.len() {
        let character = chars[index];
        if character == '\'' && !in_double_quote {
            in_single_quote = !in_single_quote;
            sanitized.push(character);
            index += 1;
            continue;
        }
        if character == '"' && !in_single_quote {
            in_double_quote = !in_double_quote;
            sanitized.push(character);
            index += 1;
            continue;
        }
        if in_single_quote || in_double_quote {
            sanitized.push(character);
            index += 1;
            continue;
        }

        let preceded_by_boundary = index == 0
            || chars
                .get(index.saturating_sub(1))
                .is_some_and(|previous| previous.is_whitespace() || matches!(previous, '|' | '&' | ';'));
        if preceded_by_boundary && matches_at(&chars, index, &['2', '>', '&', '1']) {
            index += 4;
            continue;
        }
        if cfg!(unix)
            && preceded_by_boundary
            && matches_at(&chars, index, &['2', '>', '/', 'd', 'e', 'v', '/', 'n', 'u', 'l', 'l'])
        {
            index += 11;
            continue;
        }

        match character {
            '<' | '>' => return None,
            '&' if matches_at(&chars, index, &['&', '&']) => {
                sanitized.push('&');
                sanitized.push('&');
                index += 2;
            }
            '&' => return None,
            _ => {
                sanitized.push(character);
                index += 1;
            }
        }
    }

    (!sanitized.trim().is_empty()).then_some(sanitized)
}

/// Shared extract + static-parse prologue for command-session classifiers.
///
/// Extracts the full classification candidate (including the quoted argv
/// suffix) and proves a static shape through the tree-sitter boundary.
/// Returns `None` when the args cannot prove one invocation (conflicting raw
/// alias, dynamic syntax, redirection) — every caller maps `None` to
/// fail-closed `false`, in the same order as before.
fn parsed_session_commands(args: &Value) -> Option<Vec<Vec<String>>> {
    let raw = shell_command_text_with_args(args)?;
    static_shell_command_words(&raw)
}

/// Shared read-only gate over already-parsed segments: a `&&` chain is
/// accepted only when every segment independently passes the allow-list.
/// (`sanitize_static_shell_command` already rejected `;`, `||`,
/// redirections, and background `&` up front.)
fn all_commands_readonly(commands: &[Vec<String>]) -> bool {
    commands.iter().all(|words| command_words_are_readonly(words))
}

pub fn is_readonly_command_session_command(args: &Value) -> bool {
    let Some(commands) = parsed_session_commands(args) else {
        return false;
    };

    all_commands_readonly(&commands)
}

/// A stricter read-only command shape that may run concurrently after hooks,
/// command admission, and preflight have all succeeded.
pub(crate) fn is_parallel_safe_command_session_command(args: &Value) -> bool {
    if args.get("session_id").is_some()
        || args.get("chars").is_some()
        || args.get("input").is_some()
        || args.get("tty").and_then(Value::as_bool) == Some(true)
        || args
            .get("sandbox_permissions")
            .and_then(Value::as_str)
            .is_some_and(|value| value != "use_default")
    {
        return false;
    }
    let Some(commands) = parsed_session_commands(args) else {
        return false;
    };
    commands.len() == 1 && command_words_are_parallel_safe(&commands[0])
}

/// Returns `true` when a safe shell inspection command reads the internal tool
/// output spool directory. Such reads must stay inline: spooling their output
/// again would create a recursive chain of spool references.
pub fn is_spool_file_read_command(tool_name: &str, args: &Value) -> bool {
    if super::classify::canonical_command_session_tool_name(tool_name).is_none() {
        return false;
    }

    // Parse once through the shared prologue: the same segments feed both
    // the read-only gate below and the spool-path scan, so a completed
    // command output (see `prepare_execution_output`) pays the extract +
    // sanitize + tree-sitter pipeline a single time.
    let Some(commands) = parsed_session_commands(args) else {
        return false;
    };
    if !all_commands_readonly(&commands) {
        return false;
    }
    commands
        .iter()
        .flatten()
        .any(|word| SpooledOutputReference::recognizes_path(Path::new(word)))
}

#[cfg(test)]
mod tests;
