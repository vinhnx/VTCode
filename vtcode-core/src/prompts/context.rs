use crate::config::types::CapabilityLevel;
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
}
