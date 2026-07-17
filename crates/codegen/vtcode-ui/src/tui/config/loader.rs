use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

pub use super::SyntaxHighlightingConfig;
use super::{
    AcpConfig, AgentConfig, AutomationConfig, ContextConfig, McpConfig, PromptCacheConfig,
    PtyConfig, SecurityConfig, ToolsConfig, UiConfig,
};

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct VTCodeConfig {
    pub agent: AgentConfig,
    pub ui: UiConfig,
    pub prompt_cache: PromptCacheConfig,
    pub mcp: McpConfig,
    pub acp: AcpConfig,
    pub automation: AutomationConfig,
    pub tools: ToolsConfig,
    pub security: SecurityConfig,
    pub context: ContextConfig,
    pub syntax_highlighting: SyntaxHighlightingConfig,
    pub pty: PtyConfig,
}

pub struct ConfigManager {
    path: PathBuf,
    config: VTCodeConfig,
}

impl ConfigManager {
    pub fn load() -> Result<Self> {
        let cwd = std::env::current_dir().context("failed to read current directory")?;
        Self::load_from_workspace(cwd)
    }

    pub fn load_from_workspace(workspace_root: impl AsRef<Path>) -> Result<Self> {
        let path = workspace_root.as_ref().join("vtcode.toml");

        let config = if path.exists() {
            let raw = std::fs::read_to_string(&path)
                .with_context(|| format!("failed to read {}", path.display()))?;
            toml::from_str::<VTCodeConfig>(&raw)
                .with_context(|| format!("failed to parse {}", path.display()))?
        } else {
            VTCodeConfig::default()
        };

        Ok(Self { path, config })
    }

    pub fn config(&self) -> &VTCodeConfig {
        &self.config
    }

    pub fn save_config(&self, config: &VTCodeConfig) -> Result<()> {
        let rendered = toml::to_string_pretty(config).context("failed to serialize config")?;
        std::fs::write(&self.path, rendered)
            .with_context(|| format!("failed to write {}", self.path.display()))
    }
}
