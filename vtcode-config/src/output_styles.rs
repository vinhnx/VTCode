use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputStyleConfig {
    #[serde(default = "default_output_style")]
    pub active_style: String,
}

fn default_output_style() -> String {
    "default".to_string()
}

impl Default for OutputStyleConfig {
    fn default() -> Self {
        Self {
            active_style: default_output_style(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct OutputStyleFileConfig {
    pub name: String,
    pub description: Option<String>,
    #[serde(default)]
    pub keep_coding_instructions: bool,
}

#[derive(Debug, Clone)]
pub struct OutputStyle {
    pub config: OutputStyleFileConfig,
    pub content: String,
}

#[derive(Debug)]
pub struct OutputStyleManager {
    styles: HashMap<String, OutputStyle>,
}

impl OutputStyleManager {
    pub fn new() -> Self {
        Self {
            styles: HashMap::new(),
        }
    }

    pub fn load_from_directory<P: AsRef<Path>>(dir: P) -> Result<Self, Box<dyn std::error::Error>> {
        let mut manager = Self::new();
        let dir = dir.as_ref();

        if !dir.exists() {
            return Ok(manager);
        }

        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.extension().and_then(|s| s.to_str()) == Some("md") {
                if let Ok(output_style) = Self::load_from_file(&path) {
                    manager
                        .styles
                        .insert(output_style.config.name.clone(), output_style);
                }
            }
        }

        Ok(manager)
    }

    fn load_from_file<P: AsRef<Path>>(path: P) -> Result<OutputStyle, Box<dyn std::error::Error>> {
        let content = fs::read_to_string(path)?;
        Self::parse_output_style(&content)
    }

    fn parse_output_style(content: &str) -> Result<OutputStyle, Box<dyn std::error::Error>> {
        // Look for frontmatter (between --- and ---)
        if let Some(frontmatter_end) = content.find("\n---\n") {
            let frontmatter_start = if content.starts_with("---\n") {
                0
            } else {
                content.find("---\n").unwrap_or(0)
            };
            let frontmatter = &content[frontmatter_start..frontmatter_end + 4];

            // Parse the frontmatter
            let frontmatter_content = &frontmatter[4..frontmatter.len() - 4]; // Remove the ---\n and \n---
            let config: OutputStyleFileConfig = serde_yaml::from_str(frontmatter_content)?;

            // Get the content after the frontmatter
            let content_start = frontmatter_end + 5; // Skip past "\n---\n"
            let actual_content = if content_start < content.len() {
                &content[content_start..]
            } else {
                ""
            };

            Ok(OutputStyle {
                config,
                content: actual_content.to_string(),
            })
        } else {
            // No frontmatter, create default config
            Ok(OutputStyle {
                config: OutputStyleFileConfig {
                    name: "default".to_string(),
                    description: Some("Default output style".to_string()),
                    keep_coding_instructions: true,
                },
                content: content.to_string(),
            })
        }
    }

    pub fn get_style(&self, name: &str) -> Option<&OutputStyle> {
        self.styles.get(name)
    }

    pub fn list_styles(&self) -> Vec<(&String, &str)> {
        self.styles
            .iter()
            .map(|(name, style)| {
                (
                    name,
                    style
                        .config
                        .description
                        .as_deref()
                        .unwrap_or("No description"),
                )
            })
            .collect()
    }

    pub fn apply_style(&self, name: &str, base_prompt: &str) -> String {
        if let Some(style) = self.get_style(name) {
            if style.config.keep_coding_instructions {
                // Combine base prompt with style content
                format!("{}\n\n{}", base_prompt, style.content)
            } else {
                // Replace base prompt with style content
                style.content.clone()
            }
        } else {
            base_prompt.to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_parse_output_style_with_frontmatter() {
        let content = r#"---
name: Test Style
description: A test output style
keep-coding-instructions: false
---

# Test Output Style

This is a test output style."#;

        let style = OutputStyleManager::parse_output_style(content).unwrap();
        assert_eq!(style.config.name, "Test Style");
        assert_eq!(
            style.config.description,
            Some("A test output style".to_string())
        );
        assert_eq!(style.config.keep_coding_instructions, false);
        assert!(style.content.contains("This is a test output style"));
    }

    #[test]
    fn test_parse_output_style_without_frontmatter() {
        let content = r#"This is a plain output style without frontmatter."#;

        let style = OutputStyleManager::parse_output_style(content).unwrap();
        assert_eq!(style.config.name, "default");
        assert!(style.content.contains("This is a plain output style"));
    }

    #[test]
    fn test_load_from_directory() {
        let temp_dir = TempDir::new().unwrap();
        let style_file = temp_dir.path().join("test_style.md");

        fs::write(
            &style_file,
            r#"---
name: Test Style
description: A test output style
keep-coding-instructions: true
---

# Test Output Style

This is a test output style."#,
        )
        .unwrap();

        let manager = OutputStyleManager::load_from_directory(temp_dir.path()).unwrap();
        assert!(manager.get_style("Test Style").is_some());
    }

    #[test]
    fn test_apply_style_with_keep_instructions() {
        let content = r#"---
name: Test Style
description: A test output style
keep-coding-instructions: true
---

## Custom Instructions

Custom instructions here."#;

        let style = OutputStyleManager::parse_output_style(content).unwrap();
        let mut manager = OutputStyleManager::new();
        manager.styles.insert("Test Style".to_string(), style);

        let base_prompt = "Base system prompt";
        let result = manager.apply_style("Test Style", base_prompt);

        assert!(result.contains("Base system prompt"));
        assert!(result.contains("Custom instructions here"));
    }

    #[test]
    fn test_apply_style_without_keep_instructions() {
        let content = r#"---
name: Test Style
description: A test output style
keep-coding-instructions: false
---

## Custom Instructions

Custom instructions here."#;

        let style = OutputStyleManager::parse_output_style(content).unwrap();
        let mut manager = OutputStyleManager::new();
        manager.styles.insert("Test Style".to_string(), style);

        let base_prompt = "Base system prompt";
        let result = manager.apply_style("Test Style", base_prompt);

        assert!(!result.contains("Base system prompt"));
        assert!(result.contains("Custom instructions here"));
    }
}
