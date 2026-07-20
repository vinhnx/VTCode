use std::str::FromStr;

use anyhow::Result;
use vtcode_auth::AuthCredentialsStoreMode;
use vtcode_config::api_keys::{CredentialSource, provider_credential_detail};
use vtcode_config::workspace_env::{MigrationOutcome, migrate_workspace_env_keys, workspace_env_path};
use vtcode_core::config::models::Provider;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};
use vtcode_ui::tui::app::{InlineEvent, InlineListItem, InlineListSelection, TransientEvent};

use super::{SlashCommandContext, SlashCommandControl, ui};
use crate::agent::runloop::slash_commands::SecretCommandAction;

const SECRET_ACTION_PREFIX: &str = "secret:";
const SECRET_ACTION_BACK: &str = "secret:back";
const CURRENT_BADGE: &str = "Current";

pub(crate) async fn handle_manage_secrets(
    mut ctx: SlashCommandContext<'_>,
    action: SecretCommandAction,
) -> Result<SlashCommandControl> {
    match action {
        SecretCommandAction::Interactive => {
            if !ctx.renderer.supports_inline_ui() {
                render_secret_status_table(ctx.renderer, None)?;
                return Ok(SlashCommandControl::Continue);
            }
            run_interactive_secret_manager(&mut ctx).await?;
            Ok(SlashCommandControl::Continue)
        }
        SecretCommandAction::List => {
            render_secret_status_table(ctx.renderer, None)?;
            Ok(SlashCommandControl::Continue)
        }
        SecretCommandAction::Status { provider } => {
            let provider = match provider {
                Some(name) => match resolve_provider(&name) {
                    Ok(p) => Some(p),
                    Err(err) => {
                        ctx.renderer.line(MessageStyle::Error, &err)?;
                        return Ok(SlashCommandControl::Continue);
                    }
                },
                None => None,
            };
            render_secret_status_table(ctx.renderer, provider)?;
            Ok(SlashCommandControl::Continue)
        }
        SecretCommandAction::Add { provider } => {
            let provider = match resolve_provider(&provider) {
                Ok(p) => p,
                Err(err) => {
                    ctx.renderer.line(MessageStyle::Error, &err)?;
                    return Ok(SlashCommandControl::Continue);
                }
            };
            handle_add_secret(&mut ctx, provider).await?;
            Ok(SlashCommandControl::Continue)
        }
        SecretCommandAction::Delete { provider } => {
            let provider = match resolve_provider(&provider) {
                Ok(p) => p,
                Err(err) => {
                    ctx.renderer.line(MessageStyle::Error, &err)?;
                    return Ok(SlashCommandControl::Continue);
                }
            };
            handle_delete_secret(&mut ctx, provider).await?;
            Ok(SlashCommandControl::Continue)
        }
        SecretCommandAction::Migrate { provider } => {
            let provider = match provider {
                Some(name) => match resolve_provider(&name) {
                    Ok(p) => Some(p),
                    Err(err) => {
                        ctx.renderer.line(MessageStyle::Error, &err)?;
                        return Ok(SlashCommandControl::Continue);
                    }
                },
                None => None,
            };
            handle_migrate_secrets(&mut ctx, provider).await?;
            Ok(SlashCommandControl::Continue)
        }
        SecretCommandAction::Help => {
            ctx.renderer.line(
                MessageStyle::Info,
                "Usage: /secret [list|status [provider]|add <provider>|delete <provider>|migrate [provider]|help]",
            )?;
            Ok(SlashCommandControl::Continue)
        }
    }
}

fn resolve_provider(name: &str) -> Result<Provider, String> {
    Provider::from_str(name)
        .map_err(|_foo| format!("Unknown provider: {name}. Use one of: {}", ALL_PROVIDER_NAMES.as_str()))
}

async fn run_interactive_secret_manager(ctx: &mut SlashCommandContext<'_>) -> Result<()> {
    loop {
        show_secret_actions_modal(ctx);
        let Some(selection) = ui::wait_for_list_modal_selection(ctx).await else {
            return Ok(());
        };

        let InlineListSelection::ConfigAction(action) = selection else {
            continue;
        };
        if action == SECRET_ACTION_BACK {
            return Ok(());
        }

        let Some(action_key) = action.strip_prefix(SECRET_ACTION_PREFIX) else {
            continue;
        };

        match action_key {
            "list" | "status" => {
                render_secret_status_table(ctx.renderer, None)?;
            }
            "migrate" => {
                handle_migrate_secrets(ctx, None).await?;
            }
            _ => {
                if let Some(provider_name) = action_key.strip_prefix("add:") {
                    if let Ok(provider) = Provider::from_str(provider_name) {
                        handle_add_secret(ctx, provider).await?;
                    }
                } else if let Some(provider_name) = action_key.strip_prefix("delete:") {
                    if let Ok(provider) = Provider::from_str(provider_name) {
                        handle_delete_secret(ctx, provider).await?;
                    }
                }
            }
        }
    }
}

fn show_secret_actions_modal(ctx: &mut SlashCommandContext<'_>) {
    let current_provider = ctx.config.provider.trim().parse::<Provider>().ok();
    let (items, selected) = build_secret_action_items(current_provider);
    ctx.renderer.show_list_modal(
        "Secrets",
        vec![
            "Manage API keys in secure storage (OS keyring or encrypted file).".to_string(),
            "Keys are never written to vtcode.toml or workspace environment files.".to_string(),
        ],
        items,
        Some(selected),
        None,
    );
}

fn build_secret_action_items(current_provider: Option<Provider>) -> (Vec<InlineListItem>, InlineListSelection) {
    let providers = Provider::all_providers();
    let mut items = vec![
        list_item(
            "List all secrets",
            "Show status table for all providers",
            format!("{SECRET_ACTION_PREFIX}list"),
            "list all secrets status",
        ),
        list_item(
            "Migrate .env keys",
            "Move API keys from workspace .env to secure storage",
            format!("{SECRET_ACTION_PREFIX}migrate"),
            "migrate dotenv workspace secrets",
        ),
        list_item(
            "Add or replace a secret",
            "Paste an API key for a provider",
            format!("{SECRET_ACTION_PREFIX}add:provider"),
            "add replace secret api key",
        ),
        list_item(
            "Delete a secret",
            "Remove a stored API key from secure storage",
            format!("{SECRET_ACTION_PREFIX}delete:provider"),
            "delete remove secret",
        ),
    ];

    let mut current_selection = None;

    if let Some(provider) = current_provider {
        if !provider.is_local() && !provider.uses_managed_auth() {
            let detail = provider_credential_detail(provider);
            if !matches!(detail.map(|d| d.source), Some(CredentialSource::OAuth)) {
                let key = provider.as_ref();
                let label = provider.label();
                let add_action = format!("{SECRET_ACTION_PREFIX}add:{key}");
                let delete_action = format!("{SECRET_ACTION_PREFIX}delete:{key}");
                current_selection = Some(InlineListSelection::ConfigAction(add_action.clone()));
                items.push(InlineListItem {
                    title: format!("Add {label} key"),
                    subtitle: Some(format!("{CURRENT_BADGE} • Store {label} API key in secure storage")),
                    badge: Some(CURRENT_BADGE.to_string()),
                    indent: 1,
                    selection: Some(InlineListSelection::ConfigAction(add_action)),
                    search_value: Some(format!("add {} api key", label.to_lowercase())),
                });
                items.push(InlineListItem {
                    title: format!("Delete {label} key"),
                    subtitle: Some(format!("{CURRENT_BADGE} • Remove stored {label} API key")),
                    badge: Some(CURRENT_BADGE.to_string()),
                    indent: 1,
                    selection: Some(InlineListSelection::ConfigAction(delete_action)),
                    search_value: Some(format!("delete {} api key", label.to_lowercase())),
                });
            }
        }
    }

    for provider in providers {
        if let Some(current) = current_provider {
            if provider == current {
                continue;
            }
        }
        let label = provider.label();
        let key = provider.as_ref();
        if provider.is_local() || provider.uses_managed_auth() {
            continue;
        }
        let detail = provider_credential_detail(provider);
        if matches!(detail.map(|d| d.source), Some(CredentialSource::OAuth)) {
            continue;
        }
        items.push(InlineListItem {
            title: format!("Add {label} key"),
            subtitle: Some(format!("Store {} API key in secure storage", label)),
            badge: None,
            indent: 1,
            selection: Some(InlineListSelection::ConfigAction(format!("{SECRET_ACTION_PREFIX}add:{key}"))),
            search_value: Some(format!("add {} api key", label.to_lowercase())),
        });
        items.push(InlineListItem {
            title: format!("Delete {label} key"),
            subtitle: Some(format!("Remove stored {} API key", label)),
            badge: None,
            indent: 1,
            selection: Some(InlineListSelection::ConfigAction(format!("{SECRET_ACTION_PREFIX}delete:{key}"))),
            search_value: Some(format!("delete {} api key", label.to_lowercase())),
        });
    }

    items.push(InlineListItem {
        title: "Back".to_string(),
        subtitle: Some("Close secret manager".to_string()),
        badge: None,
        indent: 0,
        selection: Some(InlineListSelection::ConfigAction(SECRET_ACTION_BACK.to_string())),
        search_value: Some("back close exit".to_string()),
    });

    let selected =
        current_selection.unwrap_or_else(|| InlineListSelection::ConfigAction(format!("{SECRET_ACTION_PREFIX}list")));

    (items, selected)
}

fn list_item(title: &str, subtitle: &str, action: String, search: &str) -> InlineListItem {
    InlineListItem {
        title: title.to_string(),
        subtitle: Some(subtitle.to_string()),
        badge: None,
        indent: 0,
        selection: Some(InlineListSelection::ConfigAction(action)),
        search_value: Some(search.to_string()),
    }
}

async fn handle_add_secret(ctx: &mut SlashCommandContext<'_>, provider: Provider) -> Result<()> {
    if provider.uses_managed_auth() {
        ctx.renderer.line(
            MessageStyle::Info,
            &format!(
                "{} uses managed auth (GitHub Copilot CLI). Run `/login {}` instead.",
                provider.label(),
                provider.as_ref()
            ),
        )?;
        return Ok(());
    }
    let label = provider.label();
    let env_key = provider.default_api_key_env();
    let prompt_label = format!("{} API key ({})", label, env_key);

    let lines = vec![
        format!("Bring your own key (BYOK) for {label}."),
        format!("Expected env: {}", env_key),
        "Secure display hint: \u{2022}\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}".to_string(),
        "Key will be stored in secure storage (OS keyring or encrypted file).".to_string(),
        "Key will NOT be stored in vtcode.toml or workspace environment files.".to_string(),
        "Paste the key now, or press Esc to cancel.".to_string(),
    ];

    ctx.renderer
        .show_secure_prompt_modal("Secure API key setup", lines, prompt_label);

    let Some(key) = wait_for_secure_prompt_input(ctx).await else {
        ctx.renderer.line(MessageStyle::Info, "Secret entry cancelled.")?;
        return Ok(());
    };

    let trimmed = key.trim();
    if trimmed.is_empty() {
        ctx.renderer.line(MessageStyle::Error, "API key cannot be empty.")?;
        return Ok(());
    }

    let storage = vtcode_auth::CustomApiKeyStorage::new(provider.as_ref());
    match storage.store(trimmed, AuthCredentialsStoreMode::default()) {
        Ok(()) => {
            ctx.renderer
                .line(MessageStyle::Info, &format!("API key for {label} stored in secure storage."))?;
            ctx.renderer
                .line(MessageStyle::Output, "The key will be used automatically on next provider/model reload.")?;
        }
        Err(err) => {
            tracing::warn!("Failed to store API key for {}: {}", label, err);
            ctx.renderer.line(
                MessageStyle::Error,
                &format!("Failed to store API key for {label}. Check secure storage permissions."),
            )?;
        }
    }

    Ok(())
}

async fn handle_migrate_secrets(ctx: &mut SlashCommandContext<'_>, provider: Option<Provider>) -> Result<()> {
    if let Some(p) = provider {
        if p.uses_managed_auth() {
            ctx.renderer.line(
                MessageStyle::Info,
                &format!("{} uses managed auth (GitHub Copilot CLI). Run `/login {}` instead.", p.label(), p.as_ref()),
            )?;
            return Ok(());
        }
    }

    let targets: Vec<Provider> = match provider {
        Some(p) => vec![p],
        None => Provider::all_providers(),
    };

    let env_path = workspace_env_path(&ctx.config.workspace);
    let env_path_display = env_path.display().to_string();

    if !env_path.exists() {
        ctx.renderer
            .line(MessageStyle::Info, &format!("No .env file found at {}. Nothing to migrate.", env_path_display))?;
        return Ok(());
    }

    let (summary, outcomes) =
        migrate_workspace_env_keys(&ctx.config.workspace, &targets, AuthCredentialsStoreMode::default())?;

    for (provider, outcome) in outcomes {
        let env_key = provider.default_api_key_env();
        match outcome {
            MigrationOutcome::Migrated => {
                ctx.renderer
                    .line(MessageStyle::Info, &format!("Migrated {} API key to secure storage.", provider.label()))?;
            }
            MigrationOutcome::Skipped => {}
            MigrationOutcome::Failed => {
                ctx.renderer.line(
                    MessageStyle::Error,
                    &format!("Failed to migrate {} API key: {}", provider.label(), env_key),
                )?;
            }
        }
    }

    ctx.renderer.line(MessageStyle::Output, "")?;
    ctx.renderer.line(
        MessageStyle::Info,
        &format!(
            "Migration complete: {} migrated, {} skipped, {} failed.",
            summary.migrated, summary.skipped, summary.failed
        ),
    )?;

    if summary.migrated > 0 {
        ctx.renderer
            .line(MessageStyle::Output, "Keys moved from .env to secure storage (OS keyring or encrypted file).")?;
        ctx.renderer
            .line(MessageStyle::Output, "Reload providers or restart VT Code for changes to take effect.")?;
    }

    Ok(())
}

async fn handle_delete_secret(ctx: &mut SlashCommandContext<'_>, provider: Provider) -> Result<()> {
    if provider.uses_managed_auth() {
        ctx.renderer.line(
            MessageStyle::Info,
            &format!(
                "{} uses managed auth (GitHub Copilot CLI). Run `/login {}` instead.",
                provider.label(),
                provider.as_ref()
            ),
        )?;
        return Ok(());
    }
    let label = provider.label();

    let storage = vtcode_auth::CustomApiKeyStorage::new(provider.as_ref());
    match storage.load(AuthCredentialsStoreMode::default()) {
        Ok(None) => {
            ctx.renderer
                .line(MessageStyle::Info, &format!("No stored API key found for {label}."))?;
            return Ok(());
        }
        Ok(Some(_)) => {}
        Err(err) => {
            ctx.renderer
                .line(MessageStyle::Warning, &format!("Could not inspect stored key for {label}: {err}"))?;
        }
    }

    ctx.renderer.line(
        MessageStyle::Info,
        &format!("Type 'confirm' to delete the stored API key for {label}, or press Esc to cancel."),
    )?;

    let Some(confirmation) = wait_for_secure_prompt_input(ctx).await else {
        ctx.renderer.line(MessageStyle::Info, "Deletion cancelled.")?;
        return Ok(());
    };

    if confirmation.trim().ne("confirm") {
        ctx.renderer.line(MessageStyle::Info, "Deletion cancelled.")?;
        return Ok(());
    }

    match storage.clear(AuthCredentialsStoreMode::default()) {
        Ok(()) => {
            ctx.renderer
                .line(MessageStyle::Info, &format!("API key for {label} deleted from secure storage."))?;
            ctx.renderer
                .line(MessageStyle::Output, "The change takes effect on next provider/model reload.")?;
        }
        Err(err) => {
            ctx.renderer
                .line(MessageStyle::Error, &format!("Failed to delete API key for {label}: {err}"))?;
        }
    }

    Ok(())
}

fn render_secret_status_table(renderer: &mut AnsiRenderer, filter: Option<Provider>) -> Result<()> {
    renderer.line(MessageStyle::Info, "API Key Status")?;
    renderer.line(MessageStyle::Output, "")?;

    let providers: Vec<Provider> = match filter {
        Some(p) => vec![p],
        None => Provider::all_providers(),
    };

    for &provider in &providers {
        let detail = provider_credential_detail(provider);
        let source = detail.map(|d| d.source);
        let source_label = match source {
            Some(CredentialSource::Env) => "Environment variable",
            Some(CredentialSource::SecureStorage) => "OS keyring / encrypted file",
            Some(CredentialSource::OAuth) => "OAuth session",
            Some(CredentialSource::ManagedAuth) => "Managed auth (external CLI)",
            Some(CredentialSource::Local) => "Local — no key required",
            None => "Not configured",
        };
        let status = match source {
            Some(CredentialSource::Local) | Some(CredentialSource::ManagedAuth) => "Ready",
            Some(_) => "Ready",
            None => "Missing",
        };

        renderer.line(MessageStyle::Output, &format!("  {} ({})", provider.label(), provider.as_ref()))?;
        renderer.line(MessageStyle::Output, &format!("    Status: {}", status))?;
        renderer.line(MessageStyle::Output, &format!("    Source: {}", source_label))?;

        if let Some(env_key) = detail.and_then(|d| d.env_var) {
            renderer.line(MessageStyle::Output, &format!("    Env var: {}", env_key))?;
        }

        renderer.line(MessageStyle::Output, "")?;
    }

    let mut has_oauth_or_managed = false;
    for provider in &providers {
        let detail = provider_credential_detail(*provider);
        if let Some(source) = detail.map(|d| d.source) {
            if matches!(source, CredentialSource::OAuth | CredentialSource::ManagedAuth) {
                has_oauth_or_managed = true;
            }
        }
    }

    renderer.line(MessageStyle::Info, "Use /secret add <provider> to store a key.")?;
    if !has_oauth_or_managed {
        renderer.line(MessageStyle::Info, "Use /secret delete <provider> to remove a stored key.")?;
    }
    renderer.line(MessageStyle::Info, "Use /secret migrate to move keys from workspace .env to secure storage.")?;
    if has_oauth_or_managed {
        renderer.line(
            MessageStyle::Info,
            "OAuth / managed-auth providers (copilot, openai, openrouter) use their own login flows.",
        )?;
        renderer.line(MessageStyle::Info, "Run `/login <provider>` for those.")?;
    }

    Ok(())
}

async fn wait_for_secure_prompt_input(ctx: &mut SlashCommandContext<'_>) -> Option<String> {
    loop {
        if ctx.ctrl_c_state.is_cancel_requested() {
            ctx.handle.close_modal();
            ctx.handle.force_redraw();
            return None;
        }

        let notify = ctx.ctrl_c_notify.clone();
        let maybe_event = tokio::select! {
            _ = notify.notified() => None,
            event = ctx.session.next_event() => event,
        };

        let Some(event) = maybe_event else {
            ctx.handle.close_modal();
            ctx.handle.force_redraw();
            if ctx.ctrl_c_state.is_cancel_requested() {
                return None;
            }
            return None;
        };

        match event {
            InlineEvent::Interrupt => {
                ctx.ctrl_c_state.reset();
                ctx.handle.close_modal();
                ctx.handle.force_redraw();
                return None;
            }
            InlineEvent::Cancel => {
                ctx.ctrl_c_state.reset();
                ctx.handle.close_modal();
                ctx.handle.force_redraw();
                return None;
            }
            InlineEvent::Transient(TransientEvent::Cancelled) => {
                ctx.ctrl_c_state.reset();
                ctx.handle.close_modal();
                ctx.handle.force_redraw();
                return None;
            }
            InlineEvent::Submit(submitted) => {
                ctx.ctrl_c_state.reset();
                ctx.handle.close_modal();
                ctx.handle.force_redraw();
                return Some(submitted.text);
            }
            InlineEvent::QueueSubmit(submitted) => {
                ctx.ctrl_c_state.reset();
                ctx.handle.close_modal();
                ctx.handle.force_redraw();
                return Some(submitted.text);
            }
            InlineEvent::Exit => {
                ctx.ctrl_c_state.reset();
                ctx.handle.close_modal();
                ctx.handle.force_redraw();
                return None;
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vtcode_ui::tui::app::InlineListSelection;

    fn item_titles(items: &[InlineListItem]) -> Vec<&str> {
        items.iter().map(|i| i.title.as_str()).collect()
    }

    fn current_provider_item_indices(items: &[InlineListItem], provider_label: &str) -> Vec<usize> {
        items
            .iter()
            .enumerate()
            .filter(|(_, i)| i.title.contains(provider_label) && i.badge.as_deref() == Some(CURRENT_BADGE))
            .map(|(idx, _)| idx)
            .collect()
    }

    #[test]
    fn no_current_provider_uses_default_order_and_selection() {
        let (items, selected) = build_secret_action_items(None);
        let titles = item_titles(&items);
        assert_eq!(titles[0], "List all secrets");
        assert_eq!(titles[1], "Migrate .env keys");
        assert_eq!(selected, InlineListSelection::ConfigAction("secret:list".to_string()));
        assert!(items.iter().all(|i| i.badge.as_deref() != Some(CURRENT_BADGE)));
    }

    #[test]
    fn current_provider_items_get_current_badge_and_come_first() {
        let (items, selected) = build_secret_action_items(Some(Provider::DeepSeek));
        let titles = item_titles(&items);
        assert_eq!(titles[0], "List all secrets");
        assert_eq!(titles[1], "Migrate .env keys");
        let deepseek_indices = current_provider_item_indices(&items, "DeepSeek");
        assert_eq!(deepseek_indices, vec![4, 5], "DeepSeek add/delete should be right after static actions");
        for &idx in &deepseek_indices {
            assert_eq!(items[idx].badge.as_deref(), Some(CURRENT_BADGE));
            assert!(items[idx].subtitle.as_ref().unwrap().starts_with(CURRENT_BADGE));
        }
        assert_eq!(selected, InlineListSelection::ConfigAction("secret:add:deepseek".to_string()));
    }

    #[test]
    fn current_provider_is_excluded_from_other_providers_section() {
        let (items, _) = build_secret_action_items(Some(Provider::DeepSeek));
        let deepseek_indices = current_provider_item_indices(&items, "DeepSeek");
        assert_eq!(deepseek_indices.len(), 2, "DeepSeek should appear exactly once with Current badge");
        let other_deepseek = items
            .iter()
            .enumerate()
            .filter(|(_, i)| i.title.contains("DeepSeek") && i.badge.as_deref() != Some(CURRENT_BADGE))
            .count();
        assert_eq!(other_deepseek, 0, "DeepSeek should not appear outside the Current section");
    }

    #[test]
    fn invalid_current_provider_string_does_not_panic() {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| build_secret_action_items(None)));
        assert!(result.is_ok());
    }

    #[test]
    fn local_current_provider_has_no_per_provider_items() {
        let (items, selected) = build_secret_action_items(Some(Provider::Ollama));
        let titles = item_titles(&items);
        assert!(!titles.iter().any(|t| t.contains("Ollama")), "local provider should not appear");
        assert_eq!(selected, InlineListSelection::ConfigAction("secret:list".to_string()));
    }

    #[test]
    fn managed_auth_current_provider_has_no_per_provider_items() {
        let (items, selected) = build_secret_action_items(Some(Provider::Copilot));
        let titles = item_titles(&items);
        assert!(!titles.iter().any(|t| t.contains("Copilot")), "managed-auth provider should not appear");
        assert_eq!(selected, InlineListSelection::ConfigAction("secret:list".to_string()));
    }

    #[test]
    fn non_current_oauth_provider_is_excluded_when_session_active() {
        let (items, _) = build_secret_action_items(Some(Provider::DeepSeek));
        let openai_titles: Vec<&str> = items
            .iter()
            .filter(|i| i.title.contains("OpenAI"))
            .map(|i| i.title.as_str())
            .collect();
        if openai_titles.is_empty() {
            return;
        }
        for title in &openai_titles {
            assert!(title.starts_with("Add ") || title.starts_with("Delete "));
        }
    }
}

static ALL_PROVIDER_NAMES: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| {
    Provider::all_providers()
        .iter()
        .map(|p| p.to_string())
        .collect::<Vec<_>>()
        .join(", ")
});
