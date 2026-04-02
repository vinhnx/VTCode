use anyhow::{Context, Result};
use serde::de::DeserializeOwned;
use vtcode_config::{SubagentSource, SubagentSpec, builtin_subagents};
use vtcode_core::commands::init::{
    GuidedInitAnswer, GuidedInitAnswers, GuidedInitGrounding, GuidedInitOverwriteState,
    GuidedInitPlan, GuidedInitQuestion, GuidedInitQuestionKey, prepare_guided_init,
    render_agents_md, write_agents_file,
};
use vtcode_core::llm::provider::{AssistantPhase, MessageRole};
use vtcode_core::subagents::SpawnAgentRequest;
use vtcode_core::subagents::SubagentStatusEntry;
use vtcode_core::utils::ansi::MessageStyle;
use vtcode_core::{llm::provider::Message, persistent_memory::scaffold_persistent_memory};
use vtcode_tui::app::{InlineListItem, InlineListSelection, WizardModalMode, WizardStep};

use crate::agent::runloop::unified::turn::workspace::{
    bootstrap_config_files, build_workspace_index,
};
use crate::agent::runloop::unified::wizard_modal::{
    WizardModalOutcome, show_wizard_modal_and_wait,
};

use super::{SlashCommandContext, SlashCommandControl, ui};

const OVERWRITE_PROMPT_ID: &str = "overwrite_agents";
const INIT_GROUNDING_TIMEOUT_MS: u64 = 120_000;
const INIT_GROUNDING_AGENT_NAME: &str = "init-grounding-explorer";
const INIT_GROUNDING_AGENT_PROMPT: &str = r#"You are the VT Code `/init` project grounding explorer.

Inspect the repository directly and extract only the agent-facing facts needed to ground `AGENTS.md`.
Stay read-only, prefer direct repository evidence, and return concise structured results."#;
const INIT_GROUNDING_TASK: &str = r#"Inspect the current repository and ground `/init` setup.

Return JSON only with this shape:
{
  "project_summary": string | null,
  "verification_command": string | null,
  "orientation_doc": string | null,
  "critical_instruction": string | null
}

Requirements:
- Base every field on repository evidence you can inspect directly.
- `project_summary`: one concise sentence describing what this repository is for.
- `verification_command`: the best default command agents should run before claiming work complete, or null.
- `orientation_doc`: the best file path to read first for orientation, or null.
- `critical_instruction`: one repo-wide always-follow instruction, or null.
- Use null for anything that is not well supported by the repository.
- Do not include explanations, markdown fences, or extra keys."#;

pub(crate) async fn handle_initialize_workspace(
    ctx: SlashCommandContext<'_>,
    force: bool,
) -> Result<SlashCommandControl> {
    let mut ctx = ctx;
    let workspace_path = ctx.config.workspace.clone();
    let workspace_label = workspace_path.display().to_string();

    ctx.renderer.line(
        MessageStyle::Info,
        &format!(
            "Initializing VT Code configuration in {}...",
            workspace_label
        ),
    )?;

    let created_files = match bootstrap_config_files(workspace_path.clone(), force).await {
        Ok(files) => files,
        Err(err) => {
            ctx.renderer.line(
                MessageStyle::Error,
                &format!("Failed to initialize configuration: {}", err),
            )?;
            return Ok(SlashCommandControl::Continue);
        }
    };

    if created_files.is_empty() {
        ctx.renderer.line(
            MessageStyle::Info,
            "Existing configuration detected; no files were changed.",
        )?;
    } else {
        ctx.renderer.line(
            MessageStyle::Info,
            &format!(
                "Created {}: {}",
                if created_files.len() == 1 {
                    "file"
                } else {
                    "files"
                },
                created_files.join(", "),
            ),
        )?;
    }

    match prepare_guided_init(workspace_path.as_path(), force) {
        Ok(plan) => {
            let plan = match maybe_ground_project_context(&mut ctx).await {
                Ok(Some(grounding)) => plan.with_grounding(grounding),
                Ok(None) => plan,
                Err(err) => {
                    ctx.renderer.line(
                        MessageStyle::Info,
                        &format!("Skipped VT Code explorer grounding for `/init`: {}", err),
                    )?;
                    plan
                }
            };
            run_guided_agents_generation(&mut ctx, &plan).await?;
        }
        Err(err) => {
            ctx.renderer.line(
                MessageStyle::Error,
                &format!("Failed to analyze workspace for AGENTS.md: {}", err),
            )?;
        }
    }

    let persistent_memory_config = ctx
        .vt_cfg
        .as_ref()
        .map(|cfg| cfg.agent.persistent_memory.clone())
        .unwrap_or_default();
    match scaffold_persistent_memory(&persistent_memory_config, workspace_path.as_path()).await {
        Ok(Some(status)) => {
            ctx.renderer.line(
                MessageStyle::Info,
                &format!("Persistent memory: {}", status.directory.display()),
            )?;
        }
        Ok(None) => {}
        Err(err) => {
            ctx.renderer.line(
                MessageStyle::Error,
                &format!("Failed to scaffold persistent memory: {}", err),
            )?;
        }
    }

    ctx.renderer.line(
        MessageStyle::Info,
        "Indexing workspace context (this may take a moment)...",
    )?;
    match build_workspace_index(workspace_path).await {
        Ok(()) => {
            ctx.renderer.line(
                MessageStyle::Info,
                "Workspace indexing complete. Stored under .vtcode/index.",
            )?;
        }
        Err(err) => {
            ctx.renderer.line(
                MessageStyle::Error,
                &format!("Failed to index workspace: {}", err),
            )?;
        }
    }

    Ok(SlashCommandControl::Continue)
}

async fn maybe_ground_project_context(
    ctx: &mut SlashCommandContext<'_>,
) -> Result<Option<GuidedInitGrounding>> {
    let Some(controller) = ctx.tool_registry.subagent_controller() else {
        return Ok(None);
    };

    ctx.renderer.line(
        MessageStyle::Info,
        "Grounding project context with VT Code explorer subagent...",
    )?;
    let spec = build_init_grounding_subagent_spec(controller.effective_specs().await.as_slice());
    let spawned = controller
        .spawn_custom(
            spec,
            SpawnAgentRequest {
                message: Some(INIT_GROUNDING_TASK.to_owned()),
                max_turns: Some(4),
                ..SpawnAgentRequest::default()
            },
        )
        .await;

    let spawned = match spawned {
        Ok(spawned) => spawned,
        Err(err) => {
            ctx.renderer.line(
                MessageStyle::Info,
                &format!("VT Code explorer grounding unavailable: {}", err),
            )?;
            return Ok(None);
        }
    };

    let grounding = wait_for_init_grounding(ctx, &controller, &spawned).await;
    let _ = controller.close(&spawned.id).await;
    let grounding = grounding.with_context(|| format!("grounding subagent {}", spawned.id))?;
    Ok(grounding)
}

async fn wait_for_init_grounding(
    ctx: &mut SlashCommandContext<'_>,
    controller: &std::sync::Arc<vtcode_core::subagents::SubagentController>,
    spawned: &SubagentStatusEntry,
) -> Result<Option<GuidedInitGrounding>> {
    let Some(status) = controller
        .wait(
            std::slice::from_ref(&spawned.id),
            Some(INIT_GROUNDING_TIMEOUT_MS),
        )
        .await?
    else {
        ctx.renderer.line(
            MessageStyle::Info,
            "VT Code explorer grounding timed out; continuing with repository heuristics.",
        )?;
        return Ok(None);
    };

    let grounding = parse_grounding_from_status(&status)
        .or_else(|| status.error.as_deref().and_then(parse_grounding_from_text));
    let grounding = match grounding {
        Some(grounding) => Some(grounding),
        None => parse_grounding_from_snapshot(controller, &spawned.id).await,
    };
    if let Some(grounding) = grounding {
        ctx.renderer.line(
            MessageStyle::Info,
            "VT Code explorer grounding added repo context to `/init` suggestions.",
        )?;
        return Ok(Some(grounding));
    }

    let status_message = status
        .error
        .as_deref()
        .unwrap_or_else(|| status.status.as_str());
    ctx.renderer.line(
        MessageStyle::Info,
        &format!(
            "VT Code explorer grounding finished without usable structured output ({}).",
            status_message
        ),
    )?;
    Ok(None)
}

fn build_init_grounding_subagent_spec(effective_specs: &[SubagentSpec]) -> SubagentSpec {
    let mut spec = effective_specs
        .iter()
        .find(|candidate| candidate.matches_name("explorer") && candidate.is_read_only())
        .cloned()
        .or_else(|| {
            builtin_subagents()
                .into_iter()
                .find(|candidate| candidate.name == "explorer")
        })
        .expect("builtin explorer subagent");
    spec.name = INIT_GROUNDING_AGENT_NAME.to_string();
    spec.description =
        "VT Code explorer specialized for grounding `/init` AGENTS.md suggestions.".to_string();
    spec.prompt = if spec.prompt.trim().is_empty() {
        INIT_GROUNDING_AGENT_PROMPT.to_string()
    } else {
        format!("{}\n\n{}", spec.prompt.trim(), INIT_GROUNDING_AGENT_PROMPT)
    };
    spec.background = false;
    spec.initial_prompt = None;
    spec.nickname_candidates = vec!["init-grounding".to_string()];
    spec.aliases.clear();
    spec.source = SubagentSource::ProjectVtcode;
    spec.file_path = None;
    spec.warnings.clear();
    spec
}

fn parse_grounding_from_status(status: &SubagentStatusEntry) -> Option<GuidedInitGrounding> {
    status
        .summary
        .as_deref()
        .and_then(parse_grounding_from_text)
}

async fn parse_grounding_from_snapshot(
    controller: &std::sync::Arc<vtcode_core::subagents::SubagentController>,
    target: &str,
) -> Option<GuidedInitGrounding> {
    let snapshot = controller.snapshot_for_thread(target).await.ok()?;
    extract_grounding_from_messages(&snapshot.snapshot.messages)
}

fn extract_grounding_from_messages(messages: &[Message]) -> Option<GuidedInitGrounding> {
    messages
        .iter()
        .rev()
        .filter(|message| message.role == MessageRole::Assistant)
        .find_map(|message| {
            if message.phase == Some(AssistantPhase::FinalAnswer) || message.phase.is_none() {
                parse_grounding_from_text(message.get_text_content().as_ref())
            } else {
                None
            }
        })
}

fn parse_grounding_from_text(text: &str) -> Option<GuidedInitGrounding> {
    let grounding = parse_json_like::<GuidedInitGrounding>(text).ok()?;
    grounding.has_any().then_some(grounding)
}

fn parse_json_like<T>(text: &str) -> Result<T>
where
    T: DeserializeOwned,
{
    let trimmed = text.trim();
    if trimmed.is_empty() {
        anyhow::bail!("empty grounding payload");
    }

    if let Ok(parsed) = serde_json::from_str::<T>(trimmed) {
        return Ok(parsed);
    }

    if let Some(json_block) = extract_first_json_block(trimmed) {
        return serde_json::from_str::<T>(json_block).context("decode grounded json block");
    }

    serde_json::from_str::<T>(trimmed).context("decode grounding payload")
}

fn extract_first_json_block(text: &str) -> Option<&str> {
    let (start, opening) = text
        .char_indices()
        .find(|(_, ch)| matches!(ch, '{' | '['))?;
    let mut stack = vec![opening];
    let mut in_string = false;
    let mut escaped = false;

    for (offset, ch) in text[start + opening.len_utf8()..].char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }

        match ch {
            '"' => in_string = true,
            '{' | '[' => stack.push(ch),
            '}' => {
                if stack.pop() != Some('{') {
                    return None;
                }
            }
            ']' => {
                if stack.pop() != Some('[') {
                    return None;
                }
            }
            _ => {}
        }

        if stack.is_empty() {
            return Some(&text[start..=start + opening.len_utf8() + offset]);
        }
    }

    None
}

async fn run_guided_agents_generation(
    ctx: &mut SlashCommandContext<'_>,
    plan: &GuidedInitPlan,
) -> Result<()> {
    let should_write = match plan.overwrite_state {
        GuidedInitOverwriteState::Skip | GuidedInitOverwriteState::Force => true,
        GuidedInitOverwriteState::Confirm => {
            if !can_use_inline_wizard(ctx, "confirming AGENTS.md overwrite")? {
                ctx.renderer.line(
                    MessageStyle::Info,
                    "AGENTS.md already exists. Re-run `/init --force` to overwrite without an interactive confirmation.",
                )?;
                false
            } else {
                match prompt_overwrite_confirmation(ctx, &plan.path).await? {
                    Some(result) => result,
                    None => {
                        ctx.renderer
                            .line(MessageStyle::Info, "AGENTS.md generation cancelled.")?;
                        false
                    }
                }
            }
        }
    };

    if !should_write {
        return Ok(());
    }

    let answers = if plan.questions.is_empty() {
        GuidedInitAnswers::default()
    } else if can_use_inline_wizard(ctx, "answering guided `/init` questions")? {
        match prompt_guided_answers(ctx, &plan.questions).await? {
            Some(answers) => answers,
            None => {
                ctx.renderer
                    .line(MessageStyle::Info, "AGENTS.md generation cancelled.")?;
                return Ok(());
            }
        }
    } else {
        ctx.renderer.line(
            MessageStyle::Info,
            "Guided `/init` questions require inline UI. Using inferred defaults for AGENTS.md.",
        )?;
        GuidedInitAnswers::default()
    };

    let content = render_agents_md(plan, &answers)?;
    let overwrite_existing = matches!(
        plan.overwrite_state,
        GuidedInitOverwriteState::Confirm | GuidedInitOverwriteState::Force
    );
    match write_agents_file(ctx.config.workspace.as_path(), &content, overwrite_existing) {
        Ok(report) => {
            ctx.renderer.line(
                MessageStyle::Info,
                &format!("AGENTS.md: {}", report.path.display()),
            )?;
        }
        Err(err) => {
            ctx.renderer.line(
                MessageStyle::Error,
                &format!("Failed to scaffold AGENTS.md: {}", err),
            )?;
        }
    }

    Ok(())
}

fn can_use_inline_wizard(ctx: &mut SlashCommandContext<'_>, activity: &str) -> Result<bool> {
    if !ctx.renderer.supports_inline_ui() {
        return Ok(false);
    }
    ui::ensure_selection_ui_available(ctx, activity)
}

async fn prompt_overwrite_confirmation(
    ctx: &mut SlashCommandContext<'_>,
    path: &std::path::Path,
) -> Result<Option<bool>> {
    let step = WizardStep {
        title: "Overwrite".to_string(),
        question: format!(
            "AGENTS.md already exists at {}. Overwrite it?",
            path.display()
        ),
        items: vec![
            InlineListItem {
                title: "1. Overwrite existing AGENTS.md".to_string(),
                subtitle: Some("Replace the file with the newly generated guidance.".to_string()),
                badge: None,
                indent: 0,
                selection: Some(InlineListSelection::RequestUserInputAnswer {
                    question_id: OVERWRITE_PROMPT_ID.to_string(),
                    selected: vec!["overwrite".to_string()],
                    other: None,
                }),
                search_value: Some("overwrite replace yes".to_string()),
            },
            InlineListItem {
                title: "2. Keep current AGENTS.md".to_string(),
                subtitle: Some(
                    "Skip AGENTS.md generation and leave the existing file untouched.".to_string(),
                ),
                badge: None,
                indent: 0,
                selection: Some(InlineListSelection::RequestUserInputAnswer {
                    question_id: OVERWRITE_PROMPT_ID.to_string(),
                    selected: vec!["keep".to_string()],
                    other: None,
                }),
                search_value: Some("keep skip no".to_string()),
            },
        ],
        completed: false,
        answer: None,
        allow_freeform: false,
        freeform_label: None,
        freeform_placeholder: None,
        freeform_default: None,
    };

    let outcome = show_wizard_modal_and_wait(
        ctx.handle,
        ctx.session,
        "AGENTS.md".to_string(),
        vec![step],
        0,
        None,
        WizardModalMode::MultiStep,
        ctx.ctrl_c_state,
        ctx.ctrl_c_notify,
    )
    .await?;

    Ok(match outcome {
        WizardModalOutcome::Submitted(selections) => {
            selections
                .into_iter()
                .find_map(|selection| match selection {
                    InlineListSelection::RequestUserInputAnswer {
                        question_id,
                        selected,
                        ..
                    } if question_id == OVERWRITE_PROMPT_ID => {
                        selected.first().map(|value| value == "overwrite")
                    }
                    _ => None,
                })
        }
        WizardModalOutcome::Cancelled { .. } => None,
    })
}

async fn prompt_guided_answers(
    ctx: &mut SlashCommandContext<'_>,
    questions: &[GuidedInitQuestion],
) -> Result<Option<GuidedInitAnswers>> {
    let steps = questions
        .iter()
        .map(build_question_step)
        .collect::<Vec<_>>();

    let outcome = show_wizard_modal_and_wait(
        ctx.handle,
        ctx.session,
        "Guided /init".to_string(),
        steps,
        0,
        None,
        WizardModalMode::MultiStep,
        ctx.ctrl_c_state,
        ctx.ctrl_c_notify,
    )
    .await?;

    Ok(match outcome {
        WizardModalOutcome::Submitted(selections) => {
            let mut answers = GuidedInitAnswers::default();
            for selection in selections {
                if let Some(answer) = selection_to_answer(selection) {
                    answers.insert(answer);
                }
            }
            Some(answers)
        }
        WizardModalOutcome::Cancelled { .. } => None,
    })
}

fn build_question_step(question: &GuidedInitQuestion) -> WizardStep {
    let mut items = question
        .options
        .iter()
        .enumerate()
        .map(|(index, option)| InlineListItem {
            title: format!(
                "{}. {}{}",
                index + 1,
                option.label,
                if option.recommended {
                    " (Recommended)"
                } else {
                    ""
                }
            ),
            subtitle: Some(option.description.clone()),
            badge: None,
            indent: 0,
            selection: Some(InlineListSelection::RequestUserInputAnswer {
                question_id: question.key.as_str().to_string(),
                selected: vec![option.value.clone()],
                other: None,
            }),
            search_value: Some(format!("{} {}", option.label, option.description)),
        })
        .collect::<Vec<_>>();

    if question.allow_custom {
        items.push(InlineListItem {
            title: format!("{}. Custom", items.len() + 1),
            subtitle: Some("Type a custom answer inline, then press Enter.".to_string()),
            badge: None,
            indent: 0,
            selection: Some(InlineListSelection::RequestUserInputAnswer {
                question_id: question.key.as_str().to_string(),
                selected: vec![],
                other: Some(String::new()),
            }),
            search_value: Some("custom freeform".to_string()),
        });
    }

    WizardStep {
        title: question.header.clone(),
        question: question.prompt.clone(),
        items,
        completed: false,
        answer: None,
        allow_freeform: question.allow_custom,
        freeform_label: question
            .allow_custom
            .then(|| question.key.custom_label().to_string()),
        freeform_placeholder: question
            .allow_custom
            .then(|| question.key.custom_placeholder().to_string()),
        freeform_default: None,
    }
}

fn selection_to_answer(selection: InlineListSelection) -> Option<GuidedInitAnswer> {
    match selection {
        InlineListSelection::RequestUserInputAnswer {
            question_id,
            selected,
            other,
        } => {
            let key = question_id.parse::<GuidedInitQuestionKey>().ok()?;
            GuidedInitAnswer::from_input(
                key,
                selected.first().map(String::as_str),
                other.as_deref(),
            )
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vtcode_core::llm::provider::{AssistantPhase, Message};

    #[test]
    fn init_grounding_subagent_spec_is_vtcode_native_and_read_only() {
        let spec = build_init_grounding_subagent_spec(&[]);

        assert_eq!(spec.name, INIT_GROUNDING_AGENT_NAME);
        assert_eq!(spec.source, SubagentSource::ProjectVtcode);
        assert!(spec.is_read_only());
        assert_eq!(
            spec.nickname_candidates.as_slice(),
            ["init-grounding".to_string()]
        );
        assert!(spec.prompt.contains("`/init`"));
    }

    #[test]
    fn init_grounding_subagent_spec_uses_read_only_explorer_as_base() {
        let spec = build_init_grounding_subagent_spec(&[SubagentSpec {
            name: "explorer".to_string(),
            description: "Project explorer".to_string(),
            prompt: "Use project-specific search guidance.".to_string(),
            tools: Some(vec!["unified_search".to_string()]),
            disallowed_tools: vec!["unified_file".to_string()],
            model: Some("inherit".to_string()),
            color: Some("green".to_string()),
            reasoning_effort: Some("medium".to_string()),
            permission_mode: Some(vtcode_core::config::PermissionMode::Plan),
            skills: vec!["repo-skill".to_string()],
            mcp_servers: Vec::new(),
            hooks: None,
            background: true,
            max_turns: Some(9),
            nickname_candidates: vec!["repo".to_string()],
            initial_prompt: Some("ignored".to_string()),
            memory: None,
            isolation: None,
            aliases: vec!["explore".to_string()],
            source: SubagentSource::ProjectVtcode,
            file_path: None,
            warnings: vec!["warning".to_string()],
        }]);

        assert_eq!(spec.model.as_deref(), Some("inherit"));
        assert_eq!(spec.skills.as_slice(), ["repo-skill".to_string()]);
        assert!(!spec.background);
        assert_eq!(spec.initial_prompt, None);
        assert!(
            spec.prompt
                .contains("Use project-specific search guidance.")
        );
        assert!(spec.prompt.contains("`/init`"));
    }

    #[test]
    fn parse_grounding_from_text_accepts_fenced_json() {
        let grounding = parse_grounding_from_text(
            r#"
Before the payload:
```json
{
  "project_summary": "Terminal-first coding agent for repository work.",
  "verification_command": "cargo nextest run",
  "orientation_doc": "docs/ARCHITECTURE.md",
  "critical_instruction": null
}
```
"#,
        )
        .expect("grounding");

        assert_eq!(
            grounding.project_summary.as_deref(),
            Some("Terminal-first coding agent for repository work.")
        );
        assert_eq!(
            grounding.verification_command.as_deref(),
            Some("cargo nextest run")
        );
    }

    #[test]
    fn extract_grounding_from_messages_prefers_final_assistant_answer() {
        let messages = vec![
            Message::assistant("working".to_owned()).with_phase(Some(AssistantPhase::Commentary)),
            Message::assistant(
                r#"{"project_summary":"Grounded summary","verification_command":null,"orientation_doc":null,"critical_instruction":null}"#
                    .to_owned(),
            )
            .with_phase(Some(AssistantPhase::FinalAnswer)),
        ];

        let grounding = extract_grounding_from_messages(&messages).expect("grounding");
        assert_eq!(
            grounding.project_summary.as_deref(),
            Some("Grounded summary")
        );
    }
}
