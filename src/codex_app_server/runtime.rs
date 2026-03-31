use super::client::{
    CODEX_PROVIDER, CodexAppServerClient, CodexThreadEnvelope, CodexThreadRequest,
    CodexTurnRequest, ServerEvent,
};
use crate::agent::runloop::ResumeSession;
use anyhow::{Context, Result, anyhow, bail};
use async_trait::async_trait;
use dialoguer::{Select, theme::ColorfulTheme};
use serde_json::{Value, json};
use std::io::Write as _;
use tokio::sync::broadcast;
use vtcode_core::cli::args::AskCommandOptions;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::core::interfaces::session::{
    PlanModeEntrySource, SessionRuntime, SessionRuntimeParams,
};
use vtcode_core::core::threads::build_thread_archive_metadata;
use vtcode_core::llm::provider::{FinishReason, LLMResponse, MessageRole};
use vtcode_core::ui::terminal;
use vtcode_core::utils::session_archive::{
    SessionArchive, SessionArchiveMetadata, SessionMessage, SessionProgressArgs,
    generate_session_archive_identifier, history_persistence_enabled,
    reserve_session_archive_identifier,
};

const APPROVAL_POLICY_INTERACTIVE: &str = "on-request";
const APPROVAL_POLICY_AUTOMATIC: &str = "never";

#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct CodexSessionRuntime;

#[async_trait]
impl SessionRuntime<ResumeSession> for CodexSessionRuntime {
    async fn run_session(&self, params: SessionRuntimeParams<'_, ResumeSession>) -> Result<()> {
        if params.full_auto {
            bail!("provider=codex currently supports interactive chat and ask only");
        }

        if !matches!(params.plan_mode_entry_source, PlanModeEntrySource::None) {
            eprintln!(
                "warning: plan mode is not yet supported for provider=codex; continuing in chat mode"
            );
        }

        run_interactive_session(
            params.agent_config,
            params.vt_config.as_ref(),
            params.skip_confirmations,
            params.resume,
        )
        .await
    }
}

pub(crate) async fn handle_codex_ask_command(
    config: CoreAgentConfig,
    prompt: Vec<String>,
    vt_cfg: Option<&vtcode_config::VTCodeConfig>,
    options: AskCommandOptions,
) -> Result<()> {
    let prompt_text = prompt.join(" ").trim().to_string();
    if prompt_text.is_empty() {
        bail!("Prompt is empty. Provide text after `vtcode ask`.");
    }

    let client = CodexAppServerClient::connect(vt_cfg).await?;
    let thread = client
        .thread_start(
            build_thread_request(&config, true, options.skip_confirmations),
            true,
        )
        .await?;
    let output = run_turn(
        &client,
        build_turn_request(
            &config,
            thread.thread.id,
            prompt_text,
            true,
            options.skip_confirmations,
        ),
        false,
    )
    .await?;

    if let Some(vtcode_core::cli::args::AskOutputFormat::Json) = options.output_format {
        let response = LLMResponse {
            content: Some(output.clone()),
            model: config.model.clone(),
            tool_calls: None,
            usage: None,
            finish_reason: FinishReason::Stop,
            reasoning: None,
            reasoning_details: None,
            organization_id: None,
            request_id: None,
            tool_references: Vec::new(),
        };
        let payload = json!({
            "response": response,
            "provider": {
                "kind": CODEX_PROVIDER,
                "model": config.model,
            }
        });
        let mut stdout = std::io::stdout().lock();
        serde_json::to_writer_pretty(&mut stdout, &payload)?;
        writeln!(stdout)?;
        return Ok(());
    }

    println!("{output}");
    Ok(())
}

async fn run_interactive_session(
    config: &CoreAgentConfig,
    vt_cfg: Option<&vtcode_config::VTCodeConfig>,
    skip_confirmations: bool,
    resume: Option<ResumeSession>,
) -> Result<()> {
    let client = CodexAppServerClient::connect(vt_cfg).await?;
    let history_enabled = history_persistence_enabled();
    let (thread, mut archive, mut messages, mut turn_number) =
        prepare_session_state(&client, config, resume, history_enabled, skip_confirmations).await?;

    println!("Codex thread: {}", thread.thread.id);
    println!("Type `exit` or `/exit` to end the session.");

    loop {
        let Some(input) = read_user_prompt()? else {
            break;
        };
        if input.trim().is_empty() {
            continue;
        }
        if should_exit_session(&input) {
            break;
        }

        messages.push(SessionMessage::new(MessageRole::User, input.clone()));

        match run_turn(
            &client,
            build_turn_request(
                config,
                thread.thread.id.clone(),
                input,
                false,
                skip_confirmations,
            ),
            true,
        )
        .await
        {
            Ok(output) => {
                messages.push(SessionMessage::new(MessageRole::Assistant, output));
                turn_number += 1;
                persist_archive_progress(archive.as_ref(), &messages, turn_number)?;
            }
            Err(err) => {
                eprintln!("error: {err}");
            }
        }
    }

    finalize_archive(archive.take(), messages)?;
    Ok(())
}

async fn prepare_session_state(
    client: &CodexAppServerClient,
    config: &CoreAgentConfig,
    resume: Option<ResumeSession>,
    history_enabled: bool,
    skip_confirmations: bool,
) -> Result<(
    CodexThreadEnvelope,
    Option<SessionArchive>,
    Vec<SessionMessage>,
    usize,
)> {
    let thread_request = build_thread_request(config, false, skip_confirmations);

    let Some(resume) = resume else {
        let thread = client.thread_start(thread_request, false).await?;
        let archive = create_new_archive(config, &thread.thread.id, history_enabled, None).await?;
        return Ok((thread, archive, Vec::new(), 0));
    };

    let upstream_thread_id = resume
        .snapshot()
        .metadata
        .external_thread_id
        .clone()
        .ok_or_else(|| anyhow!("archived session is missing its Codex thread id"))?;
    let thread = if resume.is_fork() {
        client
            .thread_fork(&upstream_thread_id, thread_request, false)
            .await?
    } else {
        client.thread_resume(&upstream_thread_id).await?
    };
    let messages = if resume.is_fork() && resume.summarize_fork() {
        Vec::new()
    } else {
        resume.history().iter().map(SessionMessage::from).collect()
    };
    let archive = if history_enabled {
        Some(if resume.is_fork() {
            let custom_suffix = resume.custom_suffix().map(ToOwned::to_owned);
            create_new_archive(config, &thread.thread.id, true, custom_suffix)
                .await?
                .ok_or_else(|| anyhow!("failed to create archive for forked Codex session"))?
        } else {
            let metadata = build_archive_metadata(config, &thread.thread.id);
            SessionArchive::resume_from_listing(resume.listing(), metadata)
        })
    } else {
        None
    };

    let turn_number = messages.len() / 2;
    Ok((thread, archive, messages, turn_number))
}

async fn create_new_archive(
    config: &CoreAgentConfig,
    thread_id: &str,
    history_enabled: bool,
    custom_suffix: Option<String>,
) -> Result<Option<SessionArchive>> {
    if !history_enabled {
        return Ok(None);
    }

    let workspace_label = workspace_archive_label(config.workspace.as_path());
    let archive_id = reserve_session_archive_identifier(&workspace_label, custom_suffix.clone())
        .await
        .unwrap_or_else(|_| generate_session_archive_identifier(&workspace_label, custom_suffix));
    let metadata = build_archive_metadata(config, thread_id);
    Ok(Some(
        SessionArchive::new_with_identifier(metadata, archive_id)
            .await
            .context("failed to create Codex session archive")?,
    ))
}

fn build_archive_metadata(config: &CoreAgentConfig, thread_id: &str) -> SessionArchiveMetadata {
    build_thread_archive_metadata(
        &config.workspace,
        &config.model,
        CODEX_PROVIDER,
        &config.theme,
        config.reasoning_effort.as_str(),
    )
    .with_external_thread_id(thread_id.to_string())
    .with_debug_log_path(
        crate::main_helpers::runtime_debug_log_path()
            .map(|path| path.to_string_lossy().to_string()),
    )
}

fn build_thread_request(
    config: &CoreAgentConfig,
    read_only: bool,
    skip_confirmations: bool,
) -> CodexThreadRequest {
    CodexThreadRequest {
        cwd: config.workspace.to_string_lossy().to_string(),
        model: Some(config.model.clone()),
        approval_policy: approval_policy(skip_confirmations),
        sandbox: if read_only {
            "read-only"
        } else {
            "workspace-write"
        },
    }
}

fn build_turn_request(
    config: &CoreAgentConfig,
    thread_id: String,
    input: String,
    read_only: bool,
    skip_confirmations: bool,
) -> CodexTurnRequest {
    CodexTurnRequest {
        thread_id,
        input,
        cwd: config.workspace.to_string_lossy().to_string(),
        model: Some(config.model.clone()),
        approval_policy: approval_policy(skip_confirmations),
        sandbox_policy: if read_only {
            json!({ "type": "readOnly", "networkAccess": false })
        } else {
            json!({ "type": "workspaceWrite", "networkAccess": false })
        },
        reasoning_effort: Some(config.reasoning_effort.as_str().to_string())
            .filter(|value| value != "none"),
    }
}

fn approval_policy(skip_confirmations: bool) -> &'static str {
    if skip_confirmations {
        APPROVAL_POLICY_AUTOMATIC
    } else {
        APPROVAL_POLICY_INTERACTIVE
    }
}

async fn run_turn(
    client: &CodexAppServerClient,
    request: CodexTurnRequest,
    render_stream: bool,
) -> Result<String> {
    let mut events = client.subscribe();
    let started = client.turn_start(request.clone()).await?;
    let turn_id = started.turn.id;
    let mut output = String::new();

    loop {
        let event = next_event(&mut events).await?;
        if let Some(request_id) = event.id.clone() {
            if approval_request_matches(&event, &request.thread_id, &turn_id) {
                handle_approval_request(client, request_id, &event).await?;
            }
            continue;
        }

        match event.method.as_str() {
            "item/agentMessage/delta"
                if event.params["threadId"].as_str() == Some(request.thread_id.as_str())
                    && event.params["turnId"].as_str() == Some(turn_id.as_str()) =>
            {
                if let Some(delta) = event.params["delta"].as_str() {
                    output.push_str(delta);
                    if render_stream {
                        print!("{delta}");
                        terminal::flush_stdout();
                    }
                }
            }
            "turn/completed"
                if event.params["threadId"].as_str() == Some(request.thread_id.as_str())
                    && event.params["turn"]["id"].as_str() == Some(turn_id.as_str()) =>
            {
                if render_stream && !output.ends_with('\n') {
                    println!();
                }
                let status = event.params["turn"]["status"].as_str().unwrap_or("unknown");
                if status != "completed" {
                    let message = event.params["turn"]["error"]["message"]
                        .as_str()
                        .unwrap_or("turn failed");
                    bail!("Codex turn ended with status '{status}': {message}");
                }
                return Ok(output.trim_end().to_string());
            }
            "error" if event.params["threadId"].as_str() == Some(request.thread_id.as_str()) => {
                let message = event.params["error"]["message"]
                    .as_str()
                    .unwrap_or("Codex turn failed");
                bail!(message.to_string());
            }
            _ => {}
        }
    }
}

fn approval_request_matches(event: &ServerEvent, thread_id: &str, turn_id: &str) -> bool {
    matches!(
        event.method.as_str(),
        "item/commandExecution/requestApproval" | "item/fileChange/requestApproval"
    ) && event.params["threadId"].as_str() == Some(thread_id)
        && event.params["turnId"].as_str() == Some(turn_id)
}

async fn handle_approval_request(
    client: &CodexAppServerClient,
    request_id: Value,
    event: &ServerEvent,
) -> Result<()> {
    let decision = tokio::task::spawn_blocking({
        let method = event.method.clone();
        let params = event.params.clone();
        move || prompt_for_approval_decision(&method, &params)
    })
    .await
    .context("approval prompt task failed")??;

    client.respond_to_server_request(request_id, decision)?;
    Ok(())
}

fn prompt_for_approval_decision(method: &str, params: &Value) -> Result<Value> {
    if terminal::is_piped_input() || terminal::is_piped_output() {
        return Ok(json!({ "decision": "decline" }));
    }

    let (prompt, options) = match method {
        "item/commandExecution/requestApproval" => {
            let command = params["command"].as_str().unwrap_or("command");
            let cwd = params["cwd"].as_str().unwrap_or(".");
            (
                format!("Approve Codex command?\n  {command}\n  cwd: {cwd}"),
                vec![
                    ("Approve once", json!({ "decision": "accept" })),
                    (
                        "Approve for session",
                        json!({ "decision": "acceptForSession" }),
                    ),
                    ("Decline", json!({ "decision": "decline" })),
                    ("Cancel turn", json!({ "decision": "cancel" })),
                ],
            )
        }
        "item/fileChange/requestApproval" => (
            "Approve Codex file changes?".to_string(),
            vec![
                ("Approve once", json!({ "decision": "accept" })),
                (
                    "Approve for session",
                    json!({ "decision": "acceptForSession" }),
                ),
                ("Decline", json!({ "decision": "decline" })),
                ("Cancel turn", json!({ "decision": "cancel" })),
            ],
        ),
        _ => return Ok(json!({ "decision": "decline" })),
    };

    let labels = options.iter().map(|(label, _)| *label).collect::<Vec<_>>();
    let selection = Select::with_theme(&ColorfulTheme::default())
        .with_prompt(prompt)
        .items(&labels)
        .default(0)
        .interact()?;

    Ok(options[selection].1.clone())
}

async fn next_event(receiver: &mut broadcast::Receiver<ServerEvent>) -> Result<ServerEvent> {
    loop {
        match receiver.recv().await {
            Ok(event) => return Ok(event),
            Err(broadcast::error::RecvError::Lagged(_)) => continue,
            Err(broadcast::error::RecvError::Closed) => {
                bail!("lost connection to Codex app-server")
            }
        }
    }
}

fn read_user_prompt() -> Result<Option<String>> {
    tokio::task::block_in_place(|| -> Result<Option<String>> {
        print!("> ");
        terminal::flush_stdout();
        let mut buffer = String::new();
        let bytes_read = std::io::stdin()
            .read_line(&mut buffer)
            .context("failed to read user input")?;
        if bytes_read == 0 {
            Ok(None)
        } else {
            Ok(Some(buffer.trim().to_string()))
        }
    })
}

fn should_exit_session(input: &str) -> bool {
    matches!(input.trim(), "exit" | "quit" | "/exit" | "/quit")
}

fn persist_archive_progress(
    archive: Option<&SessionArchive>,
    messages: &[SessionMessage],
    turn_number: usize,
) -> Result<()> {
    let Some(archive) = archive else {
        return Ok(());
    };

    archive.persist_progress(SessionProgressArgs {
        total_messages: messages.len(),
        distinct_tools: Vec::new(),
        recent_messages: messages.to_vec(),
        turn_number,
        token_usage: None,
        max_context_tokens: None,
        loaded_skills: None,
    })?;
    Ok(())
}

fn finalize_archive(archive: Option<SessionArchive>, messages: Vec<SessionMessage>) -> Result<()> {
    let Some(archive) = archive else {
        return Ok(());
    };

    let transcript = messages
        .iter()
        .map(|message| {
            let role = match message.role {
                MessageRole::User => "user",
                MessageRole::Assistant => "assistant",
                MessageRole::System => "system",
                MessageRole::Tool => "tool",
            };
            format!("{role}: {}", message.content.as_text())
        })
        .collect::<Vec<_>>();

    archive.finalize(transcript, messages.len(), Vec::new(), messages)?;
    Ok(())
}

fn workspace_archive_label(workspace: &std::path::Path) -> String {
    workspace
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("workspace")
        .to_string()
}
