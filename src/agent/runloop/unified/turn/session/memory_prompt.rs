use anyhow::Result;
use vtcode_core::llm::provider as uni;
use vtcode_core::persistent_memory::{
    MemoryOpCandidate, MemoryOpKind, MemoryOpPlan, cleanup_persistent_memory,
    forget_planned_persistent_memory_matches, list_persistent_memory_candidates,
    persist_remembered_memory_plan, persistent_memory_status, plan_forget_persistent_memory,
    plan_remember_persistent_memory,
};
use vtcode_core::utils::ansi::MessageStyle;
use vtcode_tui::app::{InlineListItem, InlineListSelection, WizardModalMode, WizardStep};

use crate::agent::runloop::slash_commands::SlashCommandOutcome;
use crate::agent::runloop::unified::display::display_user_message;
use crate::agent::runloop::unified::turn::session::interaction_loop::{
    InteractionLoopContext, InteractionOutcome, InteractionState,
};
use crate::agent::runloop::unified::turn::session::slash_commands::{
    SlashCommandContext, SlashCommandControl, handle_outcome,
};
use crate::agent::runloop::unified::ui_interaction::start_loading_status;
use crate::agent::runloop::unified::wizard_modal::{
    WizardModalOutcome, show_wizard_modal_and_wait,
};

const MEMORY_MISSING_QUESTION_ID: &str = "memory.missing";
const MEMORY_CONFIRM_ACCEPT: &str = "memory.confirm.accept";
const MEMORY_CONFIRM_CANCEL: &str = "memory.confirm.cancel";
const MEMORY_CLEANUP_ACCEPT: &str = "memory.cleanup.accept";
const MEMORY_CLEANUP_CANCEL: &str = "memory.cleanup.cancel";
const MEMORY_MATCH_PREVIEW_LIMIT: usize = 5;

#[derive(Debug, Clone, PartialEq, Eq)]
enum MemoryPromptIntent {
    Remember { request: String },
    Forget { request: String },
    Show,
}

enum MemoryPromptConfirmation {
    Confirmed,
    Cancelled,
    Unavailable,
}

fn memory_operation_notice(intent: &MemoryPromptIntent) -> &'static str {
    match intent {
        MemoryPromptIntent::Remember { .. } => {
            "Memory save requested. VT Code is preparing a normalized note."
        }
        MemoryPromptIntent::Forget { .. } => {
            "Memory removal requested. VT Code is matching normalized notes."
        }
        MemoryPromptIntent::Show => "Opening persistent memory view.",
    }
}

fn start_memory_loading(
    ctx: &InteractionLoopContext<'_>,
    state: &InteractionState<'_>,
    message: impl Into<String>,
) -> crate::agent::runloop::unified::ui_interaction::PlaceholderSpinner {
    start_loading_status(ctx.handle, state.input_status_state, message)
}

fn cleanup_fingerprint(
    status: &vtcode_core::persistent_memory::PersistentMemoryStatus,
) -> (usize, usize) {
    (
        status.cleanup_status.suspicious_facts,
        status.cleanup_status.suspicious_summary_lines,
    )
}

fn should_suppress_cleanup_confirmation(
    dismissed_fingerprint: Option<(usize, usize)>,
    status: &vtcode_core::persistent_memory::PersistentMemoryStatus,
) -> bool {
    dismissed_fingerprint == Some(cleanup_fingerprint(status))
}

pub(crate) async fn handle_memory_prompt(
    input: &str,
    ctx: &mut InteractionLoopContext<'_>,
    state: &mut InteractionState<'_>,
) -> Result<Option<InteractionOutcome>> {
    let Some(intent) = detect_memory_prompt_intent(input) else {
        return Ok(None);
    };

    ctx.renderer
        .line(MessageStyle::Info, memory_operation_notice(&intent))?;

    match intent {
        MemoryPromptIntent::Show => handle_show_memory_intent(ctx, state).await,
        MemoryPromptIntent::Remember { request } => {
            if !ctx
                .vt_cfg
                .as_ref()
                .map(|cfg| cfg.agent.persistent_memory.enabled)
                .unwrap_or(true)
            {
                respond_to_memory_prompt(
                    ctx,
                    input,
                    "Persistent memory is disabled. Use `/memory` or `/config memory` to enable it.",
                )?;
                return Ok(Some(InteractionOutcome::DirectToolHandled));
            }

            if !maybe_cleanup_before_memory_mutation(ctx, state, input).await? {
                return Ok(Some(InteractionOutcome::DirectToolHandled));
            }

            let Some(plan) = resolve_remember_plan(ctx, state, input, &request).await? else {
                return Ok(Some(InteractionOutcome::DirectToolHandled));
            };
            if plan.kind == MemoryOpKind::Noop {
                respond_to_memory_prompt(
                    ctx,
                    input,
                    plan.message
                        .as_deref()
                        .unwrap_or("No durable memory note was identified, so nothing changed."),
                )?;
                return Ok(Some(InteractionOutcome::DirectToolHandled));
            }

            match confirm_memory_plan(
                ctx,
                state,
                "Save Memory Note",
                "save",
                &describe_memory_plan(&plan),
            )
            .await?
            {
                MemoryPromptConfirmation::Confirmed => {}
                MemoryPromptConfirmation::Cancelled => {
                    respond_to_memory_prompt(ctx, input, "Cancelled memory save.")?;
                    return Ok(Some(InteractionOutcome::DirectToolHandled));
                }
                MemoryPromptConfirmation::Unavailable => {
                    respond_to_memory_prompt(
                        ctx,
                        input,
                        "Memory updates require the inline confirmation UI. Open `/memory` in inline UI to continue.",
                    )?;
                    return Ok(Some(InteractionOutcome::DirectToolHandled));
                }
            }

            let save_spinner = start_memory_loading(ctx, state, "Saving memory note...");
            let reply = match persist_remembered_memory_plan(ctx.config, ctx.vt_cfg.as_ref(), &plan)
                .await
            {
                Ok(Some(report)) if report.added_facts > 0 => format!(
                    "Saved {} normalized memory note(s) under {}.",
                    report.added_facts,
                    report.directory.display()
                ),
                Ok(Some(report)) => format!(
                    "No new memory note was added. The normalized fact may already exist in {}.",
                    report.directory.display()
                ),
                Ok(None) => "No memory note was added.".to_string(),
                Err(err) => format!(
                    "Couldn't save memory. VT Code blocks memory writes unless the LLM planner succeeds: {}",
                    err
                ),
            };
            drop(save_spinner);
            respond_to_memory_prompt(ctx, input, &reply)?;
            Ok(Some(InteractionOutcome::DirectToolHandled))
        }
        MemoryPromptIntent::Forget { request } => {
            if !ctx
                .vt_cfg
                .as_ref()
                .map(|cfg| cfg.agent.persistent_memory.enabled)
                .unwrap_or(true)
            {
                respond_to_memory_prompt(
                    ctx,
                    input,
                    "Persistent memory is disabled. Use `/memory` or `/config memory` to enable it.",
                )?;
                return Ok(Some(InteractionOutcome::DirectToolHandled));
            }

            if !maybe_cleanup_before_memory_mutation(ctx, state, input).await? {
                return Ok(Some(InteractionOutcome::DirectToolHandled));
            }

            let candidates = load_memory_candidates(ctx).await?;
            if candidates.is_empty() {
                respond_to_memory_prompt(
                    ctx,
                    input,
                    "No persistent memory notes are available to forget.",
                )?;
                return Ok(Some(InteractionOutcome::DirectToolHandled));
            }

            let plan_spinner = start_memory_loading(ctx, state, "Planning memory removal...");
            let plan = match plan_forget_persistent_memory(
                ctx.config,
                ctx.vt_cfg.as_ref(),
                &request,
                &candidates,
            )
            .await
            {
                Ok(Some(plan)) => plan,
                Ok(None) => {
                    drop(plan_spinner);
                    respond_to_memory_prompt(ctx, input, "Persistent memory is disabled.")?;
                    return Ok(Some(InteractionOutcome::DirectToolHandled));
                }
                Err(err) => {
                    drop(plan_spinner);
                    respond_to_memory_prompt(
                        ctx,
                        input,
                        &format!(
                            "Couldn't remove memory. VT Code blocks memory writes unless the LLM planner succeeds: {}",
                            err
                        ),
                    )?;
                    return Ok(Some(InteractionOutcome::DirectToolHandled));
                }
            };
            drop(plan_spinner);
            if plan.kind == MemoryOpKind::Noop || plan.selected_ids.is_empty() {
                respond_to_memory_prompt(
                    ctx,
                    input,
                    plan.message
                        .as_deref()
                        .unwrap_or("No matching persistent memory notes were found."),
                )?;
                return Ok(Some(InteractionOutcome::DirectToolHandled));
            }

            match confirm_memory_plan(
                ctx,
                state,
                "Forget Memory Note",
                "forget",
                &describe_forget_selection(&candidates, &plan),
            )
            .await?
            {
                MemoryPromptConfirmation::Confirmed => {}
                MemoryPromptConfirmation::Cancelled => {
                    respond_to_memory_prompt(ctx, input, "Cancelled memory removal.")?;
                    return Ok(Some(InteractionOutcome::DirectToolHandled));
                }
                MemoryPromptConfirmation::Unavailable => {
                    respond_to_memory_prompt(
                        ctx,
                        input,
                        "Memory removals require the inline confirmation UI. Open `/memory` in inline UI to continue.",
                    )?;
                    return Ok(Some(InteractionOutcome::DirectToolHandled));
                }
            }

            let remove_spinner = start_memory_loading(ctx, state, "Removing memory note...");
            let reply = match forget_planned_persistent_memory_matches(
                ctx.config,
                ctx.vt_cfg.as_ref(),
                &candidates,
                &plan,
            )
            .await
            {
                Ok(Some(report)) if report.removed_facts > 0 => format!(
                    "Removed {} matching memory note(s) from {}.",
                    report.removed_facts,
                    report.directory.display()
                ),
                Ok(Some(_)) | Ok(None) => {
                    "No matching persistent memory notes were removed.".to_string()
                }
                Err(err) => format!(
                    "Couldn't remove memory. VT Code blocks memory writes unless the LLM planner succeeds: {}",
                    err
                ),
            };
            drop(remove_spinner);
            respond_to_memory_prompt(ctx, input, &reply)?;
            Ok(Some(InteractionOutcome::DirectToolHandled))
        }
    }
}

async fn handle_show_memory_intent(
    ctx: &mut InteractionLoopContext<'_>,
    state: &mut InteractionState<'_>,
) -> Result<Option<InteractionOutcome>> {
    let control = handle_outcome(
        SlashCommandOutcome::ShowMemory,
        SlashCommandContext {
            thread_id: ctx.thread_id,
            active_thread_label: ctx.active_thread_label,
            renderer: ctx.renderer,
            handle: ctx.handle,
            session: ctx.session,
            header_context: ctx.header_context,
            ide_context_bridge: ctx.ide_context_bridge,
            config: ctx.config,
            vt_cfg: ctx.vt_cfg,
            provider_client: ctx.provider_client,
            session_bootstrap: ctx.session_bootstrap,
            model_picker_state: state.model_picker_state,
            palette_state: state.palette_state,
            tool_registry: ctx.tool_registry,
            conversation_history: ctx.conversation_history,
            decision_ledger: ctx.decision_ledger,
            context_manager: ctx.context_manager,
            session_stats: ctx.session_stats,
            input_status_state: state.input_status_state,
            tools: ctx.tools,
            tool_catalog: ctx.tool_catalog,
            async_mcp_manager: ctx.async_mcp_manager.as_ref(),
            mcp_panel_state: ctx.mcp_panel_state,
            linked_directories: ctx.linked_directories,
            ctrl_c_state: ctx.ctrl_c_state,
            ctrl_c_notify: ctx.ctrl_c_notify,
            full_auto: ctx.full_auto,
            loaded_skills: ctx.loaded_skills,
            checkpoint_manager: ctx.checkpoint_manager,
            lifecycle_hooks: ctx.lifecycle_hooks,
            harness_emitter: ctx.harness_emitter,
        },
    )
    .await?;

    match control {
        SlashCommandControl::Continue => Ok(Some(InteractionOutcome::DirectToolHandled)),
        SlashCommandControl::SubmitPrompt(prompt) => Ok(Some(InteractionOutcome::Continue {
            input: prompt,
            prompt_message_index: None,
        })),
        SlashCommandControl::ReplaceInput(content) => {
            ctx.handle.set_input(content);
            Ok(Some(InteractionOutcome::DirectToolHandled))
        }
        SlashCommandControl::BreakWithReason(reason) => {
            Ok(Some(InteractionOutcome::Exit { reason }))
        }
    }
}

async fn maybe_cleanup_before_memory_mutation(
    ctx: &mut InteractionLoopContext<'_>,
    state: &mut InteractionState<'_>,
    input: &str,
) -> Result<bool> {
    let memory_config = ctx
        .vt_cfg
        .as_ref()
        .map(|cfg| cfg.agent.persistent_memory.clone())
        .unwrap_or_default();
    let status = persistent_memory_status(&memory_config, &ctx.config.workspace)?;
    if !status.cleanup_status.needed {
        *state.dismissed_memory_cleanup_fingerprint = None;
        return Ok(true);
    }

    let fingerprint = cleanup_fingerprint(&status);
    if should_suppress_cleanup_confirmation(*state.dismissed_memory_cleanup_fingerprint, &status) {
        respond_to_memory_prompt(
            ctx,
            input,
            "Persistent memory still needs one-time cleanup before VT Code can change it. Use `/memory` to run cleanup when you're ready.",
        )?;
        return Ok(false);
    }

    ctx.renderer.line(
        MessageStyle::Info,
        "Persistent memory needs one-time cleanup. VT Code is asking for confirmation before continuing.",
    )?;
    match confirm_memory_cleanup(ctx, state, &status).await? {
        MemoryPromptConfirmation::Confirmed => {
            *state.dismissed_memory_cleanup_fingerprint = None;
        }
        MemoryPromptConfirmation::Cancelled => {
            *state.dismissed_memory_cleanup_fingerprint = Some(fingerprint);
            respond_to_memory_prompt(
                ctx,
                input,
                "Persistent memory needs one-time cleanup before VT Code can change it. Use `/memory` to run cleanup when you're ready.",
            )?;
            return Ok(false);
        }
        MemoryPromptConfirmation::Unavailable => {
            respond_to_memory_prompt(
                ctx,
                input,
                "Persistent memory needs one-time cleanup. Open `/memory` in inline UI to confirm cleanup before changing memory.",
            )?;
            return Ok(false);
        }
    }

    let cleanup_spinner = start_memory_loading(ctx, state, "Cleaning persistent memory...");
    match cleanup_persistent_memory(ctx.config, ctx.vt_cfg.as_ref(), true).await {
        Ok(Some(report)) => {
            drop(cleanup_spinner);
            ctx.renderer.line(
                MessageStyle::Info,
                &format!(
                    "Cleaned persistent memory under {} and rebuilt the normalized files.",
                    report.directory.display()
                ),
            )?;
            Ok(true)
        }
        Ok(None) => {
            drop(cleanup_spinner);
            Ok(true)
        }
        Err(err) => {
            *state.dismissed_memory_cleanup_fingerprint = Some(fingerprint);
            drop(cleanup_spinner);
            respond_to_memory_prompt(
                ctx,
                input,
                &format!(
                    "Persistent memory cleanup failed, so VT Code did not change memory: {}",
                    err
                ),
            )?;
            Ok(false)
        }
    }
}

async fn resolve_remember_plan(
    ctx: &mut InteractionLoopContext<'_>,
    state: &mut InteractionState<'_>,
    input: &str,
    request: &str,
) -> Result<Option<MemoryOpPlan>> {
    let mut supplemental: Option<String> = None;
    for _ in 0..2 {
        let plan_spinner = start_memory_loading(ctx, state, "Planning memory save...");
        let plan = match plan_remember_persistent_memory(
            ctx.config,
            ctx.vt_cfg.as_ref(),
            request,
            supplemental.as_deref(),
        )
        .await
        {
            Ok(Some(plan)) => plan,
            Ok(None) => {
                drop(plan_spinner);
                return Ok(None);
            }
            Err(err) => {
                drop(plan_spinner);
                respond_to_memory_prompt(
                    ctx,
                    input,
                    &format!(
                        "Couldn't save memory. VT Code blocks memory writes unless the LLM planner succeeds: {}",
                        err
                    ),
                )?;
                return Ok(None);
            }
        };
        drop(plan_spinner);

        if plan.kind != MemoryOpKind::AskMissing {
            return Ok(Some(plan));
        }

        let Some(missing) = plan.missing.as_ref() else {
            respond_to_memory_prompt(
                ctx,
                input,
                "Couldn't save memory because the LLM planner did not return the missing information prompt.",
            )?;
            return Ok(None);
        };
        let Some(answer) =
            prompt_missing_memory_value(ctx, state, &missing.field, &missing.prompt).await?
        else {
            respond_to_memory_prompt(ctx, input, "Cancelled memory save.")?;
            return Ok(None);
        };
        supplemental = Some(answer);
    }

    respond_to_memory_prompt(
        ctx,
        input,
        "Couldn't save memory because the LLM planner still needs more information.",
    )?;
    Ok(None)
}

async fn load_memory_candidates(
    ctx: &mut InteractionLoopContext<'_>,
) -> Result<Vec<MemoryOpCandidate>> {
    let memory_config = ctx
        .vt_cfg
        .as_ref()
        .map(|cfg| cfg.agent.persistent_memory.clone())
        .unwrap_or_default();
    let matches = list_persistent_memory_candidates(&memory_config, &ctx.config.workspace)
        .await?
        .unwrap_or_default();
    Ok(matches
        .into_iter()
        .enumerate()
        .map(|(index, entry)| MemoryOpCandidate {
            id: index,
            source: entry.source,
            fact: entry.fact,
        })
        .collect())
}

fn describe_memory_plan(plan: &MemoryOpPlan) -> String {
    plan.facts
        .iter()
        .map(|fact| {
            let topic = match fact.topic {
                vtcode_core::persistent_memory::MemoryPlannedTopic::Preferences => "preferences",
                vtcode_core::persistent_memory::MemoryPlannedTopic::RepositoryFacts => {
                    "repository facts"
                }
            };
            format!("- [{}] {}", topic, fact.fact)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn describe_forget_selection(candidates: &[MemoryOpCandidate], plan: &MemoryOpPlan) -> String {
    let selected = plan
        .selected_ids
        .iter()
        .filter_map(|id| candidates.iter().find(|candidate| candidate.id == *id))
        .take(MEMORY_MATCH_PREVIEW_LIMIT)
        .map(|candidate| format!("- [{}] {}", candidate.source, candidate.fact))
        .collect::<Vec<_>>();
    if selected.is_empty() {
        "- No matching notes selected.".to_string()
    } else {
        selected.join("\n")
    }
}

async fn confirm_memory_plan(
    ctx: &mut InteractionLoopContext<'_>,
    state: &mut InteractionState<'_>,
    title: &str,
    action_label: &str,
    body: &str,
) -> Result<MemoryPromptConfirmation> {
    if !ctx.renderer.supports_inline_ui() {
        return Ok(MemoryPromptConfirmation::Unavailable);
    }
    if state.model_picker_state.is_some() || state.palette_state.is_some() {
        ctx.renderer.line(
            MessageStyle::Info,
            "Close the active picker before confirming a memory action.",
        )?;
        return Ok(MemoryPromptConfirmation::Unavailable);
    }

    let confirm_step = WizardStep {
        title: "Confirm".to_string(),
        question: format!(
            "Review the normalized memory action below, then confirm.\n\n{}",
            body
        ),
        items: vec![
            InlineListItem {
                title: format!("Confirm {}", action_label),
                subtitle: Some("Apply the memory change now.".to_string()),
                badge: Some("Confirm".to_string()),
                indent: 0,
                selection: Some(InlineListSelection::ConfigAction(
                    MEMORY_CONFIRM_ACCEPT.to_string(),
                )),
                search_value: Some("confirm accept yes".to_string()),
            },
            InlineListItem {
                title: "Cancel".to_string(),
                subtitle: Some("Dismiss without changing memory.".to_string()),
                badge: None,
                indent: 0,
                selection: Some(InlineListSelection::ConfigAction(
                    MEMORY_CONFIRM_CANCEL.to_string(),
                )),
                search_value: Some("cancel no dismiss".to_string()),
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
        title.to_string(),
        vec![confirm_step],
        0,
        None,
        WizardModalMode::MultiStep,
        ctx.ctrl_c_state,
        ctx.ctrl_c_notify,
    )
    .await?;

    let WizardModalOutcome::Submitted(selections) = outcome else {
        return Ok(MemoryPromptConfirmation::Cancelled);
    };

    let confirmed = selections.iter().any(|selection| {
        matches!(
            selection,
            InlineListSelection::ConfigAction(action) if action == MEMORY_CONFIRM_ACCEPT
        )
    });

    if !confirmed {
        return Ok(MemoryPromptConfirmation::Cancelled);
    }

    Ok(MemoryPromptConfirmation::Confirmed)
}

async fn confirm_memory_cleanup(
    ctx: &mut InteractionLoopContext<'_>,
    state: &mut InteractionState<'_>,
    status: &vtcode_core::persistent_memory::PersistentMemoryStatus,
) -> Result<MemoryPromptConfirmation> {
    if !ctx.renderer.supports_inline_ui() {
        return Ok(MemoryPromptConfirmation::Unavailable);
    }
    if state.model_picker_state.is_some() || state.palette_state.is_some() {
        ctx.renderer.line(
            MessageStyle::Info,
            "Close the active picker before confirming a memory action.",
        )?;
        return Ok(MemoryPromptConfirmation::Unavailable);
    }

    let step = WizardStep {
        title: "Cleanup".to_string(),
        question: format!(
            "Persistent memory contains legacy raw prompts or serialized payloads.\n\n- Suspicious facts: {}\n- Suspicious summary lines: {}\n\nRun the one-time cleanup now?",
            status.cleanup_status.suspicious_facts, status.cleanup_status.suspicious_summary_lines
        ),
        items: vec![
            InlineListItem {
                title: "Run cleanup now".to_string(),
                subtitle: Some(
                    "Rewrite durable memory through the LLM-assisted normalization path."
                        .to_string(),
                ),
                badge: Some("Confirm".to_string()),
                indent: 0,
                selection: Some(InlineListSelection::ConfigAction(
                    MEMORY_CLEANUP_ACCEPT.to_string(),
                )),
                search_value: Some("cleanup memory now".to_string()),
            },
            InlineListItem {
                title: "Cancel".to_string(),
                subtitle: Some("Leave memory unchanged and stop this mutation.".to_string()),
                badge: None,
                indent: 0,
                selection: Some(InlineListSelection::ConfigAction(
                    MEMORY_CLEANUP_CANCEL.to_string(),
                )),
                search_value: Some("cancel cleanup".to_string()),
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
        "Cleanup Persistent Memory".to_string(),
        vec![step],
        0,
        None,
        WizardModalMode::MultiStep,
        ctx.ctrl_c_state,
        ctx.ctrl_c_notify,
    )
    .await?;

    let WizardModalOutcome::Submitted(selections) = outcome else {
        return Ok(MemoryPromptConfirmation::Cancelled);
    };
    let confirmed = selections.iter().any(|selection| {
        matches!(
            selection,
            InlineListSelection::ConfigAction(action) if action == MEMORY_CLEANUP_ACCEPT
        )
    });
    Ok(if confirmed {
        MemoryPromptConfirmation::Confirmed
    } else {
        MemoryPromptConfirmation::Cancelled
    })
}

async fn prompt_missing_memory_value(
    ctx: &mut InteractionLoopContext<'_>,
    state: &mut InteractionState<'_>,
    field: &str,
    prompt: &str,
) -> Result<Option<String>> {
    if !ctx.renderer.supports_inline_ui() {
        return Ok(None);
    }
    if state.model_picker_state.is_some() || state.palette_state.is_some() {
        ctx.renderer.line(
            MessageStyle::Info,
            "Close the active picker before entering missing memory details.",
        )?;
        return Ok(None);
    }

    ctx.renderer.line(
        MessageStyle::Info,
        "VT Code needs one more detail before it can save this memory.",
    )?;
    let step = WizardStep {
        title: "Missing Detail".to_string(),
        question: prompt.to_string(),
        items: vec![InlineListItem {
            title: "Submit".to_string(),
            subtitle: Some(
                "Press Tab to type the missing detail, then Enter to submit.".to_string(),
            ),
            badge: None,
            indent: 0,
            selection: Some(InlineListSelection::RequestUserInputAnswer {
                question_id: MEMORY_MISSING_QUESTION_ID.to_string(),
                selected: vec![],
                other: Some(String::new()),
            }),
            search_value: Some("submit memory detail".to_string()),
        }],
        completed: false,
        answer: None,
        allow_freeform: true,
        freeform_label: Some(field.to_string()),
        freeform_placeholder: Some(String::new()),
        freeform_default: None,
    };
    let outcome = show_wizard_modal_and_wait(
        ctx.handle,
        ctx.session,
        "Complete Memory Detail".to_string(),
        vec![step],
        0,
        None,
        WizardModalMode::MultiStep,
        ctx.ctrl_c_state,
        ctx.ctrl_c_notify,
    )
    .await?;

    let WizardModalOutcome::Submitted(selections) = outcome else {
        return Ok(None);
    };
    Ok(selections
        .into_iter()
        .find_map(|selection| match selection {
            InlineListSelection::RequestUserInputAnswer {
                question_id,
                other,
                selected,
            } if question_id == MEMORY_MISSING_QUESTION_ID => {
                other.or_else(|| selected.first().cloned())
            }
            _ => None,
        }))
}

fn respond_to_memory_prompt(
    ctx: &mut InteractionLoopContext<'_>,
    input: &str,
    reply: &str,
) -> Result<()> {
    display_user_message(ctx.renderer, input)?;
    ctx.conversation_history
        .push(uni::Message::user(input.to_string()));
    ctx.renderer.line(MessageStyle::Response, reply)?;
    ctx.conversation_history.push(
        uni::Message::assistant(reply.to_string())
            .with_phase(Some(uni::AssistantPhase::FinalAnswer)),
    );
    ctx.handle.clear_input();
    if let Some(placeholder) = ctx.default_placeholder.as_ref() {
        ctx.handle.set_placeholder(Some(placeholder.clone()));
    }
    Ok(())
}

fn detect_memory_prompt_intent(input: &str) -> Option<MemoryPromptIntent> {
    let normalized = normalize_prompt_clause(input)?;
    if normalized.ends_with('?') {
        if normalized.starts_with("what do you remember")
            || normalized.starts_with("show memory")
            || normalized.starts_with("open memory")
            || normalized.starts_with("list memory")
        {
            return Some(MemoryPromptIntent::Show);
        }
        return None;
    }

    let lowered = normalized.to_ascii_lowercase();
    if matches_show_memory(&lowered) {
        return Some(MemoryPromptIntent::Show);
    }
    if extract_memory_clause(&lowered, &normalized, remember_markers()).is_some() {
        return Some(MemoryPromptIntent::Remember {
            request: normalized.clone(),
        });
    }
    extract_memory_clause(&lowered, &normalized, forget_markers()).map(|_| {
        MemoryPromptIntent::Forget {
            request: normalized,
        }
    })
}

fn matches_show_memory(lowered: &str) -> bool {
    [
        "show memory",
        "open memory",
        "list memory",
        "browse memory",
        "what do you remember",
        "what's in memory",
    ]
    .iter()
    .any(|pattern| lowered.starts_with(pattern))
}

fn remember_markers() -> &'static [&'static str] {
    &[
        "save to memory,",
        "save to memory:",
        "save to memory ",
        "save this to memory ",
        "store this in memory ",
        "store in memory ",
        "remember that ",
        "remember ",
        "add to memory ",
        "note this for memory ",
    ]
}

fn forget_markers() -> &'static [&'static str] {
    &[
        "forget that ",
        "forget about ",
        "forget ",
        "remove from memory ",
        "delete from memory ",
        "drop from memory ",
        "don't remember ",
        "stop remembering ",
    ]
}

fn extract_memory_clause(lowered: &str, original: &str, markers: &[&str]) -> Option<String> {
    markers.iter().find_map(|marker| {
        let index = lowered.find(marker)?;
        let start = index + marker.len();
        let suffix = original.get(start..)?.trim();
        normalize_prompt_clause(strip_leading_memory_markers(suffix))
    })
}

fn strip_leading_memory_markers(input: &str) -> &str {
    let trimmed = input.trim_start_matches([',', ':', '-', ' ']).trim();
    let lowered = trimmed.to_ascii_lowercase();
    for marker in remember_markers().iter().chain(forget_markers()) {
        if lowered.starts_with(marker) {
            return trimmed[marker.len()..].trim();
        }
    }
    trimmed
}

fn normalize_prompt_clause(input: &str) -> Option<String> {
    let mut text = input.trim();
    while let Some(stripped) = [
        "please ",
        "can you ",
        "could you ",
        "would you ",
        "vt code, ",
        "vt code ",
    ]
    .iter()
    .find_map(|prefix| {
        text.to_ascii_lowercase()
            .starts_with(prefix)
            .then(|| &text[prefix.len()..])
    }) {
        text = stripped.trim_start();
    }

    let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
    (!normalized.is_empty()).then_some(normalized)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_remember_intent_from_natural_language() {
        assert_eq!(
            detect_memory_prompt_intent("Please remember that I prefer pnpm"),
            Some(MemoryPromptIntent::Remember {
                request: "remember that I prefer pnpm".to_string(),
            })
        );
    }

    #[test]
    fn detects_forget_intent_from_natural_language() {
        assert_eq!(
            detect_memory_prompt_intent("forget that I prefer pnpm"),
            Some(MemoryPromptIntent::Forget {
                request: "forget that I prefer pnpm".to_string(),
            })
        );
    }

    #[test]
    fn detects_show_memory_questions() {
        assert_eq!(
            detect_memory_prompt_intent("what do you remember about this repo?"),
            Some(MemoryPromptIntent::Show)
        );
    }

    #[test]
    fn ignores_non_imperative_memory_questions() {
        assert_eq!(
            detect_memory_prompt_intent("do you remember my name?"),
            None
        );
    }

    #[test]
    fn strips_nested_memory_markers() {
        assert_eq!(
            detect_memory_prompt_intent("save to memory, remember my preferred shell is zsh"),
            Some(MemoryPromptIntent::Remember {
                request: "save to memory, remember my preferred shell is zsh".to_string(),
            })
        );
    }

    #[test]
    fn normalize_prompt_clause_does_not_panic_on_short_input() {
        assert_eq!(
            normalize_prompt_clause("my name"),
            Some("my name".to_string())
        );
    }

    #[test]
    fn memory_operation_notice_describes_remember_flow() {
        let intent = MemoryPromptIntent::Remember {
            request: "remember my editor theme".to_string(),
        };
        assert_eq!(
            memory_operation_notice(&intent),
            "Memory save requested. VT Code is preparing a normalized note."
        );
    }

    #[test]
    fn memory_operation_notice_describes_forget_flow() {
        let intent = MemoryPromptIntent::Forget {
            request: "forget my editor theme".to_string(),
        };
        assert_eq!(
            memory_operation_notice(&intent),
            "Memory removal requested. VT Code is matching normalized notes."
        );
    }

    #[test]
    fn suppresses_repeated_cleanup_confirmation_for_same_fingerprint() {
        let status = vtcode_core::persistent_memory::PersistentMemoryStatus {
            enabled: true,
            auto_write: true,
            directory: std::path::PathBuf::from("/tmp/memory"),
            summary_file: std::path::PathBuf::from("/tmp/memory/memory_summary.md"),
            memory_file: std::path::PathBuf::from("/tmp/memory/MEMORY.md"),
            preferences_file: std::path::PathBuf::from("/tmp/memory/preferences.md"),
            repository_facts_file: std::path::PathBuf::from("/tmp/memory/repository-facts.md"),
            rollout_summaries_dir: std::path::PathBuf::from("/tmp/memory/rollout_summaries"),
            summary_exists: true,
            registry_exists: true,
            pending_rollout_summaries: 0,
            cleanup_status: vtcode_core::persistent_memory::MemoryCleanupStatus {
                needed: true,
                suspicious_facts: 2,
                suspicious_summary_lines: 1,
            },
        };

        assert!(should_suppress_cleanup_confirmation(Some((2, 1)), &status));
        assert!(!should_suppress_cleanup_confirmation(Some((1, 1)), &status));
        assert!(!should_suppress_cleanup_confirmation(None, &status));
    }
}
