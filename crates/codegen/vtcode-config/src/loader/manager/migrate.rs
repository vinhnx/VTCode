use super::*;

pub(super) fn migrate_custom_api_keys_if_needed(config: &mut VTCodeConfig) -> Result<()> {
    let storage_mode = config.agent.credential_storage_mode;

    // Check if there are any non-empty API keys in the config
    let has_plain_text_keys = config.agent.custom_api_keys.values().any(|key| !key.is_empty());

    if has_plain_text_keys {
        tracing::info!("Detected plain-text API keys in config, migrating to secure storage...");

        let mut migrated_count = 0;
        let pending = config
            .agent
            .custom_api_keys
            .iter()
            .filter(|(_, key)| !key.is_empty())
            .map(|(provider, key)| (provider.clone(), key.clone()))
            .collect::<Vec<_>>();
        for (provider, api_key) in pending {
            let key_name = config
                .configured_api_key_env(&provider)
                .unwrap_or_else(|| api_key_env_var(&provider));
            if let Some(identity) = store_credential_with_mode(&provider, &key_name, &api_key, storage_mode)? {
                config.agent.custom_api_keys.remove(&provider);
                if let Some(metadata_key) = credential_metadata_key(identity.provider(), identity.key_name())? {
                    config.agent.custom_api_keys.insert(metadata_key, String::new());
                }
                migrated_count += 1;
            }
        }

        if migrated_count > 0 {
            tracing::info!("Successfully migrated {} API key(s) to secure storage", migrated_count);
            tracing::warn!(
                "Plain-text API keys have been cleared from config file. \
                 Please commit the updated config to remove sensitive data from version control."
            );
        }
    }

    Ok(())
}
