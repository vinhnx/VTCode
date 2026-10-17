use std::future::Future;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use dialoguer::console::style;
use hashbrown::{HashMap, HashSet};
use indexmap::IndexMap;
use regex::Regex;

use super::ToolPolicy;
use super::alternative::AlternativeToolPolicyConfig;
use super::mcp::{McpPolicyStore, mcp_standard_logging, parse_mcp_policy_key};
use super::model::{
    ApprovalCacheConfig, PermissionPromptHandler, ToolConstraints, ToolExecutionDecision, ToolPolicyConfig,
};
use super::shell_approval::{
    derived_shell_approval_prefixes, shell_command_words_match_prefix, shell_words_lossy, split_shell_approval_entry,
};
use super::{APPROVAL_REGEX_CACHE, AUTO_ALLOW_TOOLS, DEFAULT_APPROVAL_SCOPE_SIGNATURE};
use crate::config::core::tools::ToolsConfig;
use crate::config::loader::{ConfigManager, VTCodeConfig};
use crate::config::mcp::{McpAllowListConfig, McpAllowListRules};
use crate::utils::error_messages::ERR_CREATE_POLICY_DIR;
use crate::utils::file_utils::{ensure_dir_exists, read_file_with_context, write_file_atomic_with_context};
use crate::utils::tool_name_parsing::canonical_tool_name;
use vtcode_commons::VtCodePaths;
use vtcode_commons::fs::{read_private_file_no_follow, write_private_file_atomic};

/// Tool policy manager
pub struct ToolPolicyManager {
    pub(crate) config_path: PathBuf,
    pub(crate) config: ToolPolicyConfig,
    pub(crate) permission_handler: Option<Box<dyn PermissionPromptHandler>>,
    pub(crate) workspace_root: Option<PathBuf>,
    pub(crate) private_storage: bool,
}

impl Clone for ToolPolicyManager {
    fn clone(&self) -> Self {
        // Note: Permission handler is not cloned - this is intentional as handlers
        // typically contain UI state that shouldn't be duplicated
        Self {
            config_path: self.config_path.clone(),
            config: self.config.clone(),
            permission_handler: None, // Handler is not cloned
            workspace_root: self.workspace_root.clone(),
            private_storage: self.private_storage,
        }
    }
}

impl ToolPolicyManager {
    /// Create a new tool policy manager
    pub async fn new() -> Result<Self> {
        let config_path = Self::get_config_path().await?;
        let config = Self::load_or_create_config_with_policy(&config_path, true).await?;

        Ok(Self {
            config_path,
            config,
            permission_handler: None,
            workspace_root: None,
            private_storage: true,
        })
    }

    /// Create a new tool policy manager with workspace-specific config
    pub async fn new_with_workspace(workspace_root: &Path) -> Result<Self> {
        let config_path = Self::get_workspace_config_path(workspace_root).await?;
        let config = Self::load_or_create_config_with_policy(&config_path, false).await?;

        Ok(Self {
            config_path,
            config,
            permission_handler: None,
            workspace_root: Some(workspace_root.to_path_buf()),
            private_storage: false,
        })
    }

    /// Create a new tool policy manager backed by a custom configuration path.
    ///
    /// This helper allows downstream consumers to store policy data alongside
    /// their own configuration hierarchy instead of writing to the default
    /// `.vtcode` directory.
    pub async fn new_with_config_path<P: Into<PathBuf>>(config_path: P) -> Result<Self> {
        let config_path = config_path.into();

        if let Some(parent) = config_path.parent()
            && !tokio::fs::try_exists(parent).await.unwrap_or(false)
        {
            ensure_dir_exists(parent)
                .await
                .with_context(|| format!("{} at {}", ERR_CREATE_POLICY_DIR, parent.display()))?;
        }

        let config = Self::load_or_create_config_with_policy(&config_path, false).await?;

        Ok(Self {
            config_path,
            config,
            permission_handler: None,
            workspace_root: None,
            private_storage: false,
        })
    }

    /// Set the permission handler for this manager
    pub fn set_permission_handler(&mut self, handler: Box<dyn PermissionPromptHandler>) {
        self.permission_handler = Some(handler);
    }

    /// Get the path to the tool policy configuration file
    async fn get_config_path() -> Result<PathBuf> {
        let paths = VtCodePaths::resolve().context("Could not resolve VT Code paths")?;
        paths.ensure_config_dir().context("Failed to create VT Code config directory")?;
        paths.config_path("tool-policy.json")
    }

    /// Get the path to the workspace-specific tool policy configuration file
    async fn get_workspace_config_path(workspace_root: &Path) -> Result<PathBuf> {
        let workspace_vtcode_dir = workspace_root.join(".vtcode");

        if !tokio::fs::try_exists(&workspace_vtcode_dir).await.unwrap_or(false) {
            ensure_dir_exists(&workspace_vtcode_dir).await.with_context(|| {
                format!("Failed to create workspace policy directory at {}", workspace_vtcode_dir.display())
            })?;
        }

        Ok(workspace_vtcode_dir.join("tool-policy.json"))
    }

    pub(crate) async fn load_or_create_config_with_policy(
        config_path: &Path,
        private_storage: bool,
    ) -> Result<ToolPolicyConfig> {
        let content = if private_storage {
            match tokio::fs::symlink_metadata(config_path).await {
                Ok(metadata) if metadata.file_type().is_symlink() => {
                    return Err(anyhow::anyhow!(
                        "Refusing to read symlinked tool policy config: {}",
                        config_path.display()
                    ));
                }
                Ok(metadata) if !metadata.is_file() => {
                    return Err(anyhow::anyhow!("Tool policy config is not a regular file: {}", config_path.display()));
                }
                Ok(_) => Some(
                    String::from_utf8(read_private_file_no_follow(config_path).await?)
                        .context("Tool policy config is not valid UTF-8")?,
                ),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                Err(error) => {
                    return Err(error).with_context(|| format!("Failed to inspect {}", config_path.display()));
                }
            }
        } else if tokio::fs::try_exists(config_path).await.unwrap_or(false) {
            Some(
                read_file_with_context(config_path, "tool policy config")
                    .await
                    .context("Failed to read tool policy config")?,
            )
        } else {
            None
        };

        if let Some(content) = content {
            // Try to parse as alternative format first
            if let Ok(alt_config) = serde_json::from_str::<AlternativeToolPolicyConfig>(&content) {
                // Convert alternative format to standard format
                return Ok(Self::convert_from_alternative(alt_config));
            }

            // Fall back to standard format with graceful recovery on parse errors
            match serde_json::from_str(&content) {
                Ok(mut config) => {
                    Self::apply_auto_allow_defaults(&mut config);
                    Self::ensure_network_constraints(&mut config);
                    Ok(config)
                }
                Err(parse_err) => {
                    tracing::warn!(
                        "Invalid tool policy config at {} ({}). Resetting to defaults.",
                        config_path.display(),
                        parse_err
                    );
                    Self::reset_to_default(config_path, private_storage).await
                }
            }
        } else {
            // Create new config with empty tools list
            let mut config = ToolPolicyConfig::default();
            Self::apply_auto_allow_defaults(&mut config);
            Self::ensure_network_constraints(&mut config);
            Ok(config)
        }
    }

    fn apply_auto_allow_defaults(config: &mut ToolPolicyConfig) {
        for &tool in AUTO_ALLOW_TOOLS {
            if !config.policies.contains_key(tool) {
                config.policies.insert(tool.into(), ToolPolicy::Allow);
            }
            if !config.available_tools.iter().any(|t| t == tool) {
                config.available_tools.push(tool.into());
            }
        }
        Self::ensure_network_constraints(config);
    }

    fn ensure_network_constraints(_config: &mut ToolPolicyConfig) {
        // Network constraints removed with curl tool removal
    }

    async fn reset_to_default(config_path: &Path, private_storage: bool) -> Result<ToolPolicyConfig> {
        let backup_path = config_path.with_extension("json.bak");

        if let Err(err) = tokio::fs::rename(config_path, &backup_path).await {
            tracing::warn!("Unable to back up invalid tool policy config ({}). {}", config_path.display(), err);
        } else {
            tracing::info!("Backed up invalid tool policy config to {}", backup_path.display());
        }

        let default_config = ToolPolicyConfig::default();
        Self::write_config_with_policy(config_path, &default_config, private_storage).await?;
        Ok(default_config)
    }

    async fn write_config_with_policy(path: &Path, config: &ToolPolicyConfig, private_storage: bool) -> Result<()> {
        if let Some(parent) = path.parent()
            && !tokio::fs::try_exists(parent).await.unwrap_or(false)
        {
            if private_storage {
                VtCodePaths::ensure_user_dir(parent)
                    .with_context(|| format!("{} at {}", ERR_CREATE_POLICY_DIR, parent.display()))?;
            } else {
                ensure_dir_exists(parent)
                    .await
                    .with_context(|| format!("{} at {}", ERR_CREATE_POLICY_DIR, parent.display()))?;
            }
        }

        let serialized = serde_json::to_string_pretty(config).context("Failed to serialize tool policy config")?;

        if private_storage {
            write_private_file_atomic(path, serialized.as_bytes())
                .await
                .with_context(|| format!("Failed to write tool policy config: {}", path.display()))
        } else {
            write_file_atomic_with_context(path, &serialized, "tool policy config")
                .await
                .with_context(|| format!("Failed to write tool policy config: {}", path.display()))
        }
    }

    /// Convert alternative format to standard format
    fn convert_from_alternative(alt_config: AlternativeToolPolicyConfig) -> ToolPolicyConfig {
        let mut policies = IndexMap::new();

        // Convert tool policies
        for (tool_name, alt_policy) in alt_config.tools {
            let policy = if alt_policy.allow {
                ToolPolicy::Allow
            } else {
                ToolPolicy::Deny
            };
            policies.insert(tool_name, policy);
        }

        let mut config = ToolPolicyConfig {
            version: alt_config.version,
            available_tools: policies.keys().cloned().collect(),
            policies,
            constraints: alt_config.constraints,
            mcp: McpPolicyStore::default(),
            approval_cache: ApprovalCacheConfig::default(),
        };
        Self::apply_auto_allow_defaults(&mut config);
        config
    }

    fn apply_config_policy(&mut self, tool_name: &str, policy: ToolPolicy) {
        let canonical = canonical_tool_name(tool_name);
        self.config.policies.insert(canonical.to_owned(), policy);
    }

    pub(crate) fn resolve_config_policy(tools_config: &ToolsConfig, tool_name: &str) -> ToolPolicy {
        let canonical = canonical_tool_name(tool_name);

        if let Some(policy) = tools_config.policies.get(canonical) {
            return policy.clone();
        }

        tools_config.default_policy.clone()
    }

    /// Apply policies defined in vtcode.toml to the runtime policy manager.
    ///
    /// Auto-allow defaults are NOT re-applied here — they are seeded once
    /// during `load_or_create_config`. Re-applying them after user overrides
    /// would silently revert explicit Deny/Prompt settings in vtcode.toml.
    pub async fn apply_tools_config(&mut self, tools_config: &ToolsConfig) -> Result<()> {
        if self.config.available_tools.is_empty() {
            return Ok(());
        }

        let tools: Vec<_> = self.config.available_tools.to_vec();
        for tool in tools {
            let canonical = canonical_tool_name(&tool);
            if tools_config.policies.contains_key(canonical) || !self.config.policies.contains_key(canonical) {
                self.apply_config_policy(&tool, Self::resolve_config_policy(tools_config, &tool));
            }
        }

        self.save_config().await
    }

    /// Update the tool list and save configuration
    pub async fn update_available_tools(&mut self, tools: Vec<String>) -> Result<()> {
        // OPTIMIZATION: Use HashSet for deduplication, then convert to sorted Vec
        let mut canonical_tools = Vec::with_capacity(tools.len());
        let mut seen = HashSet::with_capacity(tools.len());

        for tool in tools {
            let canonical = canonical_tool_name(&tool).to_owned();
            if seen.insert(canonical.clone()) {
                canonical_tools.push(canonical);
            }
        }
        canonical_tools.sort();

        let current_tools: HashSet<_> = self.config.policies.keys().cloned().collect();
        let new_tools: HashSet<_> = canonical_tools
            .iter()
            .filter(|name| !name.starts_with("mcp::"))
            .cloned()
            .collect();

        let mut has_changes = false;

        for tool in canonical_tools
            .iter()
            .filter(|tool| !tool.starts_with("mcp::") && !current_tools.contains(*tool))
        {
            let default_policy = if AUTO_ALLOW_TOOLS.contains(&tool.as_str()) {
                ToolPolicy::Allow
            } else {
                ToolPolicy::Prompt
            };
            self.config.policies.insert(tool.clone(), default_policy);
            has_changes = true;
        }

        let tools_to_remove: Vec<_> = self
            .config
            .policies
            .keys()
            .filter(|tool| !new_tools.contains(*tool))
            .cloned()
            .collect();

        for tool in tools_to_remove {
            self.config.policies.shift_remove(&tool);
            has_changes = true;
        }

        // Only clone if we need to compare/sort
        let mut sorted_available = self.config.available_tools.clone();
        sorted_available.sort();
        if sorted_available != canonical_tools {
            self.config.available_tools = canonical_tools;
            has_changes = true;
        }

        Self::ensure_network_constraints(&mut self.config);

        if has_changes { self.save_config().await } else { Ok(()) }
    }

    /// Synchronize MCP provider tool lists with persisted policies
    pub async fn update_mcp_tools(&mut self, provider_tools: &HashMap<String, Vec<String>>) -> Result<()> {
        let stored_providers: HashSet<String> = self.config.mcp.providers.keys().cloned().collect();
        let mut has_changes = false;

        // Update or insert provider entries
        for (provider, tools) in provider_tools {
            let entry = self.config.mcp.providers.entry(provider.clone()).or_default();

            let existing_tools: HashSet<_> = entry.tools.keys().cloned().collect();
            let advertised: HashSet<_> = tools.iter().cloned().collect();

            // Add new tools with default Prompt policy
            for tool in tools {
                if !existing_tools.contains(tool) {
                    entry.tools.insert(tool.clone(), ToolPolicy::Prompt);
                    has_changes = true;
                }
            }

            // Remove tools no longer advertised
            for stale in existing_tools.difference(&advertised) {
                entry.tools.shift_remove(stale);
                has_changes = true;
            }
        }

        // Remove providers that are no longer present
        let advertised_providers: HashSet<String> = provider_tools.keys().cloned().collect();
        for provider in stored_providers.difference(&advertised_providers).cloned().collect::<Vec<_>>() {
            self.config.mcp.providers.shift_remove(provider.as_str());
            has_changes = true;
        }

        // Seed allowlist rules for newly advertised providers so a persisted
        // enforce-mode allowlist cannot permanently hide a connected provider.
        // Per-tool Prompt policies still gate execution after discovery.
        for provider in advertised_providers {
            let rules =
                self.config
                    .mcp
                    .allowlist
                    .providers
                    .entry(provider.clone())
                    .or_insert_with(|| McpAllowListRules {
                        tools: Some(vec!["*".to_string()]),
                        logging: Some(mcp_standard_logging()),
                        ..Default::default()
                    });
            if rules.logging.is_none() {
                rules.logging = Some(mcp_standard_logging());
                has_changes = true;
            }
        }

        // Remove any stale MCP keys from the primary policy map
        let stale_runtime_keys: Vec<_> = self
            .config
            .policies
            .keys()
            .filter(|name| name.starts_with("mcp::"))
            .cloned()
            .collect();

        for key in stale_runtime_keys {
            self.config.policies.shift_remove(&key);
            has_changes = true;
        }

        // Refresh available tools list with MCP entries included
        let mut available: Vec<String> = self
            .config
            .available_tools
            .iter()
            .filter(|name| !name.starts_with("mcp::"))
            .cloned()
            .collect();

        available.extend(
            self.config
                .mcp
                .providers
                .iter()
                .flat_map(|(provider, policy)| policy.tools.keys().map(move |tool| format!("mcp::{provider}::{tool}"))),
        );

        available.sort();
        available.dedup();

        // Check if the available tools list has actually changed
        if self.config.available_tools != available {
            self.config.available_tools = available;
            has_changes = true;
        }

        if has_changes { self.save_config().await } else { Ok(()) }
    }

    /// Retrieve policy for a specific MCP tool
    pub fn get_mcp_tool_policy(&self, provider: &str, tool: &str) -> ToolPolicy {
        self.config
            .mcp
            .providers
            .get(provider)
            .and_then(|policy| policy.tools.get(tool))
            .cloned()
            .unwrap_or(ToolPolicy::Prompt)
    }

    /// Update policy for a specific MCP tool
    pub async fn set_mcp_tool_policy(&mut self, provider: &str, tool: &str, policy: ToolPolicy) -> Result<()> {
        // OPTIMIZATION: Use into() for cleaner conversion
        let entry = self.config.mcp.providers.entry(provider.into()).or_default();
        entry.tools.insert(tool.into(), policy);
        self.save_config().await
    }

    /// Access the persisted MCP allow list configuration
    pub fn mcp_allowlist(&self) -> &McpAllowListConfig {
        &self.config.mcp.allowlist
    }

    /// Replace the persisted MCP allow list configuration
    pub async fn set_mcp_allowlist(&mut self, allowlist: McpAllowListConfig) -> Result<()> {
        self.config.mcp.allowlist = allowlist;
        self.save_config().await
    }

    /// Get policy for a specific tool
    pub fn get_policy(&self, tool_name: &str) -> ToolPolicy {
        let canonical = canonical_tool_name(tool_name);
        if let Some((provider, tool)) = parse_mcp_policy_key(tool_name) {
            return self.get_mcp_tool_policy(&provider, &tool);
        }

        self.config.policies.get(canonical).cloned().unwrap_or(ToolPolicy::Prompt)
    }

    /// Get optional constraints for a specific tool
    pub fn get_constraints(&self, tool_name: &str) -> Option<&ToolConstraints> {
        let canonical = canonical_tool_name(tool_name);
        self.config.constraints.get(canonical)
    }

    /// Check if tool should be executed based on policy
    pub async fn should_execute_tool(&mut self, tool_name: &str) -> Result<ToolExecutionDecision> {
        if let Some((provider, tool)) = parse_mcp_policy_key(tool_name) {
            return match self.get_mcp_tool_policy(&provider, &tool) {
                ToolPolicy::Allow => Ok(ToolExecutionDecision::Allowed),
                ToolPolicy::Deny => Ok(ToolExecutionDecision::Denied),
                ToolPolicy::Prompt => {
                    if ToolPolicyManager::is_auto_allow_tool(tool_name) {
                        self.set_mcp_tool_policy(&provider, &tool, ToolPolicy::Allow).await?;
                        Ok(ToolExecutionDecision::Allowed)
                    } else {
                        // Use permission handler if available
                        if let Some(ref mut handler) = self.permission_handler {
                            handler.prompt_tool_permission(tool_name)
                        } else {
                            tracing::warn!(
                                "Prompt policy for tool '{tool_name}' denied: no permission handler configured"
                            );
                            Ok(ToolExecutionDecision::Denied)
                        }
                    }
                }
            };
        }

        let canonical = canonical_tool_name(tool_name);

        match self.get_policy(canonical) {
            ToolPolicy::Allow => Ok(ToolExecutionDecision::Allowed),
            ToolPolicy::Deny => Ok(ToolExecutionDecision::Denied),
            ToolPolicy::Prompt => {
                let canonical_name = canonical;
                if AUTO_ALLOW_TOOLS.contains(&canonical_name) {
                    self.set_policy(canonical_name, ToolPolicy::Allow).await?;
                    return Ok(ToolExecutionDecision::Allowed);
                }
                // Use permission handler if available
                if let Some(ref mut handler) = self.permission_handler {
                    handler.prompt_tool_permission(tool_name)
                } else {
                    tracing::warn!("Prompt policy for tool '{tool_name}' denied: no permission handler configured");
                    Ok(ToolExecutionDecision::Denied)
                }
            }
        }
    }

    pub fn is_auto_allow_tool(tool_name: &str) -> bool {
        let canonical = canonical_tool_name(tool_name);
        AUTO_ALLOW_TOOLS.contains(&canonical)
    }

    /// Prompt user for tool execution permission using the configured handler.
    ///
    /// This function delegates to the PermissionPromptHandler if one is configured.
    /// In TUI mode, the handler should be set to use TUI-based prompts via the
    /// permission handler mechanism.
    pub fn prompt_user_for_tool(&mut self, tool_name: &str) -> Result<ToolExecutionDecision> {
        if let Some(ref mut handler) = self.permission_handler {
            handler.prompt_tool_permission(tool_name)
        } else {
            // Default behavior if no handler is configured: allow through
            Ok(ToolExecutionDecision::Allowed)
        }
    }

    /// Set policy for a specific tool
    pub async fn set_policy(&mut self, tool_name: &str, policy: ToolPolicy) -> Result<()> {
        if let Some((provider, tool)) = parse_mcp_policy_key(tool_name) {
            return self.set_mcp_tool_policy(&provider, &tool, policy).await;
        }

        let canonical = canonical_tool_name(tool_name).to_owned();
        self.config.policies.insert(canonical.clone(), policy.clone());
        self.save_config().await?;
        self.persist_policy_to_workspace_config(&canonical, policy)
    }

    pub(crate) async fn seed_default_policy(&mut self, tool_name: &str, policy: ToolPolicy) -> Result<()> {
        let canonical = canonical_tool_name(tool_name).to_owned();
        self.config.policies.insert(canonical, policy);
        self.save_config().await
    }

    /// Reset all tools to prompt
    pub async fn reset_all_to_prompt(&mut self) -> Result<()> {
        for policy in self.config.policies.values_mut() {
            *policy = ToolPolicy::Prompt;
        }
        for provider in self.config.mcp.providers.values_mut() {
            for policy in provider.tools.values_mut() {
                *policy = ToolPolicy::Prompt;
            }
        }
        self.config.approval_cache.allowed.clear();
        self.config.approval_cache.prefixes.clear();
        self.config.approval_cache.regexes.clear();
        self.save_config().await
    }

    /// Allow all tools
    pub async fn allow_all_tools(&mut self) -> Result<()> {
        self.allow_all_tools_for_tools(&[]).await
    }

    pub async fn allow_all_tools_for_tools(&mut self, all_tools: &[String]) -> Result<()> {
        for policy in self.config.policies.values_mut() {
            *policy = ToolPolicy::Allow;
        }
        for provider in self.config.mcp.providers.values_mut() {
            for policy in provider.tools.values_mut() {
                *policy = ToolPolicy::Allow;
            }
        }
        for tool in all_tools {
            let canonical = canonical_tool_name(tool);
            if !self.config.policies.contains_key(canonical) {
                self.config.policies.insert(canonical.to_owned(), ToolPolicy::Allow);
            }
        }
        self.save_config().await
    }

    /// Deny all tools
    pub async fn deny_all_tools(&mut self) -> Result<()> {
        for policy in self.config.policies.values_mut() {
            *policy = ToolPolicy::Deny;
        }
        for provider in self.config.mcp.providers.values_mut() {
            for policy in provider.tools.values_mut() {
                *policy = ToolPolicy::Deny;
            }
        }
        self.config.approval_cache.allowed.clear();
        self.config.approval_cache.prefixes.clear();
        self.config.approval_cache.regexes.clear();
        self.save_config().await
    }

    /// Get summary of current policies
    pub fn get_policy_summary(&self) -> IndexMap<String, ToolPolicy> {
        let mut summary = self.config.policies.clone();
        for (provider, policy) in &self.config.mcp.providers {
            for (tool, status) in &policy.tools {
                summary.insert(format!("mcp::{provider}::{tool}"), status.clone());
            }
        }
        summary
    }

    /// Check whether an explicit approval key is remembered for this workspace.
    ///
    /// Performs three levels of matching:
    /// 1. Exact match against persisted allowed keys
    /// 2. Word-prefix match: checks if the approval key starts with any persisted prefix
    /// 3. Regex match against persisted regex patterns
    pub fn has_approval_cache_key(&self, approval_key: &str) -> bool {
        // Exact match: simplest and fastest
        if self.config.approval_cache.allowed.contains(approval_key) {
            return true;
        }

        // Word-prefix match: check if any cached key is a word-prefix of the approval_key
        // e.g., "cargo check" matches "cargo check --target x86_64"
        for cached in &self.config.approval_cache.allowed {
            if cached.len() < approval_key.len()
                && approval_key.starts_with(cached.as_str())
                && approval_key.as_bytes().get(cached.len()) == Some(&b' ')
            {
                return true;
            }
        }

        // Word-prefix match against cached prefixes. Prefer shell tokenization
        // so persisted command approvals preserve argument/option boundaries
        // instead of raw whitespace quirks from display/rendering.
        let (approval_text, approval_scope) = split_shell_approval_entry(approval_key);
        let approval_words = shell_words_lossy(approval_text);
        for cached in &self.config.approval_cache.prefixes {
            let (prefix_text, prefix_scope) = split_shell_approval_entry(cached.as_str());
            let prefix_words = shell_words_lossy(prefix_text);
            let scope_matches = prefix_scope.unwrap_or(DEFAULT_APPROVAL_SCOPE_SIGNATURE)
                == approval_scope.unwrap_or(DEFAULT_APPROVAL_SCOPE_SIGNATURE);
            if !prefix_words.is_empty()
                && scope_matches
                && prefix_words.len() <= approval_words.len()
                && prefix_words.iter().zip(approval_words.iter()).all(|(a, b)| a == b)
            {
                return true;
            }
        }

        // Regex match against persisted regex patterns. Compiled regexes are
        // memoized per unique pattern list so we don't recompile on every check.
        let compiled = {
            let mut cache = APPROVAL_REGEX_CACHE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            match &*cache {
                Some((patterns, regexes)) if *patterns == self.config.approval_cache.regexes => regexes.clone(),
                _ => {
                    let regexes: Vec<Regex> = self
                        .config
                        .approval_cache
                        .regexes
                        .iter()
                        .filter_map(|pattern| Regex::new(pattern).ok())
                        .collect();
                    *cache = Some((self.config.approval_cache.regexes.clone(), regexes.clone()));
                    regexes
                }
            }
        };
        compiled.iter().any(|regex| regex.is_match(approval_key))
    }

    /// Find the best matching cache key for a given approval key using fuzzy prefix matching.
    /// Returns the longest matching prefix key if one exists.
    pub fn find_matching_cache_prefix(&self, approval_key: &str) -> Option<String> {
        let mut best_match: Option<String> = None;
        let mut best_len = 0usize;

        for cached in &self.config.approval_cache.allowed {
            if cached.len() < approval_key.len()
                && approval_key.starts_with(cached.as_str())
                && approval_key.as_bytes().get(cached.len()) == Some(&b' ')
                && cached.len() > best_len
            {
                best_len = cached.len();
                best_match = Some(cached.clone());
            }
        }

        best_match
    }

    /// Persist an explicit approval key for future prompts in this workspace.
    pub async fn add_approval_cache_key(&mut self, approval_key: impl Into<String>) -> Result<()> {
        if self.config.approval_cache.allowed.insert(approval_key.into()) {
            self.config.approval_cache.trim_to_budget();
            self.save_config().await?;
        }
        Ok(())
    }

    /// Persist an approval key and automatically derive shorter segment-prefix keys
    /// so that future similar commands also match without re-prompting.
    ///
    /// For example, approving "cargo check --target x86_64" also caches
    /// "cargo check" as a segment prefix, so "cargo check --release" also matches.
    pub async fn add_approval_cache_key_with_segments(&mut self, approval_key: impl Into<String>) -> Result<()> {
        let key: String = approval_key.into();
        let mut changed = false;

        // Add the exact key
        if self.config.approval_cache.allowed.insert(key.clone()) {
            changed = true;
        }

        for prefix in derived_shell_approval_prefixes(&key) {
            if self.config.approval_cache.prefixes.insert(prefix) {
                changed = true;
            }
        }

        if changed {
            self.config.approval_cache.trim_to_budget();
            self.save_config().await?;
        }
        Ok(())
    }

    /// Persist a shell prefix approval entry for future prompts in this workspace.
    pub async fn add_approval_cache_prefix(&mut self, prefix_entry: impl Into<String>) -> Result<()> {
        if self.config.approval_cache.prefixes.insert(prefix_entry.into()) {
            self.config.approval_cache.trim_to_budget();
            self.save_config().await?;
        }
        Ok(())
    }

    /// Check whether a persisted shell prefix approval matches the command words and scope.
    pub fn matching_shell_approval_prefix(&self, command_words: &[String], scope_signature: &str) -> Option<String> {
        self.config.approval_cache.prefixes.iter().find_map(|entry| {
            let (prefix_text, entry_scope_signature) = split_shell_approval_entry(entry.as_str());
            let prefix_words = shell_words::split(prefix_text).ok()?;
            let entry_scope_signature = entry_scope_signature.unwrap_or(DEFAULT_APPROVAL_SCOPE_SIGNATURE);
            (entry_scope_signature == scope_signature && shell_command_words_match_prefix(command_words, &prefix_words))
                .then(|| entry.clone())
        })
    }

    /// Remove all persisted approval cache entries.
    pub async fn clear_approval_cache(&mut self) -> Result<()> {
        if !self.config.approval_cache.allowed.is_empty()
            || !self.config.approval_cache.prefixes.is_empty()
            || !self.config.approval_cache.regexes.is_empty()
        {
            self.config.approval_cache.allowed.clear();
            self.config.approval_cache.prefixes.clear();
            self.config.approval_cache.regexes.clear();
            self.save_config().await?;
        }
        Ok(())
    }

    /// Save configuration to file
    pub(crate) fn save_config(&self) -> impl Future<Output = Result<()>> + '_ {
        Self::write_config_with_policy(&self.config_path, &self.config, self.private_storage)
    }

    pub(crate) fn persist_policy_to_workspace_config(&self, tool_name: &str, policy: ToolPolicy) -> Result<()> {
        let Some(workspace_root) = self.workspace_root.as_ref() else {
            return Ok(());
        };

        let config_path = workspace_root.join("vtcode.toml");
        let mut config = if config_path.exists() {
            ConfigManager::load_from_file(&config_path)
                .with_context(|| {
                    format!("Failed to load config for tool policy persistence at {}", config_path.display())
                })?
                .config()
                .clone()
        } else {
            VTCodeConfig::default()
        };

        config.tools.policies.insert(tool_name.to_string(), policy);

        // Policy changes are repository-controlled writes. Preserve the
        // trusted provider settings in their user layer instead of flattening
        // them into the workspace config while entering or leaving plan mode.
        ConfigManager::save_repository_config_to_path(&config_path, &config)
            .with_context(|| format!("Failed to persist tool policy to {}", config_path.display()))
    }

    /// Print current policy status
    pub fn print_status(&self) {
        println!("{}", style("Tool Policy Status").cyan().bold());
        println!("Config file: {}", self.config_path.display());
        println!();

        let summary = self.get_policy_summary();

        if summary.is_empty() {
            println!("No tools configured yet.");
            return;
        }

        let mut allow_count = 0;
        let mut prompt_count = 0;
        let mut deny_count = 0;

        for (tool, policy) in &summary {
            let (status, color_name) = match policy {
                ToolPolicy::Allow => {
                    allow_count += 1;
                    ("ALLOW", "green")
                }
                ToolPolicy::Prompt => {
                    prompt_count += 1;
                    ("PROMPT", "cyan")
                }
                ToolPolicy::Deny => {
                    deny_count += 1;
                    ("DENY", "red")
                }
            };

            let status_styled = match color_name {
                "green" => style(status).green(),
                "cyan" => style(status).cyan(),
                "red" => style(status).red(),
                _ => style(status),
            };

            println!("  {} {}", style(format!("{tool:15}")).cyan(), status_styled);
        }

        println!();
        println!(
            "Summary: {} allowed, {} prompt, {} denied",
            style(allow_count).green(),
            style(prompt_count).cyan(),
            style(deny_count).red()
        );
    }

    /// Expose path of the underlying policy configuration file
    pub fn config_path(&self) -> &Path {
        &self.config_path
    }
}
