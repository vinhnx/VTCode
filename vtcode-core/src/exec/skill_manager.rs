//! Skill persistence and management for reusable code functions.
//!
//! Agents can save working code implementations as reusable "skills" in the
//! `.agents/skills/` directory. Legacy `.vtcode/skills/` locations remain
//! readable for backward compatibility. Each skill includes:
//! - Function implementation (Python or JavaScript)
//! - `SKILL.md` documentation
//! - Input/output type hints
//! - Usage examples
//!
//! Skills can be loaded across conversations and shared with other agents.

use crate::exec::ToolDependency;
use crate::utils::error_messages::*;
use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use std::fmt::Write;
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

#[derive(Debug, Clone, Copy)]
enum SkillOrigin {
    Primary,
    Legacy,
}

#[derive(Debug, Clone)]
struct SkillEntry {
    metadata: SkillMetadata,
    origin: SkillOrigin,
}

/// Manager for skill storage and retrieval.
#[derive(Clone)]
pub struct SkillManager {
    skills_dir: PathBuf,
    legacy_skills_dir: PathBuf,
}

impl SkillManager {
    /// Create a new skill manager.
    pub fn new(workspace_root: &Path) -> Self {
        Self {
            skills_dir: workspace_root.join(".agents").join("skills"),
            legacy_skills_dir: workspace_root.join(".vtcode").join("skills"),
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
            .context(ERR_CREATE_SKILLS_DIR)?;

        let skill_dir = self.skills_dir.join(&skill.metadata.name);
        tokio::fs::create_dir_all(&skill_dir)
            .await
            .context(ERR_CREATE_SKILL_DIR)?;

        // Save code file
        let code_filename = match skill.metadata.language.as_str() {
            "python3" | "python" => "skill.py",
            "javascript" | "js" => "skill.js",
            lang => return Err(anyhow!("unsupported language: {}", lang)),
        };

        let code_path = skill_dir.join(code_filename);
        tokio::fs::write(&code_path, &skill.code)
            .await
            .context(ERR_WRITE_SKILL_CODE)?;

        // Save metadata
        let metadata_path = skill_dir.join("skill.json");
        let metadata_json =
            serde_json::to_string_pretty(&skill.metadata).context(ERR_SERIALIZE_METADATA)?;
        tokio::fs::write(&metadata_path, metadata_json)
            .await
            .context(ERR_WRITE_SKILL_METADATA)?;

        // Save documentation
        let doc_path = skill_dir.join("SKILL.md");
        let documentation = Self::generate_markdown(&skill);
        tokio::fs::write(&doc_path, documentation)
            .await
            .context(ERR_WRITE_SKILL_DOCS)?;

        info!(
            skill_name = %skill.metadata.name,
            skill_dir = ?skill_dir,
            "Skill saved successfully"
        );

        // Regenerate index after saving new skill
        let _ = self.generate_index().await;

        Ok(())
    }

    /// Load a skill by name.
    pub async fn load_skill(&self, name: &str) -> Result<Skill> {
        let skill_dir = self.skills_dir.join(name);
        let legacy_skill_dir = self.legacy_skills_dir.join(name);

        // Try to find code file (python or javascript)
        let (code_path, language, skill_root) = if tokio::fs::try_exists(skill_dir.join("skill.py"))
            .await
            .unwrap_or(false)
        {
            (skill_dir.join("skill.py"), "python3", skill_dir)
        } else if tokio::fs::try_exists(skill_dir.join("skill.js"))
            .await
            .unwrap_or(false)
        {
            (skill_dir.join("skill.js"), "javascript", skill_dir)
        } else if tokio::fs::try_exists(legacy_skill_dir.join("skill.py"))
            .await
            .unwrap_or(false)
        {
            (
                legacy_skill_dir.join("skill.py"),
                "python3",
                legacy_skill_dir,
            )
        } else if tokio::fs::try_exists(legacy_skill_dir.join("skill.js"))
            .await
            .unwrap_or(false)
        {
            (
                legacy_skill_dir.join("skill.js"),
                "javascript",
                legacy_skill_dir,
            )
        } else {
            return Err(anyhow!("skill '{}' not found", name));
        };

        // Load code
        let code = tokio::fs::read_to_string(&code_path)
            .await
            .context(ERR_READ_SKILL_CODE)?;

        // Load metadata
        let metadata_path = skill_root.join("skill.json");
        let metadata_json = tokio::fs::read_to_string(&metadata_path)
            .await
            .context(ERR_READ_SKILL_METADATA)?;
        let metadata: SkillMetadata =
            serde_json::from_str(&metadata_json).context(ERR_PARSE_SKILL_METADATA)?;

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
        Ok(self
            .list_skills_with_origin()
            .await?
            .into_iter()
            .map(|entry| entry.metadata)
            .collect())
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
        let legacy_skill_dir = self.legacy_skills_dir.join(name);
        if tokio::fs::try_exists(&skill_dir).await.unwrap_or(false) {
            tokio::fs::remove_dir_all(&skill_dir)
                .await
                .context(ERR_DELETE_SKILL)?;
        } else if tokio::fs::try_exists(&legacy_skill_dir)
            .await
            .unwrap_or(false)
        {
            tokio::fs::remove_dir_all(&legacy_skill_dir)
                .await
                .context(ERR_DELETE_SKILL)?;
        } else {
            return Err(anyhow!("skill '{}' not found", name));
        }

        info!(skill_name = %name, "Skill deleted successfully");

        // Regenerate index after deletion
        let _ = self.generate_index().await;

        Ok(())
    }

    /// Generate INDEX.md with all skill names and descriptions
    ///
    /// This implements dynamic context discovery: agents can read the index
    /// to discover available skills, then load specific skills as needed.
    /// This is more token-efficient than loading all skill definitions.
    pub async fn generate_index(&self) -> Result<std::path::PathBuf> {
        let skills = self.list_skills_with_origin().await?;

        let mut content = String::new();
        content.push_str("# Skills Index\n\n");
        content.push_str("This file lists all available skills for dynamic discovery.\n");
        content.push_str(
            "Use `read_file` on individual skill directories for full documentation.\n\n",
        );
        if skills
            .iter()
            .any(|entry| matches!(entry.origin, SkillOrigin::Legacy))
        {
            content.push_str(
                "Legacy skills from `.vtcode/skills/` are included but deprecated. Move them to `.agents/skills/`.\n\n",
            );
        }

        if skills.is_empty() {
            content.push_str("*No skills available yet.*\n\n");
            content.push_str("Create skills using the `save_skill` tool.\n");
        } else {
            content.push_str("## Available Skills\n\n");
            content.push_str("| Name | Language | Description | Tags |\n");
            content.push_str("|------|----------|-------------|------|\n");

            for entry in &skills {
                let skill = &entry.metadata;
                let tags = if skill.tags.is_empty() {
                    "-".to_string()
                } else {
                    skill.tags.join(", ")
                };
                let desc = skill.description.replace('|', "\\|");
                let _ = writeln!(
                    content,
                    "| `{}` | {} | {} | {} |",
                    skill.name, skill.language, desc, tags
                );
            }

            content.push_str("\n## Quick Reference\n\n");
            for entry in &skills {
                let skill = &entry.metadata;
                let base_path = match entry.origin {
                    SkillOrigin::Primary => ".agents/skills",
                    SkillOrigin::Legacy => ".vtcode/skills",
                };
                let _ = writeln!(content, "### {}\n", skill.name);
                let _ = writeln!(content, "{}\n", skill.description);
                let _ = writeln!(
                    content,
                    "- **Language**: {}\n- **Path**: `{}/{}/SKILL.md`\n",
                    skill.language, base_path, skill.name
                );
            }
        }

        content.push_str("\n---\n");
        content.push_str("*Generated automatically. Do not edit manually.*\n");

        // Ensure directory exists
        tokio::fs::create_dir_all(&self.skills_dir)
            .await
            .context(ERR_CREATE_SKILLS_DIR)?;

        let index_path = self.skills_dir.join("INDEX.md");
        tokio::fs::write(&index_path, &content)
            .await
            .with_context(|| format!("Failed to write skills index: {}", index_path.display()))?;

        info!(
            skills_count = skills.len(),
            path = %index_path.display(),
            "Generated skills INDEX.md"
        );

        Ok(index_path)
    }

    /// Get the path to the INDEX.md file
    pub fn index_path(&self) -> std::path::PathBuf {
        self.skills_dir.join("INDEX.md")
    }

    async fn list_skills_with_origin(&self) -> Result<Vec<SkillEntry>> {
        let mut entries = Vec::new();
        let mut seen = std::collections::HashSet::new();

        let primary = self
            .read_skills_from_dir(&self.skills_dir)
            .await
            .context(ERR_READ_SKILLS_DIR)?;
        for metadata in primary {
            seen.insert(metadata.name.clone());
            entries.push(SkillEntry {
                metadata,
                origin: SkillOrigin::Primary,
            });
        }

        let legacy = self
            .read_skills_from_dir(&self.legacy_skills_dir)
            .await
            .context(ERR_READ_SKILLS_DIR)?;
        for metadata in legacy {
            if seen.contains(&metadata.name) {
                continue;
            }
            entries.push(SkillEntry {
                metadata,
                origin: SkillOrigin::Legacy,
            });
        }

        Ok(entries)
    }

    async fn read_skills_from_dir(&self, dir: &Path) -> Result<Vec<SkillMetadata>> {
        if !tokio::fs::try_exists(dir).await.unwrap_or(false) {
            return Ok(Vec::new());
        }

        // Pre-allocate skills vector - typically 10-20 skills per directory
        let mut skills = Vec::with_capacity(16);
        let mut dir_entries = tokio::fs::read_dir(dir)
            .await
            .context(ERR_READ_SKILLS_DIR)?;

        while let Some(entry) = dir_entries.next_entry().await.context(ERR_READ_DIR_ENTRY)? {
            let path = entry.path();
            if path.is_dir() {
                let metadata_path = path.join("skill.json");
                if let Ok(metadata_json) = tokio::fs::read_to_string(&metadata_path).await
                    && let Ok(metadata) = serde_json::from_str::<SkillMetadata>(&metadata_json)
                {
                    skills.push(metadata);
                }
            }
        }

        Ok(skills)
    }

    /// Check if a skill is compatible with given tool versions
    pub async fn check_skill_compatibility(
        &self,
        name: &str,
        tool_versions: std::collections::HashMap<String, crate::exec::ToolVersion>,
    ) -> Result<crate::exec::CompatibilityReport> {
        let skill = self.load_skill(name).await?;
        let checker = crate::exec::SkillCompatibilityChecker::new(
            skill.metadata.name,
            skill.metadata.tool_dependencies,
            tool_versions,
        );

        checker.check_compatibility()
    }

    /// Generate Markdown documentation for a skill.
    fn generate_markdown(skill: &Skill) -> String {
        // Reserve an estimated capacity to avoid multiple reallocations.
        let mut md =
            String::with_capacity(1024 + skill.code.len() + skill.metadata.description.len());

        let _ = writeln!(md, "# {}\n", skill.metadata.name);
        let _ = writeln!(md, "{}\n", skill.metadata.description);

        if !skill.metadata.tags.is_empty() {
            md.push_str("**Tags:** ");
            md.push_str(&skill.metadata.tags.join(", "));
            md.push_str("\n\n");
        }

        md.push_str("## Language\n\n");
        let _ = writeln!(md, "`{}`\n", skill.metadata.language);

        if !skill.metadata.inputs.is_empty() {
            md.push_str("## Inputs\n\n");
            for param in &skill.metadata.inputs {
                let required = if param.required {
                    "required"
                } else {
                    "optional"
                };
                let _ = writeln!(
                    md,
                    "- `{name}` ({type}, {required}): {desc}",
                    name = param.name,
                    r#type = param.r#type,
                    desc = param.description
                );
            }
            md.push('\n');
        }

        md.push_str("## Output\n\n");
        let _ = writeln!(md, "{}\n", skill.metadata.output);

        if !skill.metadata.examples.is_empty() {
            md.push_str("## Examples\n\n");
            for (i, example) in skill.metadata.examples.iter().enumerate() {
                if i > 0 {
                    md.push('\n');
                }
                md.push_str("```\n");
                md.push_str(example);
                md.push_str("\n```\n");
            }
        }

        md.push('\n');
        md.push_str("## Code\n\n");
        md.push_str("```");
        md.push_str(&skill.metadata.language);
        md.push('\n');
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
            name: "filter_files".into(),
            description: "Filter files by pattern".into(),
            language: "python3".into(),
            inputs: vec![ParameterDoc {
                name: "pattern".into(),
                r#type: "str".into(),
                description: "File pattern to match".into(),
                required: true,
            }],
            output: "List of matching filenames".into(),
            examples: vec!["filter_files(pattern='*.rs')".into()],
            tags: vec!["files".into(), "filtering".into()],
            created_at: "2025-01-01T00:00:00Z".into(),
            modified_at: "2025-01-01T00:00:00Z".into(),
            tool_dependencies: vec![],
        };

        let json = serde_json::to_string(&metadata).expect("Skill metadata should serialize");
        let deserialized: SkillMetadata =
            serde_json::from_str(&json).expect("Serialized skill metadata should deserialize");
        assert_eq!(deserialized.name, metadata.name);
    }
}
