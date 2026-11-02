//! Shared helpers for command-style tool arguments.

use std::path::Path;

use serde_json::Value;

use crate::tools::tool_intent::{command_session_action, command_session_action_in, command_session_action_is};

const INDEXED_COMMAND_TYPE_ERROR: &str = "command array must contain only strings";
const COMMAND_VALUE_TYPE_ERROR: &str = "command must be a string or array of strings";

mod text_tools;
use text_tools::{has_unsafe_awk_options, has_unsafe_sed_options};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WriteStdinDispatch {
    Write,
    Poll,
    Wait,
    Inspect,
    Terminate,
    Close,
}

impl WriteStdinDispatch {
    #[must_use]
    pub(crate) const fn command_session_action(self) -> &'static str {
        match self {
            Self::Write => "write",
            Self::Poll => "poll",
            Self::Wait => "wait",
            Self::Inspect => "inspect",
            Self::Terminate => "terminate",
            Self::Close => "close",
        }
    }
}

pub(crate) fn write_stdin_dispatch(args: &Value) -> Result<WriteStdinDispatch, &'static str> {
    let payload = args.as_object().ok_or("write_stdin requires a JSON object")?;
    if let Some(action) = payload.get("action") {
        let action = action.as_str().ok_or("write_stdin action must be a string")?;
        // Zero-alloc ASCII case-insensitive match: identical verdicts to the
        // old `to_ascii_lowercase()` + `match` (both fold ASCII only), but
        // without heap-allocating the lowered action on this hot path.
        if action.eq_ignore_ascii_case("wait") {
            return Ok(WriteStdinDispatch::Wait);
        }
        if action.eq_ignore_ascii_case("inspect") {
            return Ok(WriteStdinDispatch::Inspect);
        }
        if action.eq_ignore_ascii_case("terminate") {
            return Ok(WriteStdinDispatch::Terminate);
        }
        if action.eq_ignore_ascii_case("close") {
            return Ok(WriteStdinDispatch::Close);
        }
        if action.eq_ignore_ascii_case("poll") {
            if payload.get("chars").is_some_and(|chars| chars.as_str() != Some("")) {
                return Err("write_stdin poll cannot send chars");
            }
            return Ok(WriteStdinDispatch::Poll);
        }
        if !action.eq_ignore_ascii_case("write") {
            return Err("write_stdin action must be write, poll, wait, inspect, terminate, or close");
        }
    }
    let chars = payload
        .get("chars")
        .and_then(Value::as_str)
        .ok_or("write_stdin requires string chars")?;

    if chars.is_empty() {
        Ok(WriteStdinDispatch::Poll)
    } else {
        Ok(WriteStdinDispatch::Write)
    }
}

fn collect_indexed_command_parts(
    payload: &serde_json::Map<String, Value>,
    start_index: usize,
) -> Result<Vec<String>, &'static str> {
    let mut parts = Vec::new();
    let mut index = start_index;
    while let Some(value) = payload.get(&format!("command.{index}")) {
        let Some(part) = value.as_str() else {
            return Err(INDEXED_COMMAND_TYPE_ERROR);
        };
        parts.push(part.to_string());
        index += 1;
    }
    Ok(parts)
}

pub fn has_indexed_command_parts(args: &Value) -> bool {
    let Some(payload) = args.as_object() else {
        return false;
    };

    payload.contains_key("command.0") || payload.contains_key("command.1")
}

pub fn parse_indexed_command_parts(
    payload: &serde_json::Map<String, Value>,
) -> Result<Option<Vec<String>>, &'static str> {
    let zero_based = collect_indexed_command_parts(payload, 0)?;
    if !zero_based.is_empty() {
        return Ok(Some(zero_based));
    }

    let one_based = collect_indexed_command_parts(payload, 1)?;
    if one_based.is_empty() {
        Ok(None)
    } else {
        Ok(Some(one_based))
    }
}

pub fn normalize_indexed_command_args(args: &Value) -> Result<Option<Value>, &'static str> {
    let Some(payload) = args.as_object() else {
        return Ok(None);
    };
    if payload.get("command").is_some() {
        return Ok(None);
    }

    let Some(parts) = parse_indexed_command_parts(payload)? else {
        return Ok(None);
    };

    let mut normalized = payload.clone();
    normalized.insert("command".to_string(), Value::String(shell_words::join(parts.iter().map(String::as_str))));
    Ok(Some(Value::Object(normalized)))
}

pub fn normalized_command_value(args: &Value) -> Result<Option<Value>, &'static str> {
    if let Some(command) = args
        .get("command")
        .or_else(|| args.get("cmd"))
        .or_else(|| args.get("raw_command"))
    {
        return Ok(Some(command.clone()));
    }

    Ok(normalize_indexed_command_args(args)?.and_then(|normalized| normalized.get("command").cloned()))
}

pub fn command_words(args: &Value) -> Result<Option<Vec<String>>, &'static str> {
    let Some(command) = normalized_command_value(args)? else {
        return Ok(None);
    };

    let mut parts = match command {
        Value::String(command) => shell_words::split(&command).map_err(|_e| COMMAND_VALUE_TYPE_ERROR)?,
        Value::Array(values) => values
            .iter()
            .map(|value| value.as_str().map(ToOwned::to_owned).ok_or(COMMAND_VALUE_TYPE_ERROR))
            .collect::<Result<Vec<_>, _>>()?,
        _ => return Err(COMMAND_VALUE_TYPE_ERROR),
    };

    if let Some(extra_args) = args.get("args").and_then(Value::as_array) {
        for value in extra_args {
            let Some(part) = value.as_str() else {
                return Err(COMMAND_VALUE_TYPE_ERROR);
            };
            parts.push(part.to_string());
        }
    }

    if parts.is_empty() { Ok(None) } else { Ok(Some(parts)) }
}

pub fn command_text(args: &Value) -> Result<Option<String>, &'static str> {
    let Some(parts) = command_words(args)? else {
        return Ok(None);
    };
    Ok(Some(shell_words::join(parts.iter().map(String::as_str))))
}

/// Ordered argument keys that may carry a shell command string for display.
///
/// Canonical display precedence shared by the binary's tool summaries so
/// headline and detail extraction can never drift. Safety classification must
/// keep using [`raw_command_text`]/[`command_words`], which re-parse via
/// `shell_words` and intentionally exclude the legacy display-only
/// `bash_command` key that core handlers never execute.
pub const DISPLAY_COMMAND_KEYS: &[&str] = &["command", "raw_command", "bash_command", "cmd"];

/// Display-only extraction of a shell command string plus the key it came from.
///
/// Unlike [`raw_command_text`], array commands are joined with plain spaces
/// (no quoting) because the result is rendered, never executed or re-parsed:
/// `["bash", "-lc", "ls -R"]` displays as `bash -lc ls -R`. The `command` key
/// trims before the emptiness check while other keys use raw emptiness,
/// preserving historical per-key behavior. Indexed `command.N` parts are
/// handled by [`command_words`]'s primary path, not here.
///
/// Returns `None` when no key carries a non-empty command.
pub fn extract_command_text_with_key(args: &Value) -> Option<(String, &'static str)> {
    if let Some(array) = args.get("command").and_then(Value::as_array) {
        let joined: String = array
            .iter()
            .filter_map(|value| value.as_str())
            .filter(|segment| !segment.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        if !joined.is_empty() {
            return Some((joined, "command"));
        }
    }
    for &key in DISPLAY_COMMAND_KEYS {
        let Some(value) = args.get(key).and_then(Value::as_str) else {
            continue;
        };
        // The `command` key trims before the emptiness check; the others do not,
        // matching historical per-key behavior.
        let (text, ok) = if key == "command" {
            let trimmed = value.trim();
            (trimmed.to_string(), !trimmed.is_empty())
        } else {
            (value.to_string(), !value.is_empty())
        };
        if ok {
            return Some((text, key));
        }
    }
    None
}

fn has_nonempty_string_field(args: &Value, key: &str) -> bool {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .is_some_and(|value| !value.is_empty())
}

pub fn interactive_input_text(args: &Value) -> Option<&str> {
    args.get("input")
        .and_then(Value::as_str)
        .or_else(|| args.get("chars").and_then(Value::as_str))
        .or_else(|| args.get("text").and_then(Value::as_str))
        .filter(|value| !value.is_empty())
}

pub fn session_id_text_from_payload(payload: &serde_json::Map<String, Value>) -> Option<&str> {
    payload
        .get("session_id")
        .or_else(|| payload.get("s"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

pub fn session_id_text(args: &Value) -> Option<&str> {
    args.as_object().and_then(session_id_text_from_payload)
}

pub fn command_session_missing_required_args(args: &Value) -> Vec<&'static str> {
    if command_session_action(args).is_none() {
        return Vec::new();
    }

    let mut missing = Vec::new();
    if command_session_action_is(args, "run") {
        if command_text(args).ok().flatten().is_none() {
            missing.push("command");
        }
    } else if command_session_action_is(args, "write") {
        if session_id_text(args).is_none() {
            missing.push("session_id");
        }
        if interactive_input_text(args).is_none() {
            missing.push("input or chars or text");
        }
    } else if command_session_action_in(args, &["poll", "wait", "continue", "close"]) {
        if session_id_text(args).is_none() {
            missing.push("session_id");
        }
    } else if command_session_action_is(args, "inspect") {
        let has_session_id = session_id_text(args).is_some();
        let has_spool_path = has_nonempty_string_field(args, "spool_path");
        if !has_session_id && !has_spool_path {
            missing.push("session_id or spool_path");
        }
    } else if command_session_action_is(args, "code") {
        let has_code = has_nonempty_string_field(args, "code") || has_nonempty_string_field(args, "command");
        if !has_code {
            missing.push("code or command");
        }
    }

    missing
}

pub fn command_session_requires_command_safety(args: &Value) -> bool {
    command_session_action_is(args, "run")
}

pub fn working_dir_text_from_payload(payload: &serde_json::Map<String, Value>) -> Option<&str> {
    payload
        .get("working_dir")
        .or_else(|| payload.get("cwd"))
        .or_else(|| payload.get("workdir"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

pub fn working_dir_text(args: &Value) -> Option<&str> {
    args.as_object().and_then(working_dir_text_from_payload)
}

/// Extract the raw command string from command_session-style arguments without
/// splitting it into words. This is useful for checking shell syntax that
/// would be lost after `shell_words::split`, such as redirections and pipes.
pub fn raw_command_text(args: &Value) -> Option<String> {
    let payload = args.as_object()?;

    if let Some(command) = payload
        .get("command")
        .or_else(|| payload.get("cmd"))
        .or_else(|| payload.get("raw_command"))
    {
        match command {
            Value::String(text) => return Some(text.clone()),
            Value::Array(values) => {
                let parts: Option<Vec<&str>> = values.iter().map(|v| v.as_str()).collect();
                return parts.map(shell_words::join);
            }
            _ => return None,
        }
    }

    let indexed = parse_indexed_command_parts(payload).ok().flatten()?;
    Some(shell_words::join(indexed.iter().map(String::as_str)))
}

/// Complete static-classification candidate, including the quoted argv suffix
/// that execution appends to shell text. Conflicting raw overrides cannot prove
/// one invocation, so callers must fail closed instead of treating it as a read
/// or verifier. Dynamic syntax and command options still need normal validation.
pub(crate) fn shell_command_text_with_args(args: &Value) -> Option<String> {
    let mut command = raw_command_text(args)?;
    if args
        .get("raw_command")
        .and_then(Value::as_str)
        .is_some_and(|raw| raw != command)
    {
        return None;
    }
    if let Some(arguments) = args.get("args") {
        let arguments = arguments.as_array()?;
        let suffix_words = arguments.iter().map(Value::as_str).collect::<Option<Vec<_>>>()?;
        if !suffix_words.is_empty() {
            command.push(' ');
            command.push_str(&shell_words::join(suffix_words));
        }
    }
    Some(command)
}

/// Returns whether a shell command contains syntax whose meaning depends on
/// shell expansion rather than the literal argument text.
///
/// Read-only classification and approval-family learning must only operate on
/// static command shapes. Parameter expansion, command substitution, brace
/// expansion, globbing, and unquoted backslash escapes can otherwise turn a
/// harmless-looking token into a different executable argument at runtime.
pub fn contains_dynamic_shell_syntax(command: &str) -> bool {
    crate::command_safety::shell_parser::contains_dynamic_shell_syntax(command)
}

/// Single-walk result for an `env` / leading-assignment prefix.
///
/// `stripped_start` is the index of the executable that will actually run;
/// `has_injection` reports whether the skipped prefix sets a known
/// config/loader-injection key or uses `env -S` (which re-parses at runtime
/// and cannot be audited). One walk produces both so safety checks never
/// re-scan the same prefix twice and the `env`-flag table cannot drift
/// between the stripping and injection paths.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct EnvPrefixScan {
    stripped_start: usize,
    has_injection: bool,
}

fn is_env_split_string_flag(word: &str) -> bool {
    word == "-S" || word == "--split-string" || word.starts_with("-S") || word.starts_with("--split-string=")
}

fn env_flag_consumes_next(word: &str) -> bool {
    matches!(word, "-u" | "--unset" | "-C" | "--chdir")
}

/// Walk an optional `env` prefix plus leading `KEY=value` assignments once.
///
/// Shared by [`command_words_after_environment_prefix`] (stripping) and
/// [`environment_prefix_has_injection_keys`] (injection audit) so both paths
/// inspect the same command shape. Fail-closed notes:
///
/// - `env -S`/`--split-string` re-parses the rest at runtime: reported as
///   injection with an empty remainder, matching both historical paths.
/// - Words with an empty key (`=foo`) are NOT assignments: scanning stops and
///   they are kept as the program (unknown, therefore mutating). The old
///   stripping path skipped them via a broad `contains('=')` test; stopping is
///   strictly fail-closed and the injection path is unchanged.
fn scan_environment_prefix(words: &[String]) -> EnvPrefixScan {
    let mut start = 0;
    let mut has_injection = false;
    loop {
        if words.get(start).is_some_and(|word| word == "env") {
            start += 1;
            while let Some(word) = words.get(start) {
                if word == "--" {
                    start += 1;
                    break;
                }
                if is_env_split_string_flag(word) {
                    // Split-string re-parses the rest at runtime; cannot audit
                    // assignments. Treat as unsafe with no auditable remainder.
                    return EnvPrefixScan { stripped_start: words.len(), has_injection: true };
                }
                let consumes_next = env_flag_consumes_next(word);
                start += 1;
                if consumes_next {
                    // `-u/--unset` removes a key; `-C/--chdir` takes a path.
                    if words.get(start).is_some() {
                        start += 1;
                    }
                    continue;
                }
                if let Some(key) = env_assignment_key(word) {
                    if is_env_injection_key(key) {
                        has_injection = true;
                    }
                    continue;
                }
                if word.starts_with('-') {
                    continue;
                }
                start -= 1;
                break;
            }
            continue;
        }
        if let Some(key) = words.get(start).and_then(|word| env_assignment_key(word)) {
            if is_env_injection_key(key) {
                has_injection = true;
            }
            start += 1;
            continue;
        }
        break;
    }
    EnvPrefixScan { stripped_start: start, has_injection }
}

/// Skip an optional `env` prefix and leading shell environment assignments.
///
/// The returned slice starts at the executable that will actually run. Keep
/// this helper shared by intent and activity classification so both paths
/// inspect the same command shape.
pub fn command_words_after_environment_prefix(words: &[String]) -> &[String] {
    let scan = scan_environment_prefix(words);
    &words[scan.stripped_start..]
}

/// Exact environment keys that can inject config, load foreign code, or launch
/// helpers into an otherwise read-only program.
const ENV_INJECTION_KEYS: &[&str] = &[
    "GIT_CONFIG_COUNT",
    "GIT_CONFIG_GLOBAL",
    "GIT_CONFIG_SYSTEM",
    "GIT_CONFIG_NOSYSTEM",
    "GIT_EXTERNAL_DIFF",
    "GIT_TEXTCONV",
    "GIT_DIFF_OPTS",
    "GIT_EDITOR",
    "GIT_SEQUENCE_EDITOR",
    "GIT_SSH",
    "GIT_SSH_COMMAND",
    "GIT_EXEC_PATH",
    "LD_PRELOAD",
    "LD_AUDIT",
    "LD_LIBRARY_PATH",
    "DYLD_INSERT_LIBRARIES",
    "DYLD_LIBRARY_PATH",
    "DYLD_FRAMEWORK_PATH",
    "DYLD_FALLBACK_LIBRARY_PATH",
    "BASH_ENV",
    "ENV",
    "SHELLOPTS",
    "PERL5OPT",
    "PYTHONSTARTUP",
    "NODE_OPTIONS",
    "RUBYOPT",
    "EDITOR",
    "VISUAL",
    "PAGER",
];

/// Prefix families that always carry injection payload (`GIT_CONFIG_KEY_n` /
/// `GIT_CONFIG_VALUE_n` pair with `GIT_CONFIG_COUNT`).
const ENV_INJECTION_KEY_PREFIXES: &[&str] = &["GIT_CONFIG_KEY_", "GIT_CONFIG_VALUE_"];

fn env_assignment_key(word: &str) -> Option<&str> {
    if word.starts_with('-') {
        return None;
    }
    let (key, _) = word.split_once('=')?;
    if key.is_empty() {
        return None;
    }
    Some(key)
}

fn is_env_injection_key(key: &str) -> bool {
    ENV_INJECTION_KEYS.contains(&key) || ENV_INJECTION_KEY_PREFIXES.iter().any(|prefix| key.starts_with(prefix))
}

/// Returns whether an `env` / leading-assignment prefix sets a known
/// config-injection or loader-injection key.
///
/// `command_words_after_environment_prefix` strips these assignments so the
/// executable can be classified; the values themselves must still be checked.
/// Unset (`env -u KEY`) is not an assignment. Unknown keys stay allowed so
/// ordinary exploration (`LANG=C rg …`) keeps working.
pub fn environment_prefix_has_injection_keys(words: &[String]) -> bool {
    scan_environment_prefix(words).has_injection
}

/// Resolve the executable behind the supported flag-only `uv run` prefix.
/// Unknown/value-taking wrapper options remain unclassified.
pub(crate) fn uv_run_command_words(words: &[String]) -> Option<&[String]> {
    if words.get(1).map(String::as_str) != Some("run") {
        return None;
    }
    let index = words
        .iter()
        .enumerate()
        .skip(2)
        .find(|(_, word)| {
            !matches!(
                word.as_str(),
                "--frozen" | "--locked" | "--no-sync" | "--offline" | "--no-project" | "--quiet" | "-q"
            )
        })?
        .0;
    (!words.get(index)?.starts_with('-')).then(|| &words[index..])
}

/// Explicit source repair modes cannot serve as completion verifiers, even
/// when declared by a tracker. Command admission remains policy-owned.
pub(crate) fn command_uses_source_repair(words: &[String]) -> bool {
    let words = command_words_after_environment_prefix(words);
    let Some(program) = words
        .first()
        .and_then(|word| Path::new(word).file_name())
        .and_then(|name| name.to_str())
    else {
        return false;
    };
    let program = program.to_ascii_lowercase();
    match program.as_str() {
        "cargo" => {
            let mut arguments = words.iter().skip(1).take_while(|word| word.as_str() != "--");
            arguments.clone().any(|word| word.split('=').next() == Some("--fix"))
                || arguments
                    .find(|word| !word.starts_with(['-', '+']))
                    .is_some_and(|word| word == "fix")
        }
        "ruff" => {
            words.iter().skip(1).take_while(|word| word.as_str() != "--").any(|word| {
                matches!(word.split('=').next(), Some("--fix" | "--fix-only" | "--add-noqa" | "--add-ignore"))
            })
        }
        "uv" => uv_run_command_words(words).is_some_and(command_uses_source_repair),
        _ => false,
    }
}

/// Return whether a parsed command uses an option or environment assignment
/// that can turn an otherwise read-only inspection program into a writer or
/// command launcher.
///
/// This is deliberately option/env focused; the read-only executable/subcommand
/// allow-list remains in `tool_intent::readonly`. Keeping the unsafe option
/// rules here lets raw argument validation and activity classification share
/// the same token-aware guard without searching quoted text.
pub(crate) fn has_unsafe_readonly_options(words: &[String]) -> bool {
    if command_uses_source_repair(words) {
        return true;
    }
    // Single prefix walk: the scan yields both the injection verdict and the
    // stripped executable offset, replacing the historical two-walk sequence
    // (`environment_prefix_has_injection_keys` then
    // `command_words_after_environment_prefix`).
    let scan = scan_environment_prefix(words);
    if scan.has_injection {
        return true;
    }
    let command_words = &words[scan.stripped_start..];
    let Some(program) = command_words
        .first()
        .and_then(|word| Path::new(word).file_name())
        .and_then(|name| name.to_str())
    else {
        return false;
    };
    let program = program.to_ascii_lowercase();

    match program.as_str() {
        "rustc" => command_words.iter().enumerate().skip(1).any(|(index, word)| {
            word.starts_with("-o")
                || word.split('=').next() == Some("--out-dir")
                || ["--print", "--emit"].iter().any(|option| {
                    let value = if word == option {
                        command_words.get(index + 1).map(String::as_str)
                    } else {
                        word.strip_prefix(option).and_then(|suffix| suffix.strip_prefix('='))
                    };
                    value.is_some_and(|value| value.contains('='))
                })
        }),
        "pytest" | "python" | "python3" | "uv" => command_words.iter().skip(1).any(|word| {
            matches!(
                word.split('=').next(),
                Some("--junitxml" | "--junit-xml" | "--log-file" | "--basetemp" | "--output-file")
            )
        }),
        "ruff" => command_words
            .iter()
            .skip(1)
            .any(|word| word.split('=').next() == Some("--output-file")),
        "git" => command_words.iter().skip(1).any(|word| {
            // `-C <dir>` (bare or inline `-C<dir>`) only redirects which
            // repository is read; the subcommand allow-list still gates every
            // git call, so it cannot turn an inspection into a writer. `-c
            // key=value` stays prompt-requiring: config injection is a
            // command-execution vector (fsmonitor/pager hooks).
            let is_dir_redirect = word == "-C" || (word.starts_with("-C") && word.len() > 2);
            (!is_dir_redirect && crate::command_safety::git_global_option_requires_prompt(word))
                || word == "--ext-diff"
                || word == "--textconv"
                || word == "-o"
                || word.starts_with("-o")
                || word == "--output"
                || word.starts_with("--output=")
        }),
        "sed" => has_unsafe_sed_options(&command_words[1..]),
        "find" => command_words.iter().skip(1).any(|word| {
            matches!(
                word.as_str(),
                "-delete" | "-exec" | "-execdir" | "-ok" | "-okdir" | "-fls" | "-fprint" | "-fprint0" | "-fprintf"
            )
        }),
        "sort" => command_words.iter().skip(1).any(|word| {
            word == "-o"
                || word.starts_with("-o")
                || word == "--output"
                || word.starts_with("--output=")
                || word == "--compress-program"
                || word.starts_with("--compress-program=")
        }),
        "date" => command_words
            .iter()
            .skip(1)
            .any(|word| word == "-s" || word.starts_with("-s") || word == "--set" || word.starts_with("--set=")),
        "rg" => command_words.iter().skip(1).any(|word| {
            matches!(word.as_str(), "--pre" | "--hostname-bin" | "--search-zip" | "-z")
                || word.starts_with("--pre=")
                || word.starts_with("--hostname-bin=")
        }),
        "ast-grep" | "sg" => command_words
            .iter()
            .skip(1)
            .any(|word| word == "-r" || word == "--rewrite" || word.starts_with("--rewrite=")),
        "fd" => command_words
            .iter()
            .skip(1)
            .any(|word| word == "-x" || word == "--exec" || word == "--exec-batch" || word.starts_with("--exec=")),
        "tree" => command_words
            .iter()
            .skip(1)
            .any(|word| word == "-o" || word.starts_with("-o") || word == "--output" || word.starts_with("--output=")),
        // `awk` stays on the read-only allow-list, but its program text can
        // write (`print > file`), pipe into commands (`print | "cmd"`),
        // execute them (`system()`), call them indirectly (`@func()`), or
        // load code (`@include`, `@load`), and its options can edit in place
        // (`-i inplace`), load external code (`-l`), or write profiles
        // (`-p`). `has_unsafe_awk_options` fails closed on all of those; only
        // data-only options (`-v`, `-F`) and a write-free program pass.
        "awk" => has_unsafe_awk_options(&command_words[1..]),
        _ => false,
    }
}

pub fn normalize_shell_args(args: &Value) -> Result<Value, &'static str> {
    let mut normalized = match normalize_indexed_command_args(args)? {
        Some(value) => value,
        None => args.clone(),
    };

    let Some(payload) = normalized.as_object_mut() else {
        return Ok(normalized);
    };

    if payload.get("command").is_none() {
        if let Some(command) = payload.get("cmd").cloned() {
            payload.insert("command".to_string(), command);
        } else if let Some(command) = payload.get("raw_command").cloned() {
            payload.insert("command".to_string(), command);
        }
    }

    if payload.get("input").is_none() {
        if let Some(input) = payload.get("chars").cloned() {
            payload.insert("input".to_string(), input);
        } else if let Some(input) = payload.get("text").cloned() {
            payload.insert("input".to_string(), input);
        }
    }

    if payload.get("session_id").is_none()
        && let Some(session_id) = payload.get("s").cloned()
    {
        payload.insert("session_id".to_string(), session_id);
    }

    if payload.get("max_tokens").is_none()
        && let Some(max_output_tokens) = payload.get("max_output_tokens").cloned()
    {
        payload.insert("max_tokens".to_string(), max_output_tokens);
    }

    if payload.get("max_output_tokens").is_none()
        && let Some(max_tokens) = payload.get("max_tokens").cloned()
    {
        payload.insert("max_output_tokens".to_string(), max_tokens);
    }

    Ok(normalized)
}

#[cfg(test)]
mod tests;
