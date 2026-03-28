use serde_json::Value;
use std::fmt::Write;
use std::sync::{Arc, LazyLock};
use vtcode_core::config::constants::tools as tool_names;
use vtcode_core::llm::provider as uni;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ReplyKind {
    Immediate,
    FollowUp,
}

struct DirectToolCompletion<'a> {
    tool_call: &'a uni::ToolCall,
    payload: Option<Value>,
}

fn completion_base_text(
    history: &[uni::Message],
    reply_kind: ReplyKind,
) -> Option<(String, DirectToolCompletion<'_>)> {
    let completion = latest_direct_tool_completion(history)?;
    if completion.has_pending_follow_up() {
        return None;
    }

    let status = completion.status_text(reply_kind);
    let mut sections = Vec::with_capacity(2);
    sections.push(status);
    if let Some(observation) = completion.output_observation() {
        sections.push(observation);
    }

    Some((sections.join("\n\n"), completion))
}

/// Synchronous variant used by tests to verify fallback next-step logic.
#[cfg(test)]
fn completion_reply_text(history: &[uni::Message], reply_kind: ReplyKind) -> Option<String> {
    let (base, completion) = completion_base_text(history, reply_kind)?;
    let next_steps = completion.fallback_next_steps();
    Some(append_next_steps(&base, &next_steps))
}

/// Generate a completion reply with LLM-powered contextual suggestions.
///
/// Falls back to hardcoded suggestions if the LLM call fails or returns empty.
pub(crate) async fn generate_completion_reply_with_suggestions(
    history: &[uni::Message],
    reply_kind: ReplyKind,
    provider: &dyn uni::LLMProvider,
    model: &str,
) -> Option<String> {
    let (base, completion) = completion_base_text(history, reply_kind)?;

    // Immediate direct-command completions are on the interactive hot path.
    // Use deterministic fallback suggestions to avoid an extra LLM round-trip
    // after the command has already finished rendering.
    if reply_kind == ReplyKind::Immediate {
        return Some(append_next_steps(&base, &completion.fallback_next_steps()));
    }

    let label = completion.label();
    let exit_code = completion.exit_code();
    let has_error = completion.has_error();
    let output_snippet = completion.output_snippet();

    let llm_steps = generate_suggestions_via_llm(
        provider,
        model,
        &label,
        exit_code,
        has_error,
        output_snippet.as_deref(),
    )
    .await;

    let steps = if llm_steps.is_empty() {
        completion.fallback_next_steps()
    } else {
        llm_steps
    };
    Some(append_next_steps(&base, &steps))
}

fn append_next_steps(base: &str, next_steps: &[String]) -> String {
    if next_steps.is_empty() {
        return base.to_string();
    }
    let mut out = String::with_capacity(
        base.len() + 32 + next_steps.iter().map(|s| s.len() + 4).sum::<usize>(),
    );
    out.push_str(base);
    out.push_str("\n\nSuggested next steps:");
    for step in next_steps {
        out.push_str("\n- ");
        out.push_str(step);
    }
    out
}

static SUGGESTION_SYSTEM_PROMPT: LazyLock<Arc<String>> = LazyLock::new(|| {
    Arc::new(
        "You are a concise coding assistant. Given information about a tool execution result, \
         suggest 2-4 short, actionable next steps the user should take. \
         Each step should be one sentence. Use backticks for commands. \
         Return ONLY the bullet points, one per line, no numbering, no dashes, no extra text."
            .to_string(),
    )
});

async fn generate_suggestions_via_llm(
    provider: &dyn uni::LLMProvider,
    model: &str,
    tool_label: &str,
    exit_code: Option<i64>,
    has_error: bool,
    output_snippet: Option<&str>,
) -> Vec<String> {
    let mut user_msg = String::with_capacity(128);
    let _ = write!(user_msg, "Tool/command: `{tool_label}`\nResult: ");
    if has_error {
        user_msg.push_str("failed with an error");
    } else {
        match exit_code {
            Some(0) => user_msg.push_str("completed successfully (exit code 0)"),
            Some(code) => {
                let _ = write!(user_msg, "completed with exit code {code}");
            }
            None => user_msg.push_str("completed"),
        }
    }
    if let Some(snippet) = output_snippet {
        let _ = write!(user_msg, "\nOutput (truncated):\n```\n{snippet}\n```");
    }

    let request = uni::LLMRequest {
        messages: vec![uni::Message::user(user_msg)],
        system_prompt: Some(Arc::clone(&SUGGESTION_SYSTEM_PROMPT)),
        model: model.to_string(),
        max_tokens: Some(256),
        temperature: Some(0.3),
        tool_choice: Some(uni::ToolChoice::None),
        ..Default::default()
    };

    let response = match provider.generate(request).await {
        Ok(resp) => resp,
        Err(err) => {
            tracing::debug!(error = %err, "LLM suggestion generation failed; using fallback");
            return Vec::new();
        }
    };

    parse_suggestion_lines(response.content.as_deref().unwrap_or_default())
}

fn parse_suggestion_lines(text: &str) -> Vec<String> {
    text.lines()
        .map(|line| {
            line.trim()
                .trim_start_matches('-')
                .trim_start_matches('•')
                .trim_start_matches(|c: char| c.is_ascii_digit())
                .trim_start_matches('.')
                .trim()
                .to_string()
        })
        .filter(|line| !line.is_empty())
        .take(4)
        .collect()
}

fn latest_direct_tool_completion(history: &[uni::Message]) -> Option<DirectToolCompletion<'_>> {
    for (index, tool_message) in history.iter().enumerate().rev() {
        if !tool_message.is_tool_response() {
            continue;
        }

        let Some(tool_call_id) = tool_message
            .tool_call_id
            .as_deref()
            .filter(|id| id.starts_with("direct_"))
        else {
            continue;
        };
        let Some(assistant_message) = index.checked_sub(1).and_then(|prev| history.get(prev))
        else {
            continue;
        };
        let Some(tool_call) = assistant_message
            .get_tool_calls()
            .and_then(|calls| calls.iter().find(|call| call.id == tool_call_id))
        else {
            continue;
        };

        return Some(DirectToolCompletion {
            tool_call,
            payload: serde_json::from_str(tool_message.get_text_content().as_ref()).ok(),
        });
    }

    None
}

impl DirectToolCompletion<'_> {
    fn label(&self) -> String {
        let Some(function) = self.tool_call.function.as_ref() else {
            return "previous direct tool call".to_string();
        };

        let args = serde_json::from_str::<Value>(&function.arguments).ok();
        match function.name.as_str() {
            tool_names::SPAWN_AGENT => self
                .payload
                .as_ref()
                .and_then(|value| value.get("agent_name"))
                .and_then(Value::as_str)
                .or_else(|| {
                    args.as_ref()
                        .and_then(|value| value.get("agent_type"))
                        .and_then(Value::as_str)
                })
                .map(|agent| format!("{agent} subagent"))
                .unwrap_or_else(|| function.name.clone()),
            tool_names::UNIFIED_EXEC => args
                .as_ref()
                .and_then(|args| {
                    let action = args.get("action").and_then(Value::as_str);
                    (action.is_none() || action == Some("run"))
                        .then(|| args.get("command").and_then(Value::as_str))
                        .flatten()
                })
                .map(str::to_string)
                .unwrap_or_else(|| "previous direct command".to_string()),
            tool_names::UNIFIED_FILE => args
                .as_ref()
                .and_then(|args| {
                    (args.get("action").and_then(Value::as_str) == Some("read"))
                        .then(|| args.get("path").and_then(Value::as_str))
                        .flatten()
                })
                .map(|path| format!("read {path}"))
                .unwrap_or_else(|| function.name.clone()),
            _ => function.name.clone(),
        }
    }

    fn exit_code(&self) -> Option<i64> {
        self.payload
            .as_ref()
            .and_then(|value| value.get("exit_code"))
            .and_then(Value::as_i64)
    }

    fn error_message(&self) -> Option<String> {
        let error = self.payload.as_ref()?.get("error")?;
        match error {
            Value::String(text) => {
                let trimmed = text.trim();
                (!trimmed.is_empty()).then(|| trimmed.to_string())
            }
            Value::Object(object) => ["message", "error_summary", "original_error"]
                .iter()
                .filter_map(|key| object.get(*key))
                .find_map(Value::as_str)
                .map(str::trim)
                .filter(|text| !text.is_empty())
                .map(str::to_string),
            _ => None,
        }
    }

    fn has_error(&self) -> bool {
        self.error_message().is_some()
    }

    fn has_pending_follow_up(&self) -> bool {
        self.payload.as_ref().is_some_and(|payload| {
            payload.get("next_continue_args").is_some() || payload.get("next_read_args").is_some()
        })
    }

    fn status_text(&self, reply_kind: ReplyKind) -> String {
        let label = self.label();
        let is_spawn_agent = self
            .tool_call
            .function
            .as_ref()
            .is_some_and(|function| function.name == tool_names::SPAWN_AGENT);
        let is_background_subagent = self
            .payload
            .as_ref()
            .and_then(|value| value.get("background"))
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if self.has_error() {
            if is_spawn_agent {
                return match reply_kind {
                    ReplyKind::Immediate => format!("`{label}` failed to start."),
                    ReplyKind::FollowUp => format!("`{label}` already failed to start."),
                };
            }
            return match reply_kind {
                ReplyKind::Immediate => format!("`{label}` completed with an error."),
                ReplyKind::FollowUp => format!("`{label}` already completed with an error."),
            };
        }
        if is_spawn_agent {
            let started = if is_background_subagent {
                "started in the background"
            } else {
                "started"
            };
            return match reply_kind {
                ReplyKind::Immediate => format!("`{label}` {started}."),
                ReplyKind::FollowUp => format!("`{label}` is already {started}."),
            };
        }
        match self.exit_code() {
            Some(0) => match reply_kind {
                ReplyKind::Immediate => format!("`{label}` completed successfully (exit code 0)."),
                ReplyKind::FollowUp => {
                    format!("`{label}` already completed successfully (exit code 0).")
                }
            },
            Some(code) => match reply_kind {
                ReplyKind::Immediate => format!("`{label}` completed with exit code {code}."),
                ReplyKind::FollowUp => {
                    format!("`{label}` already completed with exit code {code}.")
                }
            },
            None => match reply_kind {
                ReplyKind::Immediate => format!("`{label}` completed."),
                ReplyKind::FollowUp => format!("`{label}` already completed."),
            },
        }
    }

    fn output_observation(&self) -> Option<String> {
        let payload = self.payload.as_ref()?;
        if self.has_error() {
            if self
                .tool_call
                .function
                .as_ref()
                .is_some_and(|function| function.name == tool_names::SPAWN_AGENT)
            {
                return Some(
                    "The subagent did not start. Failure details are shown above.".to_string(),
                );
            }
            return Some("Failure details are shown above.".to_string());
        }
        if self
            .tool_call
            .function
            .as_ref()
            .is_some_and(|function| function.name == tool_names::SPAWN_AGENT)
        {
            let background = payload
                .get("background")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            return Some(if background {
                "The subagent is running without blocking this turn. Use `/agent` to inspect or continue it.".to_string()
            } else {
                "The subagent was started. Use `/agent` to inspect or continue it.".to_string()
            });
        }
        let has_output = ["output", "stdout", "content"]
            .iter()
            .filter_map(|key| payload.get(*key))
            .any(|value| value.as_str().is_some_and(|text| !text.trim().is_empty()));
        if has_output {
            return Some("Command output is shown above.".to_string());
        }

        let has_error_output = payload
            .get("stderr")
            .and_then(Value::as_str)
            .is_some_and(|text| !text.trim().is_empty());
        if has_error_output {
            return Some("Error output is shown above.".to_string());
        }

        Some("No terminal output was produced.".to_string())
    }

    fn output_snippet(&self) -> Option<String> {
        if let Some(error_message) = self.error_message() {
            return Some(error_message);
        }
        let payload = self.payload.as_ref()?;
        for key in ["output", "stdout", "content", "stderr"] {
            if let Some(text) = payload.get(key).and_then(Value::as_str) {
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    let truncated: String = trimmed.chars().take(500).collect();
                    return Some(truncated);
                }
            }
        }
        None
    }

    fn fallback_next_steps(&self) -> Vec<String> {
        let Some(function) = self.tool_call.function.as_ref() else {
            return Vec::new();
        };

        if let Some(next_action) = self
            .payload
            .as_ref()
            .and_then(|value| value.get("next_action"))
            .and_then(Value::as_str)
            .filter(|text| !text.trim().is_empty())
        {
            return vec![
                next_action.to_string(),
                "Ask me to inspect the result and handle the next step.".to_string(),
            ];
        }

        let args = serde_json::from_str::<Value>(&function.arguments).ok();
        match function.name.as_str() {
            tool_names::UNIFIED_EXEC => {
                let command = args
                    .as_ref()
                    .and_then(|value| value.get("command"))
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|command| !command.is_empty());
                match command {
                    Some("cargo fmt") => vec![
                        "Verify the build with `cargo check`.".to_string(),
                        "Run tests with `cargo nextest run`.".to_string(),
                        "Check lint warnings with `cargo clippy --workspace --all-targets -- -D warnings`.".to_string(),
                    ],
                    Some(command) if command.starts_with("cargo check") => vec![
                        "Run tests with `cargo nextest run`.".to_string(),
                        "Check lint warnings with `cargo clippy --workspace --all-targets -- -D warnings`.".to_string(),
                    ],
                    Some(command)
                        if command.starts_with("cargo nextest")
                            || command.starts_with("cargo test") =>
                    {
                        vec![
                            "Check lint warnings with `cargo clippy --workspace --all-targets -- -D warnings`.".to_string(),
                            "Ask me to investigate any failing tests or summarize the output.".to_string(),
                        ]
                    }
                    Some(_) => vec![
                        "Run the next verification or follow-up command.".to_string(),
                        "Ask me to inspect or act on the output above.".to_string(),
                    ],
                    None => Vec::new(),
                }
            }
            tool_names::SPAWN_AGENT => {
                if self.has_error() {
                    vec![
                        "Retry the subagent with a supported model or provider configuration.".to_string(),
                        "Ask me to inspect the spawn error and patch the underlying model handling.".to_string(),
                    ]
                } else {
                    vec![
                        "Use `/agent` to inspect or continue the delegated run.".to_string(),
                        "Ask me to review its output or queue follow-up work.".to_string(),
                    ]
                }
            }
            tool_names::UNIFIED_FILE => {
                vec!["Ask me to summarize the file contents or make a targeted edit.".to_string()]
            }
            _ => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ReplyKind, completion_reply_text};
    use vtcode_core::config::constants::tools as tool_names;
    use vtcode_core::llm::provider as uni;
    use vtcode_core::llm::provider::{LLMError, LLMRequest, LLMResponse};

    struct PanicProvider;

    #[async_trait::async_trait]
    impl uni::LLMProvider for PanicProvider {
        fn name(&self) -> &str {
            "panic"
        }

        async fn generate(&self, _request: LLMRequest) -> Result<LLMResponse, LLMError> {
            panic!("immediate direct tool completion should not call generate");
        }

        fn supported_models(&self) -> Vec<String> {
            vec!["panic-model".to_string()]
        }

        fn validate_request(&self, _request: &LLMRequest) -> Result<(), LLMError> {
            Ok(())
        }
    }

    #[test]
    fn completion_reply_text_reports_successful_run_command() {
        let history = vec![
            uni::Message::assistant_with_tools(
                String::new(),
                vec![uni::ToolCall::function(
                    "direct_unified_exec_1".to_string(),
                    tool_names::UNIFIED_EXEC.to_string(),
                    serde_json::json!({"action":"run","command":"cargo check"}).to_string(),
                )],
            ),
            uni::Message::tool_response(
                "direct_unified_exec_1".to_string(),
                serde_json::json!({"exit_code":0}).to_string(),
            ),
        ];

        let text = completion_reply_text(&history, ReplyKind::FollowUp).expect("follow-up text");
        assert!(text.contains("`cargo check` already completed successfully (exit code 0)."));
        assert!(text.contains("Suggested next steps:"));
        assert!(text.contains("cargo nextest run"));
    }

    #[test]
    fn completion_reply_text_reports_success_with_implicit_exec_run_action() {
        let history = vec![
            uni::Message::assistant_with_tools(
                String::new(),
                vec![uni::ToolCall::function(
                    "direct_unified_exec_1".to_string(),
                    tool_names::UNIFIED_EXEC.to_string(),
                    serde_json::json!({"command":"cargo check"}).to_string(),
                )],
            ),
            uni::Message::tool_response(
                "direct_unified_exec_1".to_string(),
                serde_json::json!({"exit_code":0}).to_string(),
            ),
        ];

        let text = completion_reply_text(&history, ReplyKind::FollowUp).expect("follow-up text");
        assert!(text.contains("`cargo check` already completed successfully (exit code 0)."));
    }

    #[test]
    fn completion_reply_text_reports_failed_read_call() {
        let history = vec![
            uni::Message::assistant_with_tools(
                String::new(),
                vec![uni::ToolCall::function(
                    "direct_unified_file_1".to_string(),
                    tool_names::UNIFIED_FILE.to_string(),
                    serde_json::json!({"action":"read","path":"docs/project/TODO.md"}).to_string(),
                )],
            ),
            uni::Message::tool_response(
                "direct_unified_file_1".to_string(),
                serde_json::json!({"error":"limit must be greater than zero"}).to_string(),
            ),
        ];

        let text = completion_reply_text(&history, ReplyKind::FollowUp).expect("follow-up text");
        assert!(text.contains("`read docs/project/TODO.md` already completed with an error."));
        assert!(text.contains("Failure details are shown above."));
    }

    #[test]
    fn completion_reply_text_reports_started_background_subagent() {
        let history = vec![
            uni::Message::assistant_with_tools(
                String::new(),
                vec![uni::ToolCall::function(
                    "direct_spawn_agent_1".to_string(),
                    tool_names::SPAWN_AGENT.to_string(),
                    serde_json::json!({
                        "agent_type":"rust-engineer",
                        "message":"review code",
                        "background": true
                    })
                    .to_string(),
                )],
            ),
            uni::Message::tool_response(
                "direct_spawn_agent_1".to_string(),
                serde_json::json!({
                    "agent_name":"rust-engineer",
                    "background": true,
                    "status":"queued"
                })
                .to_string(),
            ),
        ];

        let text =
            completion_reply_text(&history, ReplyKind::Immediate).expect("direct completion reply");
        assert!(text.contains("`rust-engineer subagent` started in the background."));
        assert!(text.contains("Use `/agent` to inspect or continue it."));
    }

    #[test]
    fn completion_reply_text_reports_failed_spawn_agent() {
        let history = vec![
            uni::Message::assistant_with_tools(
                String::new(),
                vec![uni::ToolCall::function(
                    "direct_spawn_agent_1".to_string(),
                    tool_names::SPAWN_AGENT.to_string(),
                    serde_json::json!({
                        "agent_type":"rust-engineer",
                        "message":"review code changes",
                        "background": false
                    })
                    .to_string(),
                )],
            ),
            uni::Message::tool_response(
                "direct_spawn_agent_1".to_string(),
                serde_json::json!({
                    "error": {
                        "message": "Failed to resolve model 'claude-haiku-4.5' for subagent rust-engineer",
                        "error_summary": "[Execution failed] Failed to resolve model 'claude-haiku-4.5' for subagent rust-engineer"
                    }
                })
                .to_string(),
            ),
        ];

        let text =
            completion_reply_text(&history, ReplyKind::Immediate).expect("direct completion reply");
        assert!(text.contains("`rust-engineer subagent` failed to start."));
        assert!(text.contains("The subagent did not start. Failure details are shown above."));
        assert!(!text.contains("Use `/agent` to inspect or continue it."));
        assert!(
            text.contains("Retry the subagent with a supported model or provider configuration.")
        );
    }

    #[test]
    fn completion_reply_text_returns_none_without_direct_tool_tail() {
        let history = vec![
            uni::Message::tool_response(
                "direct_unified_exec_1".to_string(),
                serde_json::json!({"exit_code":0}).to_string(),
            ),
            uni::Message::assistant("cargo check completed.".to_string()),
        ];

        assert!(completion_reply_text(&history, ReplyKind::FollowUp).is_none());
    }

    #[test]
    fn completion_reply_text_immediate_for_cargo_fmt_suggests_next_steps() {
        let history = vec![
            uni::Message::assistant_with_tools(
                String::new(),
                vec![uni::ToolCall::function(
                    "direct_unified_exec_1".to_string(),
                    tool_names::UNIFIED_EXEC.to_string(),
                    serde_json::json!({"action":"run","command":"cargo fmt"}).to_string(),
                )],
            ),
            uni::Message::tool_response(
                "direct_unified_exec_1".to_string(),
                serde_json::json!({"exit_code":0,"output":""}).to_string(),
            ),
        ];

        let text =
            completion_reply_text(&history, ReplyKind::Immediate).expect("direct completion reply");
        assert!(text.contains("`cargo fmt` completed successfully (exit code 0)."));
        assert!(text.contains("No terminal output was produced."));
        assert!(text.contains("cargo check"));
        assert!(text.contains("cargo clippy --workspace --all-targets -- -D warnings"));
    }

    #[tokio::test]
    async fn immediate_completion_skips_llm_suggestion_generation() {
        let history = vec![
            uni::Message::assistant_with_tools(
                String::new(),
                vec![uni::ToolCall::function(
                    "direct_unified_exec_1".to_string(),
                    tool_names::UNIFIED_EXEC.to_string(),
                    serde_json::json!({"action":"run","command":"cargo fmt"}).to_string(),
                )],
            ),
            uni::Message::tool_response(
                "direct_unified_exec_1".to_string(),
                serde_json::json!({"exit_code":0,"output":""}).to_string(),
            ),
        ];

        let text = super::generate_completion_reply_with_suggestions(
            &history,
            ReplyKind::Immediate,
            &PanicProvider,
            "panic-model",
        )
        .await
        .expect("direct completion reply");

        assert!(text.contains("`cargo fmt` completed successfully (exit code 0)."));
        assert!(text.contains("cargo check"));
    }

    #[test]
    fn completion_reply_text_finds_latest_tool_before_assistant_summary() {
        let history = vec![
            uni::Message::assistant_with_tools(
                String::new(),
                vec![uni::ToolCall::function(
                    "direct_unified_exec_1".to_string(),
                    tool_names::UNIFIED_EXEC.to_string(),
                    serde_json::json!({"action":"run","command":"cargo fmt"}).to_string(),
                )],
            ),
            uni::Message::tool_response(
                "direct_unified_exec_1".to_string(),
                serde_json::json!({"exit_code":0,"output":""}).to_string(),
            ),
            uni::Message::assistant("`cargo fmt` completed successfully.".to_string()),
        ];

        let text = completion_reply_text(&history, ReplyKind::FollowUp).expect("follow-up text");
        assert!(text.contains("`cargo fmt` already completed successfully (exit code 0)."));
    }

    #[test]
    fn parse_suggestion_lines_strips_bullet_prefixes() {
        let input = "- Verify the build with `cargo check`.\n• Run tests.\n3. Check clippy.\n  \n";
        let steps = super::parse_suggestion_lines(input);
        assert_eq!(steps.len(), 3);
        assert_eq!(steps[0], "Verify the build with `cargo check`.");
        assert_eq!(steps[1], "Run tests.");
        assert_eq!(steps[2], "Check clippy.");
    }

    #[test]
    fn parse_suggestion_lines_caps_at_four() {
        let input = "a\nb\nc\nd\ne\nf\n";
        let steps = super::parse_suggestion_lines(input);
        assert_eq!(steps.len(), 4);
    }

    #[test]
    fn parse_suggestion_lines_handles_empty_input() {
        assert!(super::parse_suggestion_lines("").is_empty());
        assert!(super::parse_suggestion_lines("   \n  \n").is_empty());
    }

    #[test]
    fn append_next_steps_empty_returns_base() {
        let base = "command completed.";
        assert_eq!(super::append_next_steps(base, &[]), base);
    }

    #[test]
    fn append_next_steps_formats_bullets() {
        let base = "status";
        let steps = vec!["step one".to_string(), "step two".to_string()];
        let result = super::append_next_steps(base, &steps);
        assert_eq!(
            result,
            "status\n\nSuggested next steps:\n- step one\n- step two"
        );
    }
}
