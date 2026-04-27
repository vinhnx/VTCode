use crate::skills::types::SkillManifest;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Skill scope indicating where the skill is defined
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default, Hash)]
#[serde(rename_all = "snake_case")]
pub enum SkillScope {
    /// User-level skill (`~/.agents/skills`)
    #[default]
    User,
    /// Repository-level skill (`.agents/skills`)
    Repo,
    /// System-level bundled skill
    System,
    /// Admin-level skill (`/etc/codex/skills`)
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
    pub manifest: Option<Box<SkillManifest>>,
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

#[cfg(test)]
mod tests {
    use super::SkillMetadata;
    use crate::skills::types::SkillManifest;

    #[test]
    fn boxed_manifest_is_smaller_than_inline_option() {
        use std::mem::size_of;

        assert!(size_of::<Option<Box<SkillManifest>>>() < size_of::<Option<SkillManifest>>());
        assert!(size_of::<SkillMetadata>() < size_of::<SkillMetadataInlineManifest>());
    }

    #[allow(dead_code)]
    struct SkillMetadataInlineManifest {
        name: String,
        description: String,
        short_description: Option<String>,
        path: std::path::PathBuf,
        scope: super::SkillScope,
        manifest: Option<SkillManifest>,
    }
}
