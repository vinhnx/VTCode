use super::*;

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

pub(super) fn is_verification_invocation(words: &[String]) -> bool {
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

pub(super) fn contains_verification_invocation(command: &str) -> bool {
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
    super::super::readonly::parse_shell_command_words(&command)
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

pub(super) fn has_logical_sequencing(words: &[String]) -> bool {
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
pub(super) fn rewrite_verifier_pipeline(args: &Value) -> Option<String> {
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
