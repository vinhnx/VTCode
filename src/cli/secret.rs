use anyhow::{Context, Result};
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use std::io::{self, IsTerminal, Write};
use std::path::Path;
use std::str::FromStr;
use unicode_width::UnicodeWidthChar;
use vtcode_config::VTCodeConfig;
use vtcode_config::api_keys::{
    CredentialSource, clear_credential_with_mode, load_stored_credential_with_mode, resolve_credential_with_mode,
    store_credential_with_mode,
};
use vtcode_config::auth::AuthCredentialsStoreMode;
use vtcode_config::workspace_env::{read_workspace_env_value, remove_workspace_env_value, workspace_env_path};
use vtcode_core::cli::args::{MigrateArgs, SecretSubcommand};
use vtcode_core::config::models::Provider;

#[derive(Clone)]
struct SecretTarget {
    provider_name: String,
    label: String,
    env_key: String,
    local: bool,
    managed_auth: bool,
}

pub async fn handle_secret_command(
    command: SecretSubcommand,
    config: &VTCodeConfig,
    workspace: &Path,
    storage_mode: AuthCredentialsStoreMode,
) -> Result<()> {
    match command {
        SecretSubcommand::List => render_secret_status_table(config, workspace, None, None, storage_mode),
        SecretSubcommand::Status { provider_name, key_name } => {
            render_secret_status_table(config, workspace, provider_name.as_deref(), key_name.as_deref(), storage_mode)
        }
        SecretSubcommand::Add { provider_name, key_name } => {
            let target = secret_target(config, &provider_name, key_name.as_deref())?;
            handle_add(target, workspace, storage_mode).await
        }
        SecretSubcommand::Delete { provider_name, key_name } => {
            let target = secret_target(config, &provider_name, key_name.as_deref())?;
            handle_delete(target, workspace, storage_mode).await
        }
        SecretSubcommand::Migrate(args) => handle_migrate(args, config, workspace, storage_mode).await,
    }
}

fn secret_target(config: &VTCodeConfig, name: &str, key_name: Option<&str>) -> Result<SecretTarget> {
    let provider_name = name.trim().to_ascii_lowercase();
    if provider_name.is_empty() {
        anyhow::bail!("Provider name cannot be empty.");
    }

    if let Ok(provider) = Provider::from_str(&provider_name) {
        let env_key = key_name
            .filter(|key| !key.trim().is_empty())
            .map(ToOwned::to_owned)
            .or_else(|| config.configured_api_key_env(&provider_name))
            .unwrap_or_else(|| provider.default_api_key_env().to_string());
        return Ok(SecretTarget {
            provider_name,
            label: provider.label().to_string(),
            env_key,
            local: provider.is_local(),
            managed_auth: provider.uses_managed_auth(),
        });
    }

    let Some(custom) = config.custom_provider(&provider_name) else {
        let configured = config
            .custom_providers
            .iter()
            .map(|provider| provider.name.as_str())
            .collect::<Vec<_>>();
        let suffix = if configured.is_empty() {
            String::new()
        } else {
            format!(" Configured custom providers: {}.", configured.join(", "))
        };
        anyhow::bail!("Unknown provider: {name}.{suffix}");
    };

    let env_key = if custom.uses_command_auth() {
        String::new()
    } else {
        key_name
            .filter(|key| !key.trim().is_empty())
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| custom.resolved_api_key_env())
    };
    Ok(SecretTarget {
        provider_name,
        label: custom.display_name.clone(),
        env_key,
        local: false,
        managed_auth: custom.uses_command_auth(),
    })
}

fn all_secret_targets(config: &VTCodeConfig) -> Vec<SecretTarget> {
    let mut targets = Provider::all_providers()
        .into_iter()
        .filter_map(|provider| secret_target(config, provider.as_ref(), None).ok())
        .collect::<Vec<_>>();
    targets.extend(
        config
            .custom_providers
            .iter()
            .filter_map(|provider| secret_target(config, &provider.name, None).ok()),
    );
    targets
}

fn render_secret_status_table(
    config: &VTCodeConfig,
    workspace: &Path,
    filter: Option<&str>,
    key_name: Option<&str>,
    storage_mode: AuthCredentialsStoreMode,
) -> Result<()> {
    println!("API Key Status");
    println!();

    let targets = match filter {
        Some(name) => vec![secret_target(config, name, key_name)?],
        None => all_secret_targets(config),
    };

    let mut has_oauth_or_managed = false;
    for target in &targets {
        let source = if target.local {
            Some(CredentialSource::Local)
        } else if target.managed_auth {
            Some(CredentialSource::ManagedAuth)
        } else {
            resolve_credential_with_mode(&target.provider_name, &target.env_key, Some(workspace), storage_mode)?
                .map(|resolved| resolved.source)
        };
        let source_label = match source {
            Some(CredentialSource::Env) => "Environment variable",
            Some(CredentialSource::Workspace) => "Workspace .env",
            Some(CredentialSource::SecureStorage) => "OS keyring / encrypted file",
            Some(CredentialSource::OAuth) => "OAuth session",
            Some(CredentialSource::ManagedAuth) => "Managed auth (external CLI)",
            Some(CredentialSource::Local) => "Local — no key required",
            None => "Not configured",
        };
        let status = if source.is_some() { "Ready" } else { "Missing" };
        if matches!(source, Some(CredentialSource::OAuth | CredentialSource::ManagedAuth)) {
            has_oauth_or_managed = true;
        };

        println!("  {} ({})", target.label, target.provider_name);
        println!("    Status: {}", status);
        println!("    Source: {}", source_label);

        if !target.env_key.is_empty() {
            println!("    Env var: {}", target.env_key);
        }

        println!();
    }

    println!("Use `vtcode secret add <provider> [--key-name NAME]` to store a key.");
    if !has_oauth_or_managed {
        println!("Use `vtcode secret delete <provider>` to remove a stored key.");
    }
    if has_oauth_or_managed {
        println!("OAuth / managed-auth providers (copilot, openai, openrouter) use their own login flows.");
        println!("Run `vtcode login <provider>` or `/login <provider>` for those.");
    }
    Ok(())
}

async fn handle_add(target: SecretTarget, workspace: &Path, storage_mode: AuthCredentialsStoreMode) -> Result<()> {
    if target.local || target.managed_auth || target.env_key.is_empty() {
        println!("{} does not use a static API key. Use its configured authentication flow instead.", target.label);
        return Ok(());
    }
    let label = target.label.as_str();
    let env_key = target.env_key.as_str();

    println!("Bring your own key (BYOK) for {label}.");
    println!("Expected env: {}", env_key);
    println!("Secure display hint: ****************");
    println!("Key will be stored in secure storage (OS keyring or encrypted file).");
    println!("Key will NOT be stored in vtcode.toml or workspace environment files.");
    println!();

    let key = if io::stdin().is_terminal() {
        prompt_hidden_input(&format!("{} API key: ", label))?
    } else {
        vtcode_commons::diagnostics::stderr(format_args!(
            "Warning: stdin is not a terminal — the pasted key will be visible."
        ));
        print!("{} API key: ", label);
        io::stdout().flush()?;
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        input.trim().to_string()
    };

    if key.is_empty() {
        anyhow::bail!("API key cannot be empty.");
    }

    store_credential_with_mode(&target.provider_name, env_key, &key, storage_mode)?;

    // Purge stale entry from workspace .env so it doesn't shadow the
    // keyring entry (env var takes priority in get_api_key resolution).
    if !env_key.is_empty() {
        if read_workspace_env_value(workspace, env_key)?.is_some() {
            remove_workspace_env_value(workspace, env_key)?;
            println!("Removed stale {env_key} from workspace .env to avoid conflicts.");
        }
    }

    println!();
    println!("API key for {label} stored in secure storage.");
    println!("The key will be used automatically.");
    Ok(())
}

async fn handle_delete(target: SecretTarget, workspace: &Path, storage_mode: AuthCredentialsStoreMode) -> Result<()> {
    if target.local || target.managed_auth || target.env_key.is_empty() {
        println!("{} does not use a static API key. Use its configured authentication flow instead.", target.label);
        return Ok(());
    }
    let label = target.label.as_str();
    let env_key = target.env_key.as_str();

    let stored = load_stored_credential_with_mode(&target.provider_name, env_key, storage_mode)?;
    let workspace_value = read_workspace_env_value(workspace, env_key)?;
    if stored.is_none() && workspace_value.is_none() {
        println!("No stored API key found for {label}.");
        return Ok(());
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

    if stored.is_some() {
        clear_credential_with_mode(&target.provider_name, env_key, storage_mode)?;
    }

    // Also purge the env var from workspace .env to prevent stale entries.
    if workspace_value.is_some() {
        remove_workspace_env_value(workspace, env_key)?;
        println!("Also removed {env_key} from workspace .env.");
    }

    println!();
    if stored.is_some() {
        println!("API key for {label} deleted from secure storage.");
    } else {
        println!("API key for {label} removed from workspace environment.");
    }
    println!("The change takes effect immediately.");
    Ok(())
}

async fn handle_migrate(
    args: MigrateArgs,
    config: &VTCodeConfig,
    workspace: &Path,
    storage_mode: AuthCredentialsStoreMode,
) -> Result<()> {
    let env_path = workspace_env_path(workspace);
    if !env_path.exists() {
        println!("No .env file found at {}. Nothing to migrate.", env_path.display());
        return Ok(());
    }

    println!("Scanning {} for API keys to migrate...", env_path.display());
    println!();

    let targets = if let Some(name) = args.provider_name {
        vec![secret_target(config, &name, None)?]
    } else {
        all_secret_targets(config)
            .into_iter()
            .filter(|target| !target.local && !target.managed_auth && !target.env_key.is_empty())
            .collect::<Vec<_>>()
    };
    let targets = targets
        .into_iter()
        .filter(|target| !target.local && !target.managed_auth && !target.env_key.is_empty())
        .collect::<Vec<_>>();

    if args.dry_run {
        println!("[dry-run] Would migrate the following keys from .env to secure storage:");
        println!();
        for target in &targets {
            if read_workspace_env_value(workspace, &target.env_key)?.is_some() {
                println!("  {} ({})", target.label, target.env_key);
            }
        }
        println!();
        println!("No changes were made.");
        return Ok(());
    }

    let mut migrated = 0u32;
    let mut skipped = 0u32;
    let mut failed = 0u32;

    for target in targets {
        let env_key = target.env_key.as_str();

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

        let Some(value) = read_workspace_env_value(workspace, env_key)? else {
            skipped += 1;
            continue;
        };
        match store_credential_with_mode(&target.provider_name, env_key, value.trim(), storage_mode) {
            Ok(Some(_)) => {
                remove_workspace_env_value(workspace, env_key)?;
                println!("Migrated {} to secure storage.", env_key);
                migrated += 1;
            }
            Ok(None) => {
                skipped += 1;
            }
            Err(err) => {
                vtcode_commons::diagnostics::stderr(format_args!("Failed to migrate {}: {}", env_key, err));
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

#[allow(
    clippy::let_unit_value,
    reason = "Intentional compatibility, platform, or test-only suppression."
)]
fn prompt_hidden_input(prompt: &str) -> Result<String> {
    if !io::stdin().is_terminal() {
        anyhow::bail!("Cannot prompt for hidden input: stdin is not a terminal");
    }

    enable_raw_mode().with_context(|| "Failed to enable raw mode for secret input")?;
    struct RawModeGuard;
    impl Drop for RawModeGuard {
        fn drop(&mut self) {
            let _ = disable_raw_mode();
        }
    }
    let _raw = RawModeGuard;

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

#[derive(Debug, PartialEq, Eq)]
enum KeyAction {
    Continue,
    Submit,
    Abort,
}

fn handle_key(event: Event, buffer: &mut String) -> Result<KeyAction> {
    handle_key_with_writer(event, buffer, &mut io::stdout())
}

fn handle_key_with_writer(event: Event, buffer: &mut String, stdout: &mut impl Write) -> Result<KeyAction> {
    let Event::Key(key) = event else {
        return Ok(KeyAction::Continue);
    };
    if key.kind != KeyEventKind::Press {
        return Ok(KeyAction::Continue);
    }
    match key.code {
        KeyCode::Enter => Ok(KeyAction::Submit),
        KeyCode::Esc => Ok(KeyAction::Abort),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_input_masks_characters_and_escape_cancels_without_submission() {
        let mut buffer = String::new();
        let mut displayed = Vec::new();
        for character in "private-key-42".chars() {
            let event = Event::Key(event::KeyEvent::new(KeyCode::Char(character), event::KeyModifiers::NONE));
            assert_eq!(
                handle_key_with_writer(event, &mut buffer, &mut displayed).expect("keypress"),
                KeyAction::Continue
            );
        }
        assert_eq!(buffer, "private-key-42");
        assert_eq!(displayed, b"**************");
        let escape = Event::Key(event::KeyEvent::new(KeyCode::Esc, event::KeyModifiers::NONE));
        assert_eq!(handle_key_with_writer(escape, &mut buffer, &mut displayed).expect("escape"), KeyAction::Abort);
        assert_eq!(displayed, b"**************");
    }
}
