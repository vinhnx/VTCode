use crate::startup::{SessionResumeMode, StartupContext};
use vtcode_core::cli::args::{Cli, Commands, ExecSubcommand};
use vtcode_core::core::threads::{SessionQueryScope, list_recent_sessions_in_scope};
use vtcode_core::utils::session_archive::{
    generate_session_archive_identifier, history_persistence_enabled,
    reserve_session_archive_identifier,
};

use super::{build_command_debug_session_id, configure_runtime_debug_context};

fn resolve_mode_hint(
    args: &Cli,
    startup: &StartupContext,
    print_mode: &Option<String>,
    potential_prompt: &Option<String>,
) -> &'static str {
    if startup.session_resume.is_some() {
        "resume"
    } else if print_mode.is_some() || potential_prompt.is_some() {
        "ask"
    } else if startup.automation_prompt.is_some() {
        "auto"
    } else {
        match args.command {
            Some(Commands::Chat) => "chat",
            Some(Commands::ChatVerbose) => "chat-verbose",
            Some(Commands::Ask { .. }) => "ask",
            Some(Commands::Exec { .. }) => "exec",
            Some(Commands::Review(_)) => "review",
            Some(Commands::Schema { .. }) => "schema",
            Some(Commands::Benchmark { .. }) => "benchmark",
            Some(Commands::Analyze { .. }) => "analyze",
            Some(Commands::Notify { .. }) => "notify",
            Some(Commands::AgentClientProtocol { .. }) => "acp",
            Some(Commands::AppServer { .. }) => "app-server",
            Some(_) => "command",
            None => "chat",
        }
    }
}

fn interactive_archive_backed_session(
    args: &Cli,
    startup: &StartupContext,
    print_mode: &Option<String>,
    potential_prompt: &Option<String>,
) -> bool {
    startup.session_resume.is_some()
        || matches!(
            args.command,
            Some(Commands::Chat) | Some(Commands::ChatVerbose)
        )
        || (args.command.is_none()
            && print_mode.is_none()
            && potential_prompt.is_none()
            && startup.automation_prompt.is_none())
}

fn workspace_archive_label(workspace: &std::path::Path) -> String {
    workspace
        .file_name()
        .and_then(|value| value.to_str())
        .map(|value| value.to_string())
        .unwrap_or_else(|| "workspace".to_string())
}

async fn reserve_fresh_archive_session_id(startup: &StartupContext) -> Option<String> {
    let workspace_label = workspace_archive_label(startup.workspace.as_path());
    if history_persistence_enabled() {
        reserve_session_archive_identifier(&workspace_label, None)
            .await
            .ok()
    } else {
        Some(generate_session_archive_identifier(&workspace_label, None))
    }
}

async fn resolve_archive_session_id(
    args: &Cli,
    startup: &StartupContext,
    print_mode: &Option<String>,
    potential_prompt: &Option<String>,
) -> Option<String> {
    if interactive_archive_backed_session(args, startup, print_mode, potential_prompt) {
        if let Some(mode) = startup.session_resume.as_ref() {
            match mode {
                SessionResumeMode::Specific(identifier) if startup.custom_session_id.is_none() => {
                    return Some(identifier.clone());
                }
                SessionResumeMode::Latest if startup.custom_session_id.is_none() => {
                    let scope = if startup.resume_show_all {
                        SessionQueryScope::All
                    } else {
                        SessionQueryScope::CurrentWorkspace(startup.workspace.clone())
                    };
                    return list_recent_sessions_in_scope(1, &scope)
                        .await
                        .ok()
                        .and_then(|listings| listings.first().map(|listing| listing.identifier()));
                }
                SessionResumeMode::Interactive if startup.custom_session_id.is_none() => {
                    return None;
                }
                _ => {}
            }
        }

        return reserve_fresh_archive_session_id(startup).await;
    }

    match &args.command {
        Some(Commands::Exec {
            command: Some(ExecSubcommand::Resume(resume)),
            ..
        }) => {
            if resume.last {
                let scope = if resume.all {
                    SessionQueryScope::All
                } else {
                    SessionQueryScope::CurrentWorkspace(startup.workspace.clone())
                };
                list_recent_sessions_in_scope(1, &scope)
                    .await
                    .ok()
                    .and_then(|listings| listings.first().map(|listing| listing.identifier()))
            } else {
                resume.session_or_prompt.clone()
            }
        }
        Some(Commands::Exec { .. }) | Some(Commands::Review(_)) => {
            reserve_fresh_archive_session_id(startup).await
        }
        _ => None,
    }
}

pub(crate) async fn configure_debug_session_routing(
    args: &Cli,
    startup: &StartupContext,
    print_mode: &Option<String>,
    potential_prompt: &Option<String>,
) {
    let command_debug_session_id = build_command_debug_session_id(resolve_mode_hint(
        args,
        startup,
        print_mode,
        potential_prompt,
    ));

    if let Some(session_id) =
        resolve_archive_session_id(args, startup, print_mode, potential_prompt).await
    {
        configure_runtime_debug_context(session_id.clone(), Some(session_id));
    } else {
        configure_runtime_debug_context(command_debug_session_id, None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use std::collections::BTreeMap;
    use std::path::PathBuf;
    use std::sync::{LazyLock, Mutex};

    use vtcode_config::core::PromptCachingConfig;
    use vtcode_config::types::{
        AgentConfig as StartupAgentConfig, ModelSelectionSource, ReasoningEffortLevel,
        UiSurfacePreference,
    };
    use vtcode_core::config::loader::VTCodeConfig;

    use crate::main_helpers::runtime_archive_session_id;

    static DEBUG_ROUTING_TEST_GUARD: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

    fn startup_agent_config() -> StartupAgentConfig {
        StartupAgentConfig {
            model: vtcode_core::config::constants::models::openai::GPT_5.to_string(),
            api_key: "test-key".to_string(),
            provider: "openai".to_string(),
            api_key_env: "OPENAI_API_KEY".to_string(),
            workspace: PathBuf::from("."),
            verbose: false,
            quiet: false,
            theme: vtcode_core::ui::theme::DEFAULT_THEME_ID.to_string(),
            reasoning_effort: ReasoningEffortLevel::default(),
            ui_surface: UiSurfacePreference::default(),
            prompt_cache: PromptCachingConfig::default(),
            model_source: ModelSelectionSource::WorkspaceConfig,
            custom_api_keys: BTreeMap::new(),
            checkpointing_enabled: true,
            checkpointing_storage_dir: None,
            checkpointing_max_snapshots: 50,
            checkpointing_max_age_days: Some(30),
            max_conversation_turns: 1000,
            model_behavior: None,
            openai_chatgpt_auth: None,
        }
    }

    #[test]
    fn configure_debug_session_routing_reuses_specific_resume_identifier() {
        let _guard = DEBUG_ROUTING_TEST_GUARD
            .lock()
            .expect("debug routing guard");

        let args = Cli::default();
        let startup = StartupContext {
            workspace: PathBuf::from("."),
            agent_config: startup_agent_config(),
            config: VTCodeConfig::default(),
            skip_confirmations: false,
            full_auto_requested: false,
            automation_prompt: None,
            session_resume: Some(SessionResumeMode::Specific("session-123".to_string())),
            resume_show_all: false,
            custom_session_id: None,
            summarize_fork: false,
            plan_mode_entry_source:
                vtcode_core::core::interfaces::session::PlanModeEntrySource::None,
        };

        configure_runtime_debug_context("seed".to_string(), Some("seed".to_string()));
        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        runtime.block_on(configure_debug_session_routing(
            &args, &startup, &None, &None,
        ));

        assert_eq!(runtime_archive_session_id().as_deref(), Some("session-123"));
    }

    #[test]
    fn configure_debug_session_routing_reuses_exec_resume_identifier() {
        let _guard = DEBUG_ROUTING_TEST_GUARD
            .lock()
            .expect("debug routing guard");

        let args = Cli::parse_from(["vtcode", "exec", "resume", "session-456", "continue"]);
        let startup = StartupContext {
            workspace: PathBuf::from("."),
            agent_config: startup_agent_config(),
            config: VTCodeConfig::default(),
            skip_confirmations: false,
            full_auto_requested: false,
            automation_prompt: None,
            session_resume: None,
            resume_show_all: false,
            custom_session_id: None,
            summarize_fork: false,
            plan_mode_entry_source:
                vtcode_core::core::interfaces::session::PlanModeEntrySource::None,
        };

        configure_runtime_debug_context("seed".to_string(), Some("seed".to_string()));
        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        runtime.block_on(configure_debug_session_routing(
            &args, &startup, &None, &None,
        ));

        assert_eq!(runtime_archive_session_id().as_deref(), Some("session-456"));
    }
}
