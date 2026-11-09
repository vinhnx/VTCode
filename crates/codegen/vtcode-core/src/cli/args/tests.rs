use super::*;
use clap::Parser;

#[test]
fn long_version_includes_expected_sections() {
    let text = long_version("0.181.3+test");
    assert!(text.contains("Authors:"));
    assert!(text.contains("Config directory:"));
    assert!(text.contains("Data directory:"));
    assert!(text.contains("State directory:"));
    assert!(text.contains("Cache directory:"));
    assert!(text.contains("Runtime directory:"));
    assert!(text.contains("Executable directory:"));
    assert!(text.contains("Legacy directory:"));
    assert!(text.contains("Migration marker:"));
    assert!(text.contains("Migration report:"));
    assert!(text.contains("VTCODE_CONFIG"));
    assert!(text.contains("VTCODE_DATA"));
}

#[test]
fn long_version_first_line_is_the_caller_supplied_stamp() {
    // Asymmetric fixtures: a stamped dev build, a dirty-tree stamp, and a
    // bare release version. The first line must be exactly the stamp in every
    // case, with no extra tokens — the desktop updater parses that line
    // strictly, and a previous `option_env!` read here silently produced the
    // bare fallback on every build (cargo:rustc-env never crosses packages).
    for stamp in ["0.181.3+f19a52f2f1a2b", "0.181.3+f19a52f2f1a2b.dirty", "0.181.3"] {
        let text = long_version(stamp);
        let first_line = text.lines().next().unwrap_or_default();
        assert_eq!(first_line, stamp);
    }
}

#[test]
fn config_file_api_key_env_uses_provider_default() {
    let cli = Cli::parse_from(["vtcode", "--provider", "minimax"]);

    assert_eq!(cli.get_api_key_env(), "MINIMAX_API_KEY");
}

#[test]
fn config_file_api_key_env_preserves_explicit_override() {
    let cli = Cli::parse_from(["vtcode", "--provider", "openai", "--api-key-env", "CUSTOM_OPENAI_KEY"]);

    assert_eq!(cli.get_api_key_env(), "CUSTOM_OPENAI_KEY");
}

#[test]
fn parses_app_server_command_with_stdio_listen_target() {
    let cli = Cli::parse_from(["vtcode", "app-server", "--listen", "stdio://"]);

    assert!(matches!(
        cli.command,
        Some(Commands::AppServer { ref listen }) if listen == "stdio://"
    ));
}

#[test]
fn parses_init_force_flag() {
    let cli = Cli::parse_from(["vtcode", "init", "--force"]);

    assert!(matches!(cli.command, Some(Commands::Init { force: true })));
}

#[test]
fn parses_codex_login_device_code_flag() {
    let cli = Cli::parse_from(["vtcode", "login", "codex", "--device-code"]);

    assert!(matches!(
        cli.command,
        Some(Commands::Login {
            ref provider,
            device_code: true,
            from_codex: false
        }) if provider == "codex"
    ));
}

#[test]
fn parses_codex_experimental_flags() {
    let enabled = Cli::parse_from(["vtcode", "--codex-experimental"]);
    assert_eq!(enabled.codex_experimental_override(), Some(true));

    let disabled = Cli::parse_from(["vtcode", "--no-codex-experimental"]);
    assert_eq!(disabled.codex_experimental_override(), Some(false));
}

#[test]
fn full_access_is_opt_in_and_conflicts_with_other_permission_flags() {
    assert!(!Cli::parse_from(["vtcode"]).full_access);
    assert!(Cli::parse_from(["vtcode", "--full-access"]).full_access);
    for flag in ["--full-auto", "--skip-confirmations", "--dangerously-skip-permissions"] {
        assert!(Cli::try_parse_from(["vtcode", "--full-access", flag]).is_err(), "{flag}");
    }
}

#[test]
fn codex_experimental_flags_conflict() {
    let result = Cli::try_parse_from(["vtcode", "--codex-experimental", "--no-codex-experimental"]);

    result.unwrap_err();
}

#[test]
fn parses_create_project_feature_flags() {
    let cli = Cli::parse_from([
        "vtcode",
        "create-project",
        "demo",
        "--feature",
        "web",
        "--feature",
        "db",
    ]);

    assert!(matches!(
        cli.command,
        Some(Commands::CreateProject { ref name, ref features })
            if name == "demo" && features == &vec!["web".to_string(), "db".to_string()]
    ));
}

#[test]
fn parses_revert_partial_long_flag() {
    let cli = Cli::parse_from(["vtcode", "revert", "--turn", "3", "--partial", "code"]);

    assert!(matches!(
        cli.command,
        Some(Commands::Revert {
            turn: 3,
            partial: Some(ref scope),
            session: None,
        }) if scope == "code"
    ));
}

#[test]
fn parses_revert_session_long_flag() {
    let cli = Cli::parse_from(["vtcode", "revert", "--turn", "3", "--session", "pane-b"]);

    assert!(matches!(
        cli.command,
        Some(Commands::Revert {
            turn: 3,
            partial: None,
            session: Some(ref session),
        }) if session == "pane-b"
    ));
}

#[test]
fn parses_config_reset_workspace_target_by_default() {
    let cli = Cli::parse_from(["vtcode", "config", "reset"]);

    assert!(matches!(
        cli.command,
        Some(Commands::Config {
            command: Some(ConfigCommand::Reset(ConfigResetArgs { global: false, project: false })),
            global: false,
            ..
        })
    ));
}

#[test]
fn parses_config_reset_global_and_project_targets() {
    let global = Cli::parse_from(["vtcode", "config", "reset", "--global"]);
    assert!(matches!(
        global.command,
        Some(Commands::Config {
            command: Some(ConfigCommand::Reset(ConfigResetArgs { global: true, project: false })),
            ..
        })
    ));

    let project = Cli::parse_from(["vtcode", "config", "reset", "--project"]);
    assert!(matches!(
        project.command,
        Some(Commands::Config {
            command: Some(ConfigCommand::Reset(ConfigResetArgs { global: false, project: true })),
            ..
        })
    ));
}

#[test]
fn parses_parent_global_flag_for_config_reset() {
    let cli = Cli::parse_from(["vtcode", "config", "--global", "reset"]);

    assert!(matches!(
        cli.command,
        Some(Commands::Config {
            command: Some(ConfigCommand::Reset(ConfigResetArgs { global: false, project: false })),
            global: true,
            ..
        })
    ));
}

#[test]
fn rejects_config_reset_with_conflicting_layer_flags() {
    let result = Cli::try_parse_from(["vtcode", "config", "reset", "--global", "--project"]);

    assert!(result.is_err());
}

#[test]
fn man_all_pages_requires_directory_and_rejects_single_command() {
    let cli = Cli::try_parse_from(["vtcode", "man", "--all-pages", "--output", "man/man1"])
        .expect("all pages with an output directory");
    assert!(!cli.all, "man-page generation must not enable session selection");
    assert!(matches!(cli.command, Some(Commands::Man { all_pages: true, command: None, output: Some(_) })));
    assert!(Cli::try_parse_from(["vtcode", "man", "--all-pages"]).is_err());
    assert!(Cli::try_parse_from(["vtcode", "man", "chat", "--all-pages", "--output", "man"]).is_err());
    assert!(matches!(
        Cli::try_parse_from(["vtcode", "man", "chat"]).expect("single page").command,
        Some(Commands::Man { all_pages: false, .. })
    ));
}
