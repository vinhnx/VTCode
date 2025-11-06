use std::io::{BufWriter, Write};
use std::path::Path;

use anyhow::{Context, Result, anyhow};
use tempfile::Builder;

use vtcode_core::config::api_keys::{ApiKeySources, get_api_key};
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::llm::factory::create_provider_with_config;
use vtcode_core::llm::provider::LLMProvider;
use vtcode_core::llm::rig_adapter::{reasoning_parameters_for, verify_model_with_rig};
use vtcode_core::ui::tui::InlineHandle;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};

use crate::agent::runloop::model_picker::{ModelPickerState, ModelSelectionResult};
use crate::agent::runloop::welcome::SessionBootstrap;

use super::curator::resolve_mode_label;
use crate::agent::runloop::ui::build_inline_header_context;

#[allow(clippy::too_many_arguments)]
pub(crate) async fn finalize_model_selection(
    renderer: &mut AnsiRenderer,
    picker: &ModelPickerState,
    selection: ModelSelectionResult,
    config: &mut CoreAgentConfig,
    vt_cfg: &mut Option<VTCodeConfig>,
    provider_client: &mut Box<dyn LLMProvider>,
    session_bootstrap: &SessionBootstrap,
    handle: &InlineHandle,
    full_auto: bool,
) -> Result<()> {
    let workspace = config.workspace.clone();

    let api_key = if let Some(key) = selection.api_key.as_ref() {
        persist_env_value(&workspace, &selection.env_key, key).await?;
        unsafe {
            // SAFETY: we only write ASCII-alphanumeric keys derived from known providers or
            // sanitized user input, and values are supplied directly by the user.
            std::env::set_var(&selection.env_key, key);
        }
        key.clone()
    } else if selection.provider_enum.is_some() {
        let key = get_api_key(&selection.provider, &ApiKeySources::default())
            .with_context(|| format!("API key not found for provider '{}'", selection.provider))?;
        unsafe {
            // SAFETY: see above. Keys are sanitized and values come from configuration sources.
            std::env::set_var(&selection.env_key, &key);
        }
        key
    } else {
        match std::env::var(&selection.env_key) {
            Ok(value) if !value.trim().is_empty() => value,
            _ if selection.requires_api_key => {
                return Err(anyhow!(
                    "API key not found for provider '{}'. Set {} or enter a key to continue.",
                    selection.provider,
                    selection.env_key
                ));
            }
            _ => String::new(),
        }
    };

    if let Some(provider_enum) = selection.provider_enum
        && let Err(err) = verify_model_with_rig(provider_enum, &selection.model, &api_key)
    {
        renderer.line(
            MessageStyle::Error,
            &format!(
                "Rig validation warning: unable to initialise {} via rig-core ({err}).",
                selection.model_display
            ),
        )?;
    }

    let updated_cfg = picker.persist_selection(&workspace, &selection).await?;
    *vt_cfg = Some(updated_cfg);

    if let Some(provider_enum) = selection.provider_enum {
        let provider_name = selection.provider.clone();
        let new_client = create_provider_with_config(
            &provider_name,
            Some(api_key.clone()),
            None,
            Some(selection.model.clone()),
            Some(config.prompt_cache.clone()),
        )
        .context("Failed to initialize provider for the selected model")?;
        *provider_client = new_client;
        config.provider = provider_enum.to_string();
    } else {
        renderer.line(
            MessageStyle::Info,
            "Saved selection, but custom providers require manual configuration before taking effect.",
        )?;
        config.provider = selection.provider.clone();
    }

    config.model = selection.model.clone();
    config.api_key = api_key;
    config.reasoning_effort = selection.reasoning;
    config.api_key_env = selection.env_key.clone();
    if let Some(ref key) = selection.api_key {
        let key_value: String = key.clone();
        config
            .custom_api_keys
            .insert(selection.provider.clone(), key_value);
    } else {
        config.custom_api_keys.remove(&selection.provider);
    }

    if let Some(provider_enum) = selection.provider_enum
        && selection.reasoning_supported
        && let Some(payload) = reasoning_parameters_for(provider_enum, selection.reasoning)
    {
        renderer.line(
            MessageStyle::Info,
            &format!("Rig reasoning configuration prepared: {}", payload),
        )?;
    }

    let reasoning_label = selection.reasoning.as_str().to_string();
    let mode_label = resolve_mode_label(config.ui_surface, full_auto);
    let header_context = build_inline_header_context(
        config,
        session_bootstrap,
        selection.provider_label.clone(),
        selection.model.clone(),
        mode_label,
        reasoning_label.clone(),
    )
    .await?;
    handle.set_header_context(header_context);

    renderer.line(
        MessageStyle::Info,
        &format!(
            "Model set to {} ({}) via {}.",
            selection.model_display, selection.model, selection.provider_label
        ),
    )?;

    if !selection.known_model {
        renderer.line(
            MessageStyle::Info,
            "The selected model is not part of VTCode's curated list; capabilities may vary.",
        )?;
    }

    if selection.reasoning_supported {
        let message = if selection.reasoning_changed {
            format!("Reasoning effort updated to '{}'.", selection.reasoning)
        } else {
            format!("Reasoning effort remains '{}'.", selection.reasoning)
        };
        renderer.line(MessageStyle::Info, &message)?;
    }

    if selection.api_key.is_some() {
        renderer.line(
            MessageStyle::Info,
            &format!(
                "Stored credential under {} and updated the active environment.",
                selection.env_key
            ),
        )?;
    } else if selection.requires_api_key {
        renderer.line(
            MessageStyle::Info,
            &format!(
                "Using environment variable {} for authentication.",
                selection.env_key
            ),
        )?;
    }

    Ok(())
}

async fn persist_env_value(workspace: &Path, key: &str, value: &str) -> Result<()> {
    let env_path = workspace.join(".env");
    let mut lines: Vec<String> = if env_path.exists() {
        tokio::fs::read_to_string(&env_path)
            .await
            .with_context(|| format!("Failed to read {}", env_path.display()))?
            .lines()
            .map(|line| line.to_string())
            .collect()
    } else {
        Vec::new()
    };

    let mut replaced = false;
    for line in lines.iter_mut() {
        if let Some((existing_key, _)) = line.split_once('=')
            && existing_key.trim() == key
        {
            *line = format!("{key}={value}");
            replaced = true;
        }
    }

    if !replaced {
        lines.push(format!("{key}={value}"));
    }

    let parent = env_path
        .parent()
        .map(std::path::Path::to_path_buf)
        .unwrap_or_else(|| workspace.to_path_buf());

    if !parent.exists() {
        tokio::fs::create_dir_all(&parent)
            .await
            .with_context(|| format!("Failed to create directory {}", parent.display()))?;
    }

    let temp = Builder::new()
        .prefix(".env.")
        .suffix(".tmp")
        .tempfile_in(&parent)
        .with_context(|| format!("Failed to create temporary file in {}", parent.display()))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let permissions = std::fs::Permissions::from_mode(0o600);
        temp.as_file()
            .set_permissions(permissions)
            .with_context(|| format!("Failed to set permissions on {}", temp.path().display()))?;
    }

    {
        let mut writer = BufWriter::new(temp.as_file());
        for line in &lines {
            writeln!(writer, "{line}")
                .with_context(|| format!("Failed to write .env entry for {key}"))?;
        }
        writer
            .flush()
            .with_context(|| format!("Failed to flush temporary .env for {}", key))?;
    }

    temp.as_file()
        .sync_all()
        .with_context(|| format!("Failed to sync temporary .env for {}", key))?;

    let _file = temp
        .persist(&env_path)
        .with_context(|| format!("Failed to persist {}", env_path.display()))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        tokio::fs::set_permissions(&env_path, std::fs::Permissions::from_mode(0o600))
            .await
            .with_context(|| format!("Failed to set permissions on {}", env_path.display()))?;
    }

    Ok(())
}
