use super::*;

#[tokio::test]
async fn config_permissions_opens_permissions_view() {
    let workspace = tempfile::TempDir::new().expect("workspace");
    for command in ["config permissions", "permissions"] {
        let mut renderer = renderer_for_tests();
        let outcome = handle_slash_command(command, &mut renderer, workspace.path())
            .await
            .unwrap_or_else(|error| panic!("{command} should parse: {error}"));

        assert!(matches!(outcome, SlashCommandOutcome::ShowPermissions), "{command} must open permissions");
    }
}
#[tokio::test]
async fn init_slash_command_parses_force_flag() {
    let workspace = tempfile::TempDir::new().expect("workspace");
    let mut renderer = renderer_for_tests();

    let outcome = handle_slash_command("init --force", &mut renderer, workspace.path())
        .await
        .expect("init should parse");

    assert!(matches!(outcome, SlashCommandOutcome::InitializeWorkspace { force: true }));
}
#[tokio::test]
async fn every_registered_slash_command_resolves_without_prompt_fallback() {
    let workspace = tempfile::TempDir::new().expect("workspace");

    for spec in command_skill_specs() {
        let mut renderer = renderer_for_tests();
        let outcome = handle_slash_command(spec.slash_name, &mut renderer, workspace.path())
            .await
            .unwrap_or_else(|error| panic!("{} should parse: {error}", spec.slash_name));

        assert!(
            !matches!(outcome, SlashCommandOutcome::SubmitPrompt { .. }),
            "/{} fell through to plain prompt submission",
            spec.slash_name
        );
    }
}
#[tokio::test]
async fn slash_command_aliases_resolve_to_canonical_backends() {
    let workspace = tempfile::TempDir::new().expect("workspace");

    for alias in [
        "settings",
        "setttings",
        "context",
        "subprocess",
        "subprocesses",
        "models",
        "permissions",
        "tasks",
        "jobs",
        "log",
        "notify",
        "checkup",
    ] {
        let mut renderer = renderer_for_tests();
        let outcome = handle_slash_command(alias, &mut renderer, workspace.path())
            .await
            .unwrap_or_else(|error| panic!("/{alias} should parse: {error}"));

        assert!(
            !matches!(outcome, SlashCommandOutcome::SubmitPrompt { .. }),
            "/{alias} fell through to plain prompt submission",
        );
    }
}
#[tokio::test]
async fn config_consolidated_sections_match_hidden_aliases() {
    let workspace = tempfile::TempDir::new().expect("workspace");

    for (canonical, alias) in [
        ("config permissions", "permissions"),
        ("config tasks", "tasks"),
        ("config jobs", "jobs"),
        ("config log", "log"),
        ("config subprocess", "subprocess"),
        ("config notify hello", "notify hello"),
        ("config checkup --quick", "checkup --quick"),
        ("config checkup", "config doctor"),
        ("config checkup --quick", "config doctor --quick"),
    ] {
        let mut canonical_renderer = renderer_for_tests();
        let canonical_outcome = handle_slash_command(canonical, &mut canonical_renderer, workspace.path())
            .await
            .unwrap_or_else(|error| panic!("{canonical} should parse: {error}"));
        let mut alias_renderer = renderer_for_tests();
        let alias_outcome = handle_slash_command(alias, &mut alias_renderer, workspace.path())
            .await
            .unwrap_or_else(|error| panic!("/{alias} should parse: {error}"));

        assert_eq!(
            std::mem::discriminant(&canonical_outcome),
            std::mem::discriminant(&alias_outcome),
            "/{alias} must match /{canonical}"
        );
    }
}
#[tokio::test]
async fn doctor_runs_checkup_or_routes_prompt_audit() {
    let workspace = tempfile::TempDir::new().expect("workspace");

    for command in ["doctor", "doctor --full", "doctor --quick"] {
        let mut renderer = renderer_for_tests();
        let outcome = handle_slash_command(command, &mut renderer, workspace.path())
            .await
            .unwrap_or_else(|error| panic!("{command} should parse: {error}"));
        assert!(
            matches!(outcome, SlashCommandOutcome::StartCheckupInteractive | SlashCommandOutcome::RunCheckup { .. }),
            "{command} must run the setup checkup"
        );
    }

    let mut renderer = renderer_for_tests();
    let outcome = handle_slash_command("doctor prompt-audit", &mut renderer, workspace.path())
        .await
        .expect("prompt audit should parse");
    assert!(matches!(outcome, SlashCommandOutcome::AuditInstructions));
}

#[tokio::test]
async fn local_no_args_returns_interactive() {
    let mut renderer = renderer_for_tests();
    let workspace = std::path::PathBuf::from("/tmp");
    let outcome = handle_slash_command("local", &mut renderer, &workspace).await.unwrap();
    assert!(matches!(outcome, SlashCommandOutcome::ManageLocalServer { action: LocalServerAction::Interactive }));
}
#[tokio::test]
async fn local_status_all() {
    let mut renderer = renderer_for_tests();
    let workspace = std::path::PathBuf::from("/tmp");
    let outcome = handle_slash_command("local status", &mut renderer, &workspace).await.unwrap();
    assert!(matches!(
        outcome,
        SlashCommandOutcome::ManageLocalServer {
            action: LocalServerAction::Status { provider: None }
        }
    ));
}
#[tokio::test]
async fn local_status_provider() {
    let mut renderer = renderer_for_tests();
    let workspace = std::path::PathBuf::from("/tmp");
    let outcome = handle_slash_command("local status ollama", &mut renderer, &workspace)
        .await
        .unwrap();
    assert!(matches!(
        outcome,
        SlashCommandOutcome::ManageLocalServer {
            action: LocalServerAction::Status {
                provider: Some(ref p)
            }
        } if p == "ollama"
    ));
}
#[tokio::test]
async fn local_start_provider() {
    let mut renderer = renderer_for_tests();
    let workspace = std::path::PathBuf::from("/tmp");
    let outcome = handle_slash_command("local start lm-studio", &mut renderer, &workspace)
        .await
        .unwrap();
    assert!(matches!(
        outcome,
        SlashCommandOutcome::ManageLocalServer {
            action: LocalServerAction::Start {
                provider: Some(ref p)
            }
        } if p == "lmstudio"
    ));
}
#[tokio::test]
async fn local_stop_llamacpp() {
    let mut renderer = renderer_for_tests();
    let workspace = std::path::PathBuf::from("/tmp");
    let outcome = handle_slash_command("local stop llama.cpp", &mut renderer, &workspace)
        .await
        .unwrap();
    assert!(matches!(
        outcome,
        SlashCommandOutcome::ManageLocalServer {
            action: LocalServerAction::Stop {
                provider: Some(ref p)
            }
        } if p == "llamacpp"
    ));
}
#[tokio::test]
async fn local_troubleshoot_no_provider() {
    let mut renderer = renderer_for_tests();
    let workspace = std::path::PathBuf::from("/tmp");
    let outcome = handle_slash_command("local troubleshoot", &mut renderer, &workspace)
        .await
        .unwrap();
    assert!(matches!(
        outcome,
        SlashCommandOutcome::ManageLocalServer {
            action: LocalServerAction::Troubleshoot { provider: None }
        }
    ));
}
#[tokio::test]
async fn local_configure_provider() {
    let mut renderer = renderer_for_tests();
    let workspace = std::path::PathBuf::from("/tmp");
    let outcome = handle_slash_command("local configure ollama", &mut renderer, &workspace)
        .await
        .unwrap();
    assert!(matches!(
        outcome,
        SlashCommandOutcome::ManageLocalServer {
            action: LocalServerAction::Configure {
                provider: Some(ref p)
            }
        } if p == "ollama"
    ));
}
#[tokio::test]
async fn local_provider_name_only() {
    let mut renderer = renderer_for_tests();
    let workspace = std::path::PathBuf::from("/tmp");
    let outcome = handle_slash_command("local ollama", &mut renderer, &workspace).await.unwrap();
    assert!(matches!(
        outcome,
        SlashCommandOutcome::ManageLocalServer {
            action: LocalServerAction::Provider {
                name: ref n
            }
        } if n == "ollama"
    ));
}
#[tokio::test]
async fn local_provider_then_action() {
    let mut renderer = renderer_for_tests();
    let workspace = std::path::PathBuf::from("/tmp");
    let outcome = handle_slash_command("local ollama status", &mut renderer, &workspace)
        .await
        .unwrap();
    assert!(matches!(
        outcome,
        SlashCommandOutcome::ManageLocalServer {
            action: LocalServerAction::Status {
                provider: Some(ref p)
            }
        } if p == "ollama"
    ));
}
#[tokio::test]
async fn local_help_renders_usage() {
    let mut renderer = renderer_for_tests();
    let workspace = std::path::PathBuf::from("/tmp");
    let outcome = handle_slash_command("local help", &mut renderer, &workspace).await.unwrap();
    assert!(matches!(outcome, SlashCommandOutcome::Handled));
}
#[tokio::test]
async fn desktop_opens_the_desktop_app_without_arguments() {
    let workspace = tempfile::TempDir::new().expect("workspace");
    let mut renderer = renderer_for_tests();
    let outcome = handle_slash_command("desktop", &mut renderer, workspace.path())
        .await
        .expect("desktop should parse");
    assert!(matches!(outcome, SlashCommandOutcome::OpenDesktop));
}
#[tokio::test]
async fn desktop_with_arguments_renders_usage() {
    let workspace = tempfile::TempDir::new().expect("workspace");
    let mut renderer = renderer_for_tests();
    let outcome = handle_slash_command("desktop extra", &mut renderer, workspace.path())
        .await
        .expect("desktop should parse");
    assert!(matches!(outcome, SlashCommandOutcome::Handled));
}
