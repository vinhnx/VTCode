use std::str::FromStr;

use crate::agent::runloop::ui_list;
use crate::agent::runloop::ui_list::Tone;
use crate::agent::runloop::unified::session_setup::configured_anthropic_config;
use anyhow::Result;
use vtcode_auth::AuthCredentialsStoreMode;
use vtcode_config::VTCodeConfig;
use vtcode_config::api_keys::CredentialSource;
use vtcode_config::workspace_env::workspace_env_path;
use vtcode_core::config::models::Provider;
use vtcode_core::llm::factory::{ProviderConfig, create_provider_with_config};
use vtcode_core::utils::ansi::MessageStyle;
use vtcode_ui::tui::app::{InlineEvent, InlineListItem, InlineListSelection, TransientEvent};

use super::{SlashCommandContext, SlashCommandControl};
use crate::agent::runloop::slash_commands::SecretCommandAction;

mod storage;
use storage::SecretStorage;

#[derive(Clone)]
struct SecretTarget {
    provider_name: String,
    label: String,
    provider: Option<Provider>,
    env_key: String,
    local: bool,
    managed_auth: bool,
}

pub(crate) async fn handle_manage_secrets(
    mut ctx: SlashCommandContext<'_>,
    action: SecretCommandAction,
) -> Result<SlashCommandControl> {
    match action {
        SecretCommandAction::Interactive => {
            if !ctx.renderer.supports_inline_ui() {
                let mode = storage_mode(&ctx);
                render_secret_status_table(&mut ctx, None, None, mode)?;
                return Ok(SlashCommandControl::Continue);
            }
            run_interactive_secret_manager(&mut ctx).await?;
            Ok(SlashCommandControl::Continue)
        }
        SecretCommandAction::List => {
            let mode = storage_mode(&ctx);
            render_secret_status_table(&mut ctx, None, None, mode)?;
            Ok(SlashCommandControl::Continue)
        }
        SecretCommandAction::Status { provider, key_name } => {
            let mode = storage_mode(&ctx);
            render_secret_status_table(&mut ctx, provider.as_deref(), key_name.as_deref(), mode)?;
            Ok(SlashCommandControl::Continue)
        }
        SecretCommandAction::Add { provider, key_name } => {
            let target = match resolve_secret_target(ctx.vt_cfg.as_ref(), &provider, key_name.as_deref()) {
                Ok(p) => p,
                Err(err) => {
                    ctx.renderer.line(MessageStyle::Error, &err)?;
                    return Ok(SlashCommandControl::Continue);
                }
            };
            let _ = handle_add_secret(&mut ctx, target).await?;
            Ok(SlashCommandControl::Continue)
        }
        SecretCommandAction::Delete { provider, key_name } => {
            let target = match resolve_secret_target(ctx.vt_cfg.as_ref(), &provider, key_name.as_deref()) {
                Ok(p) => p,
                Err(err) => {
                    ctx.renderer.line(MessageStyle::Error, &err)?;
                    return Ok(SlashCommandControl::Continue);
                }
            };
            handle_delete_secret(&mut ctx, target).await?;
            Ok(SlashCommandControl::Continue)
        }
        SecretCommandAction::Migrate { provider } => {
            let target = match provider {
                Some(name) => match resolve_secret_target(ctx.vt_cfg.as_ref(), &name, None) {
                    Ok(p) => Some(p),
                    Err(err) => {
                        ctx.renderer.line(MessageStyle::Error, &err)?;
                        return Ok(SlashCommandControl::Continue);
                    }
                },
                None => None,
            };
            handle_migrate_secrets(&mut ctx, target).await?;
            Ok(SlashCommandControl::Continue)
        }
        SecretCommandAction::Help => {
            ctx.renderer
                .line(MessageStyle::Info, "Secret management — store API keys securely (keyring or encrypted file).")?;
            ctx.renderer.line(MessageStyle::Output, "")?;
            ctx.renderer
                .line(MessageStyle::Output, "  /secret                          Interactive secret manager (TUI)")?;
            ctx.renderer
                .line(MessageStyle::Output, "  /secret list                     Show all provider key statuses")?;
            ctx.renderer
                .line(MessageStyle::Output, "  /secret status [provider] [key]  Check a specific provider's key")?;
            ctx.renderer
                .line(MessageStyle::Output, "  /secret add <provider> [key]     Store a new API key securely")?;
            ctx.renderer
                .line(MessageStyle::Output, "  /secret delete <provider> [key]  Remove a stored key")?;
            ctx.renderer.line(
                MessageStyle::Output,
                "  /secret migrate [provider]       Move keys from .env to secure storage",
            )?;
            ctx.renderer.line(MessageStyle::Output, "")?;
            ctx.renderer.line(
                MessageStyle::Info,
                "Providers: openai, anthropic, openrouter, copilot, google, bedrock, mistral, groq, ...",
            )?;
            ctx.renderer.line(
                MessageStyle::Info,
                "OAuth/subscription providers (openai, openrouter, copilot) use /login instead of /secret.",
            )?;
            ctx.renderer
                .line(MessageStyle::Output, "  /login openai     ChatGPT subscription auth (no API key needed)")?;
            ctx.renderer
                .line(MessageStyle::Output, "  /login openrouter  OpenRouter OAuth")?;
            ctx.renderer
                .line(MessageStyle::Output, "  /login copilot    GitHub Copilot managed auth")?;
            ctx.renderer.line(MessageStyle::Output, "")?;
            ctx.renderer.line(
                MessageStyle::Output,
                "Get your API key from the provider's platform page (e.g. https://platform.openai.com/api-keys).",
            )?;
            Ok(SlashCommandControl::Continue)
        }
    }
}

fn resolve_secret_target(
    vt_cfg: Option<&VTCodeConfig>,
    name: &str,
    key_name: Option<&str>,
) -> Result<SecretTarget, String> {
    let provider_name = name.trim().to_ascii_lowercase();
    if provider_name.is_empty() {
        return Err("Provider name cannot be empty.".to_string());
    }
    if let Ok(provider) = Provider::from_str(&provider_name) {
        return Ok(SecretTarget {
            provider_name: provider_name.clone(),
            label: provider.label().to_string(),
            provider: Some(provider),
            env_key: key_name
                .filter(|key| !key.trim().is_empty())
                .map(ToOwned::to_owned)
                .or_else(|| vt_cfg.and_then(|cfg| cfg.configured_api_key_env(&provider_name)))
                .unwrap_or_else(|| provider.default_api_key_env().to_string()),
            local: provider.is_local(),
            managed_auth: provider.uses_managed_auth(),
        });
    }
    let Some(custom) = vt_cfg.and_then(|cfg| cfg.custom_provider(&provider_name)) else {
        let configured = vt_cfg
            .map(|cfg| {
                cfg.custom_providers
                    .iter()
                    .map(|provider| provider.name.as_str())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let suffix = if configured.is_empty() {
            String::new()
        } else {
            format!(" Configured custom providers: {}.", configured.join(", "))
        };
        return Err(format!("Unknown provider: {name}.{suffix}"));
    };
    Ok(SecretTarget {
        provider_name,
        label: custom.display_name.clone(),
        provider: None,
        env_key: if custom.uses_command_auth() {
            String::new()
        } else {
            key_name
                .filter(|key| !key.trim().is_empty())
                .map(ToOwned::to_owned)
                .unwrap_or_else(|| custom.resolved_api_key_env())
        },
        local: false,
        managed_auth: custom.uses_command_auth(),
    })
}

fn all_secret_targets(vt_cfg: Option<&VTCodeConfig>) -> Vec<SecretTarget> {
    let mut targets = Provider::all_providers()
        .into_iter()
        .filter_map(|provider| resolve_secret_target(vt_cfg, provider.as_ref(), None).ok())
        .collect::<Vec<_>>();
    if let Some(cfg) = vt_cfg {
        targets.extend(
            cfg.custom_providers
                .iter()
                .filter_map(|provider| resolve_secret_target(Some(cfg), &provider.name, None).ok()),
        );
    }
    targets
}

fn should_show_secret_items(
    target: &SecretTarget,
    workspace: &std::path::Path,
    storage_mode: AuthCredentialsStoreMode,
) -> bool {
    if target.local || target.managed_auth || target.env_key.is_empty() {
        return false;
    }
    !matches!(
        vtcode_config::api_keys::resolve_credential_with_mode(
            &target.provider_name,
            &target.env_key,
            Some(workspace),
            storage_mode,
        )
        .ok()
        .flatten()
        .map(|resolved| resolved.source),
        Some(CredentialSource::OAuth)
    )
}

async fn run_interactive_secret_manager(ctx: &mut SlashCommandContext<'_>) -> Result<()> {
    loop {
        show_secret_actions_modal(ctx);
        let Some(selection) = super::handlers::ui::wait_for_list_modal_selection(ctx).await else {
            return Ok(());
        };

        let InlineListSelection::ConfigAction(action) = selection else {
            continue;
        };
        if action == "secret:back" {
            return Ok(());
        }

        let Some(action_key) = action.strip_prefix("secret:") else {
            continue;
        };

        match action_key {
            "list" | "status" => {
                let mode = storage_mode(ctx);
                render_secret_status_table(ctx, None, None, mode)?;
            }
            "migrate" => {
                handle_migrate_secrets(ctx, None).await?;
            }
            _ => {
                if let Some(provider_name) = action_key.strip_prefix("add:") {
                    let target = match resolve_secret_target(ctx.vt_cfg.as_ref(), provider_name, None) {
                        Ok(target) => target,
                        Err(err) => {
                            ctx.renderer.line(MessageStyle::Error, &err)?;
                            continue;
                        }
                    };
                    match handle_add_secret(ctx, target).await? {
                        SecretEntryOutcome::Continue => {}
                        SecretEntryOutcome::CloseManager => {
                            // `wait_for_secure_prompt_input` already dismisses
                            // the secure prompt. Returning here prevents the
                            // `/secret` manager loop from opening its menu
                            // again and returns focus to the main TUI.
                            return Ok(());
                        }
                    }
                } else if let Some(provider_name) = action_key.strip_prefix("delete:") {
                    let target = match resolve_secret_target(ctx.vt_cfg.as_ref(), provider_name, None) {
                        Ok(target) => target,
                        Err(err) => {
                            ctx.renderer.line(MessageStyle::Error, &err)?;
                            continue;
                        }
                    };
                    handle_delete_secret(ctx, target).await?;
                }
            }
        }
    }
}

// --- Actions ---

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SecretEntryOutcome {
    Continue,
    CloseManager,
}

#[derive(Clone, Copy)]
enum CredentialReadback {
    Stored,
    Resolved,
}

async fn handle_add_secret(ctx: &mut SlashCommandContext<'_>, target: SecretTarget) -> Result<SecretEntryOutcome> {
    if target.local || target.managed_auth || target.env_key.is_empty() {
        ctx.renderer.line(
            MessageStyle::Info,
            &format!("{} does not use a static API key; use its configured authentication flow instead.", target.label),
        )?;
        return Ok(SecretEntryOutcome::Continue);
    }
    let label = target.label.as_str();
    let env_key = target.env_key.as_str();
    let prompt_label = format!("{} API key ({})", label, env_key);

    let platform_hint = target
        .provider
        .and_then(|provider| provider.platform_url())
        .map(|url| format!("Get your key at {url}"))
        .unwrap_or_else(|| format!("Get your {} API key from the provider's platform.", label));
    let lines = vec![
        format!("Bring your own key (BYOK) for {label}."),
        platform_hint,
        "Secure display hint: \u{2022}\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}".to_string(),
        "Key will be stored in secure storage (OS keyring or encrypted file).".to_string(),
        "Key will NOT be stored in vtcode.toml or workspace environment files.".to_string(),
        "Paste the key now, or press Esc to cancel.".to_string(),
    ];

    ctx.renderer
        .show_secure_prompt_modal("Secure API key setup", lines, prompt_label);

    let Some(key) = wait_for_secure_prompt_input(ctx).await else {
        ctx.renderer.line(MessageStyle::Info, "Secret entry cancelled.")?;
        return Ok(SecretEntryOutcome::Continue);
    };

    let trimmed = key.trim();
    if trimmed.is_empty() {
        ctx.renderer.line(MessageStyle::Error, "API key cannot be empty.")?;
        return Ok(SecretEntryOutcome::Continue);
    }

    let storage = SecretStorage::new(storage_mode(ctx));
    match storage.store(&target.provider_name, env_key, trimmed) {
        Ok(()) => {
            ctx.renderer
                .line(MessageStyle::Info, &format!("API key for {label} stored in secure storage."))?;
            ctx.renderer.line(MessageStyle::Output, "The key will be used automatically.")?;
            if vtcode_config::read_workspace_env_value(&ctx.config.workspace, env_key)?.is_some() {
                vtcode_config::remove_workspace_env_value(&ctx.config.workspace, env_key)?;
                ctx.renderer.line(
                    MessageStyle::Info,
                    &format!("Removed stale {env_key} from workspace .env to avoid conflicts."),
                )?;
            }
            if let Err(err) = reload_provider_client_if_matching(ctx, &target, CredentialReadback::Stored) {
                ctx.renderer.line(MessageStyle::Warning, &err)?;
            }
            return Ok(SecretEntryOutcome::CloseManager);
        }
        Err(err) => {
            tracing::warn!("Failed to store API key for {}: {}", label, err);
            ctx.renderer.line(
                MessageStyle::Error,
                &format!("Failed to store API key for {label}. Check secure storage permissions."),
            )?;
        }
    }

    Ok(SecretEntryOutcome::Continue)
}

async fn handle_migrate_secrets(
    ctx: &mut SlashCommandContext<'_>,
    target: Option<SecretTarget>,
) -> Result<SlashCommandControl> {
    let targets = match target {
        Some(target) => vec![target],
        None => all_secret_targets(ctx.vt_cfg.as_ref()),
    }
    .into_iter()
    .filter(|target| !target.local && !target.managed_auth && !target.env_key.is_empty())
    .collect::<Vec<_>>();

    let env_path = workspace_env_path(&ctx.config.workspace);
    let env_path_display = env_path.display().to_string();

    if !env_path.exists() {
        ctx.renderer
            .line(MessageStyle::Info, &format!("No .env file found at {}. Nothing to migrate.", env_path_display))?;
        return Ok(SlashCommandControl::Continue);
    }

    let storage = SecretStorage::new(storage_mode(ctx));
    let mut migrated = 0usize;
    let mut skipped = 0usize;
    let mut failed = 0usize;
    for target in targets {
        let env_key = target.env_key.as_str();
        let Some(value) = vtcode_config::read_workspace_env_value(&ctx.config.workspace, env_key)? else {
            skipped += 1;
            continue;
        };
        match storage.store(&target.provider_name, env_key, value.trim()) {
            Ok(()) => {
                vtcode_config::remove_workspace_env_value(&ctx.config.workspace, env_key)?;
                ctx.renderer
                    .line(MessageStyle::Info, &format!("Migrated {} API key to secure storage.", target.label))?;
                migrated += 1;
            }
            Err(err) => {
                ctx.renderer
                    .line(MessageStyle::Error, &format!("Failed to migrate {} API key: {}", target.label, err))?;
                failed += 1;
            }
        }
    }

    ctx.renderer.line(MessageStyle::Output, "")?;
    ctx.renderer.line(
        MessageStyle::Info,
        &format!("Migration complete: {} migrated, {} skipped, {} failed.", migrated, skipped, failed),
    )?;

    if migrated > 0 {
        ctx.renderer
            .line(MessageStyle::Output, "Keys moved from .env to secure storage (OS keyring or encrypted file).")?;
        ctx.renderer
            .line(MessageStyle::Output, "The change takes effect immediately.")?;
    }

    Ok(SlashCommandControl::Continue)
}

async fn handle_delete_secret(ctx: &mut SlashCommandContext<'_>, target: SecretTarget) -> Result<SlashCommandControl> {
    if target.local || target.managed_auth || target.env_key.is_empty() {
        ctx.renderer.line(
            MessageStyle::Info,
            &format!("{} does not use a static API key; use its configured authentication flow instead.", target.label),
        )?;
        return Ok(SlashCommandControl::Continue);
    }
    let label = target.label.as_str();

    let storage = SecretStorage::new(storage_mode(ctx));
    match storage.load_stored(&target.provider_name, &target.env_key) {
        Ok(None) => {
            ctx.renderer
                .line(MessageStyle::Info, &format!("No stored API key found for {label}."))?;
            return Ok(SlashCommandControl::Continue);
        }
        Ok(Some(_)) => {}
        Err(err) => {
            ctx.renderer.line(
                MessageStyle::Error,
                &format!("Could not inspect stored key for {label}; refusing to delete it: {err}"),
            )?;
            return Ok(SlashCommandControl::Continue);
        }
    }

    ctx.renderer.line(
        MessageStyle::Info,
        &format!("Type 'confirm' to delete the stored API key for {label}, or press Esc to cancel."),
    )?;

    let Some(confirmation) = wait_for_secure_prompt_input(ctx).await else {
        ctx.renderer.line(MessageStyle::Info, "Deletion cancelled.")?;
        return Ok(SlashCommandControl::Continue);
    };

    if confirmation.trim().ne("confirm") {
        ctx.renderer.line(MessageStyle::Info, "Deletion cancelled.")?;
        return Ok(SlashCommandControl::Continue);
    }

    match storage.clear(&target.provider_name, &target.env_key) {
        Ok(()) => {
            ctx.renderer
                .line(MessageStyle::Info, &format!("API key for {label} deleted from secure storage."))?;
            ctx.renderer
                .line(MessageStyle::Output, "The change takes effect immediately.")?;
            if let Err(err) = reload_provider_client_if_matching(ctx, &target, CredentialReadback::Resolved) {
                ctx.renderer.line(MessageStyle::Warning, &err)?;
            }
        }
        Err(err) => {
            ctx.renderer
                .line(MessageStyle::Error, &format!("Failed to delete API key for {label}: {err}"))?;
        }
    }

    Ok(SlashCommandControl::Continue)
}

// --- Storage ---

fn reload_provider_client_if_matching(
    ctx: &mut SlashCommandContext<'_>,
    target: &SecretTarget,
    readback: CredentialReadback,
) -> Result<(), String> {
    let current_provider = ctx.config.provider.trim().to_lowercase();
    if !target.provider_name.eq_ignore_ascii_case(&current_provider) {
        return Ok(());
    }

    let storage = SecretStorage::new(storage_mode(ctx));
    let new_api_key = match match readback {
        CredentialReadback::Stored => storage.load_stored(&target.provider_name, &target.env_key),
        CredentialReadback::Resolved => {
            storage.load_resolved(&target.provider_name, &target.env_key, &ctx.config.workspace)
        }
    } {
        Ok(Some(key)) => key,
        Ok(None) => {
            return Err(format!(
                "Secure storage read-back failed: no key found for {} after store. The key may not have persisted.",
                target.provider_name
            ));
        }
        Err(err) => {
            return Err(format!(
                "Secure storage read-back failed for {}: {err}. The key is stored but the running session may need a restart.",
                target.provider_name
            ));
        }
    };

    ctx.config.api_key = new_api_key;
    let new_provider = match create_provider_with_config(
        &target.provider_name,
        ProviderConfig {
            api_key: Some(ctx.config.api_key.clone()),
            openai_chatgpt_auth: ctx.config.openai_chatgpt_auth.clone(),
            copilot_auth: ctx.vt_cfg.as_ref().map(|cfg| cfg.auth.copilot.clone()),
            base_url: None,
            model: Some(ctx.config.model.clone()),
            prompt_cache: Some(ctx.config.prompt_cache.clone()),
            timeouts: None,
            openai: ctx.vt_cfg.as_ref().map(|cfg| cfg.provider.openai.clone()),
            anthropic: configured_anthropic_config(ctx.vt_cfg.as_ref()),
            model_behavior: ctx.config.model_behavior.clone(),
            workspace_root: Some(ctx.config.workspace.clone()),
        },
    ) {
        Ok(provider) => provider,
        Err(err) => {
            return Err(format!(
                "Failed to recreate provider client for {}: {err}. The key is stored but the running session may need a restart.",
                target.provider_name
            ));
        }
    };
    *ctx.provider_client = new_provider;
    Ok(())
}

/// Resolve the storage mode from the user config, falling back to the
/// platform-aware default ([`AuthCredentialsStoreMode::default()`]).
fn storage_mode(ctx: &SlashCommandContext<'_>) -> AuthCredentialsStoreMode {
    ctx.vt_cfg
        .as_ref()
        .map(|cfg| cfg.agent.credential_storage_mode)
        .unwrap_or_default()
}

// --- Input ---

async fn wait_for_secure_prompt_input(ctx: &mut SlashCommandContext<'_>) -> Option<String> {
    let _status_wait = ctx
        .handle
        .program_status_wait(vtcode_commons::program_status::InteractionKind::Auth);
    loop {
        if ctx.ctrl_c_state.is_cancel_requested() {
            dismiss_modal(ctx);
            return None;
        }

        let notify = ctx.ctrl_c_notify.clone();
        let maybe_event = tokio::select! {
            _ = notify.notified() => None,
            event = ctx.session.next_event() => event,
        };

        let Some(event) = maybe_event else {
            dismiss_modal(ctx);
            return None;
        };

        match event {
            InlineEvent::Interrupt => {
                ctx.ctrl_c_state.reset();
                dismiss_modal(ctx);
                return None;
            }
            InlineEvent::Cancel => {
                ctx.ctrl_c_state.reset();
                dismiss_modal(ctx);
                return None;
            }
            InlineEvent::Transient(TransientEvent::Cancelled) => {
                ctx.ctrl_c_state.reset();
                dismiss_modal(ctx);
                return None;
            }
            InlineEvent::Submit(submitted) => {
                ctx.ctrl_c_state.reset();
                dismiss_modal(ctx);
                return Some(submitted.text);
            }
            InlineEvent::QueueSubmit(submitted) => {
                ctx.ctrl_c_state.reset();
                dismiss_modal(ctx);
                return Some(submitted.text);
            }
            InlineEvent::Exit => {
                ctx.ctrl_c_state.reset();
                dismiss_modal(ctx);
                return None;
            }
            _ => {}
        }
    }
}

fn dismiss_modal(ctx: &mut SlashCommandContext<'_>) {
    ctx.handle.close_modal();
    ctx.handle.force_redraw();
}

// --- Render ---

const SECRET_ACTION_PREFIX: &str = "secret:";
const SECRET_ACTION_BACK: &str = "secret:back";
const CURRENT_BADGE: &str = "Current";

fn show_secret_actions_modal(ctx: &mut SlashCommandContext<'_>) {
    let current_provider = ctx.config.provider.trim().to_ascii_lowercase();
    let (items, selected) =
        build_secret_action_items(&current_provider, ctx.vt_cfg.as_ref(), &ctx.config.workspace, storage_mode(ctx));
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

fn build_secret_action_items(
    current_provider: &str,
    vt_cfg: Option<&VTCodeConfig>,
    workspace: &std::path::Path,
    storage_mode: AuthCredentialsStoreMode,
) -> (Vec<InlineListItem>, InlineListSelection) {
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
    for target in all_secret_targets(vt_cfg) {
        if !should_show_secret_items(&target, workspace, storage_mode) {
            continue;
        }
        let is_current = target.provider_name.eq_ignore_ascii_case(current_provider);
        let label = target.label.as_str();
        let key = target.provider_name.as_str();
        let badge = is_current.then(|| CURRENT_BADGE.to_string());
        let prefix = if is_current { "Current • " } else { "" };
        let add_action = format!("{SECRET_ACTION_PREFIX}add:{key}");
        let delete_action = format!("{SECRET_ACTION_PREFIX}delete:{key}");
        items.push(InlineListItem {
            title: format!("Add {label} key"),
            subtitle: Some(format!("{prefix}Store {label} API key in secure storage")),
            badge: badge.clone(),
            indent: 1,
            selection: Some(InlineListSelection::ConfigAction(add_action.clone())),
            search_value: Some(format!("add {} api key", label.to_lowercase())),
            ..Default::default()
        });
        items.push(InlineListItem {
            title: format!("Delete {label} key"),
            subtitle: Some(format!("{prefix}Remove stored {label} API key")),
            badge,
            indent: 1,
            selection: Some(InlineListSelection::ConfigAction(delete_action)),
            search_value: Some(format!("delete {} api key", label.to_lowercase())),
            ..Default::default()
        });
        if is_current {
            current_selection = Some(InlineListSelection::ConfigAction(add_action));
        }
    }

    items.push(
        ui_list::action(
            "Back",
            "Close secret manager".to_string(),
            None,
            Tone::Neutral,
            Some(InlineListSelection::ConfigAction(SECRET_ACTION_BACK.to_string())),
        )
        .with_search_value("back close exit".to_string()),
    );

    let selected =
        current_selection.unwrap_or_else(|| InlineListSelection::ConfigAction(format!("{SECRET_ACTION_PREFIX}list")));

    (items, selected)
}

fn render_secret_status_table(
    ctx: &mut SlashCommandContext<'_>,
    filter: Option<&str>,
    key_name: Option<&str>,
    storage_mode: AuthCredentialsStoreMode,
) -> Result<()> {
    ctx.renderer.line(MessageStyle::Info, "API Key Status")?;
    ctx.renderer.line(MessageStyle::Output, "")?;

    let targets = match filter {
        Some(name) => {
            vec![resolve_secret_target(ctx.vt_cfg.as_ref(), name, key_name).map_err(|err| anyhow::anyhow!(err))?]
        }
        None => all_secret_targets(ctx.vt_cfg.as_ref()),
    };

    let mut has_oauth_or_managed = false;
    for target in &targets {
        let source = if target.local {
            Some(CredentialSource::Local)
        } else if target.managed_auth {
            Some(CredentialSource::ManagedAuth)
        } else {
            vtcode_config::api_keys::resolve_credential_with_mode(
                &target.provider_name,
                &target.env_key,
                Some(&ctx.config.workspace),
                storage_mode,
            )?
            .map(|resolved| resolved.source)
        };
        let source_label = match source {
            Some(CredentialSource::Env) => "Environment variable",
            Some(CredentialSource::Workspace) => "Workspace .env",
            Some(CredentialSource::SecureStorage) => "OS keyring / encrypted file",
            Some(CredentialSource::OAuth) => "OAuth session",
            Some(CredentialSource::ManagedAuth) => "Managed auth (external CLI)",
            Some(CredentialSource::Local) => "Local \u{2014} no key required",
            None => "Not configured",
        };
        let status = if source.is_some() { "Ready" } else { "Missing" };
        if matches!(source, Some(CredentialSource::OAuth | CredentialSource::ManagedAuth)) {
            has_oauth_or_managed = true;
        }

        ctx.renderer
            .line(MessageStyle::Output, &format!("  {} ({})", target.label, target.provider_name))?;
        ctx.renderer.line(MessageStyle::Output, &format!("    Status: {}", status))?;
        ctx.renderer
            .line(MessageStyle::Output, &format!("    Source: {}", source_label))?;

        if !target.env_key.is_empty() {
            ctx.renderer
                .line(MessageStyle::Output, &format!("    Env var: {}", target.env_key))?;
        }
        if let Some(url) = target.provider.and_then(|provider| provider.platform_url()) {
            ctx.renderer.line(MessageStyle::Output, &format!("    Platform: {}", url))?;
        }

        ctx.renderer.line(MessageStyle::Output, "")?;
    }

    ctx.renderer
        .line(MessageStyle::Info, "Use /secret add <provider> [key-name] to store a key.")?;
    if !has_oauth_or_managed {
        ctx.renderer
            .line(MessageStyle::Info, "Use /secret delete <provider> [key-name] to remove a stored key.")?;
    }
    ctx.renderer
        .line(MessageStyle::Info, "Use /secret migrate to move keys from workspace .env to secure storage.")?;
    if has_oauth_or_managed {
        ctx.renderer.line(
            MessageStyle::Info,
            "OAuth / managed-auth providers (copilot, openai, openrouter) use their own login flows.",
        )?;
        ctx.renderer.line(MessageStyle::Info, "Run `/login <provider>` for those.")?;
        ctx.renderer.line(
            MessageStyle::Output,
            "  Tip: If you have Codex CLI installed, VT Code automatically reuses its ChatGPT auth.json.",
        )?;
    }

    Ok(())
}

fn list_item(title: &str, subtitle: &str, action: String, search: &str) -> InlineListItem {
    ui_list::action(
        title.to_string(),
        subtitle.to_string(),
        None,
        Tone::Neutral,
        Some(InlineListSelection::ConfigAction(action)),
    )
    .with_search_value(search.to_string())
}
