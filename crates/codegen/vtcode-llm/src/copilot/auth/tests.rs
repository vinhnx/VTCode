use super::{
    CapturedCommandState, CapturedLine, CapturedStream, CommandKind, CopilotAuthSource, CopilotCliConfig,
    CopilotCliUser, CopilotHost, copilot_token_login_for_host, drain_complete_lines, env_auth_source_with,
    extract_account_from_status_message, login_command_args, logout_command_args,
    missing_copilot_command_help_lines_with, normalize_captured_line, parse_device_flow_code,
};

#[test]
fn env_auth_source_respects_documented_precedence() {
    let source = env_auth_source_with(|name| match name {
        "COPILOT_GITHUB_TOKEN" => None,
        "GH_TOKEN" => Some("ghp_example".to_string()),
        "GITHUB_TOKEN" => Some("github_example".to_string()),
        _ => None,
    });

    assert!(matches!(source, Some(CopilotAuthSource::Environment("GH_TOKEN"))));
}

#[test]
fn host_parser_accepts_bare_hostname() {
    let host = CopilotHost::parse("github.com").unwrap();

    assert_eq!(host.url, "https://github.com");
    assert_eq!(host.gh_hostname, "github.com");
}

#[test]
fn stored_credentials_match_host() {
    let host = CopilotHost::parse("https://github.com").unwrap();
    let config = CopilotCliConfig {
        logged_in_users: vec![CopilotCliUser {
            host: Some("https://github.com".to_string()),
            login: Some("vinhnx".to_string()),
        }],
        ..CopilotCliConfig::default()
    };

    let matched = config
        .logged_in_users
        .iter()
        .find(|user| user.host_matches(&host))
        .and_then(|user| user.login.as_deref());

    assert_eq!(matched, Some("vinhnx"));
}

#[test]
fn stored_plaintext_token_keys_match_host_and_extract_login() {
    let host = CopilotHost::parse("https://example.ghe.com:8443").unwrap();

    let login = copilot_token_login_for_host(&host, "https://example.ghe.com:8443:vinhnx");

    assert_eq!(login.as_deref(), Some("vinhnx"));
}

#[test]
fn auth_source_message_does_not_include_token_value() {
    let host = CopilotHost::parse("https://github.com").unwrap();
    let message = CopilotAuthSource::Environment("GH_TOKEN").message(&host);

    assert!(!message.contains("ghp_"));
    assert_eq!(message, "Using GH_TOKEN for GitHub Copilot authentication.");
}

#[test]
fn device_flow_code_parser_extracts_url_and_code() {
    let parsed =
        parse_device_flow_code("To authenticate, visit https://github.com/login/device and enter code D8E1-101D.");

    assert_eq!(parsed, Some(("https://github.com/login/device".to_string(), "D8E1-101D".to_string())));
}

#[test]
fn device_flow_code_parser_handles_ansi_styled_output() {
    let normalized = normalize_captured_line(
        "\u{1b}[1mTo authenticate, visit https://github.com/login/device and enter code D8E1-101D.\u{1b}[0m",
    );

    let parsed = parse_device_flow_code(&normalized);

    assert_eq!(parsed, Some(("https://github.com/login/device".to_string(), "D8E1-101D".to_string())));
}

#[test]
fn drain_complete_lines_splits_on_carriage_return_and_newline() {
    let mut buffer = b"To authenticate, visit https://github.com/login/device and enter code D8E1-101D.\rWaiting for authorization...\npartial".to_vec();

    let lines = drain_complete_lines(&mut buffer);

    assert_eq!(
        lines,
        vec![
            "To authenticate, visit https://github.com/login/device and enter code D8E1-101D.".to_string(),
            "Waiting for authorization...".to_string(),
        ]
    );
    assert_eq!(buffer, b"partial");
}

#[test]
fn login_args_include_host_for_non_default_host() {
    let host = CopilotHost::parse("https://example.ghe.com").unwrap();

    let args = login_command_args(&host);

    assert_eq!(args, vec!["login", "--host", "https://example.ghe.com"]);
}

#[test]
fn logout_args_do_not_include_host() {
    let args = logout_command_args();

    assert_eq!(args, vec!["logout"]);
}

#[test]
fn captured_failure_prefers_specific_error_over_help_hint() {
    let mut state = CapturedCommandState::default();

    state
        .handle_line(
            CommandKind::Logout,
            CapturedLine {
                stream: CapturedStream::Stderr,
                text: "ERROR: SecItemCopyMatching failed -50".to_string(),
            },
            &mut |_| Ok(()),
        )
        .unwrap();
    state
        .handle_line(
            CommandKind::Logout,
            CapturedLine {
                stream: CapturedStream::Stderr,
                text: "Try 'copilot --help' for more information.".to_string(),
            },
            &mut |_| Ok(()),
        )
        .unwrap();

    assert_eq!(
        state.last_safe_message.as_deref(),
        Some("GitHub Copilot CLI failed to access the macOS Keychain while clearing credentials.")
    );
}

#[test]
fn account_extraction_reads_stored_credential_message() {
    let login = extract_account_from_status_message("Using Copilot CLI stored credentials for vinhnx on github.com.");

    assert_eq!(login, Some("vinhnx"));
}

#[test]
fn missing_copilot_help_explains_required_cli_and_optional_gh() {
    let lines = missing_copilot_command_help_lines_with("copilot", false, false).expect("help");

    assert!(lines.iter().any(|line| line.contains("Install `copilot`")));
    assert!(lines.iter().any(|line| line.contains("`gh` is also not installed")));
    assert!(lines.iter().any(|line| line.contains("docs/providers/copilot.md")));
}

#[test]
fn missing_copilot_help_is_suppressed_when_command_exists() {
    let lines = missing_copilot_command_help_lines_with("copilot", true, true);

    assert!(lines.is_none());
}
