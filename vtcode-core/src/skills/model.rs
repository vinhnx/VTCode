use crate::skills::types::SkillManifest;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Skill scope indicating where the skill is defined
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SkillScope {
    /// User-level skill (~/.vtcode/skills)
    #[default]
    User,
    /// Repository-level skill (.agents/skills, .vtcode/skills, or .codex/skills in project root)
    Repo,
    /// System-level skill (embedded or system-wide)
    System,
    /// Admin-level skill (system administrator enforced)
    Admin,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillMetadata {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub short_description: Option<String>,
    pub path: PathBuf,
    pub scope: SkillScope,
    #[serde(default)]
    pub manifest: Option<SkillManifest>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillErrorInfo {
    pub path: PathBuf,
    pub message: String,
}

#[derive(Debug, Clone, Default)]
pub struct SkillLoadOutcome {
    pub skills: Vec<SkillMetadata>,
    pub errors: Vec<SkillErrorInfo>,
}
