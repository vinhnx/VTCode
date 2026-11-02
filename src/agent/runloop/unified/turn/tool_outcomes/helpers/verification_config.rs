//! Verification-gate configuration accessors and gate constants.

use super::*;

pub(crate) const BLIND_EDITING_THRESHOLD: usize = 6;
/// Ends with the text of [`VERIFIER_SHELL_FORM_NOTE`] so the shell forms it
/// describes match what the execution kernel elides; a test keeps the two in
/// lockstep because `concat!` cannot splice a cross-crate const.
pub(crate) const ANTI_BLIND_EDITING_DIRECTIVE: &str = "Verification is required before successful completion. Continue implementation and repairs; then run only missing checks. Run your project's build/test/lint tool with `exec_command` (e.g. `cargo check`, `go test`, `npm test`, or `pytest`), standalone or as a pure `&&` chain. Cap output with `max_output_tokens`. Pure `head`/`tail` verifier tails run standalone; static read-only filtering pipelines run with fail-closed `pipefail`. Only terminal exit 0 clears the gate; `;`, `||`, dynamic syntax, and mutating tails do not qualify.";
/// Warning rendered when a pending gate's verifier result was lost (the exec
/// session ended before the verifier's output was captured).
pub(crate) const VERIFICATION_RESULT_LOST_WARNING: &str =
    "[!] Verification result lost: the exec session ended before the verifier's output was captured.";
/// Model-facing directive paired with [`VERIFICATION_RESULT_LOST_WARNING`]:
/// a successful verifier re-run is required to clear the pending gate.
pub(crate) const VERIFICATION_RESULT_LOST_DIRECTIVE: &str = "Verification result lost: the exec session ended before the verifier's output was captured. Re-run the verification command standalone or as a pure `&&` chain to confirm or reject the recent edits.";
/// Once-per-turn warning for a completed checker in an unverified shell form,
/// regardless of gate state. Pure truncator and safe static filtering shapes
/// never land here: they execute standalone or with pipefail. The tracker
/// classifies the command as executed
/// ([`vtcode_core::tools::tool_intent::shell_args_as_executed`]).
pub(crate) const PIPED_VERIFICATION_WARNING: &str =
    "[!] Verification was not recorded: the shell sequence cannot establish the verifier's exit status.";
/// Model-facing directive paired with [`PIPED_VERIFICATION_WARNING`]: without
/// this feedback a masked status reads as "verified" to the model, even in
/// docs-only work where no verification gate is pending.
pub(crate) const PIPED_VERIFICATION_DIRECTIVE: &str = "Verification was not recorded because the shell sequence cannot establish verifier success. Re-run the checker standalone or as a pure `&&` chain of verifiers. Use `max_output_tokens` and the structured exit code; do not append `; echo $?`. This notice does not grant repair edits or change the verification gate.";
/// Legacy defaults retained for configuration parsing; runtime recovery is capped at one pass.
pub(crate) const MAX_VERIFICATION_AUTO_RECOVERY_ATTEMPTS: u8 = 2;
/// Cross-turn counterpart to [`MAX_VERIFICATION_AUTO_RECOVERY_ATTEMPTS`]:
/// how many additional autonomous turns the session loop may schedule after
/// a verification-blocked turn before requiring manual `continue`.
pub(crate) const MAX_VERIFICATION_AUTO_RECOVERY_TURNS: u8 = 2;
/// Consecutive failed harness auto-verifications before the harness stops
/// executing verifiers itself and escalates to a manual blocked handoff
/// carrying the failure log. Reset by any success, completed turn, or fresh
/// user input, so only a genuinely stuck suite trips it.
pub(crate) const MAX_VERIFICATION_CONSECUTIVE_FAILURES: u8 = 3;
/// Bound (in chars) for the harness auto-verification failure excerpt kept
/// for the escalated blocked handoff. The full tool output stays in history
/// and the spool; the handoff carries only the tail needed to triage.
pub(crate) const VERIFICATION_FAILURE_EXCERPT_CHARS: usize = 2000;
/// Tool-call id for the harness-synthesized verifier execution. Fixed (not
/// model-issued) so transcripts and history unambiguously attribute the call
/// to autonomous recovery rather than the model.
pub(crate) const HARNESS_AUTO_VERIFY_CALL_ID: &str = "harness-auto-verify";

/// Effective in-turn directive-retry budget, honoring
/// `[agent.harness.verification].in_turn_attempts` with the compiled constant
/// as fallback when no workspace config is present (tests, headless paths).
pub(crate) fn verification_in_turn_attempts(vt_cfg: Option<&vtcode_core::config::loader::VTCodeConfig>) -> u8 {
    vt_cfg
        .map(|cfg| cfg.agent.harness.verification.in_turn_attempts)
        .unwrap_or(MAX_VERIFICATION_AUTO_RECOVERY_ATTEMPTS)
}

/// Effective cross-turn recovery-turn budget, honoring
/// `[agent.harness.verification].cross_turn_turns`.
pub(crate) fn verification_cross_turn_turns(vt_cfg: Option<&vtcode_core::config::loader::VTCodeConfig>) -> u8 {
    vt_cfg
        .map(|cfg| cfg.agent.harness.verification.cross_turn_turns)
        .unwrap_or(MAX_VERIFICATION_AUTO_RECOVERY_TURNS)
        .min(1)
        .min(verification_in_turn_attempts(vt_cfg))
}

/// Whether tracker-aware auto-continuation is enabled
/// (`[agent.harness.continuation].auto_continue_tracker`).
pub(crate) fn tracker_auto_continue_enabled(vt_cfg: Option<&vtcode_core::config::loader::VTCodeConfig>) -> bool {
    vt_cfg
        .map(|cfg| cfg.agent.harness.continuation.auto_continue_tracker)
        .unwrap_or(true)
}

/// Effective cross-turn tracker auto-continue budget
/// (`[agent.harness.continuation].cross_turn_turns`).
pub(crate) fn tracker_cross_turn_turns(vt_cfg: Option<&vtcode_core::config::loader::VTCodeConfig>) -> u8 {
    vt_cfg.map(|cfg| cfg.agent.harness.continuation.cross_turn_turns).unwrap_or(32)
}

/// Whether automatic interactive completion checks and corrective recovery are enabled.
pub(crate) fn verification_auto_execute_enabled(vt_cfg: Option<&vtcode_core::config::loader::VTCodeConfig>) -> bool {
    vt_cfg.map(|cfg| cfg.agent.harness.verification.auto_execute).unwrap_or(true)
}

/// Effective consecutive-failure escalation threshold, honoring
/// `[agent.harness.verification].max_consecutive_failures`.
pub(crate) fn verification_max_consecutive_failures(vt_cfg: Option<&vtcode_core::config::loader::VTCodeConfig>) -> u8 {
    vt_cfg
        .map(|cfg| cfg.agent.harness.verification.max_consecutive_failures)
        .unwrap_or(MAX_VERIFICATION_CONSECUTIVE_FAILURES)
}

/// Resolve the verifier command the harness should run itself: the explicit
/// `[agent.harness.verification].default_verifier_override` first (validated
/// as a standalone verifier or pure `&&` chain — anything else falls back to
/// detection so a misconfigured override can never smuggle a mutation or a
/// status-masking pipeline into autonomous execution), then a recorded failed
/// checker for documentation-only work, then
/// [`vtcode_core::tools::tool_intent::default_verifier_for_workspace`].
/// Returns `None` when neither yields a runnable verifier; callers must then
/// fall through to the manual blocked handoff.
/// Code/unknown mutations and unsafe command shapes retain the ordinary
/// project verifier. Execution uses the existing gates.
pub(crate) fn resolve_harness_verifier_command(
    vt_cfg: Option<&vtcode_core::config::loader::VTCodeConfig>,
    workspace_root: &Path,
    history: &[uni::Message],
) -> Option<String> {
    configured_verifier(vt_cfg)
        .or_else(|| failed_docs_verifier(history))
        .or_else(|| vtcode_core::tools::tool_intent::default_verifier_for_workspace(workspace_root))
}

/// Recovery must advance mandatory checks before suggesting a generic project check.
pub(crate) fn resolve_pending_verifier_command(
    vt_cfg: Option<&vtcode_core::config::loader::VTCodeConfig>,
    registry: &vtcode_core::tools::ToolRegistry,
    history: &[uni::Message],
) -> Option<String> {
    registry
        .next_required_completion_verifier()
        .or_else(|| resolve_harness_verifier_command(vt_cfg, registry.workspace_root().as_path(), history))
}

fn configured_verifier(vt_cfg: Option<&vtcode_core::config::loader::VTCodeConfig>) -> Option<String> {
    if let Some(override_command) = vt_cfg
        .and_then(|cfg| cfg.agent.harness.verification.default_verifier_override.as_deref())
        .map(str::trim)
        .filter(|command| !command.is_empty())
    {
        let args = serde_json::json!({"cmd": override_command});
        if matches!(
            classify_shell_activity(vtcode_core::config::constants::tools::EXEC_COMMAND, &args),
            ShellActivity::Verification
        ) {
            return Some(override_command.to_string());
        }
        tracing::warn!(
            override_command,
            "Ignoring [agent.harness.verification].default_verifier_override: not a standalone verifier or pure && chain; falling back to workspace detection"
        );
    }
    None
}

#[derive(Clone, Copy)]
enum DocsVerifierCallKind {
    Launch,
    Observe,
    Cleanup,
}

struct PendingDocsVerifier<'a> {
    command: String,
    session_id: Option<vtcode_core::types::CompactStr>,
    pending_calls: FxHashMap<&'a str, DocsVerifierCallKind>,
}

fn docs_checker_session_id(output: &serde_json::Value) -> Option<&str> {
    use vtcode_core::tools::command_args::session_id_text;
    session_id_text(output)
        .or_else(|| output.get("next_wait_args").and_then(session_id_text))
        .or_else(|| output.get("next_continue_args").and_then(session_id_text))
}

fn failed_docs_verifier(history: &[uni::Message]) -> Option<String> {
    let request_start = history.iter().rposition(|message| {
        let text = message.content.as_text();
        message.role == uni::MessageRole::User
            && !crate::agent::runloop::unified::state::is_follow_up_prompt_like(text.as_ref())
            && !crate::agent::runloop::unified::turn::is_internal_harness_follow_up(text.as_ref())
    })?;
    let mut latest_checker: Option<PendingDocsVerifier<'_>> = None;
    let mut failed_command = None;
    let mut has_docs_write = false;
    for message in history.iter().skip(request_start) {
        match message.role {
            uni::MessageRole::Assistant => {
                if let Some(checker) = &mut latest_checker {
                    // Call IDs are scoped to an assistant batch; session IDs
                    // remain valid across batches until terminal completion.
                    checker.pending_calls.clear();
                }
                for call in message.tool_calls.iter().flatten() {
                    let function = call.function.as_ref()?;
                    let args: serde_json::Value = serde_json::from_str(&function.arguments).ok()?;
                    let name = canonical_tool_name(&function.name);
                    if classify_shell_activity(name, &args) == ShellActivity::Verification {
                        // A newer checker supersedes earlier failed evidence,
                        // even if its completion has not been recorded yet.
                        failed_command = None;
                        latest_checker = None;
                        // Do not drop launch context or argv suffixes when
                        // turning an invocation into a replayable command.
                        if [
                            "args",
                            "cwd",
                            "workdir",
                            "working_dir",
                            "working_directory",
                            "env",
                            "shell",
                            "login",
                            "stdin",
                            "tty",
                            "background",
                            "rows",
                            "cols",
                            "session_id",
                            "s",
                            "timeout_ms",
                            "confirm",
                            "prefix_rule",
                            "justification",
                            "sandbox_permissions",
                            "additional_permissions",
                        ]
                        .iter()
                        .any(|key| args.get(*key).is_some())
                        {
                            continue;
                        }
                        let Some(command) = vtcode_core::tools::command_args::raw_command_text(&args) else {
                            continue;
                        };
                        if classify_shell_activity(name, &serde_json::json!({"cmd": command}))
                            == ShellActivity::Verification
                        {
                            latest_checker = Some(PendingDocsVerifier {
                                command,
                                session_id: None,
                                pending_calls: FxHashMap::from_iter([(call.id.as_str(), DocsVerifierCallKind::Launch)]),
                            });
                        }
                    } else if vtcode_core::tools::tool_intent::is_exec_session_cleanup_call(name, &args) {
                        if let Some(checker) = &mut latest_checker
                            && let Some(session_id) = checker.session_id.as_deref()
                            && vtcode_core::tools::command_args::session_id_text(&args) == Some(session_id)
                        {
                            checker.pending_calls.insert(call.id.as_str(), DocsVerifierCallKind::Cleanup);
                        }
                    } else if is_session_follow_up(name, &args)
                        && !vtcode_core::tools::tool_intent::classify_tool_intent(name, &args).mutating
                    {
                        if let Some(checker) = &mut latest_checker
                            && let Some(session_id) = checker.session_id.as_deref()
                            && vtcode_core::tools::command_args::session_id_text(&args) == Some(session_id)
                        {
                            checker.pending_calls.insert(call.id.as_str(), DocsVerifierCallKind::Observe);
                        }
                    } else if is_docs_only_write(name, &args) {
                        has_docs_write = true;
                    } else if vtcode_core::tools::tool_intent::classify_tool_intent(name, &args).mutating
                        && !is_plan_artifact_write(name, &args)
                    {
                        // Any possible code mutation makes this a mixed/code
                        // task; a prose checker alone cannot verify it.
                        return None;
                    }
                }
            }
            uni::MessageRole::Tool => {
                let Some(checker) = &mut latest_checker else {
                    continue;
                };
                let Some(kind) = message.tool_call_id.as_deref().and_then(|id| checker.pending_calls.remove(id)) else {
                    continue;
                };
                let output: serde_json::Value = serde_json::from_str(message.content.as_text().as_ref()).ok()?;
                if output.get("blocked").and_then(serde_json::Value::as_bool) == Some(true)
                    || output.get("not_executed").and_then(serde_json::Value::as_bool) == Some(true)
                    || output.get("cancelled").and_then(serde_json::Value::as_bool) == Some(true)
                {
                    continue;
                }
                if let Some(error) = output.get("error") {
                    let error_text = error
                        .as_str()
                        .or_else(|| error.get("message").and_then(serde_json::Value::as_str));
                    if error_text.is_some_and(error_text_indicates_lost_session) {
                        checker.session_id = None;
                        checker.pending_calls.clear();
                    }
                    continue;
                }
                if matches!(kind, DocsVerifierCallKind::Cleanup) {
                    if output.get("success").and_then(serde_json::Value::as_bool) != Some(false) {
                        checker.session_id = None;
                        checker.pending_calls.clear();
                    }
                    continue;
                }
                if matches!(kind, DocsVerifierCallKind::Observe)
                    && docs_checker_session_id(&output).is_some_and(|id| checker.session_id.as_deref() != Some(id))
                {
                    continue;
                }
                if let Some(exit_code) = output.get("exit_code").and_then(serde_json::Value::as_i64) {
                    checker.session_id = None;
                    checker.pending_calls.clear();
                    failed_command = (exit_code != 0).then(|| checker.command.clone());
                } else if matches!(kind, DocsVerifierCallKind::Launch)
                    && let Some(session_id) = docs_checker_session_id(&output)
                {
                    checker.session_id = Some(session_id.into());
                }
            }
            _ => {}
        }
    }
    has_docs_write.then_some(failed_command).flatten()
}

pub(crate) fn resolve_max_tool_retries(
    _tool_name: &str,
    vt_cfg: Option<&vtcode_core::config::loader::VTCodeConfig>,
) -> usize {
    vt_cfg
        .map(|cfg| cfg.agent.harness.max_tool_retries as usize)
        .unwrap_or(vtcode_config::constants::defaults::DEFAULT_MAX_TOOL_RETRIES as usize)
}

#[cfg(test)]
mod tests;
