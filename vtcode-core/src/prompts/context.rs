use crate::config::ConfigManager;
use crate::config::types::CapabilityLevel;
use crate::ide_context::EditorContextSnapshot;
use crate::skills::manager::SkillsManager;
use crate::skills::model::SkillMetadata;
use crate::tools::search_runtime::snapshot_for_workspace;
use std::path::Path;
use std::path::PathBuf;

/// Context information for prompt generation
#[derive(Debug, Clone, Default)]
pub struct PromptContext {
    /// Current workspace path
    pub workspace: Option<PathBuf>,
    /// Detected programming languages
    pub languages: Vec<String>,
    /// Project type (if detected)
    pub project_type: Option<String>,
    /// Available tools
    pub available_tools: Vec<String>,
    /// Available skills (name: description)
    pub available_skills: Vec<(String, String)>,
    /// Available skill metadata for lean prompt rendering
    pub available_skill_metadata: Vec<SkillMetadata>,
    /// User preferences
    pub user_preferences: Option<UserPreferences>,
    /// Capability level (inferred from tools or explicitly set)
    pub capability_level: Option<CapabilityLevel>,
    /// Current working directory (different from workspace root)
    pub current_directory: Option<PathBuf>,
    /// Active IDE/editor context snapshot when available.
    pub editor_context: Option<EditorContextSnapshot>,
    /// Skip standard instruction blocks (project docs, user instructions)
    /// Used when these will be provided elsewhere (e.g. unified block in runloop)
    pub skip_standard_instructions: bool,
}

/// User preferences for prompt customization
#[derive(Debug, Clone)]
pub struct UserPreferences {
    /// Preferred programming languages
    pub preferred_languages: Vec<String>,
    /// Coding style preferences
    pub coding_style: Option<String>,
    /// Framework preferences
    pub preferred_frameworks: Vec<String>,
}

impl PromptContext {
    /// Create context from workspace
    pub fn from_workspace(workspace: PathBuf) -> Self {
        Self {
            workspace: Some(workspace),
            ..Default::default()
        }
    }

    /// Add detected language
    pub fn add_language(&mut self, language: String) {
        if !self.languages.contains(&language) {
            self.languages.push(language);
        }
    }

    /// Set project type
    pub fn set_project_type(&mut self, project_type: String) {
        self.project_type = Some(project_type);
    }

    /// Add available tool
    pub fn add_tool(&mut self, tool: String) {
        if !self.available_tools.contains(&tool) {
            self.available_tools.push(tool);
        }
    }

    /// Add available skill
    pub fn add_skill(&mut self, name: String, description: String) {
        if !self.available_skills.iter().any(|(n, _)| n == &name) {
            self.available_skills.push((name, description));
        }
    }

    /// Add available skill metadata
    pub fn add_skill_metadata(&mut self, metadata: SkillMetadata) {
        self.add_skill(metadata.name.clone(), metadata.description.clone());
        if !self
            .available_skill_metadata
            .iter()
            .any(|skill| skill.name == metadata.name)
        {
            self.available_skill_metadata.push(metadata);
        }
    }

    /// Add multiple skill metadata entries
    pub fn add_skill_metadata_entries(&mut self, skills: Vec<SkillMetadata>) {
        for skill in skills {
            self.add_skill_metadata(skill);
        }
    }

    /// Add multiple skills
    pub fn add_skills(&mut self, skills: Vec<(String, String)>) {
        for (name, description) in skills {
            self.add_skill(name, description);
        }
    }

    /// Set capability level explicitly
    pub fn set_capability_level(&mut self, level: CapabilityLevel) {
        self.capability_level = Some(level);
    }

    /// Infer capability level from available tools
    pub fn infer_capability_level(&mut self) {
        self.capability_level = Some(crate::prompts::guidelines::infer_capability_level(
            &self.available_tools,
        ));
    }

    /// Set current working directory
    pub fn set_current_directory(&mut self, dir: PathBuf) {
        self.current_directory = Some(dir);
    }

    pub fn set_editor_context(&mut self, snapshot: Option<EditorContextSnapshot>) {
        self.editor_context = snapshot;
    }

    pub fn load_available_skills(&mut self) {
        let home_dir = default_vtcode_home_dir();
        self.load_available_skills_with_home_dir(home_dir.as_deref());
    }

    pub(crate) fn load_available_skills_with_home_dir(&mut self, home_dir: Option<&Path>) {
        let Some(workspace) = self.workspace.as_deref() else {
            return;
        };
        let Some(home_dir) = home_dir else {
            return;
        };

        let bundled_skills_enabled = ConfigManager::load_from_workspace(workspace)
            .map(|manager| manager.config().skills.bundled.enabled)
            .unwrap_or(true);
        let manager = SkillsManager::new_with_bundled_skills_enabled(
            home_dir.to_path_buf(),
            bundled_skills_enabled,
        );
        let outcome = manager.skills_metadata_lightweight(workspace);
        self.add_skill_metadata_entries(outcome.skills);
    }

    /// Build prompt context from a workspace and the tool names exposed to the model.
    pub fn from_workspace_tools(
        workspace: impl AsRef<Path>,
        available_tools: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        let mut context = Self {
            workspace: Some(workspace.as_ref().to_path_buf()),
            skip_standard_instructions: false,
            ..Default::default()
        };

        if let Ok(cwd) = std::env::current_dir() {
            context.set_current_directory(cwd);
        }

        if let Ok(snapshot) = EditorContextSnapshot::read_from_env() {
            context.set_editor_context(snapshot);
        }

        for language in snapshot_for_workspace(workspace.as_ref()).workspace_languages {
            context.add_language(language);
        }

        for tool in available_tools {
            context.add_tool(tool.into());
        }

        if !context.available_tools.is_empty() {
            context.infer_capability_level();
        }

        context
    }
}

fn default_vtcode_home_dir() -> Option<PathBuf> {
    std::env::var_os("VTCODE_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|home| home.join(".vtcode")))
}

#[cfg(test)]
mod tests {
    use super::PromptContext;
    use std::fs;
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn write_skill(skill_dir: &std::path::Path, name: &str, description: &str) {
        fs::create_dir_all(skill_dir).expect("create skill dir");
        fs::write(
            skill_dir.join("SKILL.md"),
            format!(
                "---\nname: {name}\ndescription: {description}\nwhen-to-use: Use this skill.\nwhen-not-to-use: Avoid this skill.\n---\n# {name}\n"
            ),
        )
        .expect("write skill");
    }

    #[test]
    fn from_workspace_tools_populates_workspace_and_tools() {
        let context = PromptContext::from_workspace_tools(
            PathBuf::from("/tmp/vtcode"),
            ["unified_search", "unified_exec"],
        );

        assert_eq!(context.workspace, Some(PathBuf::from("/tmp/vtcode")));
        assert_eq!(context.available_tools.len(), 2);
        assert!(context.capability_level.is_some());
        assert!(context.current_directory.is_some());
    }

    #[test]
    fn from_workspace_tools_populates_detected_languages() {
        let workspace = TempDir::new().expect("workspace tempdir");
        fs::create_dir_all(workspace.path().join("src")).expect("create src");
        fs::create_dir_all(workspace.path().join("web")).expect("create web");
        fs::write(workspace.path().join("src/lib.rs"), "fn alpha() {}\n").expect("write rust");
        fs::write(workspace.path().join("web/app.ts"), "const app = 1;\n").expect("write ts");

        let context = PromptContext::from_workspace_tools(workspace.path(), ["unified_search"]);

        assert_eq!(
            context.languages,
            vec!["Rust".to_string(), "TypeScript".to_string()]
        );
    }

    #[test]
    fn load_available_skills_discovers_repo_and_system_skills() {
        let workspace = TempDir::new().expect("workspace tempdir");
        fs::create_dir(workspace.path().join(".git")).expect("create git dir");
        write_skill(
            &workspace.path().join(".vtcode/skills/repo-skill"),
            "repo-skill",
            "Repo-local skill",
        );

        let home = TempDir::new().expect("home tempdir");
        let mut context = PromptContext::from_workspace_tools(workspace.path(), ["unified_search"]);

        assert!(context.available_skill_metadata.is_empty());

        context.load_available_skills_with_home_dir(Some(home.path()));

        let skill_names = context
            .available_skill_metadata
            .iter()
            .map(|skill| skill.name.as_str())
            .collect::<Vec<_>>();

        assert!(skill_names.contains(&"repo-skill"));
        assert!(skill_names.contains(&"skill-creator"));
    }
}
