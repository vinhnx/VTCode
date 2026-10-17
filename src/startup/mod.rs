use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use vtcode_core::dotfile_protection::init_global_guardian;
use vtcode_core::utils::validation::validate_path_exists;

mod config_loading;
mod dependency_advisories;
mod first_run;
mod first_run_prompts;
mod resume;
mod theme;
mod validation;
mod workspace_trust;

use config_loading::load_startup_config;
pub(crate) use dependency_advisories::{SearchToolsBundleNotice, take_search_tools_bundle_notice};
use resume::{resolve_session_resume, validate_resume_all_usage};
use theme::determine_theme;
use validation::{
    apply_cli_permission_overrides, validate_full_auto_configuration, validate_runtime_provider,
    validate_startup_configuration,
};
use vtcode_config::auth::{OpenAIChatGptAuthHandle, resolve_openai_auth};
use vtcode_config::workspace_env::read_workspace_env_value;
use vtcode_config::{OpenAIPreferredMethod, PromptCacheRetention};
use vtcode_core::cli::args::{Cli, Commands};
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::config::models::{Provider, model_catalog_entry};
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::config::validator::{check_openai_hosted_shell_compat, check_prompt_cache_retention_compat};
use vtcode_core::copilot::{CopilotAuthStatusKind, probe_auth_status};
use vtcode_core::core::agent::config::{
    RuntimeModelSelection, build_runtime_agent_config, provider_label, resolve_runtime_model_selection,
};
use vtcode_core::core::interfaces::session::PlanningEntrySource;
use vtcode_core::{initialize_dot_folder, update_model_preference, update_theme_preference};
pub(crate) use workspace_trust::{
    auto_grant_tui_full_auto_workspace_trust, ensure_full_auto_workspace_trust, require_full_auto_workspace_trust,
};

/// Aggregated data required for CLI command execution after startup.
#[derive(Debug, Clone)]
pub(crate) struct StartupContext {
    pub(crate) workspace: PathBuf,
    pub(crate) config: VTCodeConfig,
    pub(crate) agent_config: CoreAgentConfig,
    pub(crate) skip_confirmations: bool,
    pub(crate) full_auto_requested: bool,
    pub(crate) automation_prompt: Option<String>,
    pub(crate) primary_agent_explicitly_configured: bool,
    pub(crate) session_resume: Option<SessionResumeMode>,
    pub(crate) resume_show_all: bool,
    pub(crate) custom_session_id: Option<String>,
    pub(crate) summarize_fork: bool,
    pub(crate) planning_entry_source: PlanningEntrySource,
}

#[derive(Debug, Clone)]
pub(crate) enum SessionResumeMode {
    Interactive,
    Latest,
    Specific(String),
    Fork(String), // Fork from specific session ID
}

/// Startup work selected by the command being launched.
///
/// The policy is intentionally centralized because startup has several
/// independent side effects: environment loading, legacy migration, provider
/// authentication, theme preference access, runtime security initialization,
/// update checks, and spool cleanup. A command that does not need one of those
/// services must not pay for it or accidentally create user state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StartupCommandKind {
    /// Offline command metadata such as schemas, model information, and man pages.
    Metadata,
    /// A provider-backed, tool-free one-shot request (`ask` or `--print`).
    Ask,
    /// Interactive chat or session continuation.
    Interactive,
    /// A path that can execute agent tools or otherwise needs the full runtime.
    ToolCapable,
    /// The Codex app-server proxy, which owns authentication but still executes tools.
    AppServer,
    /// Existing command-owned paths that intentionally do not use the LLM runtime.
    CommandOwned,
    /// Unknown commands retain the conservative historical startup behavior.
    Conservative,
}

/// Whether provider auth may be missing for this invocation.
///
/// Interactive and ACP sessions resolve auth later. `--print` with empty text
/// and a TTY stdin cannot call the model (no piped prompt), so auth is
/// irrelevant and `build_print_prompt` fails with "No prompt provided".
/// A real `--print` prompt (inline or piped) still requires credentials.
fn allow_missing_provider_auth_for(print: Option<&str>, empty_print_on_tty: bool, command: Option<&Commands>) -> bool {
    let no_command = command.is_none() || matches!(command, Some(Commands::AgentClientProtocol { .. }));
    if !no_command {
        return false;
    }
    print.is_none() || empty_print_on_tty
}

/// Central startup policy for one parsed CLI invocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct StartupPolicy {
    kind: StartupCommandKind,
    allow_missing_provider_auth: bool,
}

impl StartupPolicy {
    /// Classify parsed arguments before any command-specific startup side effects run.
    #[must_use]
    pub(crate) fn for_args(args: &Cli) -> Self {
        // `--print` is global, but it must not downgrade a tool-capable
        // subcommand to the tool-free Ask path. Doing so would skip the
        // runtime security initialization required by exec/review.
        let kind = if is_config_reset_command(args) {
            StartupCommandKind::CommandOwned
        } else if matches!(args.command, Some(Commands::Exec { .. } | Commands::Review(_))) {
            StartupCommandKind::ToolCapable
        } else if args.print.is_some() {
            StartupCommandKind::Ask
        } else {
            match args.command.as_ref() {
                Some(Commands::Ask { .. }) => StartupCommandKind::Ask,
                Some(Commands::Schema { .. } | Commands::Models { .. } | Commands::Man { .. }) => {
                    StartupCommandKind::Metadata
                }
                Some(Commands::AppServer { .. }) => StartupCommandKind::AppServer,
                Some(
                    Commands::Chat
                    | Commands::ChatVerbose
                    | Commands::Continue
                    | Commands::AgentClientProtocol { .. }
                    | Commands::Exec { .. }
                    | Commands::Review(_)
                    | Commands::Analyze { .. }
                    | Commands::Benchmark { .. }
                    | Commands::BackgroundSubagent(_)
                    | Commands::AnthropicApi { .. },
                ) => {
                    if args.full_auto.as_ref().is_some_and(|prompt| !prompt.trim().is_empty()) {
                        StartupCommandKind::ToolCapable
                    } else if matches!(args.command, Some(Commands::Chat | Commands::ChatVerbose | Commands::Continue))
                    {
                        StartupCommandKind::Interactive
                    } else {
                        StartupCommandKind::ToolCapable
                    }
                }
                Some(
                    Commands::ToolPolicy { .. }
                    | Commands::Webmcp { .. }
                    | Commands::Login { .. }
                    | Commands::Logout { .. }
                    | Commands::Auth { .. }
                    | Commands::Notify { .. }
                    | Commands::Pods { .. }
                    | Commands::Schedule { .. }
                    | Commands::Update { .. }
                    | Commands::Dependencies { .. }
                    | Commands::Secret { .. },
                ) => StartupCommandKind::CommandOwned,
                None if args.full_auto.as_ref().is_some_and(|prompt| !prompt.trim().is_empty()) => {
                    StartupCommandKind::ToolCapable
                }
                None => StartupCommandKind::Interactive,
                Some(_) => StartupCommandKind::Conservative,
            }
        };

        // `--print` with empty text and a TTY stdin cannot call the model (no
        // piped prompt): it will fail in `build_print_prompt` with "No prompt
        // provided" before any LLM call. Requiring provider auth first turns
        // that into a misleading "Authentication not found". Piped stdin may
        // still carry a prompt, so only the TTY case skips auth.
        let print_has_empty_prompt = args.print.as_ref().is_some_and(|prompt| prompt.trim().is_empty());
        let stdin_is_tty = {
            use vtcode_core::utils::tty::TtyExt;
            std::io::stdin().is_tty_ext()
        };
        let allow_missing_provider_auth = allow_missing_provider_auth_for(
            args.print.as_deref(),
            print_has_empty_prompt && stdin_is_tty,
            args.command.as_ref(),
        );

        Self { kind, allow_missing_provider_auth }
    }

    /// Whether `.env` loading is needed before this command starts.
    #[must_use]
    pub(crate) const fn load_dotenv(self) -> bool {
        !matches!(self.kind, StartupCommandKind::Metadata)
    }

    /// Whether legacy global paths should be migrated during startup.
    #[must_use]
    pub(crate) const fn migrate_legacy_paths(self) -> bool {
        !matches!(self.kind, StartupCommandKind::Metadata)
    }

    /// Whether startup should resolve provider credentials and perform auth preflight.
    #[must_use]
    pub(crate) const fn resolve_provider_auth(self) -> bool {
        matches!(
            self.kind,
            StartupCommandKind::Ask
                | StartupCommandKind::Interactive
                | StartupCommandKind::ToolCapable
                | StartupCommandKind::Conservative
        )
    }

    /// Whether a missing credential may be handed to the command to resolve later.
    #[must_use]
    pub(crate) const fn allow_missing_provider_auth(self) -> bool {
        self.allow_missing_provider_auth
    }

    /// Whether the selected provider must be known before dispatch.
    #[must_use]
    pub(crate) const fn validate_provider(self) -> bool {
        matches!(
            self.kind,
            StartupCommandKind::Ask | StartupCommandKind::Interactive | StartupCommandKind::ToolCapable
        )
    }

    /// Whether provider/model startup diagnostics should run for this command.
    #[must_use]
    pub(crate) const fn validate_startup_configuration(self) -> bool {
        !matches!(self.kind, StartupCommandKind::Metadata)
    }

    /// Whether the agent-tool runtime (guardian, gatekeeper, caches, and archive settings) is required.
    #[must_use]
    pub(crate) const fn initialize_runtime(self) -> bool {
        !matches!(self.kind, StartupCommandKind::Metadata | StartupCommandKind::Ask | StartupCommandKind::CommandOwned)
    }

    /// Whether the process-wide user dot-folder should be initialized.
    #[must_use]
    pub(crate) const fn initialize_dot_folder(self) -> bool {
        !matches!(self.kind, StartupCommandKind::Metadata)
    }

    /// Whether the user's persisted theme preference may be read.
    #[must_use]
    pub(crate) const fn read_theme_preference(self) -> bool {
        matches!(self.kind, StartupCommandKind::Interactive)
    }

    /// Whether the selected theme may be persisted after activation.
    #[must_use]
    pub(crate) const fn persist_theme_preference(self) -> bool {
        self.read_theme_preference()
    }

    /// Whether the terminal palette probe should be started before startup context resolution.
    #[must_use]
    pub(crate) const fn run_terminal_probe(self) -> bool {
        matches!(self.kind, StartupCommandKind::Interactive)
    }

    /// Whether background update checks and spool cleanup belong to this launch.
    #[must_use]
    pub(crate) const fn run_interactive_maintenance(self) -> bool {
        matches!(self.kind, StartupCommandKind::Interactive)
    }

    /// Whether this command enters the interactive agent session.
    #[must_use]
    pub(crate) const fn runs_interactive_session(self) -> bool {
        matches!(self.kind, StartupCommandKind::Interactive)
    }
}

/// Return the one startup policy for a parsed invocation.
#[must_use]
pub(crate) fn command_startup_policy(args: &Cli) -> StartupPolicy {
    StartupPolicy::for_args(args)
}

/// Whether the invocation only needs the configuration reset service.
///
/// Reset must remain usable when the layer being cleared is malformed, so it
/// cannot depend on the normal provider/auth startup path successfully
/// deserializing the current effective configuration first.
pub(crate) fn is_config_reset_command(args: &Cli) -> bool {
    matches!(
        args.command,
        Some(Commands::Config {
            command: Some(vtcode_core::cli::args::ConfigCommand::Reset(_)),
            ..
        })
    )
}

impl StartupContext {
    pub(crate) async fn from_cli_args(args: &Cli) -> Result<Self> {
        let startup_start = std::time::Instant::now();
        let startup_policy = command_startup_policy(args);
        let config_phase = vtcode_commons::startup_trace::phase_started();
        let loaded = load_startup_config(args).await?;
        vtcode_commons::startup_trace::record_phase("config", config_phase);
        tracing::debug!(
            target = "vtcode.startup",
            phase = "config",
            elapsed_ms = startup_start.elapsed().as_millis() as u64,
            "startup phase complete"
        );
        if args.workspace_path.is_some() {
            validate_path_exists(&loaded.workspace, "Workspace")?;
        }
        if loaded.full_auto_requested {
            validate_full_auto_configuration(&loaded.config, &loaded.workspace)?;
        }
        tracing::debug!(
            target = "vtcode.startup",
            phase = "validation",
            elapsed_ms = startup_start.elapsed().as_millis() as u64,
            "startup phase complete"
        );

        let mut config = loaded.config;
        apply_codex_experimental_override(&mut config, args.codex_experimental_override());
        let uses_interactive_ui = startup_policy.runs_interactive_session();

        let planning_entry_source = PlanningEntrySource::None;
        apply_cli_permission_overrides(&mut config, &args.allowed_tools, &args.disallowed_tools);

        // Validate configuration against models database
        let validation_phase = vtcode_commons::startup_trace::phase_started();
        if startup_policy.validate_startup_configuration() {
            validate_startup_configuration(&config, &loaded.workspace, args.quiet, uses_interactive_ui).await?;
        }

        let (custom_session_id, session_resume) = resolve_session_resume(args)?;
        validate_resume_all_usage(args, session_resume.as_ref())?;
        vtcode_commons::startup_trace::record_phase("validation", validation_phase);

        if session_resume.is_some() && args.command.is_some() {
            bail!(
                "--resume/--continue/--fork-session cannot be combined with other commands. Run the operation without a subcommand."
            );
        }

        let mut selection = resolve_runtime_model_selection(args, &config);
        let auth_phase = if startup_policy.resolve_provider_auth() {
            vtcode_commons::startup_trace::phase_started()
        } else {
            None
        };
        let codex_fallback_notice = if !startup_policy.resolve_provider_auth() {
            None
        } else {
            maybe_apply_codex_sidecar_fallback(
                &mut config,
                &loaded.workspace,
                &mut selection,
                loaded.first_run_occurred,
            )
            .await?
        };

        // Fail fast on an unknown provider before the terminal probe / TUI
        // starts, so an invalid `--provider` or broken `[agent].provider` yields
        // a clean, actionable error instead of an opaque mid-session failure.
        if startup_policy.validate_provider() {
            validate_runtime_provider(&config, &selection.provider)?;
        }

        // --- Parallelized / gated startup fan-out -------------------------------
        // Everything below depends only on `config`/`args`/`selection` (already
        // resolved above) and is mutually independent. Join the futures so the
        // disk I/O (dotfolder creation, guardian audit-log read, theme/config
        // load, provider auth probe) overlaps instead of running serially.
        // The tool-runtime initializers (gatekeeper, file/command caches, file
        // opener, session-archive, perf telemetry) remain enabled for every
        // path that can execute agent tools. AppServer intentionally skips
        // provider authentication but still needs these checks because its
        // proxy executes tools on the user's behalf.
        let skip_auth = !startup_policy.resolve_provider_auth();
        let skip_runtime_init = !startup_policy.initialize_runtime();

        let theme_fut = determine_theme(args, &config, startup_policy.read_theme_preference());
        let dot_folder_fut = async {
            if startup_policy.initialize_dot_folder() {
                initialize_dot_folder().await.ok();
            }
        };
        let guardian_fut = async {
            if skip_runtime_init {
                return;
            }
            if let Err(e) = init_global_guardian(config.dotfile_protection.clone()).await {
                tracing::warn!("Failed to initialize dotfile protection: {}", e);
            }
        };
        let appliers_fut = async {
            if skip_runtime_init {
                return;
            }
            vtcode_core::utils::session_archive::apply_session_history_config_from_vtcode(&config);
            vtcode_core::utils::ansi::apply_file_opener_config(config.file_opener);
            vtcode_core::telemetry::perf::initialize_perf_telemetry(&config.telemetry);
            vtcode_core::tools::cache::configure_file_cache(&config.optimization.file_read_cache);
            vtcode_core::tools::read_limits::configure_read_limits(&config.optimization.file_read_cache);
            vtcode_core::tools::command_cache::configure_command_cache(&config.optimization.command_cache);
            vtcode_core::utils::gatekeeper::initialize_gatekeeper(&config.security.gatekeeper, Some(&loaded.workspace));
        };
        let auth_fut = async {
            if skip_auth {
                return Ok::<_, anyhow::Error>((String::new(), None));
            }
            match resolve_runtime_provider_auth(
                &config,
                &loaded.workspace,
                &selection,
                loaded.first_run_occurred,
                args.command.as_ref(),
            )
            .await
            {
                Ok(auth) => Ok(auth),
                Err(err) if startup_policy.allow_missing_provider_auth() => {
                    tracing::warn!("starting VT Code without provider auth: {err}");
                    Ok((String::new(), None))
                }
                Err(err) => Err(err),
            }
        };

        let (theme_res, _dot_folder_res, _guardian_done, _appliers_done, auth_res) =
            tokio::join!(theme_fut, dot_folder_fut, guardian_fut, appliers_fut, auth_fut);

        let theme_resolution = theme_res?;
        let theme_selection = theme_resolution.theme;

        // Only interactive sessions read or persist the user's theme
        // preference. Other commands use CLI/config/terminal/default values
        // without creating or changing user dot-config state.
        if startup_policy.persist_theme_preference() {
            let theme_changed = theme_resolution
                .loaded_dot_config
                .as_ref()
                .map(|dot| dot.preferences.theme.trim() != theme_selection.as_str())
                .unwrap_or(true);
            if theme_changed {
                update_theme_preference(&theme_selection).await.ok();
            }
        }
        vtcode_core::utils::dot_config::set_startup_user_config(if startup_policy.read_theme_preference() {
            theme_resolution.loaded_dot_config
        } else {
            None
        });

        let (api_key, openai_chatgpt_auth) = auth_res?;
        vtcode_commons::startup_trace::record_phase("auth", auth_phase);
        tracing::debug!(
            target = "vtcode.startup",
            phase = "auth_and_runtime",
            elapsed_ms = startup_start.elapsed().as_millis() as u64,
            "startup phase complete"
        );

        let mut agent_config =
            build_runtime_agent_config(args, &config, loaded.workspace.clone(), selection, api_key, theme_selection);
        agent_config.openai_chatgpt_auth = openai_chatgpt_auth;

        let skip_confirmations = args.dangerously_skip_permissions || args.skip_confirmations;

        // CLI validation: warn if prompt_cache_retention is set but model does not use Responses API
        if agent_config.provider.eq_ignore_ascii_case("openai")
            && let Some(retention) = agent_config.prompt_cache.providers.openai.prompt_cache_retention
            && retention != PromptCacheRetention::Unknown
        {
            // Use constants list to identify which models use Responses API
            if let Some(msg) = check_prompt_cache_retention_compat(&config, &agent_config.model, &agent_config.provider)
            {
                tracing::warn!("{}", msg);
            }
        }

        if let Some(msg) = check_openai_hosted_shell_compat(&config, &agent_config.model, &agent_config.provider) {
            tracing::warn!("{}", msg);
        }

        if let Some(notice) = codex_fallback_notice
            && !args.quiet
        {
            tracing::warn!("{notice}");
        }

        Ok(StartupContext {
            workspace: loaded.workspace,
            config,
            agent_config,
            skip_confirmations,
            full_auto_requested: loaded.full_auto_requested,
            automation_prompt: loaded.automation_prompt,
            primary_agent_explicitly_configured: loaded.primary_agent_explicitly_configured,
            session_resume,
            resume_show_all: args.all,
            custom_session_id,
            summarize_fork: args.summarize,
            planning_entry_source,
        })
    }
}

/// Defer the warning-only prompt-size calculation until the first interactive
/// frame has been drawn. Non-interactive diagnostics call the same check
/// synchronously during validation.
pub(crate) fn defer_system_prompt_size_check(config: &VTCodeConfig, workspace: &Path) {
    if !config.agent.system_prompt_budget_warning {
        return;
    }

    let config = config.clone();
    let workspace = workspace.to_path_buf();
    vtcode_commons::startup_trace::install_first_render_hook(move || {
        tokio::spawn(async move {
            validation::check_system_prompt_size(&config, &workspace).await;
        });
    });
}

fn apply_codex_experimental_override(config: &mut VTCodeConfig, override_value: Option<bool>) {
    if let Some(enabled) = override_value {
        config.agent.codex_app_server.experimental_features = enabled;
    }
}

async fn maybe_apply_codex_sidecar_fallback(
    config: &mut VTCodeConfig,
    workspace: &Path,
    selection: &mut RuntimeModelSelection,
    first_run_occurred: bool,
) -> Result<Option<String>> {
    if !selection.provider.eq_ignore_ascii_case(crate::codex_app_server::CODEX_PROVIDER) {
        return Ok(None);
    }

    let unavailable = match crate::codex_app_server::ensure_codex_sidecar_available(Some(config)) {
        Ok(()) => return Ok(None),
        Err(err) => err,
    };

    let fallback = match resolve_codex_fallback_selection(config, workspace, selection, first_run_occurred).await {
        Ok(fallback) => fallback,
        Err(err) => return Err(anyhow!("{unavailable} {err}")),
    };
    persist_runtime_selection(config, workspace, &fallback).await?;

    let notice = format!(
        "{} Falling back to {} ({}) and updating the saved VT Code selection.",
        unavailable,
        provider_label(&fallback.provider, Some(config)),
        fallback.model
    );
    *selection = fallback;
    Ok(Some(notice))
}

async fn resolve_codex_fallback_selection(
    config: &VTCodeConfig,
    workspace: &Path,
    selection: &RuntimeModelSelection,
    first_run_occurred: bool,
) -> Result<RuntimeModelSelection> {
    let openai_candidate = RuntimeModelSelection {
        model: openai_fallback_model(&selection.model),
        provider: "openai".to_string(),
        api_key_env: Provider::OpenAI.default_api_key_env().to_string(),
        model_source: selection.model_source,
    };
    let copilot_candidate = RuntimeModelSelection {
        model: vtcode_core::config::constants::models::copilot::DEFAULT_MODEL.to_string(),
        provider: "copilot".to_string(),
        api_key_env: Provider::Copilot.default_api_key_env().to_string(),
        model_source: selection.model_source,
    };

    let (openai_result, copilot_result) = tokio::join!(
        resolve_runtime_provider_auth(config, workspace, &openai_candidate, first_run_occurred, None),
        resolve_runtime_provider_auth(config, workspace, &copilot_candidate, first_run_occurred, None)
    );

    if openai_result.is_ok() {
        return Ok(openai_candidate);
    }
    if copilot_result.is_ok() {
        return Ok(copilot_candidate);
    }

    bail!(
        "No authenticated fallback provider is available. Authenticate OpenAI (`vtcode login openai` or OPENAI_API_KEY) or GitHub Copilot (`vtcode login copilot`)."
    );
}

fn openai_fallback_model(model: &str) -> String {
    if model_catalog_entry("openai", model).is_some() {
        return model.to_string();
    }

    vtcode_core::config::constants::models::openai::DEFAULT_MODEL.to_string()
}

async fn persist_runtime_selection(
    config: &mut VTCodeConfig,
    workspace: &Path,
    selection: &RuntimeModelSelection,
) -> Result<()> {
    config.agent.provider = selection.provider.clone();
    config.agent.default_model = selection.model.clone();
    config.agent.api_key_env = selection.api_key_env.clone();
    let tier_supported = selection
        .provider
        .parse::<Provider>()
        .map(|provider| {
            use vtcode_core::config::models::ProviderModelSupport;
            provider.supports_service_tier(&selection.model)
        })
        .unwrap_or(false);
    if !tier_supported {
        config.provider.openai.service_tier = None;
    }

    let mut manager = crate::main_helpers::load_workspace_config(workspace)?;
    manager.save_config(config)?;
    update_model_preference(&selection.provider, &selection.model).await.ok();
    Ok(())
}

async fn resolve_runtime_provider_auth(
    config: &VTCodeConfig,
    workspace: &Path,
    selection: &RuntimeModelSelection,
    first_run_occurred: bool,
    command: Option<&Commands>,
) -> Result<(String, Option<OpenAIChatGptAuthHandle>)> {
    if selection.provider.eq_ignore_ascii_case(crate::codex_app_server::CODEX_PROVIDER) {
        return Ok((String::new(), None));
    }

    if selection.provider.eq_ignore_ascii_case("openai") {
        if !selection
            .api_key_env
            .eq_ignore_ascii_case(Provider::OpenAI.default_api_key_env())
        {
            let resolved = vtcode_config::api_keys::resolve_credential_with_mode(
                &selection.provider,
                &selection.api_key_env,
                Some(workspace),
                config.agent.credential_storage_mode,
            )?;
            let api_key = resolved.and_then(|credential| credential.secret).ok_or_else(|| {
                anyhow!("{}", missing_api_key_message(config, selection, first_run_occurred, command, workspace))
            })?;
            let mut auth_config = config.auth.openai.clone();
            auth_config.preferred_method = OpenAIPreferredMethod::ApiKey;
            let auth = resolve_openai_auth(&auth_config, config.agent.credential_storage_mode, Some(api_key))?;
            return Ok((auth.api_key().to_string(), auth.handle()));
        }
        let storage_mode = config.agent.credential_storage_mode;
        let resolved = vtcode_config::api_keys::resolve_credential_with_mode(
            &selection.provider,
            &selection.api_key_env,
            Some(workspace),
            storage_mode,
        )?;
        let api_key = match resolved.and_then(|credential| credential.secret) {
            Some(api_key) => Some(api_key),
            None if config.auth.openai.preferred_method == OpenAIPreferredMethod::ApiKey => {
                vtcode_config::api_keys::load_stored_api_key_with_mode("openai", storage_mode)?
            }
            None => None,
        };
        let resolved = resolve_openai_auth(&config.auth.openai, config.agent.credential_storage_mode, api_key)
            .with_context(|| missing_api_key_message(config, selection, first_run_occurred, command, workspace))?;
        return Ok((resolved.api_key().to_string(), resolved.handle()));
    }

    if selection.provider.eq_ignore_ascii_case("copilot") {
        let status = probe_auth_status(&config.auth.copilot, Some(workspace)).await;
        return match status.kind {
            CopilotAuthStatusKind::Authenticated => Ok((String::new(), None)),
            CopilotAuthStatusKind::Unauthenticated | CopilotAuthStatusKind::AuthFlowFailed => {
                Err(anyhow::anyhow!(status.message.unwrap_or_else(|| {
                    missing_api_key_message(config, selection, first_run_occurred, command, workspace)
                })))
            }
            CopilotAuthStatusKind::ServerUnavailable => Err(anyhow::anyhow!(
                status.message.unwrap_or_else(|| {
                    "GitHub Copilot CLI is unavailable. Install `copilot`, set `VTCODE_COPILOT_COMMAND`, or configure `[auth.copilot].command`."
                        .to_string()
                })
            )),
        };
    }

    if is_local_model_without_remote_auth(selection) {
        return Ok((String::new(), None));
    }

    if let Some(custom_provider) = config.custom_provider(&selection.provider) {
        if custom_provider.uses_command_auth() {
            return Ok((String::new(), None));
        }
        if let Some(credential) = vtcode_config::api_keys::resolve_credential_with_mode(
            &selection.provider,
            &selection.api_key_env,
            Some(workspace),
            config.agent.credential_storage_mode,
        )? {
            if let Some(api_key) = credential.secret {
                return Ok((api_key, None));
            }
        }
    }

    let api_key = vtcode_config::api_keys::resolve_credential_with_mode(
        &selection.provider,
        &selection.api_key_env,
        Some(workspace),
        config.agent.credential_storage_mode,
    )?
    .and_then(|credential| credential.secret)
    .ok_or_else(|| anyhow!("{}", missing_api_key_message(config, selection, first_run_occurred, command, workspace)))?;
    Ok((api_key, None))
}

fn is_local_model_without_remote_auth(selection: &RuntimeModelSelection) -> bool {
    let provider = selection.provider.trim().to_ascii_lowercase();
    matches!(provider.as_str(), "ollama" | "lmstudio" | "llamacpp" | "llama.cpp" | "llama-cpp")
        && !selection.model.to_ascii_lowercase().contains("cloud")
}

fn missing_api_key_message(
    config: &VTCodeConfig,
    selection: &RuntimeModelSelection,
    first_run_occurred: bool,
    command: Option<&Commands>,
    workspace: &Path,
) -> String {
    let provider_name = provider_label(&selection.provider, Some(config));
    let tui_hint = if command_launches_tui(command) {
        format!(
            "Run `/secret add {provider_name}` in the interactive session to store it in secure storage (recommended)."
        )
    } else {
        format!("Run `vtcode secret add {provider_name}` to store it in secure storage (recommended).")
    };

    let env_var = selection.api_key_env.clone();

    let env_hint = format!("Set {env_var} environment variable,");
    let config_hint = "configure it in vtcode.toml";

    let in_env = read_workspace_env_value(workspace, &env_var)
        .map(|v| v.is_some())
        .unwrap_or(false);
    let migrate_hint = if in_env {
        if command_launches_tui(command) {
            "Or run `/secret migrate` to move keys from workspace `.env` to secure storage."
        } else {
            "Or run `vtcode secret migrate` to move keys from workspace `.env` to secure storage."
        }
    } else {
        ""
    };

    if selection.provider.eq_ignore_ascii_case(crate::codex_app_server::CODEX_PROVIDER) {
        return format!(
            "Codex authentication is managed by the official `codex app-server`. Run `vtcode auth codex` or `vtcode login codex`. {}",
            crate::codex_app_server::codex_sidecar_requirement_note()
        );
    }

    if selection.provider.eq_ignore_ascii_case("copilot") {
        return "Authentication not found for GitHub Copilot. Run `vtcode login copilot`. Install `copilot` first if needed; `gh` is only an optional fallback."
            .to_string();
    }

    if let Some(custom_provider) = config.custom_provider(&selection.provider) {
        let custom_env_var = custom_provider.resolved_api_key_env();
        let custom_env_hint = format!("Set {custom_env_var} environment variable,");
        let custom_config_hint = "configure it in vtcode.toml under [[custom_providers]]";
        let base = if first_run_occurred {
            format!(
                "API key not found for {provider_name}. To fix:\n  1. {tui_hint}\n  2. {custom_env_hint} or\n  3. {custom_config_hint}\n\nRun `/init` anytime to reconfigure."
            )
        } else {
            format!(
                "API key not found for custom provider '{provider_name}'. {tui_hint} Or {custom_env_hint} {custom_config_hint}."
            )
        };
        let msg = if migrate_hint.is_empty() {
            base
        } else {
            format!("{}\n{}", base, migrate_hint)
        };
        return msg;
    }

    if selection.provider.eq_ignore_ascii_case("openai") {
        let base = format!(
            "Authentication not found for OpenAI. {tui_hint} Or {env_hint} {config_hint}, or run `vtcode login openai`."
        );
        if migrate_hint.is_empty() {
            return base;
        } else {
            return format!("{} {}", base, migrate_hint);
        }
    }

    if first_run_occurred {
        let base = format!(
            "API key not found for {provider_name}. To fix:\n  1. {tui_hint}\n  2. {env_hint} or\n  3. {config_hint}\n\nRun `/init` anytime to reconfigure."
        );
        if migrate_hint.is_empty() {
            base
        } else {
            format!("{}\n{}", base, migrate_hint)
        }
    } else {
        let base =
            format!("API key not found for provider '{}'. {tui_hint} Or {env_hint} {config_hint}", selection.provider,);
        if migrate_hint.is_empty() {
            base
        } else {
            format!("{} {}", base, migrate_hint)
        }
    }
}

fn command_launches_tui(command: Option<&Commands>) -> bool {
    matches!(command, None | Some(Commands::Chat | Commands::ChatVerbose | Commands::Continue))
}

#[cfg(test)]
mod validation_tests {
    use super::*;
    use assert_fs::TempDir;
    use clap::Parser;
    use std::fs;
    use std::io::Write;
    use std::path::Path;
    use vtcode_commons::env_lock;
    use vtcode_config::ConfigManager;
    use vtcode_config::OpenAIPreferredMethod;
    use vtcode_config::auth::AuthCredentialsStoreMode;
    use vtcode_core::cli::args::Cli;

    fn write_fake_executable(path: &Path) {
        fs::write(path, "#!/bin/sh\nexit 0\n").expect("write fake executable");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(path).expect("metadata").permissions();
            perms.set_mode(0o755);
            fs::set_permissions(path, perms).expect("set permissions");
        }
    }

    fn save_workspace_config(workspace: &Path, config: &VTCodeConfig) {
        let path = workspace.join("vtcode.toml");
        ConfigManager::save_config_to_path(&path, config).expect("save workspace config");
        let mut file = fs::OpenOptions::new()
            .append(true)
            .open(path)
            .expect("open workspace config for isolation marker");
        writeln!(file, "\n[workspace]\nuse_root_config = true").expect("write isolation marker");
    }

    #[test]
    fn retention_warning_for_non_responses_model() {
        let mut cfg = VTCodeConfig::default();
        cfg.prompt_cache.providers.openai.prompt_cache_retention = Some(PromptCacheRetention::H24);
        let model = "gpt-oss-20b"; // not in responses API list
        let provider = "openai";
        assert!(check_prompt_cache_retention_compat(&cfg, model, provider).is_some());
    }

    #[test]
    fn retention_ok_for_responses_model() {
        let mut cfg = VTCodeConfig::default();
        cfg.prompt_cache.providers.openai.prompt_cache_retention = Some(PromptCacheRetention::H24);
        let model = vtcode_core::config::constants::models::openai::GPT_5_6; // responses model
        let provider = "openai";
        assert!(check_prompt_cache_retention_compat(&cfg, model, provider).is_none());
    }

    fn assert_policy(
        args: Cli,
        kind: StartupCommandKind,
        loads_dotenv: bool,
        resolves_auth: bool,
        initializes_runtime: bool,
        reads_theme: bool,
        maintains: bool,
    ) {
        let policy = command_startup_policy(&args);
        assert_eq!(policy.kind, kind, "unexpected policy for {args:?}");
        assert_eq!(policy.load_dotenv(), loads_dotenv, "dotenv policy for {args:?}");
        assert_eq!(policy.migrate_legacy_paths(), loads_dotenv, "migration policy for {args:?}");
        assert_eq!(policy.resolve_provider_auth(), resolves_auth, "auth policy for {args:?}");
        assert_eq!(
            policy.validate_startup_configuration(),
            kind != StartupCommandKind::Metadata,
            "startup validation policy for {args:?}"
        );
        assert_eq!(policy.initialize_runtime(), initializes_runtime, "runtime policy for {args:?}");
        assert_eq!(
            policy.initialize_dot_folder(),
            kind != StartupCommandKind::Metadata,
            "dot folder policy for {args:?}"
        );
        assert_eq!(policy.read_theme_preference(), reads_theme, "theme read policy for {args:?}");
        assert_eq!(policy.persist_theme_preference(), reads_theme, "theme write policy for {args:?}");
        assert_eq!(policy.run_interactive_maintenance(), maintains, "maintenance policy for {args:?}");
    }

    #[test]
    fn interactive_policy_still_requests_migration_but_defers_off_paint_path() {
        // Interactive keeps `migrate_legacy_paths()` so migration still runs,
        // but `run_interactive_maintenance()` is the signal to defer it off the
        // pre-dispatch critical path (see `bootstrap_main`).
        let args = Cli::parse_from(["vtcode"]);
        let policy = command_startup_policy(&args);
        assert!(policy.migrate_legacy_paths(), "interactive must still migrate legacy paths");
        assert!(
            policy.run_interactive_maintenance(),
            "interactive maintenance marks migration as deferrable off first paint"
        );
        let metadata = Cli::parse_from(["vtcode", "schema", "tools"]);
        let meta_policy = command_startup_policy(&metadata);
        assert!(!meta_policy.migrate_legacy_paths(), "metadata must not migrate");
    }

    #[test]
    fn startup_policy_matrix_covers_metadata_ask_interactive_and_tool_paths() {
        assert_policy(
            Cli::parse_from(["vtcode", "schema", "tools"]),
            StartupCommandKind::Metadata,
            false,
            false,
            false,
            false,
            false,
        );
        assert_policy(
            Cli::parse_from(["vtcode", "models", "list"]),
            StartupCommandKind::Metadata,
            false,
            false,
            false,
            false,
            false,
        );
        assert_policy(
            Cli::parse_from(["vtcode", "man"]),
            StartupCommandKind::Metadata,
            false,
            false,
            false,
            false,
            false,
        );
        assert_policy(
            Cli::parse_from(["vtcode", "ask", "hello"]),
            StartupCommandKind::Ask,
            true,
            true,
            false,
            false,
            false,
        );
        assert_policy(
            Cli::parse_from(["vtcode", "--print", "hello"]),
            StartupCommandKind::Ask,
            true,
            true,
            false,
            false,
            false,
        );
        assert_policy(Cli::parse_from(["vtcode"]), StartupCommandKind::Interactive, true, true, true, true, true);
        assert_policy(
            Cli::parse_from(["vtcode", "--continue"]),
            StartupCommandKind::Interactive,
            true,
            true,
            true,
            true,
            true,
        );
        assert_policy(
            Cli::parse_from(["vtcode", "exec", "inspect the workspace"]),
            StartupCommandKind::ToolCapable,
            true,
            true,
            true,
            false,
            false,
        );
        assert_policy(
            Cli::parse_from(["vtcode", "exec", "--print", "inspect the workspace"]),
            StartupCommandKind::ToolCapable,
            true,
            true,
            true,
            false,
            false,
        );
        assert_policy(
            Cli::parse_from(["vtcode", "review", "--print"]),
            StartupCommandKind::ToolCapable,
            true,
            true,
            true,
            false,
            false,
        );
        assert_policy(
            Cli::parse_from(["vtcode", "acp", "zed"]),
            StartupCommandKind::ToolCapable,
            true,
            true,
            true,
            false,
            false,
        );
    }

    #[test]
    fn startup_policy_preserves_app_server_auth_exception_and_runtime_security() {
        assert_policy(
            Cli::parse_from(["vtcode", "app-server"]),
            StartupCommandKind::AppServer,
            true,
            false,
            true,
            false,
            false,
        );
    }

    #[test]
    fn startup_policy_keeps_known_command_owned_paths_off_provider_runtime() {
        assert_policy(
            Cli::parse_from(["vtcode", "tool-policy", "status"]),
            StartupCommandKind::CommandOwned,
            true,
            false,
            false,
            false,
            false,
        );
        assert_policy(
            Cli::parse_from(["vtcode", "config", "reset"]),
            StartupCommandKind::CommandOwned,
            true,
            false,
            false,
            false,
            false,
        );
    }

    #[test]
    fn startup_policy_leaves_unclassified_commands_conservative() {
        assert_policy(
            Cli::parse_from(["vtcode", "config"]),
            StartupCommandKind::Conservative,
            true,
            true,
            true,
            false,
            false,
        );
    }

    #[test]
    fn startup_policy_allows_interactive_and_acp_auth_to_be_resolved_later() {
        let interactive = command_startup_policy(&Cli::parse_from(["vtcode"]));
        assert!(interactive.allow_missing_provider_auth());

        let acp = command_startup_policy(&Cli::parse_from(["vtcode", "acp", "zed"]));
        assert!(acp.allow_missing_provider_auth());

        let ask = command_startup_policy(&Cli::parse_from(["vtcode", "ask", "hello"]));
        assert!(!ask.allow_missing_provider_auth());
    }

    #[test]
    fn startup_policy_allows_missing_auth_for_empty_print_prompt() {
        // Empty `--print` on a TTY cannot call the model: "No prompt provided"
        // is the right error, not an auth failure.
        assert!(allow_missing_provider_auth_for(Some(""), true, None));
        assert!(allow_missing_provider_auth_for(Some("   "), true, None));
        // Piped stdin may carry a prompt — require auth.
        assert!(!allow_missing_provider_auth_for(Some(""), false, None));
        // A real prompt always requires credentials (empty_print_on_tty is
        // false whenever the inline text is non-empty).
        assert!(!allow_missing_provider_auth_for(Some("hello"), false, None));
        // Interactive (no --print) resolves auth later.
        assert!(allow_missing_provider_auth_for(None, false, None));
    }

    #[test]
    fn missing_api_key_message_uses_custom_provider_label_and_env_key() {
        let mut cfg = VTCodeConfig::default();
        cfg.custom_providers.push(vtcode_config::core::CustomProviderConfig {
            name: "mycorp".to_string(),
            display_name: "MyCorporateName".to_string(),
            base_url: "https://llm.example/v1".to_string(),
            context_window: None,
            api_key_env: "MYCORP_API_KEY".to_string(),
            auth: None,
            model: "gpt-5-mini".to_string(),
            models: Vec::new(),
            ..vtcode_config::core::CustomProviderConfig::default()
        });

        let selection = RuntimeModelSelection {
            model: "gpt-5-mini".to_string(),
            provider: "mycorp".to_string(),
            api_key_env: "MYCORP_API_KEY".to_string(),
            model_source: vtcode_core::config::types::ModelSelectionSource::WorkspaceConfig,
        };

        let message = missing_api_key_message(&cfg, &selection, true, None, Path::new("/tmp"));

        assert!(message.contains("MyCorporateName"));
        assert!(message.contains("MYCORP_API_KEY"));
        assert!(message.contains("[[custom_providers]]"));
    }

    #[test]
    fn missing_api_key_message_uses_codex_guidance() {
        let cfg = VTCodeConfig::default();
        let selection = RuntimeModelSelection {
            model: vtcode_core::config::constants::models::openai::GPT_5_CODEX.to_string(),
            provider: "codex".to_string(),
            api_key_env: String::new(),
            model_source: vtcode_core::config::types::ModelSelectionSource::WorkspaceConfig,
        };

        let message = missing_api_key_message(&cfg, &selection, true, None, Path::new("/tmp"));

        assert!(message.contains("codex app-server"));
        assert!(message.contains("vtcode auth codex"));
        assert!(message.contains("`$PATH`"));
    }

    #[test]
    fn missing_api_key_message_suggests_migrate_when_key_in_workspace_env() {
        let tmp = std::env::temp_dir().join(format!("vtcode-test-{}", std::process::id()));
        let _ = fs::create_dir_all(&tmp);
        let mut env_file = fs::File::create(tmp.join(".env")).expect("create .env");
        let _ = env_file.write_all(b"OPENAI_API_KEY=sk-test\n");

        let cfg = VTCodeConfig::default();
        let selection = RuntimeModelSelection {
            model: "gpt-4o".to_string(),
            provider: "openai".to_string(),
            api_key_env: "OPENAI_API_KEY".to_string(),
            model_source: vtcode_core::config::types::ModelSelectionSource::WorkspaceConfig,
        };

        let message = missing_api_key_message(&cfg, &selection, true, None, &tmp);
        eprintln!("MESSAGE: {}", message);

        assert!(message.contains("secret migrate"));
    }

    #[test]
    fn missing_api_key_message_uses_cli_hint_for_headless_exec() {
        let cfg = VTCodeConfig::default();
        let selection = RuntimeModelSelection {
            model: "gpt-4o".to_string(),
            provider: "openai".to_string(),
            api_key_env: "OPENAI_API_KEY".to_string(),
            model_source: vtcode_core::config::types::ModelSelectionSource::WorkspaceConfig,
        };
        let exec_command = Commands::Exec {
            json: false,
            dry_run: false,
            events: None,
            last_message_file: None,
            command: None,
            prompt: Some("hello".to_string()),
        };

        let message = missing_api_key_message(&cfg, &selection, false, Some(&exec_command), Path::new("/tmp"));

        assert!(message.contains("vtcode secret add"));
        assert!(!message.contains("/secret add"));
    }

    #[test]
    fn command_launches_tui_only_for_interactive_sessions() {
        assert!(command_launches_tui(None));
        assert!(command_launches_tui(Some(&Commands::Chat)));
        assert!(command_launches_tui(Some(&Commands::ChatVerbose)));
        assert!(command_launches_tui(Some(&Commands::Continue)));
        assert!(!command_launches_tui(Some(&Commands::Ask {
            prompt: Some("hi".to_string()),
            output_format: None,
        })));
        assert!(!command_launches_tui(Some(&Commands::Exec {
            json: false,
            dry_run: false,
            events: None,
            last_message_file: None,
            command: None,
            prompt: Some("hi".to_string()),
        })));
    }

    #[test]
    fn hosted_shell_warning_for_non_responses_model() {
        let mut cfg = VTCodeConfig::default();
        cfg.provider.openai.hosted_shell.enabled = true;

        let msg = check_openai_hosted_shell_compat(&cfg, "gpt-oss-20b", "openai");
        assert!(msg.is_some());
    }

    #[test]
    fn resolve_session_resume_treats_resume_with_session_suffix_as_fork() {
        let args = Cli::parse_from(["vtcode", "--resume", "session-123", "--session-id", "fork-copy"]);

        let (custom_session_id, session_resume) = resolve_session_resume(&args).expect("session resume should resolve");

        assert_eq!(custom_session_id.as_deref(), Some("fork-copy"));
        assert!(matches!(
            session_resume,
            Some(SessionResumeMode::Fork(ref id)) if id == "session-123"
        ));
    }

    #[test]
    fn resolve_session_resume_treats_continue_with_session_suffix_as_latest_fork() {
        let args = Cli::parse_from(["vtcode", "--continue", "--session-id", "fork-copy"]);

        let (custom_session_id, session_resume) = resolve_session_resume(&args).expect("continue should resolve");

        assert_eq!(custom_session_id.as_deref(), Some("fork-copy"));
        assert!(matches!(
            session_resume,
            Some(SessionResumeMode::Fork(ref id)) if id == "__latest__"
        ));
    }

    #[test]
    fn validate_resume_all_usage_accepts_resume_and_continue_modes() {
        for args in [
            Cli::parse_from(["vtcode", "--resume", "session-123", "--all"]),
            Cli::parse_from(["vtcode", "--continue", "--all"]),
        ] {
            let (_, session_resume) = resolve_session_resume(&args).expect("session resume should resolve");
            validate_resume_all_usage(&args, session_resume.as_ref()).unwrap();
        }
    }

    #[test]
    fn validate_resume_all_usage_rejects_unscoped_all_flag() {
        let args = Cli::parse_from(["vtcode", "--all"]);
        let (_, session_resume) = resolve_session_resume(&args).expect("session resume");
        let err = validate_resume_all_usage(&args, session_resume.as_ref()).expect_err("all flag should be rejected");

        assert!(
            err.to_string()
                .contains("--all can only be used with resume, continue, fork-session, or exec resume")
        );
    }

    #[test]
    fn validate_resume_all_usage_accepts_summarized_interactive_fork_via_session_suffix() {
        let args = Cli::parse_from(["vtcode", "--resume", "--session-id", "fork-copy", "--summarize"]);

        let (_, session_resume) = resolve_session_resume(&args).expect("session resume");

        assert!(matches!(session_resume, Some(SessionResumeMode::Interactive)));
        validate_resume_all_usage(&args, session_resume.as_ref()).unwrap();
    }

    #[tokio::test]
    async fn cli_model_override_updates_merged_startup_config() {
        let env_guard = env_lock::lock();
        let temp = TempDir::new().expect("temp dir");
        let workspace = temp.path().to_path_buf();
        fs::write(workspace.join("vtcode.toml"), "[workspace]\nuse_root_config = true\n")
            .expect("write isolated workspace config");

        env_guard.set_var("OPENAI_API_KEY", "test");
        let args = Cli::parse_from([
            "vtcode",
            "--workspace",
            workspace.to_str().expect("workspace path"),
            "--model",
            vtcode_core::config::constants::models::openai::GPT_5,
        ]);

        let ctx = StartupContext::from_cli_args(&args).await.expect("startup success");

        assert_eq!(ctx.config.agent.default_model, vtcode_core::config::constants::models::openai::GPT_5);
        assert_eq!(ctx.agent_config.model, vtcode_core::config::constants::models::openai::GPT_5);
    }

    #[tokio::test]
    async fn cli_override_with_non_responses_model_warns() {
        let env_guard = env_lock::lock();
        let temp = TempDir::new().expect("temp dir");
        let workspace = temp.path().to_path_buf();
        fs::write(workspace.join("vtcode.toml"), "[workspace]\nuse_root_config = true\n")
            .expect("write isolated workspace config");

        env_guard.set_var("OPENAI_API_KEY", "test");
        let args = Cli::parse_from([
            "vtcode",
            "--workspace",
            workspace.to_str().expect("workspace path"),
            "--model",
            "gpt-oss-20b",
            "--config",
            "prompt_cache.providers.openai.prompt_cache_retention=24h",
        ]);

        let ctx = StartupContext::from_cli_args(&args).await.expect("startup success");
        let maybe_warning =
            check_prompt_cache_retention_compat(&ctx.config, &ctx.agent_config.model, &ctx.agent_config.provider);

        assert!(maybe_warning.is_some());
    }

    #[tokio::test]
    async fn cli_override_with_hosted_shell_on_non_responses_model_warns() {
        let env_guard = env_lock::lock();
        let temp = TempDir::new().expect("temp dir");
        let workspace = temp.path().to_path_buf();
        fs::write(workspace.join("vtcode.toml"), "[workspace]\nuse_root_config = true\n")
            .expect("write isolated workspace config");

        env_guard.set_var("OPENAI_API_KEY", "test");
        let args = Cli::parse_from([
            "vtcode",
            "--workspace",
            workspace.to_str().expect("workspace path"),
            "--model",
            "gpt-oss-20b",
            "--config",
            "provider.openai.hosted_shell.enabled=true",
        ]);

        let ctx = StartupContext::from_cli_args(&args).await.expect("startup success");
        let maybe_warning = check_openai_hosted_shell_compat(&ctx.config, &ctx.agent_config.model, "openai");

        assert!(maybe_warning.is_some());
    }

    #[tokio::test]
    async fn cli_override_with_responses_model_no_warn() {
        let env_guard = env_lock::lock();
        let temp = TempDir::new().expect("temp dir");
        let workspace = temp.path().to_path_buf();
        fs::write(workspace.join("vtcode.toml"), "[workspace]\nuse_root_config = true\n")
            .expect("write isolated workspace config");

        env_guard.set_var("OPENAI_API_KEY", "test");
        let args = Cli::parse_from([
            "vtcode",
            "--workspace",
            workspace.to_str().expect("workspace path"),
            "--model",
            vtcode_core::config::constants::models::openai::GPT_5_6_SOL,
            "--config",
            "prompt_cache.providers.openai.prompt_cache_retention=24h",
        ]);

        let ctx = StartupContext::from_cli_args(&args).await.expect("startup success");
        let maybe_warning =
            check_prompt_cache_retention_compat(&ctx.config, &ctx.agent_config.model, &ctx.agent_config.provider);

        assert!(maybe_warning.is_none());
    }

    #[tokio::test]
    async fn full_auto_preserves_separate_state_without_skip_confirmations() {
        let env_guard = env_lock::lock();
        let temp = TempDir::new().expect("temp dir");
        let workspace = temp.path().to_path_buf();
        let mut config = VTCodeConfig::default();
        config.agent.provider = "openai".to_string();
        config.agent.default_model = vtcode_core::config::constants::models::openai::GPT_5.to_string();
        config.automation.full_auto.enabled = true;
        config.automation.full_auto.require_profile_ack = false;
        save_workspace_config(&workspace, &config);

        env_guard.set_var("OPENAI_API_KEY", "test");
        let args = Cli::parse_from([
            "vtcode",
            "--workspace",
            workspace.to_str().expect("workspace path"),
            "--full-auto",
        ]);

        let ctx = StartupContext::from_cli_args(&args).await.expect("startup success");

        assert!(ctx.full_auto_requested);
        assert!(!ctx.skip_confirmations);
    }

    #[test]
    fn cli_codex_experimental_override_updates_loaded_config() {
        let mut config = VTCodeConfig::default();
        assert!(!config.agent.codex_app_server.experimental_features);

        apply_codex_experimental_override(&mut config, Some(true));
        assert!(config.agent.codex_app_server.experimental_features);

        apply_codex_experimental_override(&mut config, Some(false));
        assert!(!config.agent.codex_app_server.experimental_features);
    }

    #[tokio::test]
    async fn missing_codex_sidecar_falls_back_to_openai_and_persists_selection() {
        let env_guard = env_lock::lock();
        let temp = TempDir::new().expect("temp dir");
        let workspace = temp.path().to_path_buf();
        let mut config = VTCodeConfig::default();
        config.agent.provider = "codex".to_string();
        config.agent.default_model = vtcode_core::config::constants::models::openai::GPT_5_CODEX.to_string();
        config.agent.codex_app_server.command = workspace.join("missing-codex").display().to_string();
        config.agent.credential_storage_mode = AuthCredentialsStoreMode::File;
        config.auth.openai.preferred_method = OpenAIPreferredMethod::ApiKey;
        config.auth.copilot.command = Some(workspace.join("missing-copilot").display().to_string());
        save_workspace_config(&workspace, &config);

        env_guard.set_var("OPENAI_API_KEY", "test-openai-key");
        env_guard.remove_var("GITHUB_TOKEN");
        let args = Cli::parse_from(["vtcode", "--workspace", workspace.to_str().expect("workspace path")]);

        let ctx = StartupContext::from_cli_args(&args)
            .await
            .expect("startup should fall back to openai");

        assert_eq!(ctx.agent_config.provider, "openai");
        assert_eq!(ctx.agent_config.model, vtcode_core::config::constants::models::openai::DEFAULT_MODEL);
        assert_eq!(ctx.config.agent.provider, "openai");
        assert_eq!(ctx.config.agent.default_model, vtcode_core::config::constants::models::openai::DEFAULT_MODEL);

        let persisted = ConfigManager::load_from_workspace(&workspace)
            .expect("reload persisted config")
            .config()
            .clone();
        assert_eq!(persisted.agent.provider, "openai");
        assert_eq!(persisted.agent.default_model, vtcode_core::config::constants::models::openai::DEFAULT_MODEL);
    }

    #[tokio::test]
    async fn missing_codex_sidecar_falls_back_to_copilot_when_openai_is_unavailable() {
        let env_guard = env_lock::lock();
        let temp = TempDir::new().expect("temp dir");
        let workspace = temp.path().to_path_buf();
        let fake_copilot = workspace.join("copilot");
        write_fake_executable(&fake_copilot);

        // Isolate CODEX_HOME so the Codex auth.json fallback doesn't pick up
        // a real Codex session from the user's machine during this test.
        let codex_temp = TempDir::new().expect("temp codex home");
        env_guard.set_var("CODEX_HOME", codex_temp.path());

        let mut config = VTCodeConfig::default();
        config.agent.provider = "codex".to_string();
        config.agent.default_model = vtcode_core::config::constants::models::openai::GPT_5_CODEX.to_string();
        config.agent.codex_app_server.command = workspace.join("missing-codex").display().to_string();
        config.agent.credential_storage_mode = AuthCredentialsStoreMode::File;
        config.auth.openai.preferred_method = OpenAIPreferredMethod::Chatgpt;
        config.auth.copilot.command = Some(fake_copilot.display().to_string());
        save_workspace_config(&workspace, &config);

        env_guard.remove_var("OPENAI_API_KEY");
        env_guard.set_var("GITHUB_TOKEN", "test-github-token");
        let args = Cli::parse_from(["vtcode", "--workspace", workspace.to_str().expect("workspace path")]);

        let ctx = StartupContext::from_cli_args(&args)
            .await
            .expect("startup should fall back to copilot");

        assert_eq!(ctx.agent_config.provider, "copilot");
        assert_eq!(ctx.agent_config.model, vtcode_core::config::constants::models::copilot::DEFAULT_MODEL);
        assert_eq!(ctx.config.agent.provider, "copilot");
    }

    #[tokio::test]
    async fn missing_codex_sidecar_without_fallback_reports_actionable_error() {
        let env_guard = env_lock::lock();
        let temp = TempDir::new().expect("temp dir");
        let workspace = temp.path().to_path_buf();

        // Isolate CODEX_HOME so the Codex auth.json fallback doesn't pick up
        // a real Codex session from the user's machine during this test.
        let codex_temp = TempDir::new().expect("temp codex home");
        env_guard.set_var("CODEX_HOME", codex_temp.path());

        let mut config = VTCodeConfig::default();
        config.agent.provider = "codex".to_string();
        config.agent.default_model = vtcode_core::config::constants::models::openai::GPT_5_CODEX.to_string();
        config.agent.codex_app_server.command = workspace.join("missing-codex").display().to_string();
        config.agent.credential_storage_mode = AuthCredentialsStoreMode::File;
        config.auth.openai.preferred_method = OpenAIPreferredMethod::Chatgpt;
        config.auth.copilot.command = Some(workspace.join("missing-copilot").display().to_string());
        save_workspace_config(&workspace, &config);

        env_guard.remove_var("OPENAI_API_KEY");
        env_guard.remove_var("GITHUB_TOKEN");
        let args = Cli::parse_from(["vtcode", "--workspace", workspace.to_str().expect("workspace path")]);

        let err = StartupContext::from_cli_args(&args)
            .await
            .expect_err("startup should fail without any fallback provider");
        let message = err.to_string();
        assert!(message.contains("Codex app-server sidecar is unavailable"));
        assert!(message.contains("`$PATH`"));
        assert!(message.contains("[agent.codex_app_server].command"));
        assert!(message.contains("No authenticated fallback provider is available"));
    }
}
