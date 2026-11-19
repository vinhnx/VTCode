use super::*;

#[tokio::test]
async fn secret_no_args_is_interactive() {
    let mut renderer = renderer_for_tests();
    let workspace = std::path::PathBuf::from("/tmp");
    let outcome = handle_slash_command("secret", &mut renderer, &workspace).await.unwrap();
    assert!(matches!(outcome, SlashCommandOutcome::ManageSecrets { action: SecretCommandAction::Interactive }));
}
#[tokio::test]
async fn secret_list() {
    let mut renderer = renderer_for_tests();
    let workspace = std::path::PathBuf::from("/tmp");
    let outcome = handle_slash_command("secret list", &mut renderer, &workspace).await.unwrap();
    assert!(matches!(outcome, SlashCommandOutcome::ManageSecrets { action: SecretCommandAction::List }));
}
#[tokio::test]
async fn secret_status_no_provider() {
    let mut renderer = renderer_for_tests();
    let workspace = std::path::PathBuf::from("/tmp");
    let outcome = handle_slash_command("secret status", &mut renderer, &workspace).await.unwrap();
    assert!(matches!(
        outcome,
        SlashCommandOutcome::ManageSecrets {
            action: SecretCommandAction::Status { provider: None, .. }
        }
    ));
}
#[tokio::test]
async fn secret_status_with_provider() {
    let mut renderer = renderer_for_tests();
    let workspace = std::path::PathBuf::from("/tmp");
    let outcome = handle_slash_command("secret status anthropic MIMO_TOKEN_PLAN_KEY", &mut renderer, &workspace)
        .await
        .unwrap();
    assert!(matches!(
        outcome,
        SlashCommandOutcome::ManageSecrets {
            action: SecretCommandAction::Status {
                provider: Some(ref p),
                key_name: Some(ref key_name),
            }
        } if p == "anthropic" && key_name == "MIMO_TOKEN_PLAN_KEY"
    ));
}
#[tokio::test]
async fn secret_add_requires_provider() {
    let mut renderer = renderer_for_tests();
    let workspace = std::path::PathBuf::from("/tmp");
    let outcome = handle_slash_command("secret add", &mut renderer, &workspace).await.unwrap();
    assert!(matches!(outcome, SlashCommandOutcome::Handled));
}
#[tokio::test]
async fn secret_add_provider() {
    let mut renderer = renderer_for_tests();
    let workspace = std::path::PathBuf::from("/tmp");
    let outcome = handle_slash_command("secret add mycorp --key-name MYCORP_BILLING_KEY", &mut renderer, &workspace)
        .await
        .unwrap();
    assert!(matches!(
        outcome,
        SlashCommandOutcome::ManageSecrets {
            action: SecretCommandAction::Add {
                ref provider,
                key_name: Some(ref key_name),
            }
        } if provider == "mycorp" && key_name == "MYCORP_BILLING_KEY"
    ));
}
#[tokio::test]
async fn secret_add_provider_accepts_equals_key_name() {
    let mut renderer = renderer_for_tests();
    let workspace = std::path::PathBuf::from("/tmp");
    let outcome = handle_slash_command("secret add mycorp --env=MYCORP_BILLING_KEY", &mut renderer, &workspace)
        .await
        .unwrap();
    assert!(matches!(
        outcome,
        SlashCommandOutcome::ManageSecrets {
            action: SecretCommandAction::Add {
                ref provider,
                key_name: Some(ref key_name),
            }
        } if provider == "mycorp" && key_name == "MYCORP_BILLING_KEY"
    ));
}
#[tokio::test]
async fn secret_delete_requires_provider() {
    let mut renderer = renderer_for_tests();
    let workspace = std::path::PathBuf::from("/tmp");
    let outcome = handle_slash_command("secret delete", &mut renderer, &workspace).await.unwrap();
    assert!(matches!(outcome, SlashCommandOutcome::Handled));
}
#[tokio::test]
async fn secret_delete_provider() {
    let mut renderer = renderer_for_tests();
    let workspace = std::path::PathBuf::from("/tmp");
    let outcome = handle_slash_command("secret delete openai OPENAI_BILLING_KEY", &mut renderer, &workspace)
        .await
        .unwrap();
    assert!(matches!(
        outcome,
        SlashCommandOutcome::ManageSecrets {
            action: SecretCommandAction::Delete {
                ref provider,
                key_name: Some(ref key_name),
            }
        } if provider == "openai" && key_name == "OPENAI_BILLING_KEY"
    ));
}
#[tokio::test]
async fn secret_unknown_provider_returns_manage_secrets() {
    let mut renderer = renderer_for_tests();
    let workspace = std::path::PathBuf::from("/tmp");
    let outcome = handle_slash_command("secret add notaprovider", &mut renderer, &workspace)
        .await
        .unwrap();
    assert!(matches!(
        outcome,
        SlashCommandOutcome::ManageSecrets {
            action: SecretCommandAction::Add { ref provider, .. }
        } if provider == "notaprovider"
    ));
}
#[tokio::test]
async fn secret_help_renders_usage() {
    let mut renderer = renderer_for_tests();
    let workspace = std::path::PathBuf::from("/tmp");
    let outcome = handle_slash_command("secret help", &mut renderer, &workspace).await.unwrap();
    assert!(matches!(outcome, SlashCommandOutcome::Handled));
}
#[tokio::test]
async fn secret_migrate_no_provider() {
    let mut renderer = renderer_for_tests();
    let workspace = std::path::PathBuf::from("/tmp");
    let outcome = handle_slash_command("secret migrate", &mut renderer, &workspace).await.unwrap();
    assert!(matches!(
        outcome,
        SlashCommandOutcome::ManageSecrets {
            action: SecretCommandAction::Migrate { provider: None }
        }
    ));
}
#[tokio::test]
async fn secret_migrate_with_provider() {
    let mut renderer = renderer_for_tests();
    let workspace = std::path::PathBuf::from("/tmp");
    let outcome = handle_slash_command("secret migrate openai", &mut renderer, &workspace)
        .await
        .unwrap();
    assert!(matches!(
        outcome,
        SlashCommandOutcome::ManageSecrets {
            action: SecretCommandAction::Migrate { provider: Some(ref p) }
        } if p == "openai"
    ));
}
#[tokio::test]
async fn vim_command_toggles_and_sets_mode() {
    let workspace = std::path::PathBuf::from("/tmp");

    let mut renderer = renderer_for_tests();
    let outcome = handle_slash_command("vim", &mut renderer, &workspace).await.unwrap();
    assert!(matches!(outcome, SlashCommandOutcome::ToggleVimMode { enable: None }));

    let mut renderer = renderer_for_tests();
    let outcome = handle_slash_command("vim on", &mut renderer, &workspace).await.unwrap();
    assert!(matches!(outcome, SlashCommandOutcome::ToggleVimMode { enable: Some(true) }));

    let mut renderer = renderer_for_tests();
    let outcome = handle_slash_command("vim off", &mut renderer, &workspace).await.unwrap();
    assert!(matches!(outcome, SlashCommandOutcome::ToggleVimMode { enable: Some(false) }));

    let mut renderer = renderer_for_tests();
    let outcome = handle_slash_command("vim bogus", &mut renderer, &workspace).await.unwrap();
    assert!(matches!(outcome, SlashCommandOutcome::Handled));
}
#[tokio::test]
async fn fast_command_catalog_and_parser() {
    let spec = command_skill_specs()
        .iter()
        .find(|spec| spec.slash_name == "fast")
        .expect("fast catalog entry");
    assert_eq!(spec.usage, "/fast [on|off]");
    let workspace = tempfile::tempdir().unwrap();
    let mut renderer = renderer_for_tests();
    for (input, expected) in [
        ("fast", None),
        ("  FAST  ", None),
        ("fast on", Some(true)),
        ("fast OFF", Some(false)),
        ("fast \t On \n", Some(true)),
    ] {
        let outcome = handle_slash_command(input, &mut renderer, workspace.path()).await.unwrap();
        assert!(matches!(outcome, SlashCommandOutcome::ToggleFastMode { enable } if enable == expected), "{input}");
    }
    for input in [
        "fast true",
        "fast toggle",
        "fast on off",
        "fast on extra",
        "fast --help",
    ] {
        let outcome = handle_slash_command(input, &mut renderer, workspace.path()).await.unwrap();
        assert!(matches!(outcome, SlashCommandOutcome::Handled), "{input}");
    }
    let outcome = super::super::dispatch::execute_command_skill_by_name("fast", "on", &mut renderer, workspace.path())
        .await
        .unwrap();
    assert!(matches!(outcome, SlashCommandOutcome::ToggleFastMode { enable: Some(true) }));
}

#[tokio::test]
async fn computer_use_command_catalog_and_parser() {
    let spec = command_skill_specs()
        .iter()
        .find(|spec| spec.slash_name == "computer-use")
        .expect("computer-use catalog entry");
    assert_eq!(spec.usage, "/computer-use [on|off|status|target <path>|run <task>]");
    let workspace = tempfile::tempdir().unwrap();
    let mut renderer = renderer_for_tests();
    for (input, expected) in [
        ("computer-use", ComputerUseAction::Toggle),
        ("  COMPUTER-USE  ", ComputerUseAction::Toggle),
        ("computer-use on", ComputerUseAction::On),
        ("computer-use OFF", ComputerUseAction::Off),
        ("computer-use \t Status \n", ComputerUseAction::Status),
        ("cua", ComputerUseAction::Toggle),
        ("cua on", ComputerUseAction::On),
    ] {
        let outcome = handle_slash_command(input, &mut renderer, workspace.path()).await.unwrap();
        assert!(matches!(outcome, SlashCommandOutcome::ComputerUse { action } if action == expected), "{input}");
    }
    let outcome = handle_slash_command("computer-use target /tmp/build", &mut renderer, workspace.path())
        .await
        .unwrap();
    assert!(matches!(
        outcome,
        SlashCommandOutcome::ComputerUse { action: ComputerUseAction::SetTarget { path } }
        if path == "/tmp/build"
    ));
    for (input, task) in [
        ("computer-use run take a screenshot", "take a screenshot"),
        ("computer-use RUN  open settings  and  click  save ", "open settings  and  click  save"),
        ("cua run type hello", "type hello"),
    ] {
        let outcome = handle_slash_command(input, &mut renderer, workspace.path()).await.unwrap();
        assert!(
            matches!(outcome, SlashCommandOutcome::ComputerUse { action: ComputerUseAction::Run { task: actual } } if actual == task),
            "{input}"
        );
    }
    for input in [
        "computer-use true",
        "computer-use toggle",
        "computer-use on off",
        "computer-use target",
        "computer-use target /a /b",
        "computer-use run",
        "computer-use run   ",
        "computer-use --help",
    ] {
        let outcome = handle_slash_command(input, &mut renderer, workspace.path()).await.unwrap();
        assert!(matches!(outcome, SlashCommandOutcome::Handled), "{input}");
    }
}
