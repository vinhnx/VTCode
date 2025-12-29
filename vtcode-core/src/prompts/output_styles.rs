use std::path::Path;
use std::sync::Arc;
use tokio::sync::RwLock;
use vtcode_config::{OutputStyleManager, VTCodeConfig};

pub struct OutputStyleApplier {
    manager: Arc<RwLock<OutputStyleManager>>,
}

impl OutputStyleApplier {
    pub fn new() -> Self {
        Self {
            manager: Arc::new(RwLock::new(OutputStyleManager::new())),
        }
    }

    pub async fn load_styles_from_config(
        &self,
        _config: &VTCodeConfig,
        workspace: &Path,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let output_styles_dir = workspace.join(".vtcode").join("output-styles");
        let manager = OutputStyleManager::load_from_directory(&output_styles_dir)?;

        {
            let mut guard = self.manager.write().await;
            *guard = manager;
        }

        Ok(())
    }

    pub async fn apply_style(
        &self,
        style_name: &str,
        base_prompt: &str,
        config: &VTCodeConfig,
    ) -> String {
        let guard = self.manager.read().await;

        if let Some(style) = guard.get_style(style_name) {
            if style.config.keep_coding_instructions {
                // Combine base prompt with style content
                format!("{}\n\n{}", base_prompt, style.content)
            } else {
                // Replace base prompt with style content
                style.content.clone()
            }
        } else {
            // If the requested style doesn't exist, use the active style from config
            if let Some(active_style) = guard.get_style(&config.output_style.active_style) {
                if active_style.config.keep_coding_instructions {
                    format!("{}\n\n{}", base_prompt, active_style.content)
                } else {
                    active_style.content.clone()
                }
            } else {
                // Fallback to base prompt if no style is found
                base_prompt.to_string()
            }
        }
    }

    pub async fn get_available_styles(&self) -> Vec<String> {
        let guard = self.manager.read().await;
        guard
            .list_styles()
            .into_iter()
            .map(|(name, _)| name.clone())
            .collect()
    }
}

impl Default for OutputStyleApplier {
    fn default() -> Self {
        Self::new()
    }
}
