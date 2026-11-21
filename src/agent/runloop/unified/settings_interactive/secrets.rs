use std::borrow::Cow;

use vtcode_auth::{AuthCredentialsStoreMode, OpenAIPreferredMethod};
use vtcode_commons::ui_protocol::{InlineStatus, InlineTone};
use vtcode_config::VTCodeConfig;
use vtcode_core::{config::types::AgentConfig, llm::provider::LLMProvider};
use vtcode_ui::tui::app::{InlineListItem, InlineListSelection, WizardModalMode, WizardOverlayRequest, WizardStep};

use super::{ACTION_PREFIX_OPEN, SettingsPaletteState};
use crate::agent::runloop::ui_list;
use crate::agent::runloop::unified::turn::session::slash_commands::secrets::{
    DELETE_CONFIRMATION, SecretTarget, all_secret_targets, clear_secret_core, migrate_env_core, storage_mode,
    store_secret_core,
};

pub(crate) const SECRETS_VIEW: &str = "secrets";
const ADD_VIEW: &str = "secrets/add";
const DELETE_VIEW: &str = "secrets/delete";
const MIGRATE_ACTION: &str = "settings:secrets:migrate";
const PROVIDER_ACTION_PREFIX: &str = "settings:secrets:provider:";
const ANSWER_ID: &str = "settings_secret";

/// Credential metadata from the session's effective config, including startup overrides.
#[derive(Clone)]
pub(crate) struct SecretPaletteConfig {
    mode: AuthCredentialsStoreMode,
    openai_method: OpenAIPreferredMethod,
    targets: Vec<SecretTarget>,
}

impl From<&VTCodeConfig> for SecretPaletteConfig {
    fn from(config: &VTCodeConfig) -> Self {
        Self {
            mode: storage_mode(Some(config)),
            openai_method: config.auth.openai.preferred_method,
            targets: all_secret_targets(Some(config)),
        }
    }
}

fn secret_config(state: &SettingsPaletteState) -> Cow<'_, SecretPaletteConfig> {
    match state.secret_config.as_ref() {
        Some(config) => Cow::Borrowed(config),
        None => Cow::Owned(SecretPaletteConfig::from(&state.draft)),
    }
}

#[derive(Clone)]
pub(crate) enum PendingSecretOperation {
    Add(SecretTarget),
    Delete(SecretTarget),
}

impl PendingSecretOperation {
    pub(crate) fn wizard(&self) -> WizardOverlayRequest {
        let (title, question, label, placeholder) = match self {
            Self::Add(target) => ("Add or replace a key", target.add_question(), "API key", "Paste your key"),
            Self::Delete(target) => ("Delete a key", target.delete_question(), "Confirmation", DELETE_CONFIRMATION),
        };

        WizardOverlayRequest {
            title: title.to_owned(),
            steps: vec![WizardStep {
                multi_select: false,
                title: label.to_owned(),
                question,
                items: vec![ui_list::choice(
                    label,
                    None,
                    Some(InlineListSelection::RequestUserInputAnswer {
                        question_id: ANSWER_ID.to_owned(),
                        selected: Vec::new(),
                        other: Some(String::new()),
                    }),
                )],
                completed: false,
                answer: None,
                allow_freeform: true,
                freeform_label: Some(label.to_owned()),
                freeform_placeholder: Some(placeholder.to_owned()),
                mask_input: true,
                freeform_default: Some(String::new()),
            }],
            current_step: 0,
            search: None,
            mode: WizardModalMode::TabbedList,
        }
    }
}

pub(super) fn parent_view(path: &str) -> Option<&'static str> {
    matches!(path, ADD_VIEW | DELETE_VIEW).then_some(SECRETS_VIEW)
}

pub(crate) fn view_title(path: &str) -> Option<&'static str> {
    match path {
        SECRETS_VIEW => Some("Secrets"),
        ADD_VIEW => Some("Secrets › Add or replace a key"),
        DELETE_VIEW => Some("Secrets › Delete a key"),
        _ => None,
    }
}

pub(super) fn root_item() -> InlineListItem {
    action("Secrets", "Manage provider API keys", format!("{ACTION_PREFIX_OPEN}{SECRETS_VIEW}"))
}

pub(super) fn append_items(state: &SettingsPaletteState, view: &str, items: &mut Vec<InlineListItem>) -> bool {
    if view_title(view).is_none() {
        return false;
    }

    let config = secret_config(state);
    let mode = config.mode;
    let targets = &config.targets;

    if view == SECRETS_VIEW {
        items.extend([
            action("Add or replace a key…", "Store a provider API key", format!("{ACTION_PREFIX_OPEN}{ADD_VIEW}")),
            action("Delete a key…", "Remove a key from secure storage", format!("{ACTION_PREFIX_OPEN}{DELETE_VIEW}")),
            action("Migrate .env keys", "Move workspace API keys to secure storage", MIGRATE_ACTION.to_owned()),
        ]);
        items.extend(targets.iter().map(|target| {
            let (status, source) = target.status(&state.workspace, mode);
            hint(target, format!("{status} • {source}"))
        }));
    } else if view == ADD_VIEW {
        items.extend(
            targets
                .iter()
                .map(|target| match target.settings_add_hint(config.openai_method) {
                    Some(detail) => hint(target, detail.to_owned()),
                    None => provider_item(target),
                }),
        );
    } else {
        for target in targets.iter().filter(|target| !target.env_key.is_empty()) {
            match target.has_stored_key(mode) {
                Ok(true) => items.push(provider_item(target)),
                Ok(false) => {}
                Err(error) => items.push(hint(target, error)),
            }
        }
        if !items.iter().any(|item| {
            matches!(&item.selection, Some(InlineListSelection::ConfigAction(action)) if action.starts_with(PROVIDER_ACTION_PREFIX))
        }) {
            items.push(InlineListItem { title: "No stored keys available to delete".to_owned(), ..Default::default() });
        }
    }
    true
}

fn action(title: &str, subtitle: &str, action: String) -> InlineListItem {
    ui_list::action(
        title,
        subtitle.to_owned(),
        None,
        InlineTone::Neutral,
        Some(InlineListSelection::ConfigAction(action)),
    )
}

fn provider_item(target: &SecretTarget) -> InlineListItem {
    action(&target.label, &target.env_key, format!("{PROVIDER_ACTION_PREFIX}{}", target.provider_name))
        .with_search_value(format!("{} {} {}", target.label, target.provider_name, target.env_key))
}

fn hint(target: &SecretTarget, detail: String) -> InlineListItem {
    InlineListItem {
        title: target.label.clone(),
        search_value: Some(format!("{} {} {} {detail}", target.label, target.provider_name, target.env_key)),
        subtitle: Some(detail),
        ..Default::default()
    }
}

pub(crate) enum SecretPaletteAction {
    Prompt(WizardOverlayRequest),
    Refresh,
}

pub(crate) fn handle_action(
    state: &mut SettingsPaletteState,
    action: &str,
    config: &mut AgentConfig,
    runtime_cfg: Option<&VTCodeConfig>,
    client: &mut Box<dyn LLMProvider>,
) -> Option<SecretPaletteAction> {
    if action == MIGRATE_ACTION && state.view_path.as_deref() == Some(SECRETS_VIEW) {
        let secrets = secret_config(state);
        let result = migrate_env_core(&secrets.targets, config, runtime_cfg.or(Some(&state.draft)), client);
        state.status = Some(result);
        return Some(SecretPaletteAction::Refresh);
    }

    let provider = action.strip_prefix(PROVIDER_ACTION_PREFIX)?;
    let view = state.view_path.as_deref()?;
    if !matches!(view, ADD_VIEW | DELETE_VIEW) {
        return None;
    }

    let secrets = secret_config(state);
    let pending = secrets
        .targets
        .iter()
        .find(|target| target.provider_name == provider)
        .ok_or_else(|| "This provider is no longer configured.".to_owned())
        .and_then(|target| {
            if view == ADD_VIEW {
                match target.settings_add_hint(secrets.openai_method) {
                    Some(hint) => Err(format!("{} • {hint}.", target.label)),
                    None => Ok(PendingSecretOperation::Add(target.clone())),
                }
            } else if !target.env_key.is_empty() && target.has_stored_key(secrets.mode)? {
                Ok(PendingSecretOperation::Delete(target.clone()))
            } else {
                Err(format!("No stored API key found for {}.", target.label))
            }
        });

    match pending {
        Ok(pending) => {
            state.status = None;
            let wizard = pending.wizard();
            state.pending_secret = Some(pending);
            Some(SecretPaletteAction::Prompt(wizard))
        }
        Err(message) => {
            state.status = Some(InlineStatus::error(message));
            Some(SecretPaletteAction::Refresh)
        }
    }
}

pub(crate) fn finish(
    state: &mut SettingsPaletteState,
    pending: PendingSecretOperation,
    selections: Vec<InlineListSelection>,
    config: &mut AgentConfig,
    runtime_cfg: Option<&VTCodeConfig>,
    client: &mut Box<dyn LLMProvider>,
) {
    let answer = selections.into_iter().find_map(|selection| match selection {
        InlineListSelection::RequestUserInputAnswer { question_id, selected, other }
            if question_id == ANSWER_ID && selected.is_empty() =>
        {
            other
        }
        _ => None,
    });
    let Some(answer) = answer else {
        state.status = Some(InlineStatus::info("Secret operation cancelled."));
        return;
    };

    let vt_cfg = runtime_cfg.or(Some(&state.draft));
    let result = match pending {
        PendingSecretOperation::Add(target) => store_secret_core(&target, &answer, config, vt_cfg, client),
        PendingSecretOperation::Delete(target) => clear_secret_core(&target, &answer, config, vt_cfg, client),
    };
    state.status = Some(match result {
        Ok(status) => {
            state.view_path = Some(SECRETS_VIEW.to_owned());
            status
        }
        Err(message) => InlineStatus::error(message),
    });
}
