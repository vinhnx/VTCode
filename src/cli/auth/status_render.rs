use super::*;

pub(super) fn render_openrouter_auth_status(status: AuthStatus, storage_mode: AuthCredentialsStoreMode) {
    match status {
        AuthStatus::Authenticated { label, age_seconds, expires_in } => {
            println!("OpenRouter: authenticated (OAuth)");
            if let Some(label) = label {
                println!("  Label: {label}");
            }
            println!("  Token obtained: {}", format_auth_duration(age_seconds));
            if let Some(expires_in) = expires_in {
                println!("  Expires in: {}", format_auth_duration(expires_in));
            }
        }
        AuthStatus::NotAuthenticated => {
            if vtcode_core::config::api_keys::get_api_key_with_mode(
                OPENROUTER_PROVIDER,
                &ApiKeySources::default(),
                storage_mode,
            )
            .is_ok()
            {
                println!("OpenRouter: using configured API key");
            } else {
                println!("OpenRouter: not authenticated");
            }
        }
    }
}

pub(super) fn render_openai_auth_status(
    status: OpenAIChatGptAuthStatus,
    _storage_mode: AuthCredentialsStoreMode,
    overview: &vtcode_config::auth::OpenAICredentialOverview,
) {
    use vtcode_config::auth::OpenAIResolvedAuthSource;
    // Drive the headline from overview.active_source — the same field runtime
    // resolution uses — so CLI/TUI status never disagrees with actual auth.
    match overview.active_source {
        Some(OpenAIResolvedAuthSource::ChatGpt) => match status {
            OpenAIChatGptAuthStatus::Authenticated { label, age_seconds, expires_in } => {
                println!("OpenAI: authenticated (ChatGPT)");
                if let Some(label) = label {
                    println!("  Label: {label}");
                }
                println!("  Session obtained: {}", format_auth_duration(age_seconds));
                if let Some(expires_in) = expires_in {
                    println!("  Expires in: {}", format_auth_duration(expires_in));
                }
            }
            OpenAIChatGptAuthStatus::NotAuthenticated => {
                // active_source says ChatGpt but native status is NotAuthenticated
                // → this is a Codex fallback session.
                println!("OpenAI: using Codex auth.json fallback");
                if let Some(email) = overview.chatgpt_email.as_deref() {
                    println!("  Account: {email}");
                }
                if let Some(plan) = overview.chatgpt_plan.as_deref() {
                    println!("  Plan: {plan}");
                }
                println!("  Run `vtcode login openai` for a VT Code session with full auto-refresh support.");
            }
        },
        Some(OpenAIResolvedAuthSource::ApiKey) => {
            println!("OpenAI: using configured API key");
        }
        None => {
            println!("OpenAI: not authenticated");
            println!("  Run `vtcode login openai` to authenticate with your ChatGPT subscription.");
        }
    }
    // Show dual-credential notice when both sources are available.
    if let Some(notice) = &overview.notice {
        println!("  Note: {notice}");
    }
    if let Some(rec) = &overview.recommendation {
        println!("  {rec}");
    }
}

pub(super) fn render_copilot_auth_status(status: CopilotAuthStatus) {
    match status.kind {
        CopilotAuthStatusKind::Authenticated => {
            println!("GitHub Copilot: authenticated (managed auth via Copilot CLI)")
        }
        CopilotAuthStatusKind::Unauthenticated => println!("GitHub Copilot: not authenticated"),
        CopilotAuthStatusKind::ServerUnavailable => println!("GitHub Copilot: CLI unavailable"),
        CopilotAuthStatusKind::AuthFlowFailed => println!("GitHub Copilot: auth flow failed"),
    }

    if let Some(message) = status.message.as_deref()
        && !message.trim().is_empty()
    {
        println!("  Details: {message}");
    }

    if matches!(status.kind, CopilotAuthStatusKind::ServerUnavailable) {
        println!("  Help: install `copilot` or configure `[auth.copilot].command`; see {COPILOT_AUTH_DOC_PATH}");
    }
}

pub(super) fn print_copilot_auth_event(event: CopilotAuthEvent) -> Result<()> {
    match event {
        CopilotAuthEvent::VerificationCode { url, user_code } => {
            println!("GitHub device code (copy this): {user_code}");
            vtcode_commons::ansi_codes::notify_attention(true, Some(&format!("GitHub device code: {user_code}")));
            println!("Open this URL to continue:");
            println!("{url}");
        }
        CopilotAuthEvent::Progress { message } => println!("{message}"),
        CopilotAuthEvent::Success { account } => {
            if let Some(account) = account {
                println!("GitHub Copilot account: {account}");
            }
        }
        CopilotAuthEvent::Failure { message } => {
            tracing::error!("GitHub Copilot auth failed: {message}")
        }
    }

    Ok(())
}

fn format_auth_duration(seconds: u64) -> String {
    if seconds < 60 {
        format!("{seconds}s")
    } else if seconds < 3600 {
        format!("{}m {}s", seconds / 60, seconds % 60)
    } else if seconds < 86_400 {
        format!("{}h {}m", seconds / 3600, (seconds % 3600) / 60)
    } else {
        format!("{}d {}h", seconds / 86_400, (seconds % 86_400) / 3600)
    }
}

pub(super) fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
