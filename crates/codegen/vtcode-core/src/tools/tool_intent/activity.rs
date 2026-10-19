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

fn make_like_verification_targets(words: &[String]) -> bool {
    let mut saw_target = false;
    for word in words.iter().skip(1) {
        let lower = word.to_ascii_lowercase();
        if lower.starts_with('-') || lower.contains('=') {
            continue;
        }
        if !matches!(lower.as_str(), "test" | "tests" | "check" | "checks" | "lint" | "verify" | "validate") {
            return false;
        }
        saw_target = true;
    }
    saw_target
}

fn python_verification_arguments(arguments: &[String]) -> bool {
    // Python executes the first target, not later verifier-like tokens.
    let mut arguments = arguments.iter();
    let target = arguments.find(|word| {
        !matches!(word.as_str(), "-I" | "-E" | "-s" | "-S" | "-B" | "-O" | "-OO" | "-q" | "-u" | "-b" | "-bb" | "-v")
    });
    match target.map(String::as_str) {
        Some("-m") => arguments.next().is_some_and(|module| module == "pytest"),
        Some(script) => {
            ["scripts/check_markdown.py", "scripts/check_docs_links.py"]
                .iter()
                .any(|known| Path::new(script).ends_with(known))
                && arguments.next().is_none()
        }
        None => false,
    }
}

fn is_verification_invocation(words: &[String]) -> bool {
    // Verifiers may emit build artifacts; reject source-repair modes without
    // applying the stricter inspection-only output restrictions to compilers.
    if crate::tools::command_args::command_uses_source_repair(words)
        || crate::tools::command_args::environment_prefix_has_injection_keys(words)
    {
        return false;
    }
    let command_words = crate::tools::command_args::command_words_after_environment_prefix(words);
    let first = command_words.first().map(String::as_str).unwrap_or_default();
    let second = command_words.get(1).map(|word| word.to_ascii_lowercase());
    let third = command_words.get(2).map(|word| word.to_ascii_lowercase());
    let program = Path::new(first)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(first)
        .to_ascii_lowercase();

    match program.as_str() {
        "markdownlint-cli2" => command_words.get(1..).is_some_and(markdownlint_arguments_verify),
        "npx" => {
            let package_index = command_words
                .iter()
                .enumerate()
                .skip(1)
                .find(|(_, word)| {
                    !matches!(word.as_str(), "--yes" | "-y" | "--no-install")
                        && !word.strip_prefix("--loglevel=").is_some_and(|level| {
                            matches!(
                                level,
                                "silent" | "error" | "warn" | "notice" | "http" | "info" | "verbose" | "silly"
                            )
                        })
                })
                .map(|(index, _)| index);
            package_index.is_some_and(|index| {
                let Some(package) = command_words.get(index) else {
                    return false;
                };
                let is_markdownlint = package == "markdownlint-cli2"
                    || package.strip_prefix("markdownlint-cli2@").is_some_and(|version| {
                        version.split('.').count() == 3
                            && version
                                .split('.')
                                .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
                    });
                is_markdownlint && command_words.get(index + 1..).is_some_and(markdownlint_arguments_verify)
            })
        }
        "cargo" => {
            if second.as_deref() == Some("fmt") {
                // `cargo fmt` without `--check` reformats the worktree (mutation).
                // With `--check` it is a read-only lint verification.
                return words.iter().any(|word| word == "--check");
            }
            matches!(second.as_deref(), Some("check" | "build" | "clippy" | "test"))
                || (second.as_deref() == Some("nextest") && third.as_deref() == Some("run"))
        }
        "go" => matches!(second.as_deref(), Some("test" | "build")),
        "npm" | "pnpm" | "yarn" => {
            matches!(second.as_deref(), Some("test" | "build" | "lint" | "check"))
                || (second.as_deref() == Some("run")
                    && matches!(third.as_deref(), Some("test" | "build" | "lint" | "check")))
        }
        "bun" | "bunx" => {
            matches!(second.as_deref(), Some("test"))
                || (second.as_deref() == Some("run")
                    && matches!(third.as_deref(), Some("test" | "build" | "lint" | "check")))
        }
        "deno" => matches!(second.as_deref(), Some("test" | "lint" | "check")),
        "make" | "gmake" | "just" => make_like_verification_targets(command_words),
        "uv" => {
            let Some(arguments) = crate::tools::command_args::uv_run_command_words(command_words) else {
                return false;
            };
            let mut arguments = arguments.iter();
            let target = arguments.next();
            match target.map(String::as_str) {
                Some("pytest") => true,
                Some("ruff") => arguments.next().is_some_and(|argument| argument == "check"),
                Some("python" | "python3") => python_verification_arguments(arguments.as_slice()),
                _ => false,
            }
        }
        "ruff" => {
            if second.as_deref() == Some("format") {
                return words.iter().any(|word| word == "--check");
            }
            matches!(second.as_deref(), Some("check" | "lint"))
        }
        "tsc" => {
            // Bare `tsc` emits output (mutation). Only `--noEmit` type-checks.
            words.iter().any(|word| word == "--noEmit")
        }
        "eslint" => {
            // `eslint --fix` rewrites the worktree (mutation).
            !words.iter().any(|word| word == "--fix" || word == "--fix-dry-run")
        }
        "rustc" | "pytest" | "xcodebuild" | "gradle" | "gradlew" => true,
        "python" | "python3" => python_verification_arguments(&command_words[1..]),
        _ if first.ends_with("/scripts/check.sh") || first.ends_with("/scripts/check-dev.sh") => true,
        _ => false,
    }
}

fn markdownlint_arguments_verify(arguments: &[String]) -> bool {
    !arguments.is_empty()
        && !arguments.iter().any(|argument| {
            matches!(argument.as_str(), "--fix" | "-f" | "--help" | "-h" | "--version" | "-v")
                || argument.starts_with("--fix=")
        })
}

fn contains_verification_invocation(command: &str) -> bool {
    static_shell_command_words(command)
        .is_some_and(|commands| commands.iter().any(|words| is_verification_invocation(words)))
}

/// Recognize a verifier for diagnostic feedback, including status-masking tails
/// such as `; echo $?` that cannot pass static verification admission.
/// This heuristic grants no permissions, verification credit, or repair edits.
pub fn shell_command_contains_verifier(args: &Value) -> bool {
    let Some(command) = shell_command_text_with_args(args) else {
        return false;
    };
    super::readonly::parse_shell_command_words(&command)
        .is_some_and(|commands| commands.iter().any(|words| is_verification_invocation(words)))
}

/// Whether a command inspects search results that normally produce output.
///
/// Used only for progress accounting: an empty read-only search pipeline can
/// exit successfully when its final filter masks the search's no-match status.
/// Quiet/existence probes intentionally return no text and are excluded. This
/// predicate grants no execution permissions and never verifies mutations.
pub fn shell_command_is_output_search(args: &Value) -> bool {
    let Some(command) = shell_command_text_with_args(args) else {
        return false;
    };
    let Some(segments) = static_shell_command_words(&command) else {
        return false;
    };
    let mut saw_search = false;
    for words in &segments {
        if !command_words_are_readonly(words) || is_verification_invocation(words) {
            return false;
        }
        let words = crate::tools::command_args::command_words_after_environment_prefix(words);
        if command_words_are_grep_search(words) {
            if grep_words_request_quiet_output(words) {
                return false;
            }
            saw_search = true;
        }
    }
    saw_search
}

fn grep_words_request_quiet_output(words: &[String]) -> bool {
    let is_rg = words
        .first()
        .and_then(|word| Path::new(word).file_name())
        .is_some_and(|name| name == "rg");
    let mut arguments = words.iter().skip(1);
    let mut quiet = false;
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--" => break,
            "--quiet" | "--silent" => quiet = true,
            "--no-quiet" if is_rg => quiet = false,
            _ if argument.starts_with("--") => {
                // Values can look like options, including patterns such as `-query`.
                let takes_value = matches!(
                    argument.as_str(),
                    "--regexp"
                        | "--file"
                        | "--after-context"
                        | "--before-context"
                        | "--context"
                        | "--max-count"
                        | "--include"
                        | "--exclude"
                        | "--exclude-from"
                        | "--exclude-dir"
                        | "--label"
                        | "--directories"
                        | "--devices"
                ) || (is_rg
                    && matches!(
                        argument.as_str(),
                        "--glob"
                            | "--iglob"
                            | "--ignore-file"
                            | "--pre-glob"
                            | "--type"
                            | "--type-not"
                            | "--type-add"
                            | "--type-clear"
                            | "--replace"
                            | "--encoding"
                            | "--engine"
                            | "--color"
                            | "--colors"
                            | "--threads"
                            | "--max-depth"
                            | "--max-columns"
                            | "--max-filesize"
                            | "--dfa-size-limit"
                            | "--regex-size-limit"
                            | "--context-separator"
                            | "--field-context-separator"
                            | "--field-match-separator"
                            | "--hyperlink-format"
                            | "--path-separator"
                            | "--sort"
                            | "--sortr"
                            | "--generate"
                    ));
                if takes_value {
                    arguments.next();
                }
            }
            _ if argument.starts_with('-') => {
                let mut flags = argument.chars().skip(1).peekable();
                while let Some(flag) = flags.next() {
                    if flag == 'q' {
                        quiet = true;
                    }
                    let takes_value = matches!(flag, 'e' | 'f' | 'A' | 'B' | 'C' | 'm' | 'd')
                        || (is_rg && matches!(flag, 'g' | 't' | 'T' | 'r' | 'E' | 'j' | 'M'))
                        || (!is_rg && flag == 'D');
                    if takes_value {
                        if flags.peek().is_none() {
                            arguments.next();
                        }
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    quiet
}

/// Whether exit code 1 belongs to a standalone grep-style search.
/// Compound commands, shell wrappers and redirected diagnostics cannot prove
/// the search's no-match status, even when their visible output is empty.
pub fn shell_command_is_standalone_grep_search(args: &Value) -> bool {
    let Some(command) = shell_command_text_with_args(args) else {
        return false;
    };
    let Some(segments) = static_shell_command_words(&command) else {
        return false;
    };
    let [words] = segments.as_slice() else {
        return false;
    };
    shell_words::split(&command).is_ok_and(|raw_words| raw_words == *words)
        && command_words_are_readonly(words)
        && command_words_are_grep_search(words)
}

fn command_words_are_grep_search(words: &[String]) -> bool {
    let words = crate::tools::command_args::command_words_after_environment_prefix(words);
    let program = words
        .first()
        .and_then(|word| Path::new(word).file_name())
        .and_then(|name| name.to_str());
    matches!(program, Some("rg" | "grep" | "egrep" | "fgrep"))
}

/// Project markers checked in priority order before the Node tier: each
/// entry names the files that select its verifier. First tier with any
/// marker present wins, matching the previous sequential `if` chain.
const PRE_NODE_VERIFIER_MARKERS: &[(&[&str], &str)] = &[
    (&["Cargo.toml"], "cargo check --locked"),
    (&["go.mod"], "go test ./..."),
    (&["deno.json", "deno.jsonc"], "deno test"),
    (&["bun.lockb", "bun.lock"], "bun test"),
];

/// Project markers checked after the Node tier (`package.json` is resolved
/// separately at its priority position for script sniffing).
const POST_NODE_VERIFIER_MARKERS: &[(&[&str], &str)] = &[
    (&["pytest.ini", "pyproject.toml", "setup.cfg", "tox.ini"], "pytest -q"),
    (&["Makefile", "makefile"], "make test"),
    (&["justfile", "Justfile"], "just test"),
];

/// First verifier whose tier has any marker file under `workspace_root`.
fn verifier_for_markers(workspace_root: &Path, markers: &[(&[&str], &str)]) -> Option<String> {
    markers.iter().find_map(|(files, verifier)| {
        files
            .iter()
            .any(|file| workspace_root.join(file).is_file())
            .then(|| (*verifier).to_string())
    })
}

/// Node verifier with `package.json` script sniffing: `test` runs directly,
/// other known scripts run via `npm run`, and a missing/unreadable script
/// map falls back to `npm test` so the directive never names a command that
/// does not exist. Returns `None` when there is no `package.json`.
fn default_verifier_for_node_workspace(workspace_root: &Path) -> Option<String> {
    let package_json = workspace_root.join("package.json");
    if !package_json.is_file() {
        return None;
    }
    if let Ok(content) = std::fs::read_to_string(&package_json)
        && let Ok(parsed) = serde_json::from_str::<Value>(&content)
        && let Some(scripts) = parsed.get("scripts").and_then(Value::as_object)
    {
        for (script, command) in [
            ("test", "npm test"),
            ("check", "npm run check"),
            ("lint", "npm run lint"),
            ("build", "npm run build"),
        ] {
            if scripts.contains_key(script) {
                return Some(command.to_string());
            }
        }
    }
    Some("npm test".to_string())
}

/// Detect a project-appropriate default verifier for autonomous recovery.
///
/// Inspects well-known project markers under `workspace_root` and returns a
/// concrete standalone verification command the agent can run via
/// `exec_command` (no pipes, no `;`/`||` joins). Priority follows the
/// existing `is_verification_invocation` coverage: Rust → Go → Deno/Bun →
/// Node → Python → Make/Just. Returns `None` when no marker is found so
/// callers can fall back to the generic verifier examples.
///
/// This is intentionally synchronous and allocation-light (existence checks
/// plus one small `package.json` read): it runs on the turn hot path when
/// the anti-blind-editing gate needs an actionable recovery directive.
/// Codex-style harness-managed verification (Stop-hook test gates,
/// auto-review) shows that naming the exact command — rather than listing
/// examples — is what unblocks long-running autonomous work.
pub fn default_verifier_for_workspace(workspace_root: &Path) -> Option<String> {
    if let Some(verifier) = verifier_for_markers(workspace_root, PRE_NODE_VERIFIER_MARKERS) {
        return Some(verifier);
    }
    if let Some(verifier) = default_verifier_for_node_workspace(workspace_root) {
        return Some(verifier);
    }
    // No recognised project marker; callers fall back to generic examples.
    verifier_for_markers(workspace_root, POST_NODE_VERIFIER_MARKERS)
}

/// Which shell forms of a verifier clear the anti-blind-editing gate. Shared
/// by the recovery directive and the blocked-mutation `next_action` so the
/// two surfaces cannot drift.
pub const VERIFIER_SHELL_FORM_NOTE: &str = "Cap output with `max_output_tokens`. Pure `head`/`tail` verifier tails run standalone; static read-only filtering pipelines run with fail-closed `pipefail`. Only terminal exit 0 clears the gate; `;`, `||`, dynamic syntax, and mutating tails do not qualify.";

/// Stand-in for the verifier when no project command was detected or
/// configured. It lists examples across ecosystems instead of naming one
/// command, because a single concrete fallback (such as a Cargo command in a
/// Go or Node workspace) would direct the model to a verifier that does not
/// exist.
pub const GENERIC_VERIFIER_DESCRIPTION: &str =
    "your project's build/test/lint command (e.g. `cargo check --locked`, `go test ./...`, `npm test`, or `pytest -q`)";

/// How harness text names the verifier to run: the resolved command in
/// backticks, or [`GENERIC_VERIFIER_DESCRIPTION`] when none was resolved.
/// `default_verifier` should come from [`default_verifier_for_workspace`] or
/// the harness override resolution built on it.
pub fn verifier_reference(default_verifier: Option<&str>) -> String {
    match default_verifier.map(str::trim).filter(|command| !command.is_empty()) {
        Some(command) => format!("`{command}`"),
        None => GENERIC_VERIFIER_DESCRIPTION.to_string(),
    }
}

/// Build the actionable verification-recovery directive with a concrete
/// command. `default_verifier` should come from
/// [`default_verifier_for_workspace`]; when `None`, the generic examples are
/// kept so the directive never names a command that does not exist.
pub fn verification_recovery_directive(default_verifier: Option<&str>, attempt: u8, max_attempts: u8) -> String {
    let verifier = verifier_reference(default_verifier);
    format!(
        "Verification recovery ({attempt}/{max_attempts}): completion is unverified. Continue implementation and repair reported failures; edits remain available. \
        Validate a failing generated assertion against the original request before changing code; explicitly correct erroneous tracker verify metadata without weakening user criteria. Then run only missing checks, starting with {verifier} via `exec_command`, standalone or as a pure `&&` chain of verifiers. \
        {VERIFIER_SHELL_FORM_NOTE} Reuse fresh passes; successful completion requires all mandatory checks."
    )
}

/// Return whether a shell tool call is an admitted truncation-only verification
/// attempt while the anti-blind-editing gate is pending.
///
/// Piped verifiers (e.g. `cargo check 2>&1 | head -c 4000`) must be allowed to
/// run so the model can see the failure; otherwise the generic "cap output
/// with `| head`" guidance deadlocks on `Mutation blocked until verification`.
/// Only a truthful successful verifier clears the gate; this helper only
/// decides admission, never clearance.
///
/// Fail-closed smuggling guard: every parsed shell segment must be a
/// verification invocation or an allow-listed readonly command. A chained
/// mutation such as `cargo check && rm -rf target` therefore stays blocked
/// instead of riding through on the verifier prefix. Unparseable (dynamic)
/// shell syntax also stays blocked.
pub fn shell_command_is_admitted_verification_attempt(args: &Value) -> bool {
    let Some(command) = shell_command_text_with_args(args) else {
        return false;
    };
    if crate::tools::command_args::contains_dynamic_shell_syntax(&command) {
        return false;
    }
    if command.starts_with(VERIFIER_PIPEFAIL_PREFIX) {
        return pipefail_verifier_pipeline(&command).is_some();
    }
    let segments =
        static_shell_command_words(&command).or_else(|| static_shell_command_words_with_output_plumbing(&command));
    let Some(segments) = segments else {
        return false;
    };
    if segments.is_empty() {
        return false;
    }
    let mut saw_verification = false;
    for words in &segments {
        if is_verification_invocation(words) {
            saw_verification = true;
        } else if !command_words_are_readonly(words) {
            return false;
        }
    }
    saw_verification
}

fn has_logical_sequencing(words: &[String]) -> bool {
    words.iter().any(|word| matches!(word.as_str(), "&&" | "||" | ";"))
}

/// Split a command on top-level `|` operators, tracking single/double quotes
/// and backslash escapes with the same discipline as
/// [`shell_uses_only_and_chaining`]. Returns the pipe-separated stages in
/// order, or `None` when no top-level pipe exists. `||` is not special-cased:
/// it yields an empty stage, which stage validation rejects — fail-closed
/// without a second operator table to keep in sync.
fn split_top_level_pipes(command: &str) -> Option<Vec<&str>> {
    if !command.contains('|') {
        return None;
    }
    let mut stages = Vec::new();
    let mut start = 0;
    let mut saw_pipe = false;
    let mut in_single_quote = false;
    let mut in_double_quote = false;
    let mut chars = command.char_indices().peekable();
    while let Some((index, character)) = chars.next() {
        // Outside single quotes a backslash escapes the next character for the
        // shell, so `\|` is a literal pipe, not a stage separator. Consume the
        // pair to stay aligned with the shell (mirrors the `&&`-chain scanner).
        if character == '\\' && !in_single_quote {
            chars.next();
            continue;
        }
        if character == '\'' && !in_double_quote {
            in_single_quote = !in_single_quote;
            continue;
        }
        if character == '"' && !in_single_quote {
            in_double_quote = !in_double_quote;
            continue;
        }
        if in_single_quote || in_double_quote {
            continue;
        }
        if character == '|' {
            saw_pipe = true;
            stages.push(&command[start..index]);
            start = index + character.len_utf8();
        }
    }
    if !saw_pipe {
        return None;
    }
    stages.push(&command[start..]);
    Some(stages)
}

/// Whether a pipeline tail stage is a pure output truncator: a bare `head` or
/// `tail` invocation (any flags) with no shell operators of its own. Anything
/// else (`grep`, `wc`, `sort`, redirects, backgrounding, chaining) keeps its
/// pipeline semantics and must not be elided — dropping it would discard work
/// the caller asked for or change what the exit status means.
fn is_pure_truncation_stage(stage: &str) -> bool {
    let trimmed = stage.trim();
    if trimmed.is_empty() {
        return false;
    }
    // A truncator stage is one simple command: reject any operator that would
    // indicate chaining, backgrounding, redirection, or nesting. The scan is
    // quote-aware so quoted flag values (e.g. `--sep=';'`) do not false-reject;
    // an unaware scan could only over-reject, which stays fail-closed.
    let mut in_single_quote = false;
    let mut in_double_quote = false;
    let mut chars = trimmed.chars().peekable();
    while let Some(character) = chars.next() {
        if character == '\\' && !in_single_quote {
            chars.next();
            continue;
        }
        if character == '\'' && !in_double_quote {
            in_single_quote = !in_single_quote;
            continue;
        }
        if character == '"' && !in_single_quote {
            in_double_quote = !in_double_quote;
            continue;
        }
        if in_single_quote || in_double_quote {
            continue;
        }
        if matches!(character, ';' | '&' | '<' | '>' | '\n' | '|') {
            return false;
        }
    }
    let words = match shell_words::split(trimmed) {
        Ok(words) => words,
        Err(_) => return false,
    };
    let Some(parsed) = NonEmptySlice::from_slice(&words) else {
        return false;
    };
    let program = Path::new(parsed.first())
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(parsed.first().as_str())
        .to_ascii_lowercase();
    matches!(program.as_str(), "head" | "tail")
}

/// Rewrite a truncation-only piped verifier (`cargo check 2>&1 | head -c 4000`)
/// to its standalone verifier prefix so the observed exit status is the
/// verifier's, not the truncator's.
///
/// A pipeline's status belongs to its tail: running `cargo check | tail`
/// reports `tail`'s success even when the build fails, and the model reads
/// that exit 0 as "verified" while the anti-blind gate (correctly) stays
/// pending — a deadlock manufactured by shell exit-status semantics, observed
/// in real sessions. Eliding the truncator (output stays capped via
/// `max_output_tokens`) makes the status truthful in one round trip.
///
/// Fail-closed gates, all required:
/// - the full command is an admitted verification attempt (no dynamic
///   syntax; every segment independently verification-or-readonly), so shapes
///   like `cargo check | tail; rm …` can never reach the rewrite;
/// - the head classifies as [`ShellActivity::Verification`] on its own, so
///   `;`/`||`/background joins and smuggled mutations behind a verifier
///   prefix are rejected (only standalone verifiers and pure `&&` verifier
///   chains pass, whose `&&` exit status is truthful; output redirects such
///   as `> build.log` are preserved verbatim in the head and do not mask the
///   status, so they rewrite safely);
/// - every tail stage is a pure `is_pure_truncation_stage` truncator, so
///   `grep`/`wc`/`sort` tails (whose filtering is real work) are preserved.
///
/// Returns the standalone verifier text, or `None` when the command must run
/// (or be rejected) exactly as typed. Only commands whose raw text carries a
/// top-level `|` are candidates; array and indexed spellings re-quote
/// operators when joined, so they never present a top-level pipe and keep
/// today's behavior.
pub fn rewrite_truncation_only_verifier(args: &Value) -> Option<String> {
    use crate::config::constants::tools as tool_names;

    let command = shell_command_text_with_args(args)?;
    if !shell_command_is_admitted_verification_attempt(args) {
        return None;
    }
    let stages = split_top_level_pipes(&command)?;
    let (head, tails) = stages.split_first()?;
    let head = head.trim();
    if head.is_empty() || tails.iter().any(|stage| !is_pure_truncation_stage(stage)) {
        return None;
    }
    let head_args = serde_json::json!({"cmd": head});
    if !matches!(classify_shell_activity(tool_names::EXEC_COMMAND, &head_args), ShellActivity::Verification) {
        return None;
    }
    Some(head.to_string())
}

// `&&` fails closed when the selected shell does not support pipefail.
const VERIFIER_PIPEFAIL_PREFIX: &str = "set -o pipefail && ";

/// The exact status-only shell setup, with the entire verifier pipeline validated.
/// Command policy still checks every executable and explicit deny rule.
pub(crate) fn pipefail_verifier_pipeline(command: &str) -> Option<&str> {
    let pipeline = command.strip_prefix(VERIFIER_PIPEFAIL_PREFIX)?;
    if crate::tools::command_args::contains_dynamic_shell_syntax(pipeline) {
        return None;
    }
    verifier_pipeline_head(pipeline).map(|_| pipeline)
}

/// Validate a static pipeline without granting execution or mutation permissions.
fn verifier_pipeline_head(command: &str) -> Option<&str> {
    let stages = split_top_level_pipes(command)?;
    let (head, tails) = stages.split_first()?;
    let head = head.trim();
    let head_args = serde_json::json!({"cmd": head});
    if classify_shell_activity(crate::config::constants::tools::EXEC_COMMAND, &head_args) != ShellActivity::Verification
    {
        return None;
    }
    // Tail stages must be independently static and read-only. Logical joins,
    // unsafe redirections, substitutions, and mutations cannot ride behind it.
    for stage in tails {
        let segments = static_shell_command_words(stage)?;
        if segments.len() != 1 || !segments.first().is_some_and(|words| command_words_are_readonly(words)) {
            return None;
        }
    }
    Some(head)
}

/// Preserve filtering while making pipeline success depend on every stage.
/// Pure truncator pipelines retain the existing standalone rewrite to avoid
/// SIGPIPE from early-closing head/tail. Other static read-only tails use
/// pipefail; failures (including no matches or SIGPIPE) never clear the gate.
fn rewrite_verifier_pipeline(args: &Value) -> Option<String> {
    if let Some(standalone) = rewrite_truncation_only_verifier(args) {
        return Some(standalone);
    }
    let command = shell_command_text_with_args(args)?;
    if command.starts_with(VERIFIER_PIPEFAIL_PREFIX)
        || !shell_command_is_admitted_verification_attempt(args)
        || verifier_pipeline_head(&command).is_none()
    {
        return None;
    }
    Some(format!("{VERIFIER_PIPEFAIL_PREFIX}{command}"))
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
    let mut parser = tree_sitter::Parser::new();
    if parser.set_language(&tree_sitter_bash::LANGUAGE.into()).is_err() {
        return true;
    }
    let Some(tree) = parser.parse(command, None) else {
        return true;
    };
    let root = tree.root_node();
    root.has_error() || root.named_child_count() == 0 || !nonmutating_shell_node(root, command)
}

pub(crate) fn exec_output_has_changes(output: &Value) -> bool {
    output
        .get("modified_files")
        .and_then(Value::as_array)
        .is_some_and(|files| !files.is_empty())
        || crate::tools::file_ops::diff_output_has_effective_change(output) == Some(true)
}

/// Path-prefixed globs cannot expand into options. Keep this narrower than
/// general expansion: variables, substitutions, bare globs and dynamic heads
/// remain unproven, and the parent command still checks program/options.
fn is_literal_path_glob(word: &str) -> bool {
    let Some((prefix, _)) = word.split_once('/') else {
        return false;
    };
    !prefix.starts_with('-')
        && !prefix.contains(['*', '?', '[', ']'])
        && word.contains(['*', '?'])
        && !word.contains(['[', ']'])
        && !word
            .chars()
            .any(|character| character.is_whitespace() || "\"'$`\\{}()<>&;|~!".contains(character))
}

/// Walk every supported node, including commands nested in substitutions.
/// Unlike command extraction, this proof must never discard unknown syntax.
fn nonmutating_shell_node(node: tree_sitter::Node<'_>, source: &str) -> bool {
    use crate::command_safety::shell_parser::contains_dynamic_shell_syntax;
    let Ok(text) = node.utf8_text(source.as_bytes()) else {
        return false;
    };
    match node.kind() {
        "command" => {
            let Some(name) = node.child_by_field_name("name") else {
                return false;
            };
            let Ok(name_text) = name.utf8_text(source.as_bytes()) else {
                return false;
            };
            if contains_dynamic_shell_syntax(name_text) {
                return false;
            }
            let mut words = Vec::new();
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                let Ok(word) = child.utf8_text(source.as_bytes()) else {
                    return false;
                };
                if child.kind() == "variable_assignment" {
                    return false;
                }
                if child != name
                    && child.kind() == "word"
                    && is_literal_path_glob(word)
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
        "word" | "regex" => return !contains_dynamic_shell_syntax(text) || is_literal_path_glob(text),
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
    let mut cursor = node.walk();
    node.children(&mut cursor).all(|child| nonmutating_shell_node(child, source))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::config::constants::tools;
    use crate::tools::tool_intent::is_readonly_command_session_command;

    fn exec_command(command: &str) -> Value {
        json!({"cmd": command})
    }

    #[test]
    fn literal_timers_do_not_create_mutation_debt_but_arbitrary_python_does() {
        let output = serde_json::json!({"exit_code":0});
        for command in [
            "python3 -c 'import time; time.sleep(0.01)'",
            "python -c 'import time; time.sleep(5)'",
            "sleep 30",
            "sleep 120",
            "sleep 0.01",
        ] {
            assert!(!exec_mutation_should_incur_debt(command, &output), "{command}");
            assert!(exec_mutation_should_incur_debt(command, &serde_json::json!({"modified_files":["src/lib.rs"]})));
        }
        for command in [
            "python3 -c 'import time; time.sleep(1); open(\"x\",\"w\")'",
            "python3 -c 'import time; time.sleep(float(\"nan\"))'",
            "python3 -c 'print(1)'",
            "python3 timer.py",
            "sleep $DELAY",
            "sleep 1 > timer.log",
            "sleep -1",
            "python3 -c 'import time; time.sleep(-1)'",
            "PYTHONSTARTUP=script.py python3 -c 'import time; time.sleep(1)'",
            "python3 -c 'import time; time.sleep(1)' > timer.log",
        ] {
            assert!(exec_mutation_should_incur_debt(command, &output), "{command}");
        }
    }

    #[test]
    fn source_repair_commands_create_debt_instead_of_verifying() {
        for command in [
            "cargo clippy --fix --allow-dirty",
            "env LD_PRELOAD=./rewrite.so cargo clippy",
            "cargo +nightly clippy --fix",
            "cargo fix --allow-dirty",
            "ruff check --fix .",
            "ruff check --fix-only .",
            "ruff check --add-noqa .",
            "ruff check --add-ignore=reason .",
            "uv run --frozen --offline ruff check --fix .",
            "cargo clippy --fix && cargo check",
            "ruff check --fix . | head -20",
            // Do not treat an operand value as a safe preview flag.
            "ruff check --fix --exclude --diff .",
        ] {
            let args = exec_command(command);
            assert_eq!(classify_shell_activity(tools::EXEC_COMMAND, &args), ShellActivity::Mutation, "{command}");
            assert!(!shell_command_is_admitted_verification_attempt(&args), "{command}");
            assert!(exec_mutation_should_incur_debt(command, &json!({"exit_code": 0})), "{command}");
        }
        for command in [
            "cargo clippy --locked",
            "cargo nextest run fix",
            "ruff check --no-fix .",
            "ruff check --diff .",
            "uv run --frozen --offline ruff check --no-fix .",
        ] {
            let args = exec_command(command);
            assert_eq!(classify_shell_activity(tools::EXEC_COMMAND, &args), ShellActivity::Verification, "{command}");
            assert!(shell_command_is_admitted_verification_attempt(&args), "{command}");
            assert!(!exec_mutation_should_incur_debt(command, &json!({"exit_code": 0})), "{command}");
        }
    }

    #[test]
    fn compiler_artifact_writes_remain_eligible_verification() {
        let command = "rustc --crate-type lib sample.rs -o checked.rlib";
        let args = exec_command(command);
        assert_eq!(classify_shell_activity(tools::EXEC_COMMAND, &args), ShellActivity::Verification);
        assert!(shell_command_is_admitted_verification_attempt(&args));
        // The inspection proof still sees a write; verifier eligibility owns
        // the build-artifact exemption, not the read-only permission boundary.
        assert!(exec_mutation_should_incur_debt(command, &json!({"exit_code":0})));
    }

    #[test]
    fn verifier_diagnostics_recognize_masked_status_without_granting_admission() {
        for command in [
            "npx markdownlint-cli2 README.md; echo \"lint exit: $?\"",
            "cargo check; echo $?",
            "python3 scripts/check_markdown.py; echo \"lint exit: $?\"",
            "cargo check --locked || true",
            "cargo check; rm marker",
        ] {
            let args = exec_command(command);
            assert!(shell_command_contains_verifier(&args), "{command}");
            assert_eq!(classify_shell_activity(tools::EXEC_COMMAND, &args), ShellActivity::Mutation);
        }
        let masked = exec_command("npx markdownlint-cli2 README.md; echo \"lint exit: $?\"");
        assert!(!shell_command_is_admitted_verification_attempt(&masked));
        assert_eq!(shell_args_as_executed(tools::EXEC_COMMAND, &masked).as_ref(), &masked);
        for args in [
            exec_command("echo 'cargo check; echo $?'"),
            exec_command("grep 'cargo check' README.md"),
            exec_command("npx markdownlint-cli2 --fix README.md"),
            json!({"cmd":"cargo check; true", "raw_command":"echo hello"}),
            json!({"cmd":"npx markdownlint-cli2", "args":["--fix", "README.md"]}),
        ] {
            assert!(!shell_command_contains_verifier(&args), "{args}");
        }
    }

    #[test]
    fn markdownlint_verification_preserves_command_boundaries() {
        for command in [
            "markdownlint-cli2 README.md",
            "npx --yes markdownlint-cli2 README.md",
            "npx --no-install markdownlint-cli2@0.23.3 README.md",
            "npx --yes --loglevel=error markdownlint-cli2@0.23.3 --config .markdownlint-cli2.jsonc ':README.md'",
            "npx -y markdownlint-cli2 README.md && cargo check --locked",
            "python3 scripts/check_markdown.py",
            "python ./scripts/check_markdown.py",
            "python3 scripts/check_docs_links.py",
            "python ./scripts/check_docs_links.py",
        ] {
            assert!(shell_command_is_admitted_verification_attempt(&exec_command(command)), "{command}");
            assert_eq!(
                classify_shell_activity(tools::EXEC_COMMAND, &exec_command(command)),
                ShellActivity::Verification,
                "{command}"
            );
        }
        for command in [
            "markdownlint-cli2 --fix README.md",
            "npx --yes markdownlint-cli2 -f README.md",
            "npx --yes markdownlint-cli2 --fix=true README.md",
            "markdownlint-cli2 --version",
            "npx --package evil markdownlint-cli2 README.md",
            "npx --loglevel=error --package evil markdownlint-cli2 README.md",
            "npx --loglevel=error markdownlint-cli2 --fix README.md",
            "npx --loglevel=unknown markdownlint-cli2 README.md",
            "npx markdownlint-cli2@evil README.md",
            "npx markdownlint-cli2 README.md && rm README.md",
            "python3 scripts/check_markdown.py --fix",
            "python3 scripts/check_markdown.py --list",
            "python3 scripts/check_markdown.py --help",
            "python3 other/check_markdown.py",
            "python3 scripts/check_docs_links.py --help",
            "python3 other/check_docs_links.py",
        ] {
            assert!(!shell_command_is_admitted_verification_attempt(&exec_command(command)), "{command}");
        }
        assert_ne!(
            classify_shell_activity(tools::EXEC_COMMAND, &exec_command("npx markdownlint-cli2 README.md | grep error")),
            ShellActivity::Verification
        );
    }

    #[test]
    fn output_search_recognizes_static_search_pipelines() {
        for command in [
            "rg --files --hidden -g '.markdownlint*'",
            "rg --files --hidden | rg -i 'markdownlint' | sed -n '1,15p'",
            "git ls-files | grep -i markdownlint | head -10",
            "LANG=C /usr/bin/grep -n missing README.md",
            "rg -- '-q' README.md",
            "rg -e -query src | head -10",
            "grep -ne-query README.md",
            "rg --regexp --quiet src",
            "rg -g '-q*' missing src",
            "rg '-g-q*' missing src",
            "rg -q --no-quiet missing src",
        ] {
            assert!(shell_command_is_output_search(&exec_command(command)), "{command}");
        }
        assert!(shell_command_is_output_search(&json!({"command": ["rg", "missing", "src"]})));
    }

    #[test]
    fn output_search_excludes_quiet_probes_and_unproven_commands() {
        for command in [
            "rg -q pattern src",
            "grep -nq pattern README.md",
            "rg --quiet pattern src | head -10",
            "grep --silent pattern README.md",
            "rg -e -query -q src",
            "grep -nqe-query README.md",
            "rg --regexp --quiet --quiet src",
            "rg -q --no-quiet -q missing src",
            "echo 'rg missing'",
            "cat empty.txt",
            "cargo check | rg error",
            "rg missing src; touch result.txt",
            "rg missing $(pwd)",
            "rg missing src > result.txt",
            "rg --pre 'touch result.txt' missing src",
        ] {
            assert!(!shell_command_is_output_search(&exec_command(command)), "{command}");
        }
        assert!(!shell_command_is_output_search(&json!({})));
    }

    #[test]
    fn output_search_distinguishes_separator_values_from_quiet_flags() {
        for option in [
            "--context-separator",
            "--field-context-separator",
            "--field-match-separator",
        ] {
            let command = format!("rg {option} -q missing src | head -10");
            assert!(shell_command_is_output_search(&exec_command(&command)), "{command}");
            let quiet_command = format!("rg {option} -q --quiet missing src | head -10");
            assert!(!shell_command_is_output_search(&exec_command(&quiet_command)), "{quiet_command}");
        }
    }

    #[test]
    fn standalone_grep_search_preserves_exit_status_ownership() {
        for command in [
            "rg missing src",
            "grep -q missing README.md",
            "LANG=C /usr/bin/grep -n missing README.md",
            "rg 'a|b' src",
            "fgrep missing README.md",
        ] {
            assert!(shell_command_is_standalone_grep_search(&exec_command(command)), "{command}");
        }
        assert!(shell_command_is_standalone_grep_search(&json!({"command": ["rg", "missing", "src"]})));
        for command in [
            "rg missing src; false",
            "rg missing src && false",
            "rg missing src | grep other",
            "! rg missing src",
            "(rg missing src)",
            "echo '/rg missing'; false",
            "rg missing src 2>/dev/null",
            "rg missing src 2>&1",
            "rg missing $(pwd)",
            "rg --pre 'false' missing src",
        ] {
            assert!(!shell_command_is_standalone_grep_search(&exec_command(command)), "{command}");
        }
    }

    #[test]
    fn admitted_verification_attempt_allows_truncation_but_blocks_smuggled_mutations() {
        for command in [
            "cargo check --locked 2>&1 | head -c 4000",
            "cargo check --locked",
            "cargo nextest run 2>&1 | head -c 4000",
        ] {
            assert!(
                shell_command_is_admitted_verification_attempt(&exec_command(command)),
                "expected admission: {command}"
            );
        }
        for command in [
            "cargo check && rm -rf target",
            "cargo check; rm foo.txt",
            "cargo check || rm foo.txt",
            "cargo check && cargo test && rm foo.txt",
            "sed -i '' 's/old/new/' README.md",
            "echo $(date)",
            "cargo check > build.log && rm foo.txt",
        ] {
            assert!(
                !shell_command_is_admitted_verification_attempt(&exec_command(command)),
                "expected block: {command}"
            );
        }
        assert!(!shell_command_is_admitted_verification_attempt(&json!({})));
    }

    #[test]
    fn logged_compound_inspection_commands_are_not_mutations() {
        for command in [
            "cat README.md && printf '\\n--- git status ---\\n' && git status --short",
            "wc -l README.md; rg -n '^#' README.md",
            "git diff --stat; find docs -maxdepth 2 -type f | sort | head -40",
        ] {
            assert_eq!(
                classify_shell_activity(tools::EXEC_COMMAND, &exec_command(command)),
                ShellActivity::Inspection,
                "{command}"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn captured_read_commands_with_output_suppression_are_inspection() {
        for command in [
            r#"sed -n '1,180p' README.md; sed -n '280,350p' README.md; sed -n '389,411p' README.md; printf '\n--- repo metadata ---\n'; git log -1 --format='%h %s'; sed -n '1,100p' Cargo.toml; rg -n '^version\s*=|rust-version|workspace\.package' Cargo.toml crates -g Cargo.toml | head -40"#,
            r#"sed -n '1,120p' crates/codegen/vtcode-core/src/tools/tool_intent/activity.rs; printf '\n--- readonly policy ---\n'; rg -n 'READONLY_UNIFIED_EXEC_COMMANDS|command_words_are_readonly' crates/codegen/vtcode-core/src/tools/tool_intent/readonly.rs; printf '\n--- recent commits ---\n'; git log -5 --oneline; printf '\n--- command arguments ---\n'; sed -n '1,180p' crates/codegen/vtcode-core/src/tools/command_args.rs"#,
            r###"git diff --stat; find docs -maxdepth 2 -type f | sort | head -40; rg -n "vtcode init|vtcode models|full-auto|run-debug|cargo install" docs/user-guide docs/installation docs/development 2>/dev/null | head -50"###,
        ] {
            assert_eq!(
                classify_shell_activity(tools::EXEC_COMMAND, &exec_command(command)),
                ShellActivity::Inspection,
                "{command}"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn printf_output_safety_guards_remain_mutations() {
        for command in [
            "printf 'captured output\\n' > output.txt",
            "printf '%s\\n' \"$(git status --short)\"",
            "printf '%s\\n' `git status --short`",
            "printf '\\n--- inspection ---\\n' && rm output.txt",
        ] {
            let args = exec_command(command);
            assert_eq!(classify_shell_activity(tools::EXEC_COMMAND, &args), ShellActivity::Mutation, "{command}");
            assert!(!is_readonly_command_session_command(&args), "unexpected readonly command: {command}");
        }
    }

    #[test]
    fn git_diff_check_chain_remains_inspection() {
        assert_eq!(
            classify_shell_activity(
                tools::EXEC_COMMAND,
                &exec_command("git diff --check && git status --short && git diff --stat"),
            ),
            ShellActivity::Inspection
        );
    }

    #[test]
    fn verification_detection_skips_environment_prefixes() {
        for command in [
            "env RUSTFLAGS=-Dwarnings cargo check",
            "RUSTFLAGS=-Dwarnings env cargo check",
            "env -u PATH cargo check",
            "env -C /tmp cargo check",
        ] {
            assert_eq!(
                classify_shell_activity(tools::EXEC_COMMAND, &exec_command(command)),
                ShellActivity::Verification,
                "{command}"
            );
        }
    }

    #[test]
    fn ambiguous_or_mutating_compounds_remain_mutations() {
        for command in [
            "git diff --stat; python3 -c 'open(\"out\", \"w\").write(\"x\")'",
            "cat README.md; sed -i '' 's/a/b/' README.md",
            "sed --in-place= README.md",
            "git diff --output=out",
            "git diff '--output=out'",
            "git diff -o out",
            "git diff -oout",
            "git log --output=out",
            "git show --textconv",
            // `-C <dir> <read-only sub>` is an Inspection now (the redirect
            // only changes which repository is read); config injection stays
            // a Mutation.
            "git -c core.fsmonitor=touch status",
            "find . -fprint output.txt",
            "find . -fprintf output.txt '%p'",
            "rg --hostname-bin sh pattern",
            "rg --search-zip pattern",
            "rg -z pattern",
            "sort -o generated.txt README.md",
            "sort --compress-program=sh README.md",
            "date -s now",
            "awk -i inplace '{print}' README.md",
            "sed -n 's/a/b/e' README.md",
            "fd --exec sh -c 'touch output'",
            "tree -o output.txt",
            "ast-grep -r 'README.md'",
            "sed -n -fmalicious.sed -e '1p' src/main.rs",
            "sed -I '' 's/a/b/' src/main.rs",
            "sed -n '1p\nw leaked.txt' src/main.rs",
            "cargo check & rm output",
            "cargo check > build.log | rm output",
            "cargo check | head -40 > build.log",
            "cargo check | echo x > output.log",
            "cargo check | head -40",
            "cargo check > build.log &",
            "env -S 'cargo check'",
            "echo x > output.log && cargo check",
            "cargo check < build-input.log",
            "cat README.md > copied.txt",
            "git diff --check; rm output",
            "cat README.md\nrm output",
        ] {
            assert_eq!(
                classify_shell_activity(tools::EXEC_COMMAND, &exec_command(command)),
                ShellActivity::Mutation,
                "{command}"
            );
        }
    }

    #[test]
    fn quoted_output_text_does_not_change_inspection_classification() {
        for command in ["echo 'git diff --output=out'", "printf 'sort -o out input'"] {
            assert_eq!(
                classify_shell_activity(tools::EXEC_COMMAND, &exec_command(command)),
                ShellActivity::Inspection,
                "{command}"
            );
        }
    }

    #[test]
    fn cargo_fmt_check_is_verification_but_plain_fmt_is_not() {
        for command in [
            "cargo fmt --check",
            "cargo fmt --all -- --check",
            "cargo fmt -- --check",
        ] {
            assert_eq!(
                classify_shell_activity(tools::EXEC_COMMAND, &exec_command(command)),
                ShellActivity::Verification,
                "{command}"
            );
            assert!(
                shell_command_is_admitted_verification_attempt(&exec_command(command)),
                "expected admission: {command}"
            );
        }
        // Plain `cargo fmt` rewrites the worktree: it must stay a mutation and
        // must not ride through the verification gate.
        assert_eq!(classify_shell_activity(tools::EXEC_COMMAND, &exec_command("cargo fmt")), ShellActivity::Mutation);
        assert!(!shell_command_is_admitted_verification_attempt(&exec_command("cargo fmt")));
    }

    #[test]
    fn pure_and_chained_verifiers_are_verification() {
        for command in [
            "cargo fmt --all -- --check && cargo check --locked",
            "cargo check --locked && cargo nextest run --locked -p vtcode-ui",
            "cargo check --locked && cargo clippy --locked -p vtcode-ui -- -D warnings",
            "git diff --check && cargo check --locked",
        ] {
            assert_eq!(
                classify_shell_activity(tools::EXEC_COMMAND, &exec_command(command)),
                ShellActivity::Verification,
                "{command}"
            );
            assert!(
                shell_command_is_admitted_verification_attempt(&exec_command(command)),
                "expected admission: {command}"
            );
        }
    }

    #[test]
    fn non_and_chained_verifiers_remain_mutations() {
        for command in [
            "cargo check --locked; cargo nextest run --locked -p vtcode-ui",
            "cargo check --locked || cargo nextest run --locked -p vtcode-ui",
            "cargo check --locked | head -40",
            "cargo check --locked && cargo nextest run --locked -p vtcode-ui | head -40",
            "cargo check --locked &",
        ] {
            assert_eq!(
                classify_shell_activity(tools::EXEC_COMMAND, &exec_command(command)),
                ShellActivity::Mutation,
                "{command}"
            );
        }
    }

    #[test]
    fn expanded_verifiers_classify_as_verification() {
        for command in [
            "bun test",
            "bun run test",
            "deno lint",
            "deno check mod.ts",
            "make test",
            "make lint",
            "just verify",
            "ruff check src/",
            "ruff format --check",
            "tsc --noEmit",
            "eslint src/",
            "python3 -m pytest",
            "uv run pytest",
            "npm run lint",
        ] {
            assert_eq!(
                classify_shell_activity(tools::EXEC_COMMAND, &exec_command(command)),
                ShellActivity::Verification,
                "{command}"
            );
        }
        for command in [
            "make clean",
            "make test clean",
            "just fmt",
            "tsc",
            "eslint --fix src/",
            "ruff format src/",
            "bun install",
            "uv sync",
        ] {
            assert_eq!(
                classify_shell_activity(tools::EXEC_COMMAND, &exec_command(command)),
                ShellActivity::Mutation,
                "{command}"
            );
        }
    }

    #[test]
    fn escaped_quotes_fail_closed_instead_of_hiding_operators() {
        // A backslash-escaped quote is a literal for the shell, so the trailing
        // `;` is a live separator: the aggregate exit status can mask a failed
        // verifier, and the chain must not classify as pure `&&`.
        assert!(!shell_uses_only_and_chaining("cargo check --locked \\\"; echo ok"));
        assert!(!shell_uses_only_and_chaining("echo \\\" ; cargo check --locked && echo done"));
        // Escaped quotes inside real double quotes stay inert.
        assert!(shell_uses_only_and_chaining("echo \"a\\\"b\" && cargo check --locked"));
        assert!(shell_uses_only_and_chaining("echo \"path\" && cargo fmt --check"));
        // Backslash is literal inside single quotes.
        assert!(shell_uses_only_and_chaining("echo 'a\\b' && cargo check --locked"));
    }

    #[test]
    fn default_verifier_prefers_manifest_priority_order() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert_eq!(default_verifier_for_workspace(dir.path()), None);

        std::fs::write(dir.path().join("justfile"), "test:\n\techo ok\n").expect("justfile");
        assert_eq!(default_verifier_for_workspace(dir.path()).as_deref(), Some("just test"));
        std::fs::write(dir.path().join("pyproject.toml"), "[tool.pytest]\n").expect("pyproject");
        assert_eq!(default_verifier_for_workspace(dir.path()).as_deref(), Some("pytest -q"));
        std::fs::write(dir.path().join("package.json"), r#"{"scripts":{"test":"vitest"}}"#).expect("package.json");
        assert_eq!(default_verifier_for_workspace(dir.path()).as_deref(), Some("npm test"));
        std::fs::write(dir.path().join("go.mod"), "module example\n").expect("go.mod");
        assert_eq!(default_verifier_for_workspace(dir.path()).as_deref(), Some("go test ./..."));
        std::fs::write(dir.path().join("Cargo.toml"), "[package]\nname=\"x\"\n").expect("Cargo.toml");
        assert_eq!(default_verifier_for_workspace(dir.path()).as_deref(), Some("cargo check --locked"));
    }

    #[test]
    fn default_verifier_marker_aliases_and_tier_priority() {
        // Every table row resolves, including alternate spellings — a
        // dropped alias in the marker table must fail here, not in a
        // live session. Each fixture names a different row.
        for (marker, expected) in [
            ("Justfile", "just test"),
            ("makefile", "make test"),
            ("bun.lock", "bun test"),
            ("deno.jsonc", "deno test"),
        ] {
            let dir = tempfile::tempdir().expect("tempdir");
            std::fs::write(dir.path().join(marker), "x\n").expect(marker);
            assert_eq!(default_verifier_for_workspace(dir.path()).as_deref(), Some(expected), "{marker}");
        }
        // Cross-tier priority: earlier tiers win over later ones.
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("bun.lockb"), "x\n").expect("bun.lockb");
        std::fs::write(dir.path().join("deno.json"), "{}\n").expect("deno.json");
        assert_eq!(default_verifier_for_workspace(dir.path()).as_deref(), Some("deno test"));
        std::fs::write(dir.path().join("go.mod"), "module example\n").expect("go.mod");
        assert_eq!(default_verifier_for_workspace(dir.path()).as_deref(), Some("go test ./..."));
        // The Node tier (script sniffing) sits between Bun and Python.
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("pyproject.toml"), "[tool.pytest]\n").expect("pyproject");
        std::fs::write(dir.path().join("package.json"), r#"{"scripts":{"test":"vitest"}}"#).expect("package.json");
        assert_eq!(default_verifier_for_workspace(dir.path()).as_deref(), Some("npm test"));
        // Unrecognized scripts keep the `npm test` fallback.
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("package.json"), r#"{"scripts":{"start":"serve"}}"#).expect("package.json");
        assert_eq!(default_verifier_for_workspace(dir.path()).as_deref(), Some("npm test"));
    }

    #[test]
    fn default_verifier_reads_npm_script_fallbacks() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("package.json"), r#"{"scripts":{"lint":"eslint ."}}"#).expect("package.json");
        assert_eq!(default_verifier_for_workspace(dir.path()).as_deref(), Some("npm run lint"));
    }

    #[test]
    fn verification_recovery_directive_names_concrete_command() {
        let directive = verification_recovery_directive(Some("go test ./..."), 1, 2);
        assert!(directive.contains("go test ./..."));
        assert!(directive.contains("1/2"));
        assert!(directive.contains("edits remain available"));
        assert!(directive.contains("only missing checks"));
        assert!(!directive.contains("mutations are blocked"));
        assert!(directive.contains("max_output_tokens"));
        assert!(directive.contains(VERIFIER_SHELL_FORM_NOTE));
        assert!(directive.contains("tails run standalone"));
        assert!(directive.contains("fail-closed `pipefail`"));
        let fallback = verification_recovery_directive(None, 2, 2);
        assert!(fallback.contains(GENERIC_VERIFIER_DESCRIPTION));
        assert!(fallback.contains("2/2"));
    }

    #[test]
    fn verifier_reference_names_resolved_command_or_generic_description() {
        assert_eq!(verifier_reference(Some("go test ./...")), "`go test ./...`");
        assert_eq!(verifier_reference(Some("  npm test ")), "`npm test`");
        for missing in [None, Some(""), Some("   ")] {
            let reference = verifier_reference(missing);
            assert_eq!(reference, GENERIC_VERIFIER_DESCRIPTION);
            assert!(!reference.starts_with('`'), "no single command is named: {reference}");
            for ecosystem in ["cargo check --locked", "go test ./...", "npm test", "pytest -q"] {
                assert!(reference.contains(ecosystem), "missing {ecosystem}: {reference}");
            }
        }
    }

    #[test]
    fn truncation_only_verifier_rewrites_to_standalone_prefix() {
        for (command, expected) in [
            ("cargo check --locked 2>&1 | head -c 4000", "cargo check --locked 2>&1"),
            ("cargo check --locked -p vtcode 2>&1 | tail -20", "cargo check --locked -p vtcode 2>&1"),
            ("cargo nextest run 2>&1 | tail -15", "cargo nextest run 2>&1"),
            (
                "cargo check --locked && cargo nextest run --locked -p vtcode | tail -5",
                "cargo check --locked && cargo nextest run --locked -p vtcode",
            ),
            ("cargo check | tail -5 | head -20", "cargo check"),
            ("RUSTFLAGS=-Dwarnings cargo check | head -20", "RUSTFLAGS=-Dwarnings cargo check"),
            // File-output redirects are preserved verbatim in the head slice
            // (only the status-masking pipe tail is elided), and `>` does not
            // mask the exit status — so the rewrite stays truthful here too.
            ("cargo check > build.log | tail -5", "cargo check > build.log"),
        ] {
            assert_eq!(
                rewrite_truncation_only_verifier(&exec_command(command)).as_deref(),
                Some(expected),
                "expected rewrite: {command}"
            );
        }
    }

    #[test]
    fn non_truncation_pipelines_and_mutations_are_never_rewritten() {
        // Filtering tails do real work; dropping them would discard evidence.
        // Smuggled mutations, joins, redirects, and array forms must run (or
        // be rejected) exactly as typed — the rewrite must stay silent.
        for command in [
            "cargo check | grep error",
            "cargo check | wc -l",
            "cargo check | sort | uniq",
            "cargo check && rm -rf target | tail -5",
            "cargo check; git status | tail -5",
            "cargo check || cargo test | tail -5",
            "cargo check | tail -5; rm foo.txt",
            "cargo check | tail &",
            "cargo check",
            "echo done | tail -5",
            "rg -n 'pattern' src | head -20",
            "echo $(date) | tail -5",
        ] {
            assert_eq!(rewrite_truncation_only_verifier(&exec_command(command)), None, "must not rewrite: {command}");
        }
        assert_eq!(
            rewrite_truncation_only_verifier(&serde_json::json!({"command": ["cargo", "check", "|", "tail"]})),
            None,
            "array-form commands keep today's behavior"
        );
        assert_eq!(rewrite_truncation_only_verifier(&serde_json::json!({})), None);
    }

    #[test]
    fn args_as_executed_classify_elided_truncation_verifiers_as_verification() {
        for command in [
            "cargo check --locked 2>&1 | tail -5",
            "cargo nextest run 2>&1 | head -c 4000",
        ] {
            let typed = exec_command(command);
            let executed = shell_args_as_executed(tools::EXEC_COMMAND, &typed);
            assert!(matches!(executed, std::borrow::Cow::Owned(_)), "expected rewrite: {command}");
            assert_eq!(
                classify_shell_activity(tools::EXEC_COMMAND, &executed),
                ShellActivity::Verification,
                "{command}"
            );
            // Idempotent: the executed form runs unchanged on a second pass.
            assert!(matches!(shell_args_as_executed(tools::EXEC_COMMAND, &executed), std::borrow::Cow::Borrowed(_)));
        }

        let unified = json!({"action": "run", "command": "cargo check 2>&1 | tail -5"});
        assert_eq!(shell_args_as_executed(tools::UNIFIED_EXEC, &unified)["command"], "cargo check 2>&1");
    }

    #[test]
    fn verifier_rewrite_keeps_raw_alias_and_appended_arguments_in_sync() {
        let command = "cargo check | grep";
        let typed = json!({"cmd":command, "raw_command":command, "args":["warning"]});
        let executed = shell_args_as_executed(tools::EXEC_COMMAND, &typed);
        let expected = "set -o pipefail && cargo check | grep warning";
        assert_eq!(executed["command"], expected);
        assert_eq!(executed["raw_command"], expected);
        assert!(executed.get("args").is_none(), "suffix must execute exactly once");
        assert_eq!(classify_shell_activity(tools::EXEC_COMMAND, &executed), ShellActivity::Verification);
        assert!(matches!(shell_args_as_executed(tools::EXEC_COMMAND, &executed), std::borrow::Cow::Borrowed(_)));

        let truncator = json!({"cmd":"cargo check | head", "args":["-20"]});
        assert_eq!(shell_args_as_executed(tools::EXEC_COMMAND, &truncator)["command"], "cargo check");
    }

    #[test]
    fn verifier_pipeline_rejects_conflicting_raw_alias_and_mutating_suffix() {
        for typed in [
            json!({"cmd":"cargo check | grep marker", "raw_command":"printf marker"}),
            json!({"cmd":"cargo check | sort", "args":["-o", "changed.txt"]}),
            json!({"cmd":"set -o pipefail && cargo check | sort", "args":["-o", "changed.txt"]}),
            json!({"cmd":"cargo check | grep", "args":"invalid"}),
        ] {
            assert_eq!(rewrite_verifier_pipeline(&typed), None, "{typed}");
            assert!(!shell_command_is_admitted_verification_attempt(&typed), "{typed}");
            assert_eq!(classify_shell_activity(tools::EXEC_COMMAND, &typed), ShellActivity::Mutation, "{typed}");
            assert!(super::super::classify_tool_intent(tools::EXEC_COMMAND, &typed).mutating, "{typed}");
        }
    }

    #[test]
    fn filtered_verifier_pipeline_preserves_filters_and_truthful_status() {
        for command in [
            "python3 scripts/check_markdown.py 2>&1 | grep -E '^(README|docs/SECURITY)\\.md' | head -20",
            "cargo check --locked | grep warning",
            "cargo fmt --check && cargo check --locked | wc -l",
        ] {
            let typed = exec_command(command);
            let executed = shell_args_as_executed(tools::EXEC_COMMAND, &typed);
            assert_eq!(executed["command"], format!("set -o pipefail && {command}"));
            assert_eq!(classify_shell_activity(tools::EXEC_COMMAND, &executed), ShellActivity::Verification);
            assert!(shell_command_is_admitted_verification_attempt(&executed));
            assert!(matches!(shell_args_as_executed(tools::EXEC_COMMAND, &executed), std::borrow::Cow::Borrowed(_)));
            assert_eq!(classify_shell_activity(tools::EXEC_COMMAND, &typed), ShellActivity::Mutation);
        }
        for command in [
            "cargo check | grep error; true",
            "cargo check | grep error || true",
            "cargo check | tee source.rs",
            "cargo check | sed -i 's/old/new/' source.rs",
            "cargo check | grep $(touch marker)",
            "cargo check | grep error > hidden.log",
            "cargo check | grep error &",
            "cargo check && rm source.rs | grep error",
        ] {
            assert_eq!(rewrite_verifier_pipeline(&exec_command(command)), None, "{command}");
            let prefixed = exec_command(&format!("set -o pipefail && {command}"));
            assert_ne!(
                classify_shell_activity(tools::EXEC_COMMAND, &prefixed),
                ShellActivity::Verification,
                "{command}"
            );
        }
    }

    #[test]
    fn args_as_executed_keep_unsafe_joins_and_non_run_calls_as_typed() {
        for command in [
            "cargo check 2>&1 | grep error; true",
            "cargo check; git status",
            "cargo check || true",
        ] {
            let typed = exec_command(command);
            let executed = shell_args_as_executed(tools::EXEC_COMMAND, &typed);
            assert!(matches!(executed, std::borrow::Cow::Borrowed(_)), "must run as typed: {command}");
            assert_ne!(
                classify_shell_activity(tools::EXEC_COMMAND, &executed),
                ShellActivity::Verification,
                "{command}"
            );
        }
        let poll = json!({"action": "poll", "session_id": "s1", "command": "cargo check | tail -5"});
        assert!(matches!(shell_args_as_executed(tools::UNIFIED_EXEC, &poll), std::borrow::Cow::Borrowed(_)));
        let read = json!({"path": "src/lib.rs"});
        assert!(matches!(shell_args_as_executed(tools::READ_FILE, &read), std::borrow::Cow::Borrowed(_)));
    }

    #[test]
    fn path_glob_inspections_preserve_debt_without_weakening_admission() {
        let clean = json!({"exit_code":0});
        for command in [
            "rg -n \"MD013|line_length\" docs/development/*.md | head",
            "rg -n x ./docs/file?.md | head -20",
            "grep -n x docs/*.md",
            "cat ./docs/*.md",
            "ls docs/*.md",
        ] {
            assert!(!exec_mutation_should_incur_debt(command, &clean), "{command}");
            assert_eq!(classify_shell_activity(tools::EXEC_COMMAND, &exec_command(command)), ShellActivity::Mutation);
            assert!(exec_mutation_should_incur_debt(command, &json!({"exit_code":0,"modified_files":["src/lib.rs"]})));
        }
        for command in [
            "rg -n x *",
            "rg -n x ./docs/[a-z]*.md",
            "rg -n x -docs/*.md",
            "rg -n x $dir/*.md",
            "rg --pre touch docs/*.md",
            "rg --hostname-bin=touch docs/*.md",
            "sed docs/*.md",
            "sort -o marker docs/*.md",
            "./tools/* --version",
            "rg x docs/$(touch marker)/*.md",
            "rg x docs/*.md > marker",
            "rg x docs/*.md; touch marker",
            "rg x docs/*.md; if",
            "rg x docs*/*.md",
        ] {
            assert!(exec_mutation_should_incur_debt(command, &clean), "{command}");
        }
    }

    #[test]
    fn mutation_accounting_rejects_unproven_shell_forms() {
        let output = json!({"exit_code": 0});
        for command in [
            "echo \"$(touch marker)\"",
            "echo \"`touch marker`\"",
            "for p in $(echo $(touch marker)); do test -f \"$p\"; done",
            "for p in a; do sed -i '' 's/a/b/' README.md; done",
            "for p in a; do sort -o README.md README.md; done",
            "echo x 2>/dev/null.backup",
            "echo x 2>&1>marker",
            "cat <(touch marker)",
            "$program --version",
            "\"$(echo rm)\" marker",
            "grep $options README.md",
            "if true; then touch marker; fi",
            "for p in a; do echo x",
            "echo $(",
            "echo hi &",
            "sed -n '1p' README.md > marker",
            "echo ${x:=value}",
        ] {
            assert!(exec_mutation_should_incur_debt(command, &output), "{command}");
        }
        for command in [
            "echo '>'",
            "echo '2>/dev/null.backup'",
            "echo \"$(grep -n x README.md)\"",
        ] {
            assert!(!exec_mutation_should_incur_debt(command, &output), "{command}");
        }
    }

    #[test]
    fn dynamic_readonly_inspections_do_not_incur_mutation_debt() {
        let clean_output = json!({"success": true, "exit_code": 0});
        // Session-vtcode-20261015T142625Z: `for` loops over grep results with
        // `test -f` exited 0 with no writes, bumped the completion generation,
        // invalidated tracker evidence, and looped harness auto-verification
        // on `rg` inspections instead of ending the turn. Admission stays
        // fail-closed (still Mutation, still blocked when pending); accounting
        // must not create new debt.
        for command in [
            "for p in $(grep -o '](\\./[^)#]*' README.md | sed 's/](//' | sort -u); do test -f \"$p\" || echo \"missing: $p\"; done; echo \"check done\"",
            "for p in $(grep -o '](\\./[^)#]*' README.md | sed 's/](\\.\\///' | sort -u); do test -f \"docs/${p#docs/}\" 2>/dev/null || echo \"missing: $p\"; done",
            "rg -n 'vinhnx/(VTCode|vtcode)' README.md",
            "grep -n '^## ' README.md",
            "ls docs/guides/zed-acp.md",
            "git diff --stat -- README.md",
            "git status --short",
            "npx markdownlint-cli2 README.md",
            "cargo check --locked",
        ] {
            let expected = if command.starts_with("npx markdownlint") || command.starts_with("cargo check") {
                ShellActivity::Verification
            } else if command.starts_with("for") {
                ShellActivity::Mutation
            } else {
                ShellActivity::Inspection
            };
            assert_eq!(classify_shell_activity(tools::EXEC_COMMAND, &exec_command(command)), expected, "{command}");
            assert!(!exec_mutation_should_incur_debt(command, &clean_output), "{command}");
        }
        // Dynamic `for`/`test` inspections: no debt without writes.
        assert!(!exec_mutation_should_incur_debt(
            "for p in $(grep -o 'x' README.md); do test -f \"$p\" || echo \"missing: $p\"; done",
            &clean_output,
        ));
        assert!(!exec_mutation_should_incur_debt("rg -n 'x' README.md", &clean_output));
        assert!(!exec_mutation_should_incur_debt("grep -n 'x' README.md", &clean_output));
        assert!(!exec_mutation_should_incur_debt("git status --short", &clean_output));
        // Concrete evidence still counts.
        assert!(exec_mutation_should_incur_debt(
            "for p in $(grep -o 'x' README.md); do test -f \"$p\" || echo \"missing: $p\"; done",
            &json!({"success": true, "exit_code": 0, "modified_files": ["README.md"]}),
        ));
        // Shell redirection is a potential write.
        assert!(exec_mutation_should_incur_debt("echo hi > README.md", &clean_output));
        assert!(exec_mutation_should_incur_debt("for f in a b; do echo x > \"$f\"; done", &clean_output));
        // Static mutating base still counts (fail-closed for `rm`, etc.).
        assert!(exec_mutation_should_incur_debt("rm -rf target", &clean_output));
        // Dynamic mutating base with variable still counts.
        assert!(exec_mutation_should_incur_debt("rm $f", &clean_output));
        // Stderr plumbing alone is not a file write.
        assert!(!exec_mutation_should_incur_debt("rg -n 'x' README.md 2>&1", &clean_output));
        // Shared classifiers decide verifier-shaped commands: mutating
        // variants count, clean checks do not. Asymmetric pairs.
        assert!(exec_mutation_should_incur_debt("npx markdownlint-cli2 --fix README.md", &clean_output));
        assert!(!exec_mutation_should_incur_debt("npx markdownlint-cli2 README.md", &clean_output));
        assert!(exec_mutation_should_incur_debt("cargo fmt", &clean_output));
        assert!(!exec_mutation_should_incur_debt("cargo fmt --check", &clean_output));
        assert!(exec_mutation_should_incur_debt("tsc", &clean_output));
        assert!(!exec_mutation_should_incur_debt("tsc --noEmit", &clean_output));
        // Unresolved options stay unproven even for inspection subcommands.
        assert!(exec_mutation_should_incur_debt("git diff $x", &clean_output));
        assert!(exec_mutation_should_incur_debt("git checkout $x", &clean_output));
        // Dynamic chains of inspections with exit-echo tails stay debt-free.
        assert!(!exec_mutation_should_incur_debt("grep -n 'x' README.md; echo \"exit:$?\"", &clean_output,));
    }
}
