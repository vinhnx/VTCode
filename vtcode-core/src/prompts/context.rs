use crate::config::types::CapabilityLevel;
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
    /// User preferences
    pub user_preferences: Option<UserPreferences>,
    /// Capability level (inferred from tools or explicitly set)
    pub capability_level: Option<CapabilityLevel>,
    /// Current working directory (different from workspace root)
    pub current_directory: Option<PathBuf>,
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

        for tool in available_tools {
            context.add_tool(tool.into());
        }

        if !context.available_tools.is_empty() {
            context.infer_capability_level();
        }

        context
    }
}

#[cfg(test)]
mod tests {
    use super::PromptContext;
    use std::path::PathBuf;

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
}
