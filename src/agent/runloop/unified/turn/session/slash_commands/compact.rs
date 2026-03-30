use std::fs;

use anyhow::{Context, Result};
use tempfile::Builder as TempFileBuilder;
use toml::Value as TomlValue;
use vtcode_config::OpenAIServiceTier;
use vtcode_core::config::build_openai_prompt_cache_key;
use vtcode_core::config::loader::ConfigManager;
use vtcode_core::config::{ReasoningEffortLevel, VerbosityLevel};
use vtcode_core::llm::provider::ResponsesCompactionOptions;
use vtcode_core::tools::terminal_app::{EditorLaunchConfig, TerminalAppLauncher};
use vtcode_core::utils::ansi::MessageStyle;
use vtcode_tui::app::{
    InlineListItem, InlineListSearchConfig, InlineListSelection, WizardModalMode, WizardStep,
};

use crate::agent::runloop::slash_commands::CompactConversationCommand;
use crate::agent::runloop::unified::palettes::refresh_runtime_config_from_manager;
use crate::agent::runloop::unified::wizard_modal::{
    WizardModalOutcome, show_wizard_modal_and_wait,
};

use super::apps::run_with_event_loop_suspended;
use super::config_toml::{
    ensure_child_table, load_toml_value, preferred_workspace_config_path, save_toml_value,
};
use super::ui::{ensure_selection_ui_available, wait_for_list_modal_selection};
use super::{SlashCommandContext, SlashCommandControl};

const COMPACT_ACTION_NOW: &str = "compact.action.now";
const COMPACT_ACTION_EDIT_PROMPT: &str = "compact.action.edit_prompt";
const COMPACT_ACTION_RESET_PROMPT: &str = "compact.action.reset_prompt";
const COMPACT_ACTION_BACK: &str = "compact.action.back";
const COMPACT_INPUT_ID: &str = "compact.input";

pub(crate) async fn handle_compact_conversation(
    mut ctx: SlashCommandContext<'_>,
    command: CompactConversationCommand,
) -> Result<SlashCommandControl> {
    if !manual_openai_compaction_available(&mut ctx)? {
        return Ok(SlashCommandControl::Continue);
    }

    match command {
        CompactConversationCommand::InteractiveManager if ctx.renderer.supports_inline_ui() => {
            run_compact_manager(&mut ctx).await
        }
        CompactConversationCommand::InteractiveManager => {
            ctx.renderer.line(
                MessageStyle::Info,
                "Interactive `/compact` options require inline UI. Running compaction with current OpenAI defaults.",
            )?;
            execute_manual_compaction(&mut ctx, ResponsesCompactionOptions::default()).await
        }
        CompactConversationCommand::Run { options } => {
            execute_manual_compaction(&mut ctx, options).await
        }
    }
}

async fn run_compact_manager(ctx: &mut SlashCommandContext<'_>) -> Result<SlashCommandControl> {
    if !ensure_selection_ui_available(ctx, "opening compact controls")? {
        return Ok(SlashCommandControl::Continue);
    }

    loop {
        show_compact_actions_modal(ctx);
        let Some(selection) = wait_for_list_modal_selection(ctx).await else {
            return Ok(SlashCommandControl::Continue);
        };
        let InlineListSelection::ConfigAction(action) = selection else {
            return Ok(SlashCommandControl::Continue);
        };

        match action.as_str() {
            COMPACT_ACTION_NOW => {
                let Some(options) = collect_interactive_compaction_options(ctx).await? else {
                    continue;
                };
                return execute_manual_compaction(ctx, options).await;
            }
            COMPACT_ACTION_EDIT_PROMPT => {
                edit_default_prompt(ctx).await?;
            }
            COMPACT_ACTION_RESET_PROMPT => {
                reset_default_prompt(ctx).await?;
            }
            COMPACT_ACTION_BACK => return Ok(SlashCommandControl::Continue),
            _ => return Ok(SlashCommandControl::Continue),
        }
    }
}

fn show_compact_actions_modal(ctx: &mut SlashCommandContext<'_>) {
    let default_prompt = current_default_prompt(ctx);
    let prompt_badge = default_prompt
        .as_ref()
        .map(|_| "Configured".to_string())
        .unwrap_or_else(|| "Default".to_string());
    let compact_subtitle = if ctx.conversation_history.is_empty() {
        "No conversation history yet. Prompt actions are still available.".to_string()
    } else {
        "Run a one-off OpenAI `/responses/compact` request with optional overrides.".to_string()
    };

    ctx.handle.show_list_modal(
        "Compact conversation".to_string(),
        vec![
            format!(
                "Provider: {} · Model: {}",
                ctx.provider_client.name(),
                ctx.config.model
            ),
            "Available only for native OpenAI Responses models on api.openai.com.".to_string(),
        ],
        vec![
            InlineListItem {
                title: "Compact now".to_string(),
                subtitle: Some(compact_subtitle),
                badge: Some("Recommended".to_string()),
                indent: 0,
                selection: Some(InlineListSelection::ConfigAction(
                    COMPACT_ACTION_NOW.to_string(),
                )),
                search_value: Some("compact now run manual openai".to_string()),
            },
            InlineListItem {
                title: "Edit default prompt".to_string(),
                subtitle: Some(
                    "Open the saved default manual compaction instructions in your external editor."
                        .to_string(),
                ),
                badge: Some(prompt_badge),
                indent: 0,
                selection: Some(InlineListSelection::ConfigAction(
                    COMPACT_ACTION_EDIT_PROMPT.to_string(),
                )),
                search_value: Some("edit default prompt instructions".to_string()),
            },
            InlineListItem {
                title: "Reset default prompt".to_string(),
                subtitle: Some(
                    "Remove the saved workspace default and fall back to built-in behavior."
                        .to_string(),
                ),
                badge: None,
                indent: 0,
                selection: Some(InlineListSelection::ConfigAction(
                    COMPACT_ACTION_RESET_PROMPT.to_string(),
                )),
                search_value: Some("reset default prompt".to_string()),
            },
            InlineListItem {
                title: "Back".to_string(),
                subtitle: Some("Close the compact manager.".to_string()),
                badge: None,
                indent: 0,
                selection: Some(InlineListSelection::ConfigAction(
                    COMPACT_ACTION_BACK.to_string(),
                )),
                search_value: Some("back close".to_string()),
            },
        ],
        Some(InlineListSelection::ConfigAction(
            COMPACT_ACTION_NOW.to_string(),
        )),
        Some(InlineListSearchConfig {
            label: "Search actions".to_string(),
            placeholder: Some("compact, prompt, reset".to_string()),
        }),
    );
}

async fn collect_interactive_compaction_options(
    ctx: &mut SlashCommandContext<'_>,
) -> Result<Option<ResponsesCompactionOptions>> {
    let Some(instructions) = prompt_optional_text(
        ctx,
        "Compaction instructions",
        "Optionally append one-off instructions to the standalone `/responses/compact` request. Leave blank to use defaults.",
        "Instructions:",
    )
    .await?
    else {
        return Ok(None);
    };

    let Some(max_output_tokens) = prompt_optional_text(
        ctx,
        "Max output tokens",
        "Optionally override the compaction response token limit. Leave blank to use defaults.",
        "Max output tokens:",
    )
    .await?
    else {
        return Ok(None);
    };

    let Some(reasoning_effort) = prompt_optional_text(
        ctx,
        "Reasoning effort",
        "Optionally set reasoning effort (`none`, `minimal`, `low`, `medium`, `high`, `xhigh`). Leave blank to use defaults.",
        "Reasoning effort:",
    )
    .await?
    else {
        return Ok(None);
    };

    let Some(verbosity) = prompt_optional_text(
        ctx,
        "Verbosity",
        "Optionally set text verbosity (`low`, `medium`, `high`). Leave blank to use defaults.",
        "Verbosity:",
    )
    .await?
    else {
        return Ok(None);
    };

    let Some(include) = prompt_optional_text(
        ctx,
        "Include selectors",
        "Optionally enter comma-separated Responses include selectors. Leave blank to use defaults.",
        "Include selectors:",
    )
    .await?
    else {
        return Ok(None);
    };

    let Some(store) = prompt_optional_text(
        ctx,
        "Store override",
        "Optionally set `true` or `false` for the standalone compaction request. Leave blank to use defaults.",
        "Store:",
    )
    .await?
    else {
        return Ok(None);
    };

    let Some(service_tier) = prompt_optional_text(
        ctx,
        "Service tier",
        "Optionally set `flex` or `priority`. Leave blank to use defaults.",
        "Service tier:",
    )
    .await?
    else {
        return Ok(None);
    };

    let Some(prompt_cache_key) = prompt_optional_text(
        ctx,
        "Prompt cache key",
        "Optionally override the OpenAI prompt cache routing key. Leave blank to use defaults.",
        "Prompt cache key:",
    )
    .await?
    else {
        return Ok(None);
    };

    let max_output_tokens = parse_optional_u32(&max_output_tokens, "--max-output-tokens")?;
    let reasoning_effort =
        parse_optional_reasoning_effort(&reasoning_effort, "--reasoning-effort")?;
    let verbosity = parse_optional_verbosity(&verbosity, "--verbosity")?;
    let responses_include = parse_optional_include(&include);
    let response_store = parse_optional_store(&store)?;
    let service_tier = parse_optional_service_tier(&service_tier)?;

    Ok(Some(ResponsesCompactionOptions {
        instructions: trimmed_optional(instructions),
        max_output_tokens,
        reasoning_effort,
        verbosity,
        responses_include,
        response_store,
        service_tier,
        prompt_cache_key: trimmed_optional(prompt_cache_key),
    }))
}

async fn edit_default_prompt(ctx: &mut SlashCommandContext<'_>) -> Result<()> {
    let editor_config = ctx
        .vt_cfg
        .as_ref()
        .map(|config| config.tools.editor.clone())
        .unwrap_or_default();
    if !editor_config.enabled {
        ctx.renderer.line(
            MessageStyle::Warning,
            "External editor is disabled (`tools.editor.enabled = false`).",
        )?;
        return Ok(());
    }

    let initial_prompt = current_default_prompt(ctx).unwrap_or_default();
    let temp_file = TempFileBuilder::new()
        .prefix("vtcode-compact-prompt-")
        .suffix(".md")
        .tempfile_in(&ctx.config.workspace)
        .context("Failed to create temporary prompt file")?;
    fs::write(temp_file.path(), initial_prompt).context("Failed to seed temporary prompt file")?;

    let launcher = TerminalAppLauncher::new(ctx.config.workspace.clone());
    let launch_config = EditorLaunchConfig {
        preferred_editor: if editor_config.preferred_editor.trim().is_empty() {
            None
        } else {
            Some(editor_config.preferred_editor.clone())
        },
        wait_for_editor: true,
    };
    let temp_path = temp_file.path().to_path_buf();

    run_with_event_loop_suspended(ctx.handle, editor_config.suspend_tui, || {
        launcher.launch_editor_with_config(Some(temp_path.clone()), launch_config)
    })
    .await
    .context("Failed to launch editor")?;

    let edited =
        fs::read_to_string(temp_file.path()).context("Failed to read edited compaction prompt")?;
    persist_default_prompt(ctx, trimmed_optional(edited)).await?;
    ctx.renderer.line(
        MessageStyle::Info,
        "Saved workspace default manual compaction prompt.",
    )?;
    Ok(())
}

async fn reset_default_prompt(ctx: &mut SlashCommandContext<'_>) -> Result<()> {
    persist_default_prompt(ctx, None).await?;
    ctx.renderer.line(
        MessageStyle::Info,
        "Reset workspace default manual compaction prompt.",
    )?;
    Ok(())
}

async fn persist_default_prompt(
    ctx: &mut SlashCommandContext<'_>,
    value: Option<String>,
) -> Result<()> {
    let manager = ConfigManager::load_from_workspace(&ctx.config.workspace)
        .context("Failed to load VT Code configuration")?;
    let workspace_config_path = preferred_workspace_config_path(&manager, &ctx.config.workspace);
    let mut root = load_toml_value(&workspace_config_path)?;
    let root_table = root
        .as_table_mut()
        .context("Workspace config root is not a TOML table")?;
    set_manual_compaction_prompt(root_table, value);
    save_toml_value(&workspace_config_path, &root)?;
    refresh_runtime_config_from_manager(
        ctx.renderer,
        ctx.handle,
        ctx.config,
        ctx.vt_cfg,
        ctx.provider_client.as_ref(),
        ctx.session_bootstrap,
        ctx.full_auto,
    )
    .await
}

async fn execute_manual_compaction(
    ctx: &mut SlashCommandContext<'_>,
    options: ResponsesCompactionOptions,
) -> Result<SlashCommandControl> {
    if ctx.conversation_history.is_empty() {
        ctx.renderer
            .line(MessageStyle::Info, "No conversation history to compact.")?;
        return Ok(SlashCommandControl::Continue);
    }

    let resolved_options = resolve_manual_compaction_options(ctx, options);
    let harness_snapshot = ctx.tool_registry.harness_context_snapshot();
    let outcome =
        crate::agent::runloop::unified::turn::compaction::manual_openai_compact_history_in_place(
            crate::agent::runloop::unified::turn::compaction::CompactionContext::new(
                ctx.provider_client.as_ref(),
                &ctx.config.model,
                &harness_snapshot.session_id,
                ctx.thread_id,
                &ctx.config.workspace,
                ctx.vt_cfg.as_ref(),
                ctx.lifecycle_hooks,
                ctx.harness_emitter,
            ),
            crate::agent::runloop::unified::turn::compaction::CompactionState::new(
                ctx.conversation_history,
                ctx.session_stats,
                ctx.context_manager,
            ),
            &resolved_options,
        )
        .await;

    let outcome = match outcome {
        Ok(outcome) => outcome,
        Err(err) => {
            ctx.renderer
                .line(MessageStyle::Error, &format!("Compaction failed: {}", err))?;
            return Ok(SlashCommandControl::Continue);
        }
    };

    let Some(outcome) = outcome else {
        ctx.renderer
            .line(MessageStyle::Info, "Conversation is already compact.")?;
        return Ok(SlashCommandControl::Continue);
    };

    ctx.renderer.line(
        MessageStyle::Info,
        &format!(
            "Compacted conversation history ({} -> {} messages).",
            outcome.original_len, outcome.compacted_len
        ),
    )?;
    Ok(SlashCommandControl::Continue)
}

fn resolve_manual_compaction_options(
    ctx: &SlashCommandContext<'_>,
    options: ResponsesCompactionOptions,
) -> ResponsesCompactionOptions {
    let default_prompt = current_default_prompt(ctx);
    let prompt_cache_key = options
        .prompt_cache_key
        .clone()
        .or_else(|| default_openai_prompt_cache_key(ctx));

    ResponsesCompactionOptions {
        instructions: options
            .instructions
            .and_then(trimmed_optional)
            .or(default_prompt),
        max_output_tokens: options.max_output_tokens,
        reasoning_effort: options.reasoning_effort,
        verbosity: options.verbosity,
        responses_include: options
            .responses_include
            .map(|values| {
                values
                    .into_iter()
                    .filter_map(trimmed_optional)
                    .collect::<Vec<_>>()
            })
            .filter(|values| !values.is_empty()),
        response_store: options.response_store,
        service_tier: options.service_tier.and_then(trimmed_optional),
        prompt_cache_key,
    }
}

fn default_openai_prompt_cache_key(ctx: &SlashCommandContext<'_>) -> Option<String> {
    let prompt_cache = &ctx.config.prompt_cache;
    build_openai_prompt_cache_key(
        prompt_cache.enabled && prompt_cache.providers.openai.enabled,
        &prompt_cache.providers.openai.prompt_cache_key_mode,
        ctx.session_stats.prompt_cache_lineage_id(),
    )
}

fn current_default_prompt(ctx: &SlashCommandContext<'_>) -> Option<String> {
    ctx.vt_cfg
        .as_ref()
        .and_then(|cfg| cfg.provider.openai.manual_compaction.instructions.clone())
        .and_then(trimmed_optional)
}

fn manual_openai_compaction_available(ctx: &mut SlashCommandContext<'_>) -> Result<bool> {
    if ctx
        .provider_client
        .supports_manual_openai_compaction(&ctx.config.model)
    {
        return Ok(true);
    }

    let message = ctx
        .provider_client
        .manual_openai_compaction_unavailable_message(&ctx.config.model);
    ctx.renderer.line(MessageStyle::Error, &message)?;
    Ok(false)
}

async fn prompt_optional_text(
    ctx: &mut SlashCommandContext<'_>,
    title: &str,
    question: &str,
    freeform_label: &str,
) -> Result<Option<String>> {
    let step = WizardStep {
        title: "Input".to_string(),
        question: question.to_string(),
        items: vec![InlineListItem {
            title: "Submit".to_string(),
            subtitle: Some(
                "Press Enter to keep this field blank, or Tab to type a value.".to_string(),
            ),
            badge: None,
            indent: 0,
            selection: Some(InlineListSelection::RequestUserInputAnswer {
                question_id: COMPACT_INPUT_ID.to_string(),
                selected: vec![],
                other: Some(String::new()),
            }),
            search_value: Some("submit input".to_string()),
        }],
        completed: false,
        answer: None,
        allow_freeform: true,
        freeform_label: Some(freeform_label.to_string()),
        freeform_placeholder: Some(String::new()),
    };

    let outcome = show_wizard_modal_and_wait(
        ctx.handle,
        ctx.session,
        title.to_string(),
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
                        other,
                    } if question_id == COMPACT_INPUT_ID => {
                        other.or_else(|| selected.first().cloned())
                    }
                    _ => None,
                })
        }
        WizardModalOutcome::Cancelled { .. } => None,
    })
}

fn parse_optional_u32(value: &str, flag: &str) -> Result<Option<u32>> {
    let Some(value) = trimmed_optional(value.to_string()) else {
        return Ok(None);
    };
    value
        .parse::<u32>()
        .map(Some)
        .with_context(|| format!("Invalid value for {}: {}", flag, value))
}

fn parse_optional_reasoning_effort(
    value: &str,
    flag: &str,
) -> Result<Option<ReasoningEffortLevel>> {
    let Some(value) = trimmed_optional(value.to_string()) else {
        return Ok(None);
    };
    ReasoningEffortLevel::parse(&value)
        .map(Some)
        .with_context(|| format!("Invalid value for {}: {}", flag, value))
}

fn parse_optional_verbosity(value: &str, flag: &str) -> Result<Option<VerbosityLevel>> {
    let Some(value) = trimmed_optional(value.to_string()) else {
        return Ok(None);
    };
    VerbosityLevel::parse(&value)
        .map(Some)
        .with_context(|| format!("Invalid value for {}: {}", flag, value))
}

fn parse_optional_include(value: &str) -> Option<Vec<String>> {
    let value = trimmed_optional(value.to_string())?;
    let include = value
        .split(',')
        .map(|item| item.trim())
        .filter(|item| !item.is_empty())
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    (!include.is_empty()).then_some(include)
}

fn parse_optional_store(value: &str) -> Result<Option<bool>> {
    let Some(value) = trimmed_optional(value.to_string()) else {
        return Ok(None);
    };
    match value.to_ascii_lowercase().as_str() {
        "true" | "yes" | "store" => Ok(Some(true)),
        "false" | "no" | "no-store" => Ok(Some(false)),
        _ => anyhow::bail!("Invalid value for --store: {}", value),
    }
}

fn parse_optional_service_tier(value: &str) -> Result<Option<String>> {
    let Some(value) = trimmed_optional(value.to_string()) else {
        return Ok(None);
    };
    OpenAIServiceTier::parse(&value)
        .map(|tier| Some(tier.as_str().to_string()))
        .with_context(|| format!("Invalid value for --service-tier: {}", value))
}

fn trimmed_optional(value: String) -> Option<String> {
    let trimmed = value.trim().to_string();
    (!trimmed.is_empty()).then_some(trimmed)
}

fn set_manual_compaction_prompt(
    root_table: &mut toml::map::Map<String, TomlValue>,
    value: Option<String>,
) {
    match value {
        Some(value) => {
            let provider_table = ensure_child_table(root_table, "provider");
            let openai_table = ensure_child_table(provider_table, "openai");
            let manual_table = ensure_child_table(openai_table, "manual_compaction");
            manual_table.insert("instructions".to_string(), TomlValue::String(value));
        }
        None => {
            let remove_openai_table = {
                let provider_table = ensure_child_table(root_table, "provider");
                let openai_table = ensure_child_table(provider_table, "openai");
                let remove_manual_table = {
                    let manual_table = ensure_child_table(openai_table, "manual_compaction");
                    manual_table.remove("instructions");
                    manual_table.is_empty()
                };
                if remove_manual_table {
                    openai_table.remove("manual_compaction");
                }
                openai_table.is_empty()
            };
            if remove_openai_table {
                let remove_provider_table = {
                    let provider_table = ensure_child_table(root_table, "provider");
                    provider_table.remove("openai");
                    provider_table.is_empty()
                };
                if remove_provider_table {
                    root_table.remove("provider");
                }
            }
        }
    }
}
