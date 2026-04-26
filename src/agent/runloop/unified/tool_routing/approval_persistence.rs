use anyhow::{Context, Result};
use serde_json::Value;
use vtcode_core::tools::registry::ToolRegistry;

use super::permission_prompt::{
    extract_shell_approval_command_prefix_words, extract_shell_approval_scope_signature,
    render_shell_persistent_approval_prefix_entry,
};

const SHELL_APPROVAL_SCOPE_MARKER: &str = "|sandbox_permissions=";
const DEFAULT_SHELL_APPROVAL_SCOPE_SIGNATURE: &str =
    "sandbox_permissions=\"use_default\"|additional_permissions=null";

fn shell_command_words_match_prefix(command_words: &[String], prefix_words: &[String]) -> bool {
    command_words.len() >= prefix_words.len()
        && prefix_words
            .iter()
            .zip(command_words.iter())
            .all(|(prefix, command)| prefix == command)
}

fn split_persisted_shell_approval_prefix(entry: &str) -> (&str, Option<&str>) {
    if let Some(index) = entry.find(SHELL_APPROVAL_SCOPE_MARKER) {
        let (prefix, scoped) = entry.split_at(index);
        (prefix, Some(&scoped[1..]))
    } else {
        (entry, None)
    }
}

pub(super) fn shell_command_has_persisted_approval_prefix(
    tool_registry: &ToolRegistry,
    command_words: &[String],
    scope_signature: &str,
) -> bool {
    if command_words.is_empty() {
        return false;
    }

    tool_registry
        .commands_config()
        .approval_prefixes
        .iter()
        .map(|entry| entry.trim())
        .filter(|entry| !entry.is_empty())
        .any(|entry| {
            let (prefix_text, entry_scope_signature) = split_persisted_shell_approval_prefix(entry);
            let prefix_words = shell_words::split(prefix_text).ok();
            let scope_matches = entry_scope_signature
                .unwrap_or(DEFAULT_SHELL_APPROVAL_SCOPE_SIGNATURE)
                == scope_signature;

            scope_matches
                && prefix_words
                    .as_deref()
                    .is_some_and(|prefix| shell_command_words_match_prefix(command_words, prefix))
        })
}

pub(super) async fn persisted_shell_approval(
    tool_registry: &ToolRegistry,
    normalized_tool_name: &str,
    tool_args: Option<&Value>,
) -> Option<(Vec<String>, String)> {
    let (command_words, scope_signature) =
        extract_shell_approval_command_prefix_words(normalized_tool_name, tool_args).zip(
            extract_shell_approval_scope_signature(normalized_tool_name, tool_args),
        )?;

    if tool_registry
        .find_persisted_shell_approval_prefix(&command_words, &scope_signature)
        .await
        .is_some()
        || shell_command_has_persisted_approval_prefix(
            tool_registry,
            &command_words,
            &scope_signature,
        )
    {
        Some((command_words, scope_signature))
    } else {
        None
    }
}

pub(super) async fn persist_shell_approval_prefix_rule(
    tool_registry: &ToolRegistry,
    tool_name: &str,
    tool_args: Option<&Value>,
    prefix_rule: &[String],
) -> Result<String> {
    let rendered_rule =
        render_shell_persistent_approval_prefix_entry(tool_name, tool_args, prefix_rule)
            .context("Failed to render shell approval prefix entry")?;
    tool_registry
        .persist_approval_cache_prefix(&rendered_rule)
        .await
        .context("Failed to persist shell approval prefix to tool policy")?;
    let workspace_root = tool_registry.workspace_root().clone();
    let mut manager = crate::main_helpers::load_workspace_config(&workspace_root)?;
    let mut config = manager.config().clone();

    if !config
        .commands
        .approval_prefixes
        .iter()
        .any(|existing| existing == &rendered_rule)
    {
        config
            .commands
            .approval_prefixes
            .push(rendered_rule.clone());
        manager
            .save_config(&config)
            .context("Failed to persist shell approval prefix")?;
    }

    tool_registry.apply_commands_config(&config.commands);
    Ok(rendered_rule)
}
