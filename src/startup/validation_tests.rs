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
    assert_policy(Cli::parse_from(["vtcode", "man"]), StartupCommandKind::Metadata, false, false, false, false, false);
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
