use std::collections::BTreeMap;
use std::path::Path;
use std::str::FromStr;

use anyhow::Result;
use vtcode_auth::{AuthCredentialsStoreMode, OpenAIPreferredMethod, resolve_openai_auth};
use vtcode_commons::ui_protocol::{InlineStatus, InlineTone};
use vtcode_config::VTCodeConfig;
use vtcode_config::api_keys::{CredentialSource, ResolvedCredential};
use vtcode_core::config::models::Provider;
use vtcode_core::config::types::AgentConfig;
use vtcode_core::llm::provider::LLMProvider;
use vtcode_tui_runtime::utils::ansi::MessageStyle;
use vtcode_ui::tui::app::{InlineEvent, TransientEvent};

use super::{SlashCommandContext, SlashCommandControl};
use crate::agent::runloop::slash_commands::SecretCommandAction;
use crate::agent::runloop::unified::session_setup::create_provider_client;
use crate::agent::runloop::unified::settings_interactive::secrets::SECRETS_VIEW;

mod storage;
use storage::SecretStorage;

const SECURE_STORAGE_LABEL: &str = "OS keyring / encrypted file";
pub(crate) const DELETE_CONFIRMATION: &str = "confirm";

#[derive(Clone)]
pub(crate) struct SecretTarget {
    pub(crate) provider_name: String,
    pub(crate) label: String,
    provider: Option<Provider>,
    pub(crate) env_key: String,
    local: bool,
    managed_auth: bool,
}

impl SecretTarget {
    pub(crate) fn static_key_hint(&self) -> Option<&'static str> {
        if self.local {
            Some("Local provider • no key required")
        } else if self.managed_auth {
            Some("Managed authentication • use the provider's login flow")
        } else if self.env_key.is_empty() {
            Some("OAuth • use the provider's login flow")
        } else {
            None
        }
    }

    pub(crate) fn settings_add_hint(&self, openai_method: OpenAIPreferredMethod) -> Option<&'static str> {
        self.static_key_hint().or_else(|| {
            (self.is_default_openai_key() && openai_method == OpenAIPreferredMethod::Chatgpt)
                .then_some("ChatGPT OAuth • use /login openai")
        })
    }

    fn is_default_openai_key(&self) -> bool {
        self.provider == Some(Provider::OpenAI)
            && self.env_key.eq_ignore_ascii_case(Provider::OpenAI.default_api_key_env())
    }

    pub(crate) fn add_question(&self) -> String {
        let platform = self
            .provider
            .and_then(|provider| provider.platform_url())
            .map(|url| format!("Get your key at {url}."))
            .unwrap_or_else(|| format!("Get your key from the {} platform.", self.label));
        format!("Enter the {} API key ({}). {platform}", self.label, self.env_key)
    }

    pub(crate) fn delete_question(&self) -> String {
        format!("Type `{DELETE_CONFIRMATION}` to delete the stored {} API key ({}).", self.label, self.env_key)
    }

    fn resolve(&self, workspace: &Path, mode: AuthCredentialsStoreMode) -> Result<Option<ResolvedCredential>> {
        vtcode_config::api_keys::resolve_credential_with_mode(&self.provider_name, &self.env_key, Some(workspace), mode)
    }

    pub(crate) fn status(&self, workspace: &Path, mode: AuthCredentialsStoreMode) -> (&'static str, &'static str) {
        let source = if self.local {
            Some(CredentialSource::Local)
        } else if self.managed_auth {
            Some(CredentialSource::ManagedAuth)
        } else {
            match self.resolve(workspace, mode) {
                Ok(resolved) => resolved.map(|credential| credential.source),
                Err(_) => return ("Unavailable", "Could not read credential sources"),
            }
        };
        match source {
            Some(source) => ("Ready", source_label(source)),
            None => ("Missing", "Not configured"),
        }
    }

    pub(crate) fn has_stored_key(&self, mode: AuthCredentialsStoreMode) -> Result<bool, String> {
        SecretStorage::new(mode)
            .load_stored(&self.provider_name, &self.env_key)
            .map(|key| key.is_some())
            .map_err(|_storage| format!("Could not inspect secure storage for {}.", self.label))
    }
}

fn source_label(source: CredentialSource) -> &'static str {
    match source {
        CredentialSource::Env => "Environment variable",
        CredentialSource::Workspace => "Workspace .env",
        CredentialSource::SecureStorage => SECURE_STORAGE_LABEL,
        CredentialSource::OAuth => "OAuth session",
        CredentialSource::ManagedAuth => "Managed auth (external CLI)",
        CredentialSource::Local => "Local — no key required",
    }
}

pub(crate) fn storage_mode(config: Option<&VTCodeConfig>) -> AuthCredentialsStoreMode {
    config.map(|cfg| cfg.agent.credential_storage_mode).unwrap_or_default()
}

pub(crate) async fn handle_manage_secrets(
    mut ctx: SlashCommandContext<'_>,
    action: SecretCommandAction,
) -> Result<SlashCommandControl> {
    match action {
        SecretCommandAction::Interactive => {
            return super::handlers::show_settings_at_path_from_context(&mut ctx, Some(SECRETS_VIEW)).await;
        }
        SecretCommandAction::List => render_secret_status_table(&mut ctx, None, None)?,
        SecretCommandAction::Status { provider, key_name } => {
            render_secret_status_table(&mut ctx, provider.as_deref(), key_name.as_deref())?;
        }
        SecretCommandAction::Add { provider, key_name } => {
            prompt_for_secret(&mut ctx, &provider, key_name.as_deref(), false).await?;
        }
        SecretCommandAction::Delete { provider, key_name } => {
            prompt_for_secret(&mut ctx, &provider, key_name.as_deref(), true).await?;
        }
        SecretCommandAction::Migrate { provider } => {
            let targets = if let Some(name) = provider {
                match resolve_secret_target(ctx.vt_cfg.as_ref(), &name, None) {
                    Ok(target) => vec![target],
                    Err(error) => {
                        ctx.renderer.line(MessageStyle::Error, &error)?;
                        return Ok(SlashCommandControl::Continue);
                    }
                }
            } else {
                all_secret_targets(ctx.vt_cfg.as_ref())
            };
            let result = migrate_env_core(&targets, ctx.config, ctx.vt_cfg.as_ref(), ctx.provider_client);
            render_operation_result(&mut ctx, Ok(result))?;
        }
    }
    Ok(SlashCommandControl::Continue)
}

pub(crate) fn resolve_secret_target(
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
            provider_name: provider.as_ref().to_owned(),
            label: provider.label().to_string(),
            provider: Some(provider),
            env_key: key_name
                .filter(|key| !key.trim().is_empty())
                .map(|key| key.trim().to_owned())
                .or_else(|| vt_cfg.and_then(|cfg| cfg.configured_api_key_env(provider.as_ref())))
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
                .map(|key| key.trim().to_owned())
                .unwrap_or_else(|| custom.resolved_api_key_env())
        },
        local: false,
        managed_auth: custom.uses_command_auth(),
    })
}

pub(crate) fn all_secret_targets(vt_cfg: Option<&VTCodeConfig>) -> Vec<SecretTarget> {
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

type SecretOperationResult = Result<InlineStatus, String>;

pub(crate) fn store_secret_core(
    target: &SecretTarget,
    value: &str,
    config: &mut AgentConfig,
    vt_cfg: Option<&VTCodeConfig>,
    client: &mut Box<dyn LLMProvider>,
) -> SecretOperationResult {
    if let Some(hint) = target.static_key_hint() {
        return Err(format!("{} • {hint}.", target.label));
    }
    let saved = format!("{} API key stored in {SECURE_STORAGE_LABEL}.", target.label);
    let verified = persist_secret(target, value, storage_mode(vt_cfg))?;
    if !verified {
        return Ok(InlineStatus::warning(format!(
            "{saved} Could not verify the stored key. The running session kept its credential."
        )));
    }

    let warning = clean_workspace_entry(&config.workspace, &target.env_key)
        .err()
        .map(|()| format!("Could not remove {} from workspace .env.", target.env_key));
    let (running, reload_warning) = reload_active_credential(target, config, vt_cfg, client);
    Ok(operation_status(saved, running, warning.into_iter().chain(reload_warning).collect()))
}

/// Returns false when the store reports success but the read-back could not verify it.
fn persist_secret(target: &SecretTarget, value: &str, mode: AuthCredentialsStoreMode) -> Result<bool, String> {
    let value = value.trim();
    if value.is_empty() || value.chars().any(char::is_control) {
        return Err(format!("Enter a single non-empty API key for {}.", target.label));
    }
    let storage = SecretStorage::new(mode);
    storage
        .store(&target.provider_name, &target.env_key, value)
        .map_err(|_storage| {
            format!("Could not store the {} API key. Check secure storage permissions.", target.label)
        })?;
    Ok(matches!(storage.load_stored(&target.provider_name, &target.env_key), Ok(Some(ref key)) if key == value))
}

fn clean_workspace_entry(workspace: &Path, env_key: &str) -> Result<(), ()> {
    let Some(value) = vtcode_config::read_workspace_env_value(workspace, env_key).map_err(|_read| ())? else {
        return Ok(());
    };
    // The workspace editor works on physical lines.
    if value.contains(['\n', '\r']) {
        return Err(());
    }
    vtcode_config::remove_workspace_env_value(workspace, env_key).map_err(|_remove| ())?;
    if vtcode_config::read_workspace_env_value(workspace, env_key)
        .map_err(|_verify| ())?
        .is_some()
    {
        return Err(());
    }
    Ok(())
}

pub(crate) fn clear_secret_core(
    target: &SecretTarget,
    confirmation: &str,
    config: &mut AgentConfig,
    vt_cfg: Option<&VTCodeConfig>,
    client: &mut Box<dyn LLMProvider>,
) -> SecretOperationResult {
    if confirmation.trim() != DELETE_CONFIRMATION {
        return Err(format!("Type `{DELETE_CONFIRMATION}` exactly to delete the {} API key.", target.label));
    }

    let mode = storage_mode(vt_cfg);
    if !target.has_stored_key(mode)? {
        return Err(format!("No stored API key found for {}.", target.label));
    }

    SecretStorage::new(mode)
        .clear(&target.provider_name, &target.env_key)
        .map_err(|_storage| format!("Could not delete the stored {} API key.", target.label))?;

    let deleted = format!("{} API key deleted from {SECURE_STORAGE_LABEL}.", target.label);
    let (running, warning) = reload_active_credential(target, config, vt_cfg, client);
    Ok(operation_status(deleted, running, warning.into_iter().collect()))
}

pub(crate) fn migrate_env_core(
    targets: &[SecretTarget],
    config: &mut AgentConfig,
    vt_cfg: Option<&VTCodeConfig>,
    client: &mut Box<dyn LLMProvider>,
) -> InlineStatus {
    let mut targets_by_env_key = BTreeMap::<&str, Vec<&SecretTarget>>::new();
    for target in targets.iter().filter(|target| target.static_key_hint().is_none()) {
        targets_by_env_key.entry(&target.env_key).or_default().push(target);
    }

    let mode = storage_mode(vt_cfg);
    let mut migrated = Vec::new();
    let mut warnings = Vec::new();
    let mut running = None;
    for (env_key, targets) in targets_by_env_key {
        let value = match vtcode_config::read_workspace_env_value(&config.workspace, env_key) {
            Ok(Some(value)) => value,
            Ok(None) => continue,
            Err(_) => {
                warnings.push(format!("Could not read {env_key} from workspace .env."));
                continue;
            }
        };

        // A shared variable remains in .env until every destination is verified.
        let mut stored = Vec::new();
        for target in &targets {
            match persist_secret(target, &value, mode) {
                Ok(true) => stored.push(*target),
                Ok(false) => warnings.push(format!("Could not verify the stored {} API key.", target.label)),
                Err(error) => warnings.push(error),
            }
        }

        if stored.is_empty() {
            continue;
        }
        if stored.len() == targets.len() && clean_workspace_entry(&config.workspace, env_key).is_ok() {
            migrated.extend(stored.iter().map(|target| target.label.as_str()));
        } else {
            let names = stored.iter().map(|target| target.label.as_str()).collect::<Vec<_>>().join(", ");
            warnings.push(format!("Stored keys for {names} in {SECURE_STORAGE_LABEL}. {env_key} remains in workspace .env or its removal could not be verified."));
        }
        for target in stored {
            let (source, warning) = reload_active_credential(target, config, vt_cfg, client);
            running = source.or(running);
            warnings.extend(warning);
        }
    }

    let message = if migrated.is_empty() && warnings.is_empty() {
        return InlineStatus::info("No workspace .env API keys to migrate.");
    } else if migrated.is_empty() {
        "Migration encountered issues.".to_string()
    } else {
        format!("Migrated {} from workspace .env to {SECURE_STORAGE_LABEL}.", migrated.join(", "))
    };
    operation_status(message, running, warnings)
}

fn operation_status(message: String, running: Option<String>, warnings: Vec<String>) -> InlineStatus {
    let has_warning = !warnings.is_empty();
    let message = std::iter::once(message)
        .chain(running)
        .chain(warnings)
        .collect::<Vec<_>>()
        .join(" ");
    if has_warning {
        InlineStatus::warning(message)
    } else {
        InlineStatus::success(message)
    }
}

fn reload_active_credential(
    target: &SecretTarget,
    config: &mut AgentConfig,
    vt_cfg: Option<&VTCodeConfig>,
    client: &mut Box<dyn LLMProvider>,
) -> (Option<String>, Option<String>) {
    if !target.provider_name.eq_ignore_ascii_case(&config.provider) {
        return (None, None);
    }

    match rebuild_active_credential(target, config, vt_cfg, client) {
        Ok(source) => (Some(format!("Running credential source is {}.", source_label(source))), None),
        Err(()) => (None, Some(format!("Restart the running session to refresh {} authentication.", target.label))),
    }
}

fn rebuild_active_credential(
    target: &SecretTarget,
    config: &mut AgentConfig,
    vt_cfg: Option<&VTCodeConfig>,
    client: &mut Box<dyn LLMProvider>,
) -> Result<CredentialSource, ()> {
    let mode = storage_mode(vt_cfg);
    let resolved = target.resolve(&config.workspace, mode).map_err(|_resolve| ())?;
    let mut source = resolved.as_ref().map(|credential| credential.source);
    let key = resolved.and_then(|credential| credential.secret);
    let mut replacement = config.clone();
    replacement.api_key_env.clone_from(&target.env_key);
    if target.is_default_openai_key() {
        let auth_config = vt_cfg.map(|cfg| cfg.auth.openai.clone()).unwrap_or_default();
        let key = if key.is_none() && auth_config.preferred_method == OpenAIPreferredMethod::ApiKey {
            let stored = SecretStorage::new(mode)
                .load_stored(&target.provider_name, &target.env_key)
                .map_err(|_load| ())?;
            source = stored.as_ref().map(|_| CredentialSource::SecureStorage);
            stored
        } else {
            key
        };
        let auth = resolve_openai_auth(&auth_config, mode, key).map_err(|_auth| ())?;
        replacement.api_key = auth.api_key().to_string();
        replacement.openai_chatgpt_auth = auth.handle();
        if replacement.openai_chatgpt_auth.is_some() {
            source = Some(CredentialSource::OAuth);
        }
    } else {
        replacement.api_key = key.ok_or(())?;
        replacement.openai_chatgpt_auth = None;
    }
    let source = source.ok_or(())?;

    let replacement_client = create_provider_client(&replacement, vt_cfg).map_err(|_build| ())?;
    *client = replacement_client;
    *config = replacement;
    Ok(source)
}

async fn prompt_for_secret(
    ctx: &mut SlashCommandContext<'_>,
    name: &str,
    key_name: Option<&str>,
    delete: bool,
) -> Result<()> {
    let target = match resolve_secret_target(ctx.vt_cfg.as_ref(), name, key_name) {
        Ok(target) => target,
        Err(error) => return ctx.renderer.line(MessageStyle::Error, &error),
    };
    if let Some(hint) = target.static_key_hint() {
        return ctx.renderer.line(MessageStyle::Info, &format!("{} • {hint}.", target.label));
    }
    if !ctx.renderer.supports_inline_ui() {
        let command = if delete { "delete" } else { "add" };
        let explicit_key = key_name.map(|key| format!(" --key-name {key}")).unwrap_or_default();
        return ctx.renderer.line(
            MessageStyle::Info,
            &format!("Use VT Code Desktop Settings → VT Code or run `vtcode secret {command} {}{explicit_key}` in a terminal.", target.provider_name),
        );
    }
    if !super::handlers::ui::ensure_selection_ui_available(ctx, "managing secrets")? {
        return Ok(());
    }
    if delete {
        match target.has_stored_key(storage_mode(ctx.vt_cfg.as_ref())) {
            Ok(true) => {}
            Ok(false) => {
                return ctx
                    .renderer
                    .line(MessageStyle::Info, &format!("No stored API key found for {}.", target.label));
            }
            Err(error) => return ctx.renderer.line(MessageStyle::Error, &error),
        }
    }

    let (title, question, label) = if delete {
        ("Delete API key", target.delete_question(), "Confirmation")
    } else {
        ("Add or replace a key", target.add_question(), "API key")
    };
    ctx.renderer.show_secure_prompt_modal(title, vec![question], label.to_string());
    let Some(value) = wait_for_secure_prompt_input(ctx).await else {
        return ctx.renderer.line(MessageStyle::Info, "Secret operation cancelled.");
    };

    let result = if delete {
        clear_secret_core(&target, &value, ctx.config, ctx.vt_cfg.as_ref(), ctx.provider_client)
    } else {
        store_secret_core(&target, &value, ctx.config, ctx.vt_cfg.as_ref(), ctx.provider_client)
    };
    render_operation_result(ctx, result)
}

fn render_operation_result(ctx: &mut SlashCommandContext<'_>, result: SecretOperationResult) -> Result<()> {
    match result {
        Ok(status) => {
            let style = if status.tone == InlineTone::Warning {
                MessageStyle::Warning
            } else {
                MessageStyle::Info
            };
            ctx.renderer.line(style, &status.message)
        }
        Err(error) => ctx.renderer.line(MessageStyle::Error, &error),
    }
}

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

pub(super) fn render_secret_status_table(
    ctx: &mut SlashCommandContext<'_>,
    filter: Option<&str>,
    key_name: Option<&str>,
) -> Result<()> {
    let targets = match filter {
        Some(name) => match resolve_secret_target(ctx.vt_cfg.as_ref(), name, key_name) {
            Ok(target) => vec![target],
            Err(error) => return ctx.renderer.line(MessageStyle::Error, &error),
        },
        None => all_secret_targets(ctx.vt_cfg.as_ref()),
    };

    ctx.renderer.line(MessageStyle::Info, "API Key Status")?;
    let mode = storage_mode(ctx.vt_cfg.as_ref());
    for target in targets {
        let (status, source) = target.status(&ctx.config.workspace, mode);
        ctx.renderer
            .line(MessageStyle::Output, &format!("  {} ({})", target.label, target.provider_name))?;
        ctx.renderer.line(MessageStyle::Output, &format!("    Status: {status}"))?;
        ctx.renderer.line(MessageStyle::Output, &format!("    Source: {source}"))?;
        if !target.env_key.is_empty() {
            ctx.renderer
                .line(MessageStyle::Output, &format!("    Env var: {}", target.env_key))?;
        }
        if let Some(url) = target.provider.and_then(|provider| provider.platform_url()) {
            ctx.renderer.line(MessageStyle::Output, &format!("    Platform: {url}"))?;
        }
    }

    let guidance = if ctx.renderer.supports_inline_ui() {
        "Use Settings → Secrets (/secret) to manage keys."
    } else {
        "Use VT Code Desktop Settings → VT Code or run `vtcode secret` in a terminal to manage keys."
    };
    ctx.renderer.line(MessageStyle::Info, guidance)
}
