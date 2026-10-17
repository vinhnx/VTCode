use super::*;
use crate::tools::tool_intent::{ShellActivity, classify_shell_activity};

fn exemption_key(args: &Value, cwd: &Path) -> String {
    let mut invocation = crate::tools::command_args::normalize_shell_args(args).unwrap_or_else(|_| args.clone());
    // Approval/display metadata may be added by the normal dispatch pipeline;
    // every execution-bearing field, including sandbox permissions, stays bound.
    if let Some(object) = invocation.as_object_mut() {
        for field in ["action", "confirm", "description", "max_tokens", "max_output_tokens"] {
            object.remove(field);
        }
    }
    revision(&format!("{}:{invocation}", cwd.display()))
}

/// An exact, single-use exemption owned by a trusted user-command dispatch.
/// Dropping it also removes an unconsumed exemption after admission failure.
pub struct TrustedCommandExemption {
    inspection: SourceInspection,
    key: String,
}

impl Drop for TrustedCommandExemption {
    fn drop(&mut self) {
        self.inspection.0.lock().exemptions.remove(&self.key);
    }
}

impl SourceInspection {
    /// Only trusted user-command entrypoints may call this. Tool approval and
    /// model-supplied flags must never create an inspection exemption.
    pub fn exempt_user_command(&self, args: &Value, cwd: &Path) {
        let mut ledger = self.0.lock();
        if ledger.exemptions.len() >= MAX_SUPPORTS {
            ledger.exemptions.clear();
        }
        ledger.exemptions.insert(exemption_key(args, cwd));
    }

    pub(crate) fn scoped_user_command(&self, args: &Value, cwd: &Path) -> TrustedCommandExemption {
        self.exempt_user_command(args, cwd);
        TrustedCommandExemption {
            inspection: self.clone(),
            key: exemption_key(args, cwd),
        }
    }

    pub(crate) fn check_shell(&self, args: &Value, cwd: &Path) -> Result<()> {
        if self.mode() == InspectionMode::Off {
            return Ok(());
        }
        if self.0.lock().exemptions.remove(&exemption_key(args, cwd)) {
            return Ok(());
        }
        let mut environment_words = Vec::new();
        match args.get("env") {
            Some(Value::Object(values)) => {
                environment_words.extend(values.iter().map(|(key, value)| format!("{key}={value}")))
            }
            Some(Value::Array(values)) => environment_words.extend(
                values
                    .iter()
                    .filter_map(|value| Some(format!("{}={}", value.get("name")?.as_str()?, value.get("value")?))),
            ),
            _ => {}
        }
        environment_words.push("true".to_string());
        let environment_injection = crate::tools::command_args::has_unsafe_readonly_options(&environment_words);
        let unknown_shell = args.get("shell").and_then(Value::as_str).is_some_and(|shell| {
            !matches!(
                shell,
                "/bin/bash"
                    | "/bin/zsh"
                    | "/bin/sh"
                    | "/usr/bin/bash"
                    | "/usr/bin/zsh"
                    | "/usr/bin/sh"
                    | "/usr/bin/fish"
                    | "/opt/homebrew/bin/fish"
                    | "bash"
                    | "zsh"
                    | "sh"
                    | "pwsh"
                    | "powershell"
                    | "cmd"
            )
        });
        let static_commands = crate::tools::command_args::shell_command_text_with_args(args)
            .and_then(|command| crate::tools::tool_intent::readonly::static_shell_command_words(&command));
        let safe_command_shape = static_commands.is_some_and(|commands| {
            commands.iter().all(|words| {
                !crate::tools::command_args::has_unsafe_readonly_options(words)
                    && !crate::command_safety::command_requires_approval(words)
            })
        });
        let rewriting = crate::tools::command_args::command_words(args)
            .ok()
            .flatten()
            .is_some_and(|words| {
                words.iter().any(|word| {
                    matches!(word.as_str(), "--fix" | "--write" | "--apply" | "--bless" | "--update-snapshots")
                        || word.starts_with("--fix=")
                        || word.starts_with("--write=")
                })
            });
        if safe_command_shape
            && !environment_injection
            && !unknown_shell
            && !rewriting
            && classify_shell_activity(crate::config::constants::tools::EXEC_COMMAND, args) != ShellActivity::Mutation
        {
            return Ok(());
        }
        let details = json!({"failure_kind":"source_inspection_shell_write", "message":
            "Route authored file changes through apply_patch or guarded file tools. Unclassified potential shell writes require a trusted user exemption for the complete invocation and working directory; ordinary shell approval is insufficient."});
        if self.mode() == InspectionMode::Warn {
            tracing::warn!(%details, "Source inspection warning");
            return Ok(());
        }
        Err(InspectionRequired { details }.into())
    }
}
