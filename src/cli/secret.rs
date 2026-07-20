use anyhow::{Context, Result};
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::terminal::enable_raw_mode;
use std::io::{self, IsTerminal, Write};
use std::path::Path;
use unicode_width::UnicodeWidthChar;
use vtcode_config::api_keys::{
    CredentialSource, DiscoveredProvider, has_oauth_or_managed_auth, provider_credential_detail,
};
use vtcode_config::auth::{AuthCredentialsStoreMode, CustomApiKeyStorage};
use vtcode_config::workspace_env::{
    MigrationOutcome, migrate_single_env_key, read_workspace_env_values, workspace_env_path,
};
use vtcode_core::cli::args::{MigrateArgs, SecretProvider, SecretSubcommand};
use vtcode_core::config::models::Provider;

pub async fn handle_secret_command(command: SecretSubcommand, workspace: &Path) -> Result<()> {
    match command {
        SecretSubcommand::List => render_secret_status_table(None),
        SecretSubcommand::Status { provider_name } => {
            render_secret_status_table(provider_name.map(secret_provider_to_provider))
        }
        SecretSubcommand::Add { provider_name } => handle_add(secret_provider_to_provider(provider_name)).await,
        SecretSubcommand::Delete { provider_name } => handle_delete(secret_provider_to_provider(provider_name)).await,
        SecretSubcommand::Migrate(args) => handle_migrate(args, workspace).await,
    }
}

fn render_secret_status_table(filter: Option<Provider>) -> Result<()> {
    println!("API Key Status");
    println!();

    let providers: Vec<Provider> = match filter {
        Some(p) => vec![p],
        None => Provider::all_providers(),
    };

    let details: Vec<DiscoveredProvider> = providers.iter().filter_map(|&p| provider_credential_detail(p)).collect();

    for detail in &details {
        let source = detail.source;
        let source_label = match source {
            CredentialSource::Env => "Environment variable",
            CredentialSource::SecureStorage => "OS keyring / encrypted file",
            CredentialSource::OAuth => "OAuth session",
            CredentialSource::ManagedAuth => "Managed auth (external CLI)",
            CredentialSource::Local => "Local — no key required",
        };
        let status = match source {
            CredentialSource::Local | CredentialSource::ManagedAuth => "Ready",
            CredentialSource::OAuth | CredentialSource::SecureStorage | CredentialSource::Env => "Ready",
        };

        println!("  {} ({})", detail.provider.label(), detail.provider.as_ref());
        println!("    Status: {}", status);
        println!("    Source: {}", source_label);

        if let Some(env_key) = detail.env_var {
            println!("    Env var: {}", env_key);
        }

        println!();
    }

    let has_oauth_or_managed = has_oauth_or_managed_auth(&details);

    println!("Use `vtcode secret add <provider>` to store a key.");
    if !has_oauth_or_managed {
        println!("Use `vtcode secret delete <provider>` to remove a stored key.");
    }
    if has_oauth_or_managed {
        println!("OAuth / managed-auth providers (copilot, openai, openrouter) use their own login flows.");
        println!("Run `vtcode login <provider>` or `/login <provider>` for those.");
    }
    Ok(())
}

async fn handle_add(provider: Provider) -> Result<()> {
    if provider.uses_managed_auth() {
        println!(
            "{} uses managed auth (GitHub Copilot CLI). Run `vtcode login {}` instead.",
            provider.label(),
            provider.as_ref()
        );
        return Ok(());
    }
    let label = provider.label();
    let env_key = provider.default_api_key_env();

    println!("Bring your own key (BYOK) for {label}.");
    println!("Expected env: {}", env_key);
    println!("Secure display hint: ****************");
    println!("Key will be stored in secure storage (OS keyring or encrypted file).");
    println!("Key will NOT be stored in vtcode.toml or workspace environment files.");
    println!();

    let key = if io::stdin().is_terminal() {
        prompt_hidden_input(&format!("{} API key: ", label))?
    } else {
        eprintln!("Warning: stdin is not a terminal — the pasted key will be visible.");
        print!("{} API key: ", label);
        io::stdout().flush()?;
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        input.trim().to_string()
    };

    if key.is_empty() {
        anyhow::bail!("API key cannot be empty.");
    }

    let storage = CustomApiKeyStorage::new(provider.as_ref());
    storage.store(&key, AuthCredentialsStoreMode::default())?;
    println!();
    println!("API key for {label} stored in secure storage.");
    println!("The key will be used automatically on next provider/model reload.");
    Ok(())
}

async fn handle_delete(provider: Provider) -> Result<()> {
    if provider.uses_managed_auth() {
        println!(
            "{} uses managed auth (GitHub Copilot CLI). Run `vtcode login {}` instead.",
            provider.label(),
            provider.as_ref()
        );
        return Ok(());
    }
    let label = provider.label();

    let storage = CustomApiKeyStorage::new(provider.as_ref());
    match storage.load(AuthCredentialsStoreMode::default()) {
        Ok(None) => {
            println!("No stored API key found for {label}.");
            return Ok(());
        }
        Ok(Some(_)) => {}
        Err(err) => {
            eprintln!("Warning: Could not inspect stored key for {label}: {err}");
        }
    }

    print!("Type 'confirm' to delete the stored API key for {label}, or press Enter to cancel: ");
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let trimmed = input.trim();

    if trimmed.ne("confirm") {
        println!("Deletion cancelled.");
        return Ok(());
    }

    storage.clear(AuthCredentialsStoreMode::default())?;
    println!();
    println!("API key for {label} deleted from secure storage.");
    println!("The change takes effect on next provider/model reload.");
    Ok(())
}

async fn handle_migrate(args: MigrateArgs, workspace: &Path) -> Result<()> {
    let env_path = workspace_env_path(workspace);
    if !env_path.exists() {
        println!("No .env file found at {}. Nothing to migrate.", env_path.display());
        return Ok(());
    }

    println!("Scanning {} for API keys to migrate...", env_path.display());
    println!();

    let providers: Vec<Provider> = if let Some(p) = args.provider_name {
        let provider = secret_provider_to_provider(p);
        if provider.uses_managed_auth() {
            println!(
                "{} uses managed auth (GitHub Copilot CLI). Run `vtcode login {}` instead.",
                provider.label(),
                provider.as_ref()
            );
            return Ok(());
        }
        vec![provider]
    } else {
        Provider::all_providers()
            .into_iter()
            .filter(|p| !p.is_local() && !p.uses_managed_auth())
            .collect()
    };

    if args.dry_run {
        println!("[dry-run] Would migrate the following keys from .env to secure storage:");
        println!();
        let env_keys: Vec<&str> = providers
            .iter()
            .filter_map(|p| {
                let key = p.default_api_key_env();
                if key.is_empty() { None } else { Some(key) }
            })
            .collect();
        let found = read_workspace_env_values(workspace, &env_keys)?;
        for provider in &providers {
            let env_key = provider.default_api_key_env();
            if env_key.is_empty() {
                continue;
            }
            if found.contains_key(env_key) {
                println!("  {} ({})", provider.label(), env_key);
            }
        }
        println!();
        println!("No changes were made.");
        return Ok(());
    }

    let mut migrated = 0u32;
    let mut skipped = 0u32;
    let mut failed = 0u32;

    let env_keys: Vec<&str> = providers
        .iter()
        .filter_map(|p| {
            let key = p.default_api_key_env();
            if key.is_empty() { None } else { Some(key) }
        })
        .collect();
    let env_values = read_workspace_env_values(workspace, &env_keys)?;

    for provider in providers {
        let env_key = provider.default_api_key_env();

        if !args.force && !args.all && io::stdin().is_terminal() {
            print!("Migrate {} from .env to secure storage? [Y/n] ", env_key);
            io::stdout().flush()?;
            let mut input = String::new();
            io::stdin().read_line(&mut input)?;
            let trimmed = input.trim().to_lowercase();
            if !trimmed.is_empty() && trimmed != "y" && trimmed != "yes" {
                println!("Skipped {}.", env_key);
                skipped += 1;
                continue;
            }
        }

        let value = env_values.get(env_key).map(|s| s.as_str());
        match migrate_single_env_key(workspace, provider, AuthCredentialsStoreMode::default(), value)? {
            MigrationOutcome::Migrated => {
                println!("Migrated {} to secure storage.", env_key);
                migrated += 1;
            }
            MigrationOutcome::Skipped => {
                skipped += 1;
            }
            MigrationOutcome::Failed => {
                eprintln!("Failed to migrate {}.", env_key);
                failed += 1;
            }
        }
    }

    println!();
    println!("Migration complete: {} migrated, {} skipped, {} failed", migrated, skipped, failed);
    if failed > 0 {
        anyhow::bail!("Some migrations failed. Review the errors above.");
    }
    Ok(())
}

#[allow(clippy::let_unit_value)]
fn prompt_hidden_input(prompt: &str) -> Result<String> {
    if !io::stdin().is_terminal() {
        anyhow::bail!("Cannot prompt for hidden input: stdin is not a terminal");
    }

    let _raw = enable_raw_mode().with_context(|| "Failed to enable raw mode for secret input")?;

    {
        let mut stdout = io::stdout();
        write!(stdout, "{}", prompt)?;
        stdout.flush()?;
    }

    let mut buffer = String::new();
    loop {
        let event = event::read().with_context(|| "Failed to read keypress while entering API key")?;
        match handle_key(event, &mut buffer)? {
            KeyAction::Continue => continue,
            KeyAction::Submit => {
                let mut stdout = io::stdout();
                writeln!(stdout)?;
                stdout.flush()?;
                let trimmed = buffer.trim().to_string();
                return Ok(trimmed);
            }
            KeyAction::Abort => {
                println!();
                anyhow::bail!("Secret entry cancelled.");
            }
        }
    }
}

enum KeyAction {
    Continue,
    Submit,
    Abort,
}

fn handle_key(event: Event, buffer: &mut String) -> Result<KeyAction> {
    let Event::Key(key) = event else {
        return Ok(KeyAction::Continue);
    };
    if key.kind != KeyEventKind::Press {
        return Ok(KeyAction::Continue);
    }
    let mut stdout = io::stdout();
    match key.code {
        KeyCode::Enter => Ok(KeyAction::Submit),
        KeyCode::Char('c') if key.modifiers.contains(event::KeyModifiers::CONTROL) => Ok(KeyAction::Abort),
        KeyCode::Char('d') if key.modifiers.contains(event::KeyModifiers::CONTROL) => Ok(KeyAction::Submit),
        KeyCode::Char('u') if key.modifiers.contains(event::KeyModifiers::CONTROL) => {
            let width: usize = buffer.chars().map(|c| UnicodeWidthChar::width(c).unwrap_or(0).max(1)).sum();
            for _ in 0..width {
                write!(stdout, "\u{8}")?;
            }
            for _ in 0..width {
                write!(stdout, " ")?;
            }
            for _ in 0..width {
                write!(stdout, "\u{8}")?;
            }
            stdout.flush()?;
            buffer.clear();
            Ok(KeyAction::Continue)
        }
        KeyCode::Backspace => {
            if let Some(c) = buffer.pop() {
                let width = UnicodeWidthChar::width(c).unwrap_or(0).max(1);
                for _ in 0..width {
                    write!(stdout, "\u{8}")?;
                }
                for _ in 0..width {
                    write!(stdout, " ")?;
                }
                for _ in 0..width {
                    write!(stdout, "\u{8}")?;
                }
                stdout.flush()?;
            }
            Ok(KeyAction::Continue)
        }
        KeyCode::Char(c) if !c.is_control() => {
            buffer.push(c);
            write!(stdout, "*")?;
            stdout.flush()?;
            Ok(KeyAction::Continue)
        }
        _ => Ok(KeyAction::Continue),
    }
}

fn secret_provider_to_provider(p: SecretProvider) -> Provider {
    match p {
        SecretProvider::OpenAI => Provider::OpenAI,
        SecretProvider::Anthropic => Provider::Anthropic,
        SecretProvider::Gemini => Provider::Gemini,
        SecretProvider::DeepSeek => Provider::DeepSeek,
        SecretProvider::OpenRouter => Provider::OpenRouter,
        SecretProvider::StepFun => Provider::StepFun,
        SecretProvider::Zai => Provider::ZAI,
        SecretProvider::Moonshot => Provider::Moonshot,
        SecretProvider::MiniMax => Provider::Minimax,
        SecretProvider::Mistral => Provider::Mistral,
        SecretProvider::HuggingFace => Provider::HuggingFace,
        SecretProvider::MiMo => Provider::MiMo,
        SecretProvider::OpenCodeZen => Provider::OpenCodeZen,
        SecretProvider::OpenCodeGo => Provider::OpenCodeGo,
        SecretProvider::Qwen => Provider::Qwen,
        SecretProvider::Evolink => Provider::Evolink,
        SecretProvider::Poolside => Provider::Poolside,
        SecretProvider::Ollama => Provider::Ollama,
        SecretProvider::LMStudio => Provider::LmStudio,
        SecretProvider::Copilot => Provider::Copilot,
    }
}
