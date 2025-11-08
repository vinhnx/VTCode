//! Skill persistence and management for reusable code functions.
//!
//! Agents can save working code implementations as reusable "skills" in the
//! `.vtcode/skills/` directory. Each skill includes:
//! - Function implementation (Python or JavaScript)
//! - `SKILL.md` documentation
//! - Input/output type hints
//! - Usage examples
//!
//! Skills can be loaded across conversations and shared with other agents.

use crate::exec::ToolDependency;
use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tracing::{debug, info};

/// Metadata about a saved skill.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillMetadata {
    /// Skill name (snake_case)
    pub name: String,
    /// Brief description
    pub description: String,
    /// Programming language (python3 or javascript)
    pub language: String,
    /// Input parameters documentation
    pub inputs: Vec<ParameterDoc>,
    /// Output documentation
    pub output: String,
    /// Usage examples
    pub examples: Vec<String>,
    /// Tags for searching/categorizing
    pub tags: Vec<String>,
    /// When the skill was created (ISO 8601)
    pub created_at: String,
    /// When the skill was last modified (ISO 8601)
    pub modified_at: String,
    /// Tool dependencies with version constraints
    #[serde(default)]
    pub tool_dependencies: Vec<ToolDependency>,
}

/// Parameter documentation for a skill.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParameterDoc {
    pub name: String,
    pub r#type: String,
    pub description: String,
    pub required: bool,
}

/// A saved skill with code and metadata.
#[derive(Debug, Clone)]
pub struct Skill {
    pub metadata: SkillMetadata,
    pub code: String,
}

/// Manager for skill storage and retrieval.
pub struct SkillManager {
    skills_dir: PathBuf,
}

impl SkillManager {
    /// Create a new skill manager.
    pub fn new(workspace_root: &Path) -> Self {
        Self {
            skills_dir: workspace_root.join(".vtcode").join("skills"),
        }
    }

    /// Save a skill to disk.
    ///
    /// # Arguments
    /// * `skill` - The skill to save
    /// * `code` - The skill implementation code
    pub async fn save_skill(&self, skill: Skill) -> Result<()> {
        // Create skills directory
        tokio::fs::create_dir_all(&self.skills_dir)
            .await
            .context("failed to create skills directory")?;

        let skill_dir = self.skills_dir.join(&skill.metadata.name);
        tokio::fs::create_dir_all(&skill_dir)
            .await
            .context("failed to create skill directory")?;

        // Save code file
        let code_filename = match skill.metadata.language.as_str() {
            "python3" | "python" => "skill.py",
            "javascript" | "js" => "skill.js",
            lang => return Err(anyhow!("unsupported language: {}", lang)),
        };

        let code_path = skill_dir.join(code_filename);
        tokio::fs::write(&code_path, &skill.code)
            .await
            .context("failed to write skill code")?;

        // Save metadata
        let metadata_path = skill_dir.join("skill.json");
        let metadata_json = serde_json::to_string_pretty(&skill.metadata)
            .context("failed to serialize skill metadata")?;
        tokio::fs::write(&metadata_path, metadata_json)
            .await
            .context("failed to write skill metadata")?;

        // Save documentation
        let doc_path = skill_dir.join("SKILL.md");
        let documentation = Self::generate_markdown(&skill);
        tokio::fs::write(&doc_path, documentation)
            .await
            .context("failed to write skill documentation")?;

        info!(
            skill_name = %skill.metadata.name,
            skill_dir = ?skill_dir,
            "Skill saved successfully"
        );

        Ok(())
    }

    /// Load a skill by name.
    pub async fn load_skill(&self, name: &str) -> Result<Skill> {
        let skill_dir = self.skills_dir.join(name);

        // Try to find code file (python or javascript)
        let (code_path, language) = if tokio::fs::try_exists(skill_dir.join("skill.py"))
            .await
            .unwrap_or(false)
        {
            (skill_dir.join("skill.py"), "python3".to_string())
        } else if tokio::fs::try_exists(skill_dir.join("skill.js"))
            .await
            .unwrap_or(false)
        {
            (skill_dir.join("skill.js"), "javascript".to_string())
        } else {
            return Err(anyhow!("skill '{}' not found", name));
        };

        // Load code
        let code = tokio::fs::read_to_string(&code_path)
            .await
            .context("failed to read skill code")?;

        // Load metadata
        let metadata_path = skill_dir.join("skill.json");
        let metadata_json = tokio::fs::read_to_string(&metadata_path)
            .await
            .context("failed to read skill metadata")?;
        let metadata: SkillMetadata =
            serde_json::from_str(&metadata_json).context("failed to parse skill metadata")?;

        // Ensure language matches
        if metadata.language != language {
            return Err(anyhow!(
                "skill language mismatch: expected {}, found {}",
                metadata.language,
                language
            ));
        }

        debug!(
            skill_name = %name,
            language = %language,
            "Skill loaded successfully"
        );

        Ok(Skill { metadata, code })
    }

    /// List all available skills.
    pub async fn list_skills(&self) -> Result<Vec<SkillMetadata>> {
        if !tokio::fs::try_exists(&self.skills_dir)
            .await
            .unwrap_or(false)
        {
            return Ok(Vec::new());
        }

        let mut skills = Vec::new();
        let mut dir_entries = tokio::fs::read_dir(&self.skills_dir)
            .await
            .context("failed to read skills directory")?;

        while let Some(entry) = dir_entries
            .next_entry()
            .await
            .context("failed to read directory entry")?
        {
            let path = entry.path();
            if path.is_dir() {
                let metadata_path = path.join("skill.json");
                if let Ok(metadata_json) = tokio::fs::read_to_string(&metadata_path).await {
                    if let Ok(metadata) = serde_json::from_str::<SkillMetadata>(&metadata_json) {
                        skills.push(metadata);
                    }
                }
            }
        }

        Ok(skills)
    }

    /// Search skills by tag or keyword.
    pub async fn search_skills(&self, query: &str) -> Result<Vec<SkillMetadata>> {
        let skills = self.list_skills().await?;
        let query_lower = query.to_lowercase();

        Ok(skills
            .into_iter()
            .filter(|skill| {
                skill.name.to_lowercase().contains(&query_lower)
                    || skill.description.to_lowercase().contains(&query_lower)
                    || skill
                        .tags
                        .iter()
                        .any(|tag| tag.to_lowercase().contains(&query_lower))
            })
            .collect())
    }

    /// Delete a skill.
    pub async fn delete_skill(&self, name: &str) -> Result<()> {
        let skill_dir = self.skills_dir.join(name);
        tokio::fs::remove_dir_all(&skill_dir)
            .await
            .context("failed to delete skill")?;

        info!(skill_name = %name, "Skill deleted successfully");

        Ok(())
    }

    /// Check if a skill is compatible with given tool versions
    pub async fn check_skill_compatibility(
        &self,
        name: &str,
        tool_versions: std::collections::HashMap<String, crate::exec::ToolVersion>,
    ) -> Result<crate::exec::CompatibilityReport> {
        let skill = self.load_skill(name).await?;
        let checker = crate::exec::SkillCompatibilityChecker::new(
            skill.metadata.name.clone(),
            skill.metadata.tool_dependencies.clone(),
            tool_versions,
        );

        checker.check_compatibility()
    }

    /// Generate Markdown documentation for a skill.
    fn generate_markdown(skill: &Skill) -> String {
        let mut md = String::new();

        md.push_str(&format!("# {}\n\n", skill.metadata.name));
        md.push_str(&format!("{}\n\n", skill.metadata.description));

        if !skill.metadata.tags.is_empty() {
            md.push_str("**Tags:** ");
            md.push_str(&skill.metadata.tags.join(", "));
            md.push_str("\n\n");
        }

        md.push_str("## Language\n\n");
        md.push_str(&format!("`{}`\n\n", skill.metadata.language));

        if !skill.metadata.inputs.is_empty() {
            md.push_str("## Inputs\n\n");
            for param in &skill.metadata.inputs {
                let required = if param.required {
                    "required"
                } else {
                    "optional"
                };
                md.push_str(&format!(
                    "- `{name}` ({type}, {required}): {desc}\n",
                    name = param.name,
                    r#type = param.r#type,
                    desc = param.description
                ));
            }
            md.push_str("\n");
        }

        md.push_str("## Output\n\n");
        md.push_str(&format!("{}\n\n", skill.metadata.output));

        if !skill.metadata.examples.is_empty() {
            md.push_str("## Examples\n\n");
            for (i, example) in skill.metadata.examples.iter().enumerate() {
                if i > 0 {
                    md.push_str("\n");
                }
                md.push_str("```\n");
                md.push_str(example);
                md.push_str("\n```\n");
            }
        }

        md.push_str("\n## Code\n\n");
        md.push_str("```");
        md.push_str(&skill.metadata.language);
        md.push_str("\n");
        md.push_str(&skill.code);
        md.push_str("\n```\n");

        md
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_skill_metadata_serialization() {
        let metadata = SkillMetadata {
            name: "filter_files".to_string(),
            description: "Filter files by pattern".to_string(),
            language: "python3".to_string(),
            inputs: vec![ParameterDoc {
                name: "pattern".to_string(),
                r#type: "str".to_string(),
                description: "File pattern to match".to_string(),
                required: true,
            }],
            output: "List of matching filenames".to_string(),
            examples: vec!["filter_files(pattern='*.rs')".to_string()],
            tags: vec!["files".to_string(), "filtering".to_string()],
            created_at: "2025-01-01T00:00:00Z".to_string(),
            modified_at: "2025-01-01T00:00:00Z".to_string(),
            tool_dependencies: vec![],
        };

        let json = serde_json::to_string(&metadata).unwrap();
        let deserialized: SkillMetadata = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.name, metadata.name);
    }
}
