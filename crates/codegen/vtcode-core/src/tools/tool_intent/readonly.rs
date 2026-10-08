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
const READONLY_UNIFIED_EXEC_COMMANDS: &[&str] = &[
    "rg", "ls", "cat", "diff", "find", "wc", "grep", "egrep", "fgrep", "head", "tail", "sort", "uniq", "bat", "sed",
    "awk", "cut", "tr", "ast-grep", "sg", "echo", "pwd", "printf", "true", "false", "test", "cd", "fd", "tree",
    "which", "stat", "file", "du", "df", "realpath", "basename", "dirname", "nl", "column", "jq", "date", "whoami",
    "uname",
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

/// Returns whether one parsed command is an allow-listed read-only command.
/// Option-level guards are shared with raw argument validation so activity
/// accounting cannot accidentally become less strict than tool intent.
pub(crate) fn command_words_are_readonly(words: &[String]) -> bool {
    let command_words = command_words_after_environment_prefix(words);
    let Some(first) = command_words
        .first()
        .and_then(|word| Path::new(word).file_name())
        .and_then(|name| name.to_str())
        .map(str::to_ascii_lowercase)
    else {
        return false;
    };

    if has_unsafe_readonly_options(words) {
        return false;
    }

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
    let command_words = command_words_after_environment_prefix(words);
    let Some(first) = command_words
        .first()
        .and_then(|word| Path::new(word).file_name())
        .and_then(|name| name.to_str())
        .map(str::to_ascii_lowercase)
    else {
        return false;
    };
    if has_unsafe_readonly_options(words) {
        return false;
    }

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

pub fn is_readonly_command_session_command(args: &Value) -> bool {
    let Some(raw) = shell_command_text_with_args(args) else {
        return false;
    };

    // `sanitize_static_shell_command` rejects `;`, `||`, redirections, and
    // background `&` up front; a `&&` chain is accepted only when every
    // parsed segment independently passes the allow-list below.
    static_shell_command_words(&raw)
        .is_some_and(|commands| commands.iter().all(|words| command_words_are_readonly(words)))
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
    let Some(raw) = shell_command_text_with_args(args) else {
        return false;
    };
    static_shell_command_words(&raw)
        .is_some_and(|commands| commands.len() == 1 && command_words_are_parallel_safe(&commands[0]))
}

/// Returns `true` when a safe shell inspection command reads the internal tool
/// output spool directory. Such reads must stay inline: spooling their output
/// again would create a recursive chain of spool references.
pub fn is_spool_file_read_command(tool_name: &str, args: &Value) -> bool {
    if super::classify::canonical_command_session_tool_name(tool_name).is_none() {
        return false;
    }

    if !is_readonly_command_session_command(args) {
        return false;
    }

    shell_command_text_with_args(args)
        .and_then(|command| static_shell_command_words(&command))
        .is_some_and(|commands| {
            commands
                .iter()
                .flatten()
                .any(|word| SpooledOutputReference::recognizes_path(Path::new(word)))
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn run_cmd(command: &str) -> Value {
        json!({"action": "run", "command": command})
    }

    #[test]
    fn command_availability_probes_do_not_admit_execution() {
        for command in ["command -v npx", "command -V python3 cargo", "command -v npx || true"] {
            assert!(is_readonly_command_session_command(&run_cmd(command)), "{command}");
        }
        for command in [
            "command rm file",
            "command -p rm file",
            "command -v",
            "command -v -- rm",
            "command -v $(touch file)",
        ] {
            assert!(!is_readonly_command_session_command(&run_cmd(command)), "{command}");
        }
    }

    #[test]
    fn and_chain_allows_readonly_segments() {
        assert!(is_readonly_command_session_command(&run_cmd("ls -la && echo '---' && ls -la crates/")));
        assert!(is_readonly_command_session_command(&run_cmd("pwd && ls src/")));
        assert!(is_readonly_command_session_command(&run_cmd("cat foo.txt && grep bar")));
    }

    #[test]
    fn compound_inspection_commands_share_the_readonly_policy() {
        for command in [
            "cat README.md; rg -n '^#' README.md",
            "git diff --stat; find docs -maxdepth 2 -type f | sort | head -40",
            "cat README.md\nrg -n '^version' Cargo.toml",
        ] {
            assert!(is_readonly_command_session_command(&run_cmd(command)), "expected readonly command: {command}");
        }
    }

    #[test]
    fn checkpoint_style_inspection_with_escaped_regex_is_readonly() {
        let command = r#"sed -n '180,285p' src/main.rs; sed -n '60,285p' src/startup/mod.rs; sed -n '1,220p' src/main_helpers/bootstrap.rs; rg -n "\[profile|lto|codegen-units|strip" Cargo.toml"#;

        assert!(is_readonly_command_session_command(&run_cmd(command)));
    }

    #[test]
    fn and_chain_rejects_destructive_segments() {
        assert!(!is_readonly_command_session_command(&run_cmd("ls -la && rm foo.txt")));
        assert!(!is_readonly_command_session_command(&run_cmd("cat x && mv a b")));
        assert!(!is_readonly_command_session_command(&run_cmd("true && cp a b")));
    }

    #[test]
    fn and_chain_rejects_non_allowlisted_segments() {
        assert!(!is_readonly_command_session_command(&run_cmd("ls -la && python script.py")));
        assert!(!is_readonly_command_session_command(&run_cmd("ls -la && cargo build")));
        assert!(!is_readonly_command_session_command(&run_cmd("python3 mutate.py --dry-run")));
        assert!(!is_readonly_command_session_command(&run_cmd("npm install --dry-run && rm output")));
    }

    #[test]
    fn and_chain_allows_pipeline_within_segment() {
        assert!(is_readonly_command_session_command(&run_cmd("ls -la | head -5 && echo done")));
    }

    #[test]
    fn and_chain_single_command_passes() {
        assert!(is_readonly_command_session_command(&run_cmd("ls -la")));
        assert!(is_readonly_command_session_command(&run_cmd("echo hi")));
    }

    #[test]
    fn readonly_command_session_allows_and_chain() {
        // The exact pattern from checkpoint turn_726 that was blocked.
        assert!(is_readonly_command_session_command(&run_cmd("ls -la /path/ && echo '---' && ls -la /path/crates/")));
    }

    #[test]
    fn readonly_command_session_rejects_destructive_and_chain() {
        assert!(!is_readonly_command_session_command(&run_cmd("ls -la && rm foo.txt")));
    }

    #[test]
    fn awk_range_print_is_readonly() {
        // Regression shapes from the blocked README session: `awk` paging
        // must not count as blind-editing mutations.
        for command in [
            "awk 'NR>=40 && NR<=140' README.md",
            "awk 'NR>=297 && NR<=312' README.md",
            "awk 'NR>=291 && NR<=296' README.md | cut -c1-150",
            "awk -F: '{print $1}' README.md",
            "awk -F, '{print $2}' data.csv",
            "awk -v limit=10 'NR<=limit' README.md",
            "awk -- '{print $1}' README.md",
            "awk -- '{print $1}' -- -weird",
            // `systime()` only reads the clock — must not be confused with
            // `system()`. Absolute paths resolve via file_name().
            "awk 'BEGIN{print systime()}' README.md",
            "/usr/bin/awk 'NR>=1 && NR<=5' README.md",
            "awk 'NR>=1 && NR<=5' README.md | sort",
            // Blocked-session regression: numeric `>` comparisons without
            // `print` redirection are reads, not writes.
            "awk '$3>100' README.md",
            "awk '{if(p2==0 && i>1) print}' README.md",
            "awk '{if(NR>1 && $2>10) print $1}' README.md",
            "awk 'NR>=65 && NR<=71 {n=length($0); print n}' README.md",
            "awk '{print (a>b)}' README.md",
        ] {
            assert!(is_readonly_command_session_command(&run_cmd(command)), "expected readonly command: {command}");
        }
    }

    #[test]
    fn awk_write_primitives_stay_mutating() {
        for command in [
            "awk '{print > \"out.txt\"}' README.md",
            "awk '{print >> \"out.txt\"}' README.md",
            "awk '{print x > \"out.txt\"}' README.md",
            "awk 'BEGIN { # (\n print \"written\" > \"out.txt\" }'",
            "awk 'BEGIN { # [\n printf \"written\" > \"out.txt\" }'",
            "awk 'BEGIN { # ( [ {\n print \"written\" > \"out.txt\" }'",
            "awk '{print | \"sort\"}' README.md",
            "awk '\"sort\" | getline line' README.md",
            "awk 'BEGIN{system(\"touch out\")}' README.md",
            "awk 'BEGIN{SYSTEM (\"id\")}' README.md",
            "awk 'BEGIN{System(\"id\")}' README.md",
            "awk 'BEGIN{system\t(\"id\")}' README.md",
            // gawk indirect calls and directives can execute or load code,
            // including a `system` name smuggled via `-v`. Fail closed.
            "awk -v f=system 'BEGIN{@f(\"id\")}' README.md",
            "awk 'BEGIN{@s(\"id\")}' README.md",
            "awk '@include \"x.awk\"' README.md",
            "awk '@load \"ext\"' README.md",
            "awk '{print \"a@b\"}' README.md",
            // `|` alternations inside regex stay conservative without a full
            // parser; single `>` without `print` is now a comparison above.
            "awk '/error|warning/' README.md",
            "awk -i inplace '{print}' README.md",
            "awk -f program.awk README.md",
            "awk --source '{print}' README.md",
            "awk -l injail '{print}' README.md",
            "awk --profile '{print}' README.md",
            "awk -W dump-variables '{print}' README.md",
            "awk --posix '{print}' README.md",
            "awk -F:",
            "awk",
            "awk -v",
            // Shell-level redirection stays mutating even with a safe program.
            "awk 'NR>=1' README.md > out.txt",
        ] {
            assert!(!is_readonly_command_session_command(&run_cmd(command)), "expected mutating command: {command}");
        }
    }

    #[test]
    fn awk_string_literal_operators_are_readonly() {
        // The reported false positive: a quoted `"|"` passed to `index()` is a
        // string literal, not a command pipe, so the lexer must not reject it.
        for command in [
            r#"awk '{n=index(rest,"|"); print n}' README.md"#,
            r#"awk 'index($0,">")' README.md"#,
            // `>=` comparison and `||` boolean operators are not redirection
            // or a pipe.
            r#"awk '$3>=100 && $4||$5 {print $1}' README.md"#,
            // Division must not be mistaken for a regex literal.
            r#"awk '{print $1 / $2}' README.md"#,
            "awk '{ # ( [ > system(\n if (NR>1) print $1 }' README.md",
            // Exact reported shape: a multi-line program whose `index(rest,"|")`
            // carries a quoted pipe and a `# ...` comment line.
            r#"awk 'NR>=208 && NR<=212 {line=$0; body=substr(line,1,length(line)-1); n=0; while (body ~ / \$/) { body=substr(body,1,length(body)-1); n++ }} # find guide start after label cell
rest=substr(line,3); g=index(rest,"|")+1; guide=substr(rest,g+2); gp=0; gg=guide; while (gg ~ / \$/) { gg=substr(gg,1,length(gg)-1); gp++ } print "%d: linelen=%d labelcell=%s pad_before_final_pipe=%d guide_pad=%d\n", NR, length(line), substr(line,3,20), n, gp }' README.md"#,
        ] {
            assert!(is_readonly_command_session_command(&run_cmd(command)), "expected readonly command: {command}");
        }
    }

    #[test]
    fn awk_regex_with_quote_cannot_hide_pipe() {
        // Adversarial shape for a naive quote-toggling scanner: `/a"b/` opens
        // a "string" that stays open, hiding the real `|`; the later `/c"d/`
        // rebalances the quote count, so an unbalanced-quote guard never
        // fires. The lexer treats quotes as regex content and still catches
        // the pipe.
        for command in [
            r#"awk '/a"b/ {print | "sort"} /c"d/' README.md"#,
            // Regex alternation stays mutating (pinned conservative policy).
            r#"awk '/a|b/ {print}' README.md"#,
        ] {
            assert!(!is_readonly_command_session_command(&run_cmd(command)), "expected mutating command: {command}");
        }
    }

    #[test]
    fn cd_prefixed_exploration_is_readonly() {
        // Exact patterns from checkpoint turn_810 that plan mode rejected:
        // the model habitually prefixes exploration with `cd <workspace> &&`.
        assert!(is_readonly_command_session_command(&run_cmd("cd /repo && sed -n '440,520p' Cargo.toml")));
        assert!(is_readonly_command_session_command(&run_cmd(
            "cd /repo && ls -la && echo '---' && rg --files -g 'Cargo.toml' | sed -n '1,120p'"
        )));
        // `cd` must not become a bypass for mutating chains.
        assert!(!is_readonly_command_session_command(&run_cmd("cd /repo && cargo build")));
        assert!(!is_readonly_command_session_command(&run_cmd("cd /repo && rm -rf target")));
    }

    #[test]
    fn git_readonly_subcommands_allowed_in_chains_and_pipelines() {
        assert!(is_readonly_command_session_command(&run_cmd("git log --oneline | head -20")));
        assert!(is_readonly_command_session_command(&run_cmd("cd /repo && git diff")));
        assert!(is_readonly_command_session_command(&run_cmd("git show HEAD")));
        assert!(is_readonly_command_session_command(&run_cmd("git blame src/main.rs | head")));
        // Mutating git subcommands stay rejected.
        assert!(!is_readonly_command_session_command(&run_cmd("git checkout main")));
        assert!(!is_readonly_command_session_command(&run_cmd("cd /repo && git push")));
        assert!(!is_readonly_command_session_command(&run_cmd("git commit -m 'x'")));
    }

    #[test]
    fn env_value_injection_is_not_readonly() {
        // A4: `command_words_after_environment_prefix` strips assignments, so
        // values must still be inspected. `GIT_CONFIG_*` reproduces the blocked
        // `git -c core.fsmonitor` injection without any `-c` token.
        for command in [
            "env GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=core.fsmonitor GIT_CONFIG_VALUE_0='touch /tmp/pwned' git status",
            "GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=core.fsmonitor GIT_CONFIG_VALUE_0='touch /tmp/pwned' git status",
            "env GIT_CONFIG_GLOBAL=/tmp/evil.gitconfig git status",
            "env GIT_EXTERNAL_DIFF=evil git diff",
            "env GIT_TEXTCONV=evil git show HEAD",
            "env LD_PRELOAD=./evil.so ls",
            "env DYLD_INSERT_LIBRARIES=./evil.dylib ls",
            "env BASH_ENV=./evil.sh bash -lc 'echo hi'",
            "env NODE_OPTIONS='--require ./evil.js' node --version",
            "env EDITOR=evil sed -n '1,10p' README.md",
        ] {
            assert!(
                !is_readonly_command_session_command(&run_cmd(command)),
                "expected mutating env-injection command: {command}"
            );
        }

        // Ordinary env prefixes stay read-only.
        for command in [
            "env LANG=C.UTF-8 rg 'TODO' src",
            "FOO=bar git status",
            "env FOO=bar git log --oneline",
            "env -u GIT_CONFIG_COUNT git status",
            "env -C /repo git status",
        ] {
            assert!(
                is_readonly_command_session_command(&run_cmd(command)),
                "expected readonly env-prefixed command: {command}"
            );
        }
    }

    #[test]
    fn git_worktree_and_history_plumbing_subcommands_are_readonly() {
        // turn-1030-class exploration: read-only plumbing subcommands the
        // plan gate used to deny outright.
        for command in [
            "git rev-list --count HEAD",
            "git ls-tree HEAD crates/",
            "git cat-file -p HEAD:Cargo.toml",
            "git diff-tree -r HEAD",
            "git merge-base main feat",
            "git range-diff main~2..main feat~2..feat",
            "git whatchanged -5",
            "git count-objects -v",
            "git reflog | head",
            "git worktree list",
            "git stash list",
            "git stash show",
            "git branch --show-current",
            "git --no-pager log --oneline | head -5",
            "git remote -v",
            "cd /repo && git -C . branch -a && git tag",
        ] {
            assert!(is_readonly_command_session_command(&run_cmd(command)), "expected readonly command: {command}");
        }
    }

    #[test]
    fn version_and_help_probes_are_readonly_for_any_program() {
        for command in [
            "cargo --version",
            "node --version",
            "python3 -V",
            "npm --version",
            "git --version",
            "bash --help",
        ] {
            assert!(is_readonly_command_session_command(&run_cmd(command)), "expected readonly probe: {command}");
        }
        // Only an all-flags probe is a version/help query; real operands keep
        // the command outside the allow-list. Lowercase `-v` is excluded: it
        // means verbose for most tools and reads stdin for shells.
        assert!(!is_readonly_command_session_command(&run_cmd("python3 script.py --version")));
        assert!(!is_readonly_command_session_command(&run_cmd("git commit -v")));
        assert!(!is_readonly_command_session_command(&run_cmd("bash -v")));
    }

    #[test]
    fn git_dash_c_repo_redirect_is_readonly_for_allowed_subcommands() {
        // `-C <dir>` only redirects which repository is read; plan mode must
        // not reject worktree-style exploration.
        assert!(is_readonly_command_session_command(&run_cmd("git -C .worktrees/feat log --oneline | head -5")));
        assert!(is_readonly_command_session_command(&run_cmd("git -C ../other rev-parse HEAD")));
        // Config injection (`-c`) stays denied — the `-C`/`-c` pair must not
        // be relaxed together.
        assert!(!is_readonly_command_session_command(&run_cmd("git -c core.pager=sh log")));
        // A read-only subcommand with a write option stays denied.
        assert!(!is_readonly_command_session_command(&run_cmd("git -C .worktrees/feat diff -o out.patch")));
    }

    #[test]
    fn readonly_git_and_inspection_options_stay_fail_closed() {
        for command in [
            "git diff -o output.txt",
            "git diff '--output=output.txt'",
            "git diff -ooutput.txt",
            "git log --output=output.txt",
            "git show --textconv",
            // Config injection is a command-execution vector and stays denied.
            "git -c core.fsmonitor=touch status",
            "git --exec-path=/tmp/sh status",
            // Bare `git stash` is `git stash push` — it stashes uncommitted
            // work — and flag-only push forms carry no operand to betray
            // them. Only `list`/`show` are reads.
            "git stash",
            "git stash -u",
            "git stash -k",
            // Inline-value and glued short mutators are invisible to the
            // operand finder and must stay denied.
            "git branch --unset-upstream",
            "git branch --set-upstream-to=origin/main",
            "git branch --edit-description",
            "git branch -mnew-name",
            "git branch -u=origin/main",
            // An unknown value-taking global must not smuggle a mutating
            // subcommand past the allow-list.
            "git --shallow-file log commit -m x",
            // Operand forms of flag-shaped subcommands mutate and stay denied.
            "git tag v1.0.0",
            "git branch feature-x",
            "git branch -D stale",
            "git reflog expire --expire=now --all",
            "git stash pop",
            "git remote add origin https://example.com/r.git",
            "git worktree add ../side feat",
            "find . -fprint output.txt",
            "find . -fprintf output.txt '%p'",
            "rg --hostname-bin sh pattern",
            "rg --search-zip pattern",
            "rg -z pattern",
            "sort -o output.txt README.md",
            "sort --compress-program=sh README.md",
            "date -s now",
            "awk -i inplace '{print}' README.md",
            "sed -n 's/a/b/e' README.md",
            "fd --exec sh -c 'touch out'",
            "tree -o out.txt",
            "ast-grep -r 'README.md'",
            "sed -n -fmalicious.sed -e '1p' src/main.rs",
            "sed -I '' 's/a/b/' src/main.rs",
            "sed -n '1p\nw leaked.txt' src/main.rs",
            "sed -n 'woutput.txt' src/main.rs",
            "cargo check & rm output",
        ] {
            assert!(!is_readonly_command_session_command(&run_cmd(command)), "unexpected readonly command: {command}");
        }
    }

    #[test]
    fn cargo_readonly_subcommands_allowed() {
        assert!(is_readonly_command_session_command(&run_cmd("cargo metadata")));
        assert!(is_readonly_command_session_command(&run_cmd("cargo tree | head -50")));
        assert!(is_readonly_command_session_command(&run_cmd("cargo nextest run")));
        assert!(!is_readonly_command_session_command(&run_cmd("cargo build")));
        assert!(!is_readonly_command_session_command(&run_cmd("cargo publish")));
    }

    #[test]
    fn parallel_commands_are_limited_to_side_effect_free_inspection() {
        for command in ["rg -n TODO src", "cat Cargo.toml", "git status --short"] {
            assert!(is_parallel_safe_command_session_command(&run_cmd(command)), "expected parallel-safe: {command}");
        }
        for command in [
            "cargo check",
            "cargo test",
            "cargo nextest run",
            "cargo clippy",
            "npm test",
            "pnpm run test",
            "git diff | head -20",
        ] {
            assert!(
                !is_parallel_safe_command_session_command(&run_cmd(command)),
                "command can touch shared state or crosses a sequential boundary: {command}"
            );
        }
    }

    #[test]
    fn stderr_merge_is_not_a_write_redirection() {
        assert!(is_readonly_command_session_command(&run_cmd("cargo check 2>&1 | head -c 4000")));
        // A real file redirection is still rejected.
        assert!(!is_readonly_command_session_command(&run_cmd("cargo check > out.txt")));
    }

    #[test]
    fn extra_inspection_base_commands_are_readonly() {
        assert!(is_readonly_command_session_command(&run_cmd("tree -L 2 crates/")));
        assert!(is_readonly_command_session_command(&run_cmd("fd -e rs planner")));
        assert!(is_readonly_command_session_command(&run_cmd("stat Cargo.toml && file target")));
        assert!(is_readonly_command_session_command(&run_cmd("which cargo")));
    }

    #[test]
    fn spool_file_reads_are_detected_only_for_readonly_commands() {
        for command in [
            "cat .vtcode/context/tool_outputs/run-1.txt",
            "sed -n '1,20p' .vtcode/context/tool_outputs/run-1.txt",
            "rg error .vtcode/context/tool_outputs",
            "rg -n error .vtcode/context/tool_outputs | head -20",
            "tail -n 20 .vtcode/context/tool_outputs/run-1.txt",
        ] {
            assert!(is_spool_file_read_command("exec_command", &run_cmd(command)), "expected spool read: {command}");
        }

        for command in [
            "cat .vtcode/context/tool_outputs/run-1.txt > copied.txt",
            "sed -i 's/a/b/' .vtcode/context/tool_outputs/run-1.txt",
            "rm .vtcode/context/tool_outputs/run-1.txt",
            "cat \"$VTCODE_SPOOL\"",
            "cat .vtcode/context/tool_outputs/run-1.txt |",
            "cat notes-.vtcode/context/tool_outputs/run-1.txt",
            "cat .vtcode/context/tool_outputs-extra/run-1.txt",
        ] {
            assert!(!is_spool_file_read_command("exec_command", &run_cmd(command)), "unexpected spool read: {command}");
        }

        assert!(!is_spool_file_read_command("mcp_tool", &run_cmd("cat .vtcode/context/tool_outputs/run-1.txt")));
        assert!(is_spool_file_read_command("exec_command", &run_cmd("cat .vtcode/context/tool_outputs/run-1.txt")));
    }
}
