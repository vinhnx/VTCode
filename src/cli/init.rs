use anyhow::{Context, Result, bail};
use std::collections::BTreeMap;
use std::io::{self, Write};
use std::path::Path;
use vtcode_core::commands::init::{
    GuidedInitAnswer, GuidedInitAnswers, GuidedInitOverwriteState, GuidedInitQuestion,
    GuidedInitQuestionKey, prepare_guided_init, render_agents_md, write_agents_file,
};
use vtcode_core::config::core::PromptCachingConfig;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::config::models::Provider;
use vtcode_core::config::types::{
    AgentConfig as CoreAgentConfig, ModelSelectionSource, ReasoningEffortLevel, UiSurfacePreference,
};
use vtcode_core::core::agent::snapshots::{
    DEFAULT_CHECKPOINTS_ENABLED, DEFAULT_MAX_AGE_DAYS, DEFAULT_MAX_SNAPSHOTS,
};
use vtcode_core::core::interfaces::session::PlanModeEntrySource;
use vtcode_core::ui::theme::DEFAULT_THEME_ID;
use vtcode_core::utils::colors::style;
use vtcode_core::utils::file_utils::ensure_dir_exists;
use vtcode_core::utils::tty::TtyExt;

/// Handle the init command
pub async fn handle_init_command(workspace: &Path, force: bool, run: bool) -> Result<()> {
    println!("{}", style("[INIT]").cyan().bold());
    println!("  {:16} {}", "workspace", workspace.display());
    println!("  {:16} {}", "force", force);
    println!("  {:16} {}\n", "run", run);

    ensure_dir_exists(workspace).await?;

    VTCodeConfig::bootstrap_project(workspace, force)
        .with_context(|| "failed to initialize configuration files")?;

    let plan = prepare_guided_init(workspace, force)?;
    let stdin_is_tty = io::stdin().is_tty_ext();

    if matches!(plan.overwrite_state, GuidedInitOverwriteState::Confirm) && !stdin_is_tty {
        bail!(
            "AGENTS.md already exists at {}. Re-run interactively or pass `vtcode init --force` to overwrite it non-interactively.",
            plan.path.display()
        );
    }

    let should_write = match plan.overwrite_state {
        GuidedInitOverwriteState::Skip | GuidedInitOverwriteState::Force => true,
        GuidedInitOverwriteState::Confirm => prompt_overwrite_confirmation(&plan.path)?,
    };

    if should_write {
        let answers = if plan.questions.is_empty() {
            GuidedInitAnswers::default()
        } else if stdin_is_tty {
            prompt_guided_answers(&plan.questions)?
        } else {
            println!(
                "{} Using inferred defaults for {} guided /init question(s).",
                style("Info").cyan(),
                plan.questions.len()
            );
            GuidedInitAnswers::default()
        };

        let content = render_agents_md(&plan, &answers)?;
        let overwrite_existing = matches!(
            plan.overwrite_state,
            GuidedInitOverwriteState::Confirm | GuidedInitOverwriteState::Force
        );
        let report = write_agents_file(workspace, &content, overwrite_existing)?;
        println!("  {:16} {}", "agents", report.path.display());
    } else {
        println!(
            "{} Keeping existing AGENTS.md; other workspace scaffolding still completed.",
            style("Info").cyan()
        );
    }

    if run {
        let config = CoreAgentConfig {
            model: String::new(),
            api_key: String::new(),
            provider: String::new(),
            api_key_env: Provider::Gemini.default_api_key_env().to_string(),
            workspace: workspace.to_path_buf(),
            verbose: false,
            quiet: false,
            theme: DEFAULT_THEME_ID.to_string(),
            reasoning_effort: ReasoningEffortLevel::default(),
            ui_surface: UiSurfacePreference::default(),
            prompt_cache: PromptCachingConfig::default(),
            model_source: ModelSelectionSource::WorkspaceConfig,
            custom_api_keys: BTreeMap::new(),
            checkpointing_enabled: DEFAULT_CHECKPOINTS_ENABLED,
            checkpointing_storage_dir: None,
            checkpointing_max_snapshots: DEFAULT_MAX_SNAPSHOTS,
            checkpointing_max_age_days: Some(DEFAULT_MAX_AGE_DAYS),
            max_conversation_turns: 50,
            model_behavior: None,
            openai_chatgpt_auth: None,
        };
        crate::agent::agents::run_single_agent_loop(
            &config,
            None,
            false,
            false,
            PlanModeEntrySource::None,
            None,
        )
        .await
        .with_context(|| "failed to start chat session")?;
    }

    Ok(())
}

fn prompt_overwrite_confirmation(path: &Path) -> Result<bool> {
    loop {
        print!(
            "AGENTS.md already exists at {}. Overwrite it? [y/N]: ",
            path.display()
        );
        io::stdout()
            .flush()
            .context("failed to flush overwrite prompt")?;

        let mut input = String::new();
        io::stdin()
            .read_line(&mut input)
            .context("failed to read overwrite confirmation")?;

        match input.trim().to_ascii_lowercase().as_str() {
            "" | "n" | "no" => return Ok(false),
            "y" | "yes" => return Ok(true),
            _ => println!("Enter `y` to overwrite or press Enter to keep the current file."),
        }
    }
}

fn prompt_guided_answers(questions: &[GuidedInitQuestion]) -> Result<GuidedInitAnswers> {
    let mut answers = GuidedInitAnswers::default();

    for question in questions {
        println!();
        println!(
            "{} {}",
            style(question.header.as_str()).cyan(),
            question.prompt
        );

        for (index, option) in question.options.iter().enumerate() {
            println!(
                "  {}) {}{}",
                index + 1,
                option.label,
                if option.recommended { " [default]" } else { "" }
            );
            println!("     {}", option.description);
        }

        let custom_index = question.options.len() + 1;
        if question.allow_custom {
            println!("  {}) Custom", custom_index);
            println!("     Type your own answer.");
        }

        let recommended_index = question
            .options
            .iter()
            .position(|option| option.recommended)
            .map(|index| index + 1);

        loop {
            print!(
                "Select an option{}: ",
                recommended_index
                    .map(|index| format!(" [{}]", index))
                    .unwrap_or_default()
            );
            io::stdout()
                .flush()
                .context("failed to flush guided /init prompt")?;

            let mut input = String::new();
            io::stdin()
                .read_line(&mut input)
                .context("failed to read guided /init answer")?;

            let trimmed = input.trim();
            if trimmed.is_empty()
                && let Some(index) = recommended_index
                && let Some(option) = question.options.get(index - 1)
            {
                answers.insert(
                    GuidedInitAnswer::from_input(question.key, Some(&option.value), None)
                        .expect("recommended option produces an answer"),
                );
                break;
            }

            if let Ok(index) = trimmed.parse::<usize>() {
                if let Some(option) = question.options.get(index.saturating_sub(1)) {
                    answers.insert(
                        GuidedInitAnswer::from_input(question.key, Some(&option.value), None)
                            .expect("selected option produces an answer"),
                    );
                    break;
                }
                if question.allow_custom && index == custom_index {
                    let custom = prompt_custom_answer(question.key)?;
                    answers.insert(
                        GuidedInitAnswer::from_input(question.key, None, Some(&custom))
                            .expect("custom prompt always yields an answer"),
                    );
                    break;
                }
            }

            if question.allow_custom && !trimmed.is_empty() {
                answers.insert(
                    GuidedInitAnswer::from_input(question.key, None, Some(trimmed))
                        .expect("typed custom input produces an answer"),
                );
                break;
            }

            println!("Pick one of the numbered options.");
        }
    }

    Ok(answers)
}

fn prompt_custom_answer(key: GuidedInitQuestionKey) -> Result<String> {
    print!("{} [{}]: ", key.custom_label(), key.custom_placeholder());
    io::stdout()
        .flush()
        .context("failed to flush custom guided /init prompt")?;

    let mut input = String::new();
    io::stdin()
        .read_line(&mut input)
        .context("failed to read custom guided /init answer")?;

    let trimmed = input.trim();
    Ok(trimmed.to_string())
}
