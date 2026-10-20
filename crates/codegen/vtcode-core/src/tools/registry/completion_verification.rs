//! Runtime verification evidence, independent of compactable source exposure.

use std::collections::{HashSet, VecDeque};

use serde_json::Value;

use super::ToolRegistry;
use crate::config::constants::tools;
use crate::tools::{command_args, tool_intent};

const OPTIONAL_EVIDENCE_LIMIT: usize = 32;

#[derive(Debug)]
struct VerifierEvidence {
    identity: u64,
    generation: u64,
    command: String,
    session_id: Option<String>,
    exit_code: Option<i64>,
}

struct RestoredToolCall<'a> {
    name: &'a str,
    args: Value,
}

#[derive(Clone, Copy)]
enum MutationEvidenceSource {
    Live,
    Restored,
}

#[derive(Debug, Default)]
pub(super) struct CompletionVerification {
    generation: u64,
    history_initialized: bool,
    next_identity: u64,
    evidence: VecDeque<VerifierEvidence>,
    required_commands: Vec<String>,
    pending_mutation_sessions: HashSet<String>,
}

impl CompletionVerification {
    fn mutate(&mut self) {
        self.generation = self.generation.saturating_add(1);
    }

    fn launch(&mut self, command: String) -> u64 {
        self.next_identity = self.next_identity.saturating_add(1);
        // New launches supersede older evidence for the same command.
        self.evidence.retain(|entry| entry.command != command);
        self.evidence.push_back(VerifierEvidence {
            identity: self.next_identity,
            generation: self.generation,
            command,
            session_id: None,
            exit_code: None,
        });
        self.prune_optional_evidence();
        self.next_identity
    }

    fn prune_optional_evidence(&mut self) {
        let mut pinned = HashSet::new();
        for command in &self.required_commands {
            let command = executed_command_identity(command);
            let mut newest = true;
            for entry in self
                .evidence
                .iter()
                .rev()
                .filter(|entry| command_covers_required(&entry.command, &command))
            {
                // Keep the newest verdict for each requirement and retain live
                // handles until their terminal result or cleanup is observed.
                if newest || entry.session_id.is_some() {
                    pinned.insert(entry.identity);
                }
                newest = false;
            }
        }
        let mut to_remove = self
            .evidence
            .iter()
            .filter(|entry| !pinned.contains(&entry.identity))
            .count()
            .saturating_sub(OPTIONAL_EVIDENCE_LIMIT);
        // Required verdicts and yielded handles remain available until their
        // command is replaced or released by the tracker.
        self.evidence.retain(|entry| {
            if to_remove == 0 || pinned.contains(&entry.identity) {
                true
            } else {
                to_remove -= 1;
                false
            }
        });
    }

    fn observe(&mut self, identity: u64, output: &Value) {
        let Some(evidence) = self.evidence.iter_mut().find(|entry| entry.identity == identity) else {
            return;
        };
        if output.get("error").is_some()
            || output.get("success").and_then(Value::as_bool) == Some(false)
            || ["blocked", "not_executed", "cancelled", "termination_requested"]
                .iter()
                .any(|key| output.get(key).and_then(Value::as_bool) == Some(true))
        {
            evidence.session_id = None;
            evidence.exit_code = None;
            self.prune_optional_evidence();
            return;
        }
        if evidence
            .session_id
            .as_deref()
            .is_some_and(|expected| command_args::session_id_text(output).is_some_and(|returned| returned != expected))
        {
            return;
        }
        if let Some(exit_code) = output.get("exit_code").and_then(Value::as_i64) {
            evidence.exit_code = Some(exit_code);
            evidence.session_id = None;
        } else if let Some(session_id) = command_args::session_id_text(output) {
            evidence.session_id = Some(session_id.to_owned());
        }
        self.prune_optional_evidence();
    }

    fn command_passed(&self, command: &str) -> bool {
        let command = executed_command_identity(command);
        self.evidence
            .iter()
            .rev()
            .find(|entry| command_covers_required(&entry.command, &command))
            .is_some_and(|entry| entry.generation == self.generation && entry.exit_code == Some(0))
    }

    fn requires_command(&self, command: &str) -> bool {
        let command = executed_command_identity(command);
        self.required_commands
            .iter()
            .any(|required| command_covers_required(&command, &executed_command_identity(required)))
    }

    fn pending(&self) -> bool {
        if !self.pending_mutation_sessions.is_empty() {
            return true;
        }
        if self.generation == 0 {
            return false;
        }
        if !self.required_commands.is_empty() {
            return self.required_commands.iter().any(|command| !self.command_passed(command));
        }
        !self
            .evidence
            .back()
            .is_some_and(|entry| entry.generation == self.generation && entry.exit_code == Some(0))
    }
}

fn executed_command_identity(command: &str) -> String {
    let args = serde_json::json!({"action":"run", "command":command});
    let executed = tool_intent::shell_args_as_executed(tools::UNIFIED_EXEC, &args);
    command_args::raw_command_text(&executed)
        .unwrap_or_else(|| command.to_owned())
        .trim()
        .to_owned()
}

/// Defense boundary: ingest-time verification (`task_tracking::metadata_from_input`,
/// `task_tracking::apply_metadata_line`) already strips code spans, but
/// restored/hand-written tracker content may reach here unnormalized, so the
/// store keeps stripping before `command_passed` compares or harness
/// auto-verification re-runs the value verbatim.
use crate::tools::handlers::task_tracking::unwrap_verify_code_span;

fn command_covers_required(command: &str, required: &str) -> bool {
    command == required
        || (command.contains("&&") && tool_intent::activity::shell_command_covers_required_check(command, required))
}

fn is_session_cleanup(args: &Value) -> bool {
    tool_intent::is_exec_session_cleanup_call(tools::WRITE_STDIN, args)
        || matches!(tool_intent::command_session_action(args), Some("close" | "terminate"))
}

fn mutation_targets_are_exempt(name: &str, args: &Value, output: &Value) -> bool {
    let exempt = |tool_name: &str, targets: &Value| {
        tool_intent::is_plan_artifact_write(tool_name, targets) || tool_intent::is_docs_only_write(tool_name, targets)
    };
    if !exempt(name, args) {
        return false;
    }
    // Built-in file results report canonical targets. Requested prose/planning
    // aliases must not exempt writes through symlinks to code.
    let effective_paths: Vec<_> = ["path", "from", "to"]
        .iter()
        .filter_map(|key| output.get(*key).and_then(Value::as_str))
        .chain(
            output
                .get("modified_files")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str),
        )
        .collect();
    effective_paths.is_empty() || exempt(tools::WRITE_FILE, &serde_json::json!({"paths":effective_paths}))
}

impl ToolRegistry {
    /// True when current-generation evidence satisfies every local obligation.
    pub fn has_current_completion_verification(&self) -> bool {
        let state = self.harness_context.completion_verification.lock();
        let checks_passed = if state.required_commands.is_empty() {
            state
                .evidence
                .back()
                .is_some_and(|entry| entry.generation == state.generation && entry.exit_code == Some(0))
        } else {
            state.required_commands.iter().all(|command| state.command_passed(command))
        };
        !state.pending()
            && checks_passed
            && !state
                .evidence
                .iter()
                .any(|entry| entry.generation == state.generation && entry.session_id.is_some())
    }

    /// Drain terminal command results already returned by exec tools. Notification
    /// consumers retain these identities, independently of compactable history.
    pub fn take_observed_terminal_exec_results(&self) -> Vec<String> {
        self.harness_context.terminal_exec_results.lock().drain().collect()
    }

    /// Reconstruct mutation debt once from restored history; historical checks are not runtime verdicts.
    pub fn restore_completion_verification_from_history(&self, history: &[crate::llm::provider::Message]) {
        use crate::llm::provider::MessageRole;
        {
            let mut state = self.harness_context.completion_verification.lock();
            if state.history_initialized {
                return;
            }
            state.history_initialized = true;
            // A live registry already owns reliable runtime evidence. Only a
            // newly restored registry reconstructs debt from archived messages.
            if state.generation != 0 || !state.evidence.is_empty() {
                return;
            }
        }
        let mut pending = std::collections::HashMap::new();
        for message in history {
            if message.role == MessageRole::Assistant {
                pending.clear();
                for call in message.tool_calls.iter().flatten() {
                    if let Some(function) = &call.function
                        && let Ok(args) = serde_json::from_str::<Value>(&function.arguments)
                    {
                        pending.insert(call.id.as_str(), RestoredToolCall { name: function.name.as_str(), args });
                    }
                }
            } else if message.role == MessageRole::Tool
                && let Some(call) = message.tool_call_id.as_deref().and_then(|id| pending.remove(id))
                && let Ok(output) = serde_json::from_str::<Value>(message.content.as_text().as_ref())
            {
                if self.tracker_adopted_for_request() && call.name == tools::TASK_TRACKER {
                    self.observe_required_completion_verifiers(&output);
                }
                if output.get("cleared").is_some() {
                    self.observe_completion_mutation_from(
                        call.name,
                        &call.args,
                        &serde_json::json!({"success":true,"exit_code":0}),
                        MutationEvidenceSource::Restored,
                    );
                } else {
                    self.observe_completion_mutation_from(
                        call.name,
                        &call.args,
                        &output,
                        MutationEvidenceSource::Restored,
                    );
                }
            }
        }
    }

    /// Code changes require an observed exit zero from the same mutation generation.
    pub fn completion_verification_pending(&self) -> bool {
        self.harness_context.completion_verification.lock().pending()
    }

    /// Monotonic identity of the code state used to reject stale completion drafts.
    pub fn completion_mutation_generation(&self) -> u64 {
        self.harness_context.completion_verification.lock().generation
    }

    pub(super) fn observe_required_completion_verifiers(&self, output: &Value) {
        let Some(items) = output
            .get("checklist")
            .and_then(|checklist| checklist.get("items"))
            .and_then(Value::as_array)
        else {
            return;
        };
        let mut commands = Vec::new();
        for command in items
            .iter()
            .filter_map(|item| item.get("verify"))
            .filter_map(Value::as_array)
            .flatten()
            .filter_map(Value::as_str)
        {
            let command = command.trim();
            if !command.is_empty() && !commands.iter().any(|existing| existing == command) {
                commands.push(command.to_owned());
            }
        }
        self.set_required_completion_verifiers(commands);
    }

    /// Existing tracker verification commands are mandatory, even when another check passes.
    pub(super) fn set_required_completion_verifiers(&self, commands: Vec<String>) {
        // Store executable identities: a code-span-wrapped literal can never
        // match the bare executed command and is itself rejected by preflight
        // (code spans are command substitution), deadlocking the gate.
        let mut normalized = Vec::with_capacity(commands.len());
        for command in commands {
            let command = unwrap_verify_code_span(&command).to_owned();
            if !command.is_empty() && !normalized.contains(&command) {
                normalized.push(command);
            }
        }
        let mut state = self.harness_context.completion_verification.lock();
        state.required_commands = normalized;
        state.prune_optional_evidence();
    }

    pub fn completion_verifier_passed(&self, command: &str) -> bool {
        self.harness_context.completion_verification.lock().command_passed(command)
    }

    pub(crate) fn missing_required_completion_verifiers(&self) -> Vec<String> {
        let state = self.harness_context.completion_verification.lock();
        state
            .required_commands
            .iter()
            .filter(|command| !state.command_passed(command))
            .cloned()
            .collect()
    }

    /// Resume a current-generation verifier covering the requested check.
    pub fn pending_completion_verifier_session(&self, command: &str) -> Option<String> {
        let command = executed_command_identity(command);
        let state = self.harness_context.completion_verification.lock();
        state
            .evidence
            .iter()
            .rev()
            .find(|entry| command_covers_required(&entry.command, &command) && entry.generation == state.generation)
            .and_then(|entry| entry.session_id.clone())
    }

    pub fn next_required_completion_verifier(&self) -> Option<String> {
        let state = self.harness_context.completion_verification.lock();
        state
            .required_commands
            .iter()
            .find(|command| !state.command_passed(command))
            .cloned()
    }

    /// Restore conservatively: historical success cannot establish fresh runtime evidence.
    /// Call once when restoring a session with recorded code changes or verification debt.
    pub fn restore_completion_verification_debt(&self) {
        let mut state = self.harness_context.completion_verification.lock();
        if state.generation == 0 {
            state.mutate();
        }
    }

    pub fn unverified_completion_reason(&self) -> String {
        let state = self.harness_context.completion_verification.lock();
        let evidence = state
            .evidence
            .back()
            .map(|entry| {
                format!(
                    " Last command: `{}`; terminal exit: {:?}; launch generation: {}; current generation: {}.",
                    entry.command, entry.exit_code, entry.generation, state.generation
                )
            })
            .unwrap_or_else(|| " No terminal verifier evidence was observed.".to_string());
        let remaining = state.required_commands.iter().find(|command| !state.command_passed(command));
        let required = remaining.map_or_else(String::new, |command| {
            let preview = vtcode_commons::formatting::truncate_within(command, 1024, "...");
            format!(
                " Next required tracker check: `{preview}`. Run this exact command; a different passing check does not satisfy it."
            )
        });
        format!(
            "Verification is still pending: Unverified: changed code requires fresh verification before successful completion.{evidence}{required}"
        )
    }

    pub(super) fn begin_completion_verifier(&self, args: &Value) -> Option<u64> {
        let command = self.completion_verifier_command(args)?;
        Some(self.harness_context.completion_verification.lock().launch(command))
    }

    /// Share verifier accounting with the pacing gate; this grants no execution permission.
    pub fn is_completion_verifier_call(&self, name: &str, args: &Value) -> bool {
        tool_intent::is_command_tool(crate::tools::names::canonical_tool_name(name))
            && self.completion_verifier_command(args).is_some()
    }

    fn completion_verifier_command(&self, args: &Value) -> Option<String> {
        let executed = tool_intent::shell_args_as_executed(tools::UNIFIED_EXEC, args);
        let command = command_args::raw_command_text(&executed)?.trim().to_owned();
        // A declared repair action still invalidates verification. Inspect all
        // static segments so a harmless prefix cannot hide a source rewrite.
        if tool_intent::readonly::static_shell_command_words(&command)
            .or_else(|| tool_intent::readonly::static_shell_command_words_with_output_plumbing(&command))
            .is_some_and(|segments| segments.iter().any(|words| command_args::command_uses_source_repair(words)))
        {
            return None;
        }
        if tool_intent::classify_shell_activity(tools::UNIFIED_EXEC, &executed)
            != tool_intent::ShellActivity::Verification
            && !self.harness_context.completion_verification.lock().requires_command(&command)
        {
            return None;
        }
        let cwd = self
            .pty_manager()
            .working_dir_candidate(command_args::working_dir_text(args))
            .ok()?;
        if cwd != *self.workspace_root() {
            return None;
        }
        // Empty argv/environment containers are native adapter defaults. Effective
        // launch context cannot be dropped from a command-only verifier identity.
        if args
            .get("args")
            .is_some_and(|value| !matches!(value, Value::Array(args) if args.is_empty()))
            || args.get("env").is_some_and(|value| match value {
                Value::Array(entries) => !entries.is_empty(),
                Value::Object(env) => !env.is_empty(),
                _ => true,
            })
            || ["shell", "login", "stdin"].iter().any(|key| args.get(key).is_some())
        {
            return None;
        }
        Some(command)
    }

    pub(super) fn observe_completion_verifier(
        &self,
        launch: Option<u64>,
        args: &Value,
        result: &anyhow::Result<Value>,
    ) {
        if let Ok(output) = result
            && output.get("exit_code").and_then(Value::as_i64).is_some()
            && output.get("error").is_none()
            && output.get("cancelled").and_then(Value::as_bool) != Some(true)
            && let Some(session_id) =
                command_args::session_id_text(output).or_else(|| command_args::session_id_text(args))
        {
            self.harness_context.terminal_exec_results.lock().insert(session_id.to_owned());
        }
        let mut state = self.harness_context.completion_verification.lock();
        if let Some(session_id) = command_args::session_id_text(args)
            && state.pending_mutation_sessions.contains(session_id)
            && result.as_ref().is_ok_and(|output| {
                !command_args::session_id_text(output).is_some_and(|returned| returned != session_id)
                    && (output.get("exit_code").and_then(Value::as_i64).is_some()
                        || (is_session_cleanup(args)
                            && output.get("error").is_none()
                            && output.get("success").and_then(Value::as_bool) != Some(false)))
            })
        {
            state.pending_mutation_sessions.remove(session_id);
            // A check run while a mutator was active cannot verify its final output.
            state.mutate();
        }
        let identity = launch.or_else(|| {
            let session_id = command_args::session_id_text(args)?;
            if tool_intent::classify_tool_intent(tools::UNIFIED_EXEC, args).mutating && !is_session_cleanup(args) {
                return None;
            }
            state
                .evidence
                .iter()
                .rev()
                .find(|entry| entry.session_id.as_deref() == Some(session_id))
                .map(|entry| entry.identity)
        });
        let Some(identity) = identity else {
            return;
        };
        match result {
            Ok(output) if !is_session_cleanup(args) => state.observe(identity, output),
            _ => {
                // Lost, denied, cancelled, or cleaned-up sessions carry no verdict.
                if let Some(entry) = state.evidence.iter_mut().find(|entry| entry.identity == identity) {
                    entry.session_id = None;
                    entry.exit_code = None;
                }
                state.prune_optional_evidence();
            }
        }
    }

    pub(super) fn observe_completion_mutation(&self, name: &str, args: &Value, output: &Value) {
        self.observe_completion_mutation_from(name, args, output, MutationEvidenceSource::Live);
    }

    /// Retire verification bookkeeping for exec sessions removed by bulk
    /// cleanup paths (`terminate_*_exec_sessions_async`, TUI force-cancel)
    /// that bypass per-call observation. Without this, a terminated mutating
    /// session stays in `pending_mutation_sessions` forever and every later
    /// turn reports unverified completion even after fresh checks pass.
    /// Reuses the single-session cleanup contract (retire the pending boundary
    /// and any live verifier handle); unknown or still-live ids are harmless
    /// no-ops. Callers must pass only sessions that are actually gone so a
    /// live mutator is never released early.
    pub(super) fn observe_completion_exec_sessions_terminated(&self, session_ids: &[String]) {
        for session_id in session_ids {
            let args = serde_json::json!({"action":"close", "session_id":session_id});
            let result: anyhow::Result<Value> = Ok(serde_json::json!({"success":true}));
            self.observe_completion_verifier(None, &args, &result);
        }
    }

    fn observe_completion_mutation_from(
        &self,
        name: &str,
        args: &Value,
        output: &Value,
        source: MutationEvidenceSource,
    ) {
        let name = crate::tools::names::canonical_tool_name(name);
        // Mutation intent also covers coordination and metadata tools. Only
        // workspace writes, shell mutations, or explicit changed paths incur code debt.
        if !tool_intent::is_command_tool(name)
            && !tool_intent::is_edited_file_conflict_guarded_call(name, args)
            && !matches!(
                name,
                tools::DELETE_FILE | tools::MOVE_FILE | tools::COPY_FILE | tools::SEARCH_REPLACE | tools::UNIFIED_FILE
            )
            && !output
                .get("modified_files")
                .and_then(Value::as_array)
                .is_some_and(|files| !files.is_empty())
        {
            return;
        }
        let exec_changed = tool_intent::is_command_tool(name) && tool_intent::activity::exec_output_has_changes(output);
        if !exec_changed
            && (mutation_targets_are_exempt(name, args, output)
                || !tool_intent::classify_tool_intent(name, args).mutating
                || output.get("error").is_some()
                || output.get("success").and_then(Value::as_bool) == Some(false)
                || ["blocked", "not_executed", "cancelled"]
                    .iter()
                    .any(|key| output.get(key).and_then(Value::as_bool) == Some(true)))
        {
            return;
        }
        if tool_intent::is_command_tool(name) && !exec_changed {
            let executed = tool_intent::shell_args_as_executed(name, args);
            if tool_intent::classify_shell_activity(name, &executed) != tool_intent::ShellActivity::Mutation
                || tool_intent::shell_command_is_admitted_verification_attempt(&executed)
                // Declared checks use the same eligibility rules as verifier launch.
                // This affects accounting only; execution admission remains authoritative.
                || self.completion_verifier_command(args).is_some()
                || (output.get("exit_code").and_then(Value::as_i64) != Some(0)
                    && command_args::session_id_text(output).is_none())
            {
                return;
            }
            // Dynamic inspections need a complete nonmutation proof before
            // they may preserve prior verifier evidence. Admission is unchanged.
            if let Some(command_text) = command_args::raw_command_text(&executed)
                .or_else(|| command_args::shell_command_text_with_args(&executed))
                && !tool_intent::activity::exec_mutation_should_incur_debt(&command_text, output)
            {
                return;
            }
        }
        match crate::tools::file_ops::diff_output_has_effective_change(output) {
            Some(false) if !exec_changed => return,
            Some(false) | Some(true) => {}
            None if output.get("success").and_then(Value::as_bool) != Some(true)
                && output.get("exit_code").and_then(Value::as_i64) != Some(0)
                && command_args::session_id_text(output).is_none()
                && !output
                    .get("modified_files")
                    .and_then(Value::as_array)
                    .is_some_and(|files| !files.is_empty()) =>
            {
                return;
            }
            None => {}
        }
        let mut state = self.harness_context.completion_verification.lock();
        state.mutate();
        if matches!(source, MutationEvidenceSource::Live)
            && tool_intent::is_command_tool(name)
            && output.get("exit_code").and_then(Value::as_i64).is_none()
            && let Some(session_id) = command_args::session_id_text(output)
        {
            state.pending_mutation_sessions.insert(session_id.to_owned());
        }
    }
}

#[cfg(test)]
mod tests;
