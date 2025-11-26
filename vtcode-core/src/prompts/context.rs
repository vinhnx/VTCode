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
    /// User preferences
    pub user_preferences: Option<UserPreferences>,
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
}
