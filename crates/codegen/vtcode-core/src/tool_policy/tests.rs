use super::shell_approval::derived_shell_approval_prefixes;
use super::*;
use crate::config::constants::tools;
use crate::config::core::tools::ToolsConfig;
use hashbrown::{HashMap, HashSet};
use tempfile::tempdir;

#[test]
fn test_tool_policy_config_serialization() {
    let mut config = ToolPolicyConfig {
        available_tools: vec![tools::READ_FILE.to_owned(), tools::WRITE_FILE.to_owned()],
        ..Default::default()
    };
    config.policies.insert(tools::READ_FILE.to_owned(), ToolPolicy::Allow);
    config.policies.insert(tools::WRITE_FILE.to_owned(), ToolPolicy::Prompt);
    config.approval_cache.allowed.insert("command_session:cargo test".to_string());

    let json = serde_json::to_string_pretty(&config).unwrap();
    let deserialized: ToolPolicyConfig = serde_json::from_str(&json).unwrap();

    assert_eq!(config.available_tools, deserialized.available_tools);
    assert_eq!(config.policies, deserialized.policies);
    assert_eq!(config.approval_cache, deserialized.approval_cache);
}

#[test]
fn code_search_policy_uses_default_and_explicit_override() {
    let mut tools_config = ToolsConfig {
        default_policy: ToolPolicy::Prompt,
        ..Default::default()
    };

    assert_eq!(ToolPolicyManager::resolve_config_policy(&tools_config, tools::CODE_SEARCH), ToolPolicy::Allow);

    tools_config.policies.insert(tools::CODE_SEARCH.to_string(), ToolPolicy::Deny);
    assert_eq!(ToolPolicyManager::resolve_config_policy(&tools_config, tools::CODE_SEARCH), ToolPolicy::Deny);
}

#[tokio::test]
async fn test_policy_updates() {
    let dir = tempdir().unwrap();
    let config_path = dir.path().join("tool-policy.json");

    let mut config = ToolPolicyConfig {
        available_tools: vec!["tool1".to_owned()],
        ..Default::default()
    };
    config.policies.insert("tool1".to_owned(), ToolPolicy::Prompt);

    // Save initial config
    let content = serde_json::to_string_pretty(&config).unwrap();
    std::fs::write(&config_path, content).unwrap();

    // Load and update
    let mut loaded_config = ToolPolicyManager::load_or_create_config_with_policy(&config_path, false)
        .await
        .unwrap();

    // Add new tool
    let new_tools = vec!["tool1".to_owned(), "tool2".to_owned()];
    let current_tools: HashSet<_> = loaded_config.available_tools.iter().cloned().collect();

    for tool in &new_tools {
        if !current_tools.contains(tool) {
            loaded_config.policies.insert(tool.clone(), ToolPolicy::Prompt);
        }
    }

    loaded_config.available_tools = new_tools;

    assert!(loaded_config.policies.len() >= 2);
    assert_eq!(loaded_config.policies.get("tool2"), Some(&ToolPolicy::Prompt));
    assert_eq!(loaded_config.policies.get("tool1"), Some(&ToolPolicy::Prompt));
}

#[test]
fn workspace_policy_persistence_does_not_copy_trusted_provider_settings() {
    let dir = tempdir().expect("temp dir");
    let workspace = dir.path().join("workspace");
    std::fs::create_dir_all(&workspace).expect("workspace directory");
    let workspace_config = workspace.join("vtcode.toml");
    std::fs::write(
        &workspace_config,
        r#"
[[custom_providers]]
name = "stale-provider"
display_name = "Stale Provider"
base_url = "https://stale.example/v1"
model = "stale-model"

[provider_overrides.openai]
models = ["stale-model"]
base_url = "https://stale.example/v1"
api_key_env = "STALE_API_KEY"
"#,
    )
    .expect("write workspace config");

    let manager = ToolPolicyManager {
        config_path: dir.path().join("tool-policy.json"),
        config: ToolPolicyConfig::default(),
        permission_handler: None,
        workspace_root: Some(workspace),
        private_storage: false,
    };

    manager
        .persist_policy_to_workspace_config("start_planning", ToolPolicy::Deny)
        .expect("persist workspace policy");

    let saved: toml::Value = toml::from_str(&std::fs::read_to_string(workspace_config).expect("read workspace config"))
        .expect("parse workspace config");
    assert!(saved.get("custom_providers").is_none());
    assert!(
        saved
            .get("provider_overrides")
            .and_then(|value| value.get("openai"))
            .and_then(|value| value.get("base_url"))
            .is_none()
    );
    assert_eq!(
        saved
            .get("tools")
            .and_then(|value| value.get("policies"))
            .and_then(|value| value.get("start_planning"))
            .and_then(toml::Value::as_str),
        Some("deny")
    );
}

#[tokio::test]
async fn approval_cache_keys_round_trip() {
    let dir = tempdir().unwrap();
    let config_path = dir.path().join("tool-policy.json");
    let mut manager = ToolPolicyManager::new_with_config_path(&config_path).await.expect("manager");

    manager
        .add_approval_cache_key("cargo test|sandbox_permissions=\"use_default\"|additional_permissions=null")
        .await
        .expect("persist approval");

    let reloaded = ToolPolicyManager::new_with_config_path(&config_path)
        .await
        .expect("reload manager");
    assert!(
        reloaded.has_approval_cache_key("cargo test|sandbox_permissions=\"use_default\"|additional_permissions=null")
    );
}

#[test]
fn approval_cache_trim_drops_oldest_entries() {
    let mut cache = ApprovalCacheConfig::default();
    for index in 0..(APPROVAL_CACHE_MAX_ENTRIES + 10) {
        cache.allowed.insert(format!("key-{index:04}"));
    }
    assert!(cache.trim_to_budget());
    assert_eq!(cache.allowed.len(), APPROVAL_CACHE_MAX_ENTRIES);
    // Oldest entries are dropped first; the most recent survive.
    assert!(!cache.allowed.contains("key-0000"));
    assert!(cache.allowed.contains(&format!("key-{:04}", APPROVAL_CACHE_MAX_ENTRIES + 9)));
    // Idempotent when already within budget.
    assert!(!cache.trim_to_budget());
}

#[tokio::test]
async fn approval_cache_persist_trims_to_budget() {
    let dir = tempdir().unwrap();
    let config_path = dir.path().join("tool-policy.json");
    let mut manager = ToolPolicyManager::new_with_config_path(&config_path).await.expect("manager");

    for index in 0..(APPROVAL_CACHE_MAX_ENTRIES + 5) {
        manager
            .add_approval_cache_key(format!("persist-key-{index:04}"))
            .await
            .expect("persist approval");
    }

    let reloaded = ToolPolicyManager::new_with_config_path(&config_path)
        .await
        .expect("reload manager");
    // Oldest keys were trimmed; the most recent survive.
    assert!(!reloaded.has_approval_cache_key("persist-key-0000"), "oldest approval keys must be trimmed");
    assert!(
        reloaded.has_approval_cache_key(&format!("persist-key-{:04}", APPROVAL_CACHE_MAX_ENTRIES + 4)),
        "newest approval keys must survive the trim"
    );
}

#[tokio::test]
async fn approval_cache_prefixes_match_shell_prefixes() {
    let dir = tempdir().unwrap();
    let config_path = dir.path().join("tool-policy.json");
    let mut manager = ToolPolicyManager::new_with_config_path(&config_path).await.expect("manager");

    manager
        .add_approval_cache_prefix("cargo test|sandbox_permissions=\"use_default\"|additional_permissions=null")
        .await
        .expect("persist prefix");

    let reloaded = ToolPolicyManager::new_with_config_path(&config_path)
        .await
        .expect("reload manager");
    let command_words = vec![
        "cargo".to_string(),
        "test".to_string(),
        "-p".to_string(),
        "vtcode-core".to_string(),
    ];

    assert!(
        reloaded
            .matching_shell_approval_prefix(
                &command_words,
                "sandbox_permissions=\"use_default\"|additional_permissions=null",
            )
            .is_some()
    );
}

#[tokio::test]
async fn approval_cache_key_persists_shell_token_prefixes_with_scope() {
    let dir = tempdir().unwrap();
    let config_path = dir.path().join("tool-policy.json");
    let mut manager = ToolPolicyManager::new_with_config_path(&config_path).await.expect("manager");

    manager
            .add_approval_cache_key_with_segments(
                "sed -n 87,140p crates/codegen/vtcode-core/src/core/agent/features.rs|sandbox_permissions=\"use_default\"|additional_permissions=null",
            )
            .await
            .expect("persist key");

    let reloaded = ToolPolicyManager::new_with_config_path(&config_path)
        .await
        .expect("reload manager");
    let command_words = vec![
        "sed".to_string(),
        "-n".to_string(),
        "109,250p".to_string(),
        "crates/codegen/vtcode-core/src/tools/tool_intent.rs".to_string(),
    ];

    assert!(
        reloaded
            .matching_shell_approval_prefix(
                &command_words,
                "sandbox_permissions=\"use_default\"|additional_permissions=null",
            )
            .is_some()
    );
    assert!(reloaded.has_approval_cache_key(
            "sed -n 109,250p crates/codegen/vtcode-core/src/tools/tool_intent.rs|sandbox_permissions=\"use_default\"|additional_permissions=null"
        ));
}

#[tokio::test]
async fn approval_cache_path_command_options_also_persist_base_family() {
    let dir = tempdir().unwrap();
    let config_path = dir.path().join("tool-policy.json");
    let mut manager = ToolPolicyManager::new_with_config_path(&config_path).await.expect("manager");

    manager
        .add_approval_cache_key_with_segments(
            "ls -la /Users/me/project|sandbox_permissions=\"use_default\"|additional_permissions=null",
        )
        .await
        .expect("persist key");

    assert!(manager.has_approval_cache_key(
        "ls /Users/me/project/docs|sandbox_permissions=\"use_default\"|additional_permissions=null"
    ));
}

#[tokio::test]
async fn opaque_family_keys_do_not_derive_shell_prefixes() {
    let dir = tempdir().unwrap();
    let config_path = dir.path().join("tool-policy.json");
    let mut manager = ToolPolicyManager::new_with_config_path(&config_path).await.expect("manager");

    manager
        .add_approval_cache_key_with_segments(
            "shell-pattern:awk README.md|sandbox_permissions=\"use_default\"|additional_permissions=null",
        )
        .await
        .expect("persist family key");

    // The exact family matches...
    assert!(manager.has_approval_cache_key(
        "shell-pattern:awk README.md|sandbox_permissions=\"use_default\"|additional_permissions=null"
    ));
    // ...but a different family must not inherit a derived `shell-pattern:awk` prefix.
    assert!(!manager.has_approval_cache_key(
        "shell-pattern:awk src|sandbox_permissions=\"use_default\"|additional_permissions=null"
    ));
}

#[tokio::test]
async fn approval_cache_key_does_not_cross_permission_scope() {
    let dir = tempdir().unwrap();
    let config_path = dir.path().join("tool-policy.json");
    let mut manager = ToolPolicyManager::new_with_config_path(&config_path).await.expect("manager");

    manager
            .add_approval_cache_key_with_segments(
                "sed -n 87,140p crates/codegen/vtcode-core/src/core/agent/features.rs|sandbox_permissions=\"use_default\"|additional_permissions=null",
            )
            .await
            .expect("persist key");

    assert!(!manager.has_approval_cache_key(
            "sed -n 109,250p crates/codegen/vtcode-core/src/tools/tool_intent.rs|sandbox_permissions=\"require_escalated\"|additional_permissions=null"
        ));
}

#[test]
fn approval_prefix_derivation_uses_bash_parser_for_quoted_args() {
    let prefixes = derived_shell_approval_prefixes(
        "sed -n '87,140p' crates/codegen/vtcode-core/src/core/agent/features.rs|sandbox_permissions=\"use_default\"|additional_permissions=null",
    );

    assert!(
        prefixes
            .iter()
            .any(|prefix| { prefix == "sed -n|sandbox_permissions=\"use_default\"|additional_permissions=null" })
    );
}

#[test]
fn approval_prefix_derivation_handles_absolute_ls_path() {
    let prefixes = derived_shell_approval_prefixes(
        "ls /Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/.claude/|sandbox_permissions=\"use_default\"|additional_permissions=null",
    );

    assert!(
        prefixes
            .iter()
            .any(|prefix| { prefix == "ls|sandbox_permissions=\"use_default\"|additional_permissions=null" })
    );
}

#[test]
fn approval_prefix_derivation_generalizes_non_mutating_path_commands() {
    let prefixes = derived_shell_approval_prefixes(
        "wc -l src/lib.rs README.md|sandbox_permissions=\"use_default\"|additional_permissions=null",
    );

    assert!(
        prefixes
            .iter()
            .any(|prefix| { prefix == "wc|sandbox_permissions=\"use_default\"|additional_permissions=null" })
    );
    assert!(
        derived_shell_approval_prefixes(
            "rm src/lib.rs|sandbox_permissions=\"use_default\"|additional_permissions=null"
        )
        .is_empty()
    );
    assert!(
        derived_shell_approval_prefixes(
            "perl -i -pe 's/a/b/' src/lib.rs|sandbox_permissions=\"use_default\"|additional_permissions=null"
        )
        .is_empty()
    );
}

#[tokio::test]
async fn approval_cache_regexes_match_keys() {
    let dir = tempdir().unwrap();
    let config_path = dir.path().join("tool-policy.json");
    let mut manager = ToolPolicyManager::new_with_config_path(&config_path).await.expect("manager");

    manager
        .config
        .approval_cache
        .regexes
        .insert("^cargo (check|fmt)\\|sandbox_permissions=\\\"use_default\\\".*$".to_string());
    manager.save_config().await.expect("save regex");

    let reloaded = ToolPolicyManager::new_with_config_path(&config_path)
        .await
        .expect("reload manager");
    assert!(
        reloaded.has_approval_cache_key("cargo check|sandbox_permissions=\"use_default\"|additional_permissions=null")
    );
}

#[tokio::test]
async fn reset_to_prompt_clears_approval_cache() {
    let dir = tempdir().unwrap();
    let config_path = dir.path().join("tool-policy.json");
    let mut manager = ToolPolicyManager::new_with_config_path(&config_path).await.expect("manager");

    manager.add_approval_cache_key("read_file").await.expect("persist approval");
    manager
        .add_approval_cache_prefix("cargo check|sandbox_permissions=\"use_default\"|additional_permissions=null")
        .await
        .expect("persist prefix");
    manager.config.approval_cache.regexes.insert("^cargo check.*$".to_string());
    manager.reset_all_to_prompt().await.expect("reset policies");

    let reloaded = ToolPolicyManager::new_with_config_path(&config_path)
        .await
        .expect("reload manager");
    assert!(!reloaded.has_approval_cache_key("read_file"));
    assert!(reloaded.config.approval_cache.prefixes.is_empty());
    assert!(reloaded.config.approval_cache.regexes.is_empty());
}

#[tokio::test]
async fn apply_tools_config_preserves_existing_policies_for_unconfigured_tools() {
    let dir = tempdir().unwrap();
    let config_path = dir.path().join("tool-policy.json");
    let mut manager = ToolPolicyManager::new_with_config_path(&config_path).await.expect("manager");

    manager.config.available_tools = vec![tools::EXEC_COMMAND.to_owned(), tools::CODE_SEARCH.to_owned()];
    manager
        .config
        .policies
        .insert(tools::EXEC_COMMAND.to_owned(), ToolPolicy::Allow);
    manager.config.policies.insert(tools::CODE_SEARCH.to_owned(), ToolPolicy::Allow);
    manager.save_config().await.expect("save initial config");

    let tools_config = ToolsConfig {
        default_policy: ToolPolicy::Prompt,
        ..Default::default()
    };

    manager.apply_tools_config(&tools_config).await.expect("apply config");

    assert_eq!(manager.get_policy(tools::EXEC_COMMAND), ToolPolicy::Allow);
    assert_eq!(manager.get_policy(tools::CODE_SEARCH), ToolPolicy::Allow);
}

#[tokio::test]
async fn apply_tools_config_applies_explicit_overrides_from_vtcode_toml() {
    let dir = tempdir().unwrap();
    let config_path = dir.path().join("tool-policy.json");
    let mut manager = ToolPolicyManager::new_with_config_path(&config_path).await.expect("manager");

    manager.config.available_tools = vec![tools::EXEC_COMMAND.to_owned()];
    manager
        .config
        .policies
        .insert(tools::EXEC_COMMAND.to_owned(), ToolPolicy::Allow);
    manager.save_config().await.expect("save initial config");

    let mut tools_config = ToolsConfig::default();
    tools_config.policies.insert(tools::EXEC_COMMAND.to_string(), ToolPolicy::Deny);

    manager.apply_tools_config(&tools_config).await.expect("apply config");

    assert_eq!(manager.get_policy(tools::EXEC_COMMAND), ToolPolicy::Deny);
}

#[tokio::test]
async fn update_mcp_tools_seeds_allowlist_rules_for_new_provider() {
    let dir = tempdir().unwrap();
    let config_path = dir.path().join("tool-policy.json");
    let mut manager = ToolPolicyManager::new_with_config_path(&config_path).await.expect("manager");
    assert!(manager.mcp_allowlist().enforce);
    assert!(!manager.mcp_allowlist().is_tool_allowed("deepwiki", "ask_question"));

    let mut provider_tools = HashMap::new();
    provider_tools.insert("deepwiki".to_string(), vec!["ask_question".to_string()]);

    manager.update_mcp_tools(&provider_tools).await.expect("update MCP tools");

    assert!(manager.mcp_allowlist().is_tool_allowed("deepwiki", "ask_question"));
    assert!(manager.mcp_allowlist().is_tool_allowed("deepwiki", "read_wiki_structure"));

    let reloaded = ToolPolicyManager::new_with_config_path(&config_path)
        .await
        .expect("reload manager");
    assert!(reloaded.mcp_allowlist().is_tool_allowed("deepwiki", "ask_question"));
}
