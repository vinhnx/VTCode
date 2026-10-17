use crate::audit::PermissionDecision;
use crate::config::CommandsConfig;
use crate::tools::command_cache::PermissionCache;
use crate::tools::command_resolver::CommandResolver;
use regex::Regex;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tracing::warn;

#[derive(Clone)]
pub struct CommandPolicyEvaluator {
    allow_prefixes: Vec<String>,
    deny_prefixes: Vec<String>,
    allow_regexes: Vec<Regex>,
    deny_regexes: Vec<Regex>,
    allow_glob_regexes: Vec<Regex>,
    deny_glob_regexes: Vec<Regex>,
    allow_regexes_empty: bool,
    allow_globs_empty: bool,
    // NEW: Command resolution and caching for improved security visibility
    resolver: Arc<Mutex<CommandResolver>>,
    cache: Arc<Mutex<PermissionCache>>,
}

/// Verdict of one per-segment policy evaluation.
///
/// `reason` names the rule family that decided, using static strings: the
/// command itself is never cloned into it (it is already the cache key and
/// travels as a separate audit field).
struct PolicyVerdict {
    allowed: bool,
    reason: &'static str,
}

impl CommandPolicyEvaluator {
    pub fn from_config(config: &CommandsConfig) -> Self {
        let allow_prefixes = crate::utils::merge_env_patterns(&config.allow_list, "VTCODE_COMMANDS_ALLOW_LIST");
        // Deny prefix entries are matched against policy segments whose
        // leading `KEY=value` / `env` words are already stripped, so an entry
        // authored with an assignment prefix (`FOO=1 rm`) would never match.
        // Normalize the entry the same way; this is fail-safe because it can
        // only widen deny matching, never narrow it.
        let deny_prefixes = crate::utils::merge_env_patterns(&config.deny_list, "VTCODE_COMMANDS_DENY_LIST")
            .into_iter()
            .map(|pattern| {
                let words: Vec<String> = pattern.split_whitespace().map(str::to_string).collect();
                crate::tools::command_args::command_words_after_environment_prefix(&words).join(" ")
            })
            .collect::<Vec<_>>();

        let allow_regex_patterns = crate::utils::merge_env_patterns(&config.allow_regex, "VTCODE_COMMANDS_ALLOW_REGEX");
        let deny_regex_patterns = crate::utils::merge_env_patterns(&config.deny_regex, "VTCODE_COMMANDS_DENY_REGEX");

        let allow_glob_patterns = crate::utils::merge_env_patterns(&config.allow_glob, "VTCODE_COMMANDS_ALLOW_GLOB");
        let deny_glob_patterns = crate::utils::merge_env_patterns(&config.deny_glob, "VTCODE_COMMANDS_DENY_GLOB");

        let allow_regexes = compile_regexes(&allow_regex_patterns);
        let deny_regexes = compile_regexes(&deny_regex_patterns);
        let allow_glob_regexes = compile_globs(&allow_glob_patterns);
        let deny_glob_regexes = compile_globs(&deny_glob_patterns);

        Self {
            allow_prefixes,
            deny_prefixes,
            allow_regexes,
            deny_regexes,
            allow_glob_regexes,
            deny_glob_regexes,
            allow_regexes_empty: allow_regex_patterns.is_empty(),
            allow_globs_empty: allow_glob_patterns.is_empty(),
            resolver: Arc::new(Mutex::new(CommandResolver::new())),
            cache: Arc::new(Mutex::new(PermissionCache::new())),
        }
    }

    fn cached_decision(&self, command_text: &str) -> Option<bool> {
        self.cache
            .lock()
            .unwrap_or_else(|poisoned| {
                warn!("command_policy: permission cache mutex poisoned; recovering");
                poisoned.into_inner()
            })
            .get(command_text)
    }

    fn resolve_path(&self, command_text: &str) -> Option<PathBuf> {
        self.resolver
            .lock()
            .unwrap_or_else(|poisoned| {
                warn!("command_policy: command resolver mutex poisoned; recovering");
                poisoned.into_inner()
            })
            .resolve(command_text)
            .resolved_path
            .clone()
    }

    fn cache_decision(&self, command_text: &str, allowed: bool, reason: &str) {
        let mut cache = self.cache.lock().unwrap_or_else(|poisoned| {
            warn!("command_policy: permission cache mutex poisoned; recovering");
            poisoned.into_inner()
        });
        cache.put(command_text, allowed, reason);
    }

    pub fn allows(&self, command: &[String]) -> bool {
        if command.is_empty() {
            return false;
        }
        // Preserve argv boundaries when checking a direct executable. Joining
        // raw arguments would let punctuation inside an argument (for example
        // Python's `;` or a quoted string) look like shell operators. Explicit
        // shell wrappers are handled separately so their script still receives
        // segment-level validation.
        let command_text =
            shell_script_from_argv(command).unwrap_or_else(|| shell_words::join(command.iter().map(String::as_str)));
        self.allows_text(&command_text)
    }

    pub fn allows_text(&self, command_text: &str) -> bool {
        let cmd = command_text.trim();
        if cmd.is_empty() {
            return false;
        }

        self.evaluate_policy(cmd).allowed
    }

    /// Evaluate deny/allow rules once per shell segment.
    ///
    /// Single source of truth for [`Self::allows_text`] and
    /// [`Self::evaluate_with_resolution`], so the reported reason can never
    /// disagree with the verdict. (The old resolution path re-matched the
    /// whole command string for its reason while the verdict matched per
    /// segment, so a denied stage in `cargo check | rm x` was reported as a
    /// regex match instead of the deny-list hit.)
    fn evaluate_policy(&self, cmd: &str) -> PolicyVerdict {
        let segments = policy_segments(cmd);

        // Deny takes precedence. Every shell segment is evaluated so chained,
        // piped, or list-joined commands cannot ride on an earlier match. The
        // first denying segment names its family; priority matches the
        // historical reason order (prefix, glob, regex).
        for segment in &segments {
            if self.matches_prefix(segment, &self.deny_prefixes) {
                return PolicyVerdict { allowed: false, reason: "deny_list match" };
            }
            if Self::matches_any(&self.deny_glob_regexes, segment) {
                return PolicyVerdict { allowed: false, reason: "deny_glob match" };
            }
            if Self::matches_any(&self.deny_regexes, segment) {
                return PolicyVerdict { allowed: false, reason: "deny_regex match" };
            }
        }

        // If no allow rules defined, allow by default
        if self.allow_prefixes.is_empty() && self.allow_regexes_empty && self.allow_globs_empty {
            return PolicyVerdict {
                allowed: true,
                reason: "default allow (no allow rules)",
            };
        }

        // Check allow rules: each segment must independently match
        // The harness's exact, statically validated pipefail setup only makes
        // status stricter. It needs no executable grant; every pipeline stage
        // still needs its own grant, and deny rules above include the setup.
        let verifier_setup = crate::tools::tool_intent::activity::pipefail_verifier_pipeline(cmd).is_some();
        let allowed = segments.iter().enumerate().all(|(index, segment)| {
            (index == 0 && verifier_setup && segment == "set -o pipefail")
                || self.matches_prefix(segment, &self.allow_prefixes)
                || Self::matches_any(&self.allow_regexes, segment)
                || Self::matches_any(&self.allow_glob_regexes, segment)
        });
        if !allowed {
            return PolicyVerdict { allowed: false, reason: "no allow rule matched" };
        }

        // Name the first real stage's family as the representative reason (the
        // pipefail setup line itself carries no grant). Single-stage commands,
        // the common case, are exact.
        let representative = segments
            .iter()
            .find(|segment| *segment != "set -o pipefail")
            .map(String::as_str)
            .unwrap_or("");
        if self.matches_prefix(representative, &self.allow_prefixes) {
            PolicyVerdict { allowed: true, reason: "allow_list match" }
        } else if Self::matches_any(&self.allow_glob_regexes, representative) {
            PolicyVerdict { allowed: true, reason: "allow_glob match" }
        } else {
            PolicyVerdict { allowed: true, reason: "allow_regex match" }
        }
    }

    /// Enhanced async evaluation with command resolution and caching
    /// Returns (allowed, resolved_path, reason, decision)
    pub fn evaluate_with_resolution(&self, command_text: &str) -> (bool, Option<PathBuf>, String, PermissionDecision) {
        let cmd = command_text.trim();

        // Check cache first
        if let Some(allowed) = self.cached_decision(cmd) {
            let reason = if allowed {
                "Cached allow decision"
            } else {
                "Cached deny decision"
            };
            return (allowed, None, reason.to_string(), PermissionDecision::Cached);
        }

        // Resolve command to actual path
        let resolved_path = self.resolve_path(cmd);

        // Verdict and reason come from the same per-segment evaluation, so
        // they cannot disagree and no second whole-command regex pass runs.
        let verdict = self.evaluate_policy(cmd);
        let reason = verdict.reason.to_string();

        // Cache the result (the cache stores only the verdict; the reason is
        // the return value, so no command text is cloned into it).
        self.cache_decision(cmd, verdict.allowed, &reason);

        let decision = if verdict.allowed {
            PermissionDecision::Allowed
        } else {
            PermissionDecision::Denied
        };

        (verdict.allowed, resolved_path, reason, decision)
    }

    fn matches_prefix(&self, value: &str, prefixes: &[String]) -> bool {
        prefixes
            .iter()
            .filter(|pattern| !pattern.is_empty())
            .any(|pattern| value.starts_with(pattern))
    }
    fn matches_any(regexes: &[Regex], value: &str) -> bool {
        regexes.iter().any(|re| re.is_match(value))
    }
}

/// Return the script carried by an explicit shell invocation.
///
/// The exec tool accepts both a regular argv request (`["printf", "ok"]`) and
/// an already-constructed shell request (`["/bin/sh", "-lc", "printf ok"]`).
/// Command policy must inspect the script in the latter form; matching the
/// wrapper itself would reject safe commands and could also hide a denied
/// command behind the shell name.
fn shell_script_from_argv(command: &[String]) -> Option<String> {
    let program = command.first()?;
    let basename = Path::new(program).file_name()?.to_str()?.to_ascii_lowercase();
    let shell_kind = shell_invocation_kind(basename.as_str())?;

    let flag_index = command.iter().enumerate().skip(1).find_map(|(index, argument)| {
        let is_command_flag = match shell_kind {
            ShellInvocationKind::Posix => {
                argument == "-c"
                    || (argument.starts_with('-')
                        && !argument.starts_with("--")
                        && argument.as_bytes().get(1..).is_some_and(|flags| flags.contains(&b'c')))
            }
            ShellInvocationKind::Cmd => argument.eq_ignore_ascii_case("/c") || argument.eq_ignore_ascii_case("/k"),
            ShellInvocationKind::PowerShell => {
                matches!(argument.to_ascii_lowercase().as_str(), "-command" | "-c" | "-encodedcommand" | "-e")
            }
        };
        is_command_flag.then_some(index)
    })?;

    command.get(flag_index + 1).cloned()
}

/// Return whether argv explicitly names a supported shell, even when it does
/// not carry a `-c` script. Callers use this to preserve the user's shell
/// boundary while selecting the policy representation.
pub(crate) fn is_shell_argv(command: &[String]) -> bool {
    command
        .first()
        .and_then(|program| Path::new(program).file_name())
        .and_then(|name| name.to_str())
        .is_some_and(|name| shell_invocation_kind(name.to_ascii_lowercase().as_str()).is_some())
}

#[derive(Clone, Copy)]
enum ShellInvocationKind {
    Posix,
    Cmd,
    PowerShell,
}

/// Single source of truth for the shells command policy understands.
///
/// Both [`shell_script_from_argv`] and [`is_shell_argv`] branch on this, so a
/// newly supported shell cannot be added to one path but missed in the other
/// (which would hide a denied command behind the shell name or reject a safe
/// command). Input must already be lowercased.
fn shell_invocation_kind(lowered_basename: &str) -> Option<ShellInvocationKind> {
    match lowered_basename {
        "sh" | "bash" | "zsh" | "dash" | "ksh" => Some(ShellInvocationKind::Posix),
        "cmd" | "cmd.exe" => Some(ShellInvocationKind::Cmd),
        "powershell" | "powershell.exe" | "pwsh" | "pwsh.exe" => Some(ShellInvocationKind::PowerShell),
        _ => None,
    }
}

/// Split a command string into its shell segments so prefix/regex policy
/// rules apply to every chained, piped, or list-joined command rather than
/// only to the first. Falls back to the whole string when the parser cannot
/// handle the syntax, preserving the previous whole-string behavior.
fn policy_segments(command_text: &str) -> Vec<String> {
    match crate::command_safety::shell_parser::parse_shell_commands(command_text) {
        Ok(segments) if !segments.is_empty() => segments
            .into_iter()
            .map(|argv| {
                // The shell parser keeps leading `KEY=value` assignments as
                // words so intent/activity classification can inspect injection
                // keys. For policy matching they are the command's environment,
                // not its executable: strip them so `FOO=bar rg …` still matches
                // the allow rule for `rg` (shared helper, single source of truth).
                crate::tools::command_args::command_words_after_environment_prefix(&argv).join(" ")
            })
            .collect(),
        _ => vec![command_text.to_string()],
    }
}

fn compile_regexes(patterns: &[String]) -> Vec<Regex> {
    patterns
        .iter()
        .filter_map(|pattern| {
            Regex::new(pattern)
                .map_err(|error| {
                    warn!(%error, %pattern, "Ignoring invalid command regex pattern");
                    error
                })
                .ok()
        })
        .collect()
}

fn compile_globs(patterns: &[String]) -> Vec<Regex> {
    patterns
        .iter()
        .filter_map(|pattern| {
            let escaped = regex::escape(pattern);
            let glob_regex = format!("^{}$", escaped.replace(r"\*", ".*").replace(r"\?", "."));
            Regex::new(&glob_regex)
                .map_err(|error| {
                    warn!(%error, pattern = %pattern, "Ignoring invalid command glob pattern");
                    error
                })
                .ok()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::CommandsConfig;

    #[test]
    fn verifier_pipefail_setup_preserves_stage_permissions_and_denials() {
        let config = CommandsConfig {
            allow_list: vec!["cargo check".into(), "grep".into()],
            allow_regex: vec![],
            allow_glob: vec![],
            deny_list: vec![],
            deny_regex: vec![],
            deny_glob: vec![],
            ..Default::default()
        };
        let evaluator = CommandPolicyEvaluator::from_config(&config);
        let valid = "set -o pipefail && cargo check | grep warning";
        assert!(evaluator.allows_text(valid));
        for denied in [
            "set -o pipefail && cargo build | grep warning",
            "set -o pipefail && cargo check | wc -l",
            "set -o pipefail && cargo check | grep warning; true",
            "set +o pipefail && cargo check | grep warning",
            "set -o pipefail && cargo check | grep $(touch marker)",
        ] {
            assert!(!evaluator.allows_text(denied), "{denied}");
        }
        for denied_stage in ["set", "cargo", "grep"] {
            let mut denied = config.clone();
            denied.deny_list.push(denied_stage.into());
            assert!(!CommandPolicyEvaluator::from_config(&denied).allows_text(valid), "{denied_stage}");
        }
    }

    #[test]
    fn glob_allows_cargo_commands() {
        let mut config = CommandsConfig::default();
        config.allow_list.clear();
        config.allow_regex.clear();
        config.allow_glob = vec!["cargo *".to_string()];
        let evaluator = CommandPolicyEvaluator::from_config(&config);
        assert!(evaluator.allows_text("cargo fmt"));
        assert!(evaluator.allows(&["cargo".into(), "check".into()]));
    }

    /// Environment-assignment prefixes are the command's environment, not its
    /// executable: `FOO=bar rg …` / `IFS= read …` must still match the allow
    /// rule for the real program. Regression guard for the shell parser keeping
    /// `KEY=value` words so intent/activity classification can inspect them.
    #[test]
    fn environment_prefix_does_not_block_allow_rules() {
        let prefix = CommandsConfig {
            allow_list: vec!["rg".into(), "read".into(), "printf".into()],
            ..Default::default()
        };
        let prefixed = CommandPolicyEvaluator::from_config(&prefix);
        assert!(prefixed.allows(&["LANG=C".into(), "rg".into(), "foo".into()]));
        assert!(prefixed.allows(&["IFS=".into(), "read".into(), "-r".into(), "line".into()]));
        // An explicit shell wrapper carries the prefix inside its script.
        assert!(prefixed.allows_text("LANG=C rg foo"));

        // A denied program stays denied even behind an assignment prefix.
        let deny = CommandsConfig {
            allow_list: vec!["rg".into()],
            deny_list: vec!["rm".into()],
            ..Default::default()
        };
        let denying = CommandPolicyEvaluator::from_config(&deny);
        assert!(denying.allows(&["LANG=C".into(), "rg".into()]));
        assert!(!denying.allows(&["FOO=1".into(), "rm".into(), "-rf".into(), "/".into()]));
    }

    /// A deny entry authored *with* an assignment prefix (`FOO=1 rm`) must
    /// still match: deny entries are compared against segments whose leading
    /// `KEY=value` words are stripped, so the entry is normalized the same way
    /// at construction. Fail-safe direction: this can only widen deny
    /// matching, never narrow it.
    #[test]
    fn deny_entry_with_env_prefix_matches_stripped_segment() {
        let config = CommandsConfig {
            allow_list: vec!["rg".into()],
            deny_list: vec!["FOO=1 rm".into(), "env rm".into()],
            ..Default::default()
        };
        let evaluator = CommandPolicyEvaluator::from_config(&config);
        // Both prefix forms of the deny entry collapse onto the bare program.
        assert!(!evaluator.allows(&["FOO=1".into(), "rm".into(), "-rf".into(), "/".into()]));
        assert!(!evaluator.allows(&["env".into(), "rm".into(), "x".into()]));
        assert!(!evaluator.allows_text("FOO=1 rm -rf /"));
        // The allow rule for `rg` is unaffected by deny-entry normalization.
        assert!(evaluator.allows(&["rg".into(), "foo".into()]));
        assert!(evaluator.allows(&["LANG=C".into(), "rg".into(), "foo".into()]));
    }

    #[test]
    fn glob_supports_question_mark() {
        let mut config = CommandsConfig::default();
        config.allow_list.clear();
        config.allow_regex.clear();
        config.allow_glob = vec!["go test ./pkg/?".to_string()];
        let evaluator = CommandPolicyEvaluator::from_config(&config);
        assert!(evaluator.allows_text("go test ./pkg/a"));
        assert!(!evaluator.allows_text("go test ./pkg/ab"));
    }

    #[test]
    fn glob_allows_node_ecosystem_commands() {
        let mut config = CommandsConfig::default();
        config.allow_list.clear();
        config.allow_regex.clear();
        config.allow_glob = vec!["npm *".to_string(), "bun *".to_string()];
        let evaluator = CommandPolicyEvaluator::from_config(&config);
        assert!(evaluator.allows_text("npm install"));
        assert!(evaluator.allows_text("npm run build"));
        assert!(evaluator.allows_text("bun install"));
        assert!(evaluator.allows_text("bun run check"));
    }

    #[test]
    fn allow_list_allows_exact_git_and_cargo_commands() {
        let mut config = CommandsConfig::default();
        // Clear default allow_list to reduce noise
        config.allow_list.clear();
        config.allow_list.push("git".to_string());
        config.allow_list.push("cargo".to_string());
        let evaluator = CommandPolicyEvaluator::from_config(&config);
        assert!(evaluator.allows_text("git"));
        assert!(evaluator.allows_text("cargo"));
        assert!(evaluator.allows(&["git".into()]));
        assert!(evaluator.allows(&["cargo".into()]));
    }

    #[test]
    fn shell_wrapper_policy_evaluates_inner_script() {
        let evaluator = CommandPolicyEvaluator::from_config(&CommandsConfig::default());
        assert!(evaluator.allows(&["/bin/sh".into(), "-lc".into(), "printf vtcode-terminal".into(),]));
        assert!(!evaluator.allows(&["/bin/sh".into(), "-lc".into(), "rm -rf /".into(),]));
    }

    #[test]
    fn shell_argv_and_script_extraction_share_one_shell_table() {
        // Every supported shell is recognized with and without a `-c` script;
        // near-miss names (`shim`) and regular programs are not shells.
        for shell in [
            "sh",
            "bash",
            "zsh",
            "dash",
            "ksh",
            "cmd",
            "cmd.exe",
            "powershell",
            "powershell.exe",
            "pwsh",
            "pwsh.exe",
        ] {
            assert!(is_shell_argv(&[shell.to_string()]), "{shell}");
            assert!(is_shell_argv(&[format!("/bin/{shell}")]), "/bin/{shell} should resolve by basename");
            assert!(is_shell_argv(&[shell.to_ascii_uppercase()]), "{shell} matches case-insensitively");
        }
        for program in ["rg", "git", "shim", "bashful", ""] {
            assert!(!is_shell_argv(&[program.to_string()]), "{program} is not a shell");
        }
        // A bare shell names the boundary without carrying a script.
        assert!(is_shell_argv(&["bash".to_string()]));
        assert_eq!(shell_script_from_argv(&["bash".to_string()]), None);
        assert_eq!(
            shell_script_from_argv(&["/bin/bash".to_string(), "-lc".to_string(), "echo hi".to_string()]),
            Some("echo hi".to_string())
        );
        assert_eq!(shell_script_from_argv(&["rg".to_string(), "foo".to_string()]), None);
    }

    fn policy_config_with(allow: &[&str], deny: &[&str]) -> CommandsConfig {
        CommandsConfig {
            allow_list: allow.iter().map(|rule| (*rule).to_string()).collect(),
            deny_list: deny.iter().map(|rule| (*rule).to_string()).collect(),
            ..Default::default()
        }
    }

    #[test]
    fn evaluate_with_resolution_agrees_with_allows_text() {
        let evaluator =
            CommandPolicyEvaluator::from_config(&policy_config_with(&["cargo check", "cargo", "grep"], &["rm"]));
        for command in [
            "cargo check",
            "rm -rf /",
            "cargo check | grep warning",
            "cargo check | rm -rf /",
            "LANG=C cargo check",
            "set -o pipefail && cargo check | grep warning",
            "true",
        ] {
            let expected = evaluator.allows_text(command);
            let (allowed, _, _, decision) = evaluator.evaluate_with_resolution(command);
            assert_eq!(allowed, expected, "{command}");
            assert!(matches!(decision, PermissionDecision::Allowed | PermissionDecision::Denied), "{command}");
            // The verdict is cached: a repeat reports the same outcome as a
            // cache hit without re-evaluating.
            let (cached_allowed, _, _, cached) = evaluator.evaluate_with_resolution(command);
            assert_eq!(cached_allowed, expected, "{command}");
            assert!(matches!(cached, PermissionDecision::Cached), "{command}");
        }
    }

    #[test]
    fn evaluate_with_resolution_names_the_denying_stage() {
        let evaluator =
            CommandPolicyEvaluator::from_config(&policy_config_with(&["cargo check", "cargo", "grep"], &["rm"]));
        // The denied stage is `rm`, even though the whole command starts with
        // an allowed program: the old whole-string reason reported this as a
        // regex match instead of the deny-list hit.
        let (allowed, _, reason, _) = evaluator.evaluate_with_resolution("cargo check | rm -rf /");
        assert!(!allowed);
        assert_eq!(reason, "deny_list match");
        assert!(!reason.contains("cargo check"), "reason must not clone the command");
    }

    #[test]
    fn evaluate_with_resolution_reasons_use_static_families() {
        let prefix = CommandPolicyEvaluator::from_config(&policy_config_with(&["cargo check"], &["rm"]));
        let (_, _, reason, _) = prefix.evaluate_with_resolution("cargo check");
        assert_eq!(reason, "allow_list match");
        let (_, _, reason, _) = prefix.evaluate_with_resolution("rm -rf /");
        assert_eq!(reason, "deny_list match");

        let mut globbed = CommandsConfig::default();
        globbed.allow_list.clear();
        globbed.allow_regex.clear();
        globbed.allow_glob = vec!["cargo *".to_string()];
        let globbed = CommandPolicyEvaluator::from_config(&globbed);
        let (allowed, _, reason, _) = globbed.evaluate_with_resolution("cargo fmt");
        assert!(allowed);
        assert_eq!(reason, "allow_glob match");

        let mut regexed = CommandsConfig::default();
        regexed.allow_list.clear();
        regexed.allow_glob.clear();
        regexed.allow_regex = vec!["^cargo\\s+fmt".to_string()];
        let regexed = CommandPolicyEvaluator::from_config(&regexed);
        let (allowed, _, reason, _) = regexed.evaluate_with_resolution("cargo fmt");
        assert!(allowed);
        assert_eq!(reason, "allow_regex match");

        let mut defaulted = CommandsConfig::default();
        defaulted.allow_list.clear();
        defaulted.allow_regex.clear();
        defaulted.allow_glob.clear();
        let defaulted = CommandPolicyEvaluator::from_config(&defaulted);
        let (allowed, _, reason, _) = defaulted.evaluate_with_resolution("anything goes");
        assert!(allowed);
        assert_eq!(reason, "default allow (no allow rules)");
    }
}
