use super::config::{AgentPersonality, ResponseStyle};
use crate::config::types::{ResolvedShellPromptProfile, ShellPromptProfile};
use once_cell::sync::Lazy;

static TOOL_USAGE_PROMPT: Lazy<String> = Lazy::new(|| {
    PromptTemplates::tool_usage_prompt_for_profile(ShellPromptProfile::Auto.resolve_for_current_platform())
});

/// Prompt template collection
pub struct PromptTemplates;

impl PromptTemplates {
    /// Get base system prompt
    pub fn base_system_prompt() -> &'static str {
        "You are a helpful AI coding assistant. You provide accurate, helpful responses and can execute tools to assist with coding tasks."
    }

    /// Get personality-specific prompt addition
    pub fn personality_prompt(personality: &AgentPersonality) -> &'static str {
        match personality {
            AgentPersonality::Professional => "Maintain a professional, focused approach to problem-solving.",
            AgentPersonality::Friendly => "Be friendly and encouraging while helping with coding tasks.",
            AgentPersonality::Technical => "Provide detailed technical explanations and focus on best practices.",
            AgentPersonality::Creative => "Think creatively and suggest innovative solutions to problems.",
        }
    }

    /// Get response style prompt addition
    pub fn response_style_prompt(style: &ResponseStyle) -> &'static str {
        match style {
            ResponseStyle::Concise => {
                "Lead with the conclusion. Include the evidence needed to support it, any material caveat, and the next action. Omit secondary detail and repetition."
            }
            ResponseStyle::Detailed => "Provide detailed explanations and comprehensive answers.",
            ResponseStyle::Conversational => "Use a conversational tone and explain concepts clearly.",
            ResponseStyle::Technical => "Focus on technical accuracy and include relevant implementation details.",
        }
    }

    /// Get tool usage prompt
    pub fn tool_usage_prompt() -> &'static str {
        TOOL_USAGE_PROMPT.as_str()
    }

    /// Get tool usage prompt for a resolved shell profile.
    pub fn tool_usage_prompt_for_profile(profile: ResolvedShellPromptProfile) -> String {
        match profile {
            ResolvedShellPromptProfile::UnixLike => {
                "Tools: use exec_command.cmd for Unix-like shell commands, including `ls`, `rg`, `find`, `cat`, `sed`, `awk`, build tools, test tools, and validation; use write_stdin for active command sessions; use apply_patch for file edits when exposed by the model. VT Code does not rewrite GNU flags for macOS BSD tools.".to_string()
            }
            ResolvedShellPromptProfile::PowerShell => {
                "Tools: use exec_command.cmd for native PowerShell commands, including `Get-ChildItem`, `Select-String`, `Get-Content`, `Where-Object`, build tools, test tools, and validation; use write_stdin for active command sessions; use apply_patch for file edits when exposed by the model. Use WSL for Unix-like workflows on Windows; VT Code does not translate Unix command flags to PowerShell.".to_string()
            }
        }
    }

    /// Get workspace context prompt
    pub fn workspace_context_prompt() -> &'static str {
        "Work within project workspace. Consider existing code structure."
    }

    /// Get safety guidelines prompt
    pub fn safety_guidelines_prompt() -> &'static str {
        "Safety: Follow permissions, confirm destructive ops, retry tool errors with corrected args."
    }

    /// Get pagination guidelines prompt
    pub fn pagination_guidelines_prompt() -> &'static str {
        "Pagination: per_page=50 default, reduce to 25 for large dirs, check 'has_more' flag."
    }

    /// Get skills available prompt (inspired by OpenAI Codex)
    pub fn skills_available_prompt() -> &'static str {
        "## Skills\nSpecialized capabilities from .agents/skills/. Use list_skills to discover skills by name and description, load_skill to activate, and load_skill_resource for deeper assets. For deterministic workflows, explicitly say `Use the <skill> skill`."
    }
}

#[cfg(test)]
mod tests {
    use super::PromptTemplates;
    use crate::config::types::ResolvedShellPromptProfile;

    #[test]
    fn skills_prompt_mentions_description_routing() {
        let prompt = PromptTemplates::skills_available_prompt();
        assert!(prompt.contains("name and description"));
        assert!(prompt.contains("Use the <skill> skill"));
    }

    #[test]
    fn tool_usage_prompt_prefers_codex_baseline_tools() {
        let prompt = PromptTemplates::tool_usage_prompt_for_profile(ResolvedShellPromptProfile::UnixLike);
        assert!(prompt.contains("exec_command"));
        assert!(prompt.contains("exec_command.cmd"));
        assert!(prompt.contains("write_stdin"));
        assert!(prompt.contains("apply_patch"));
        for command in ["ls", "rg", "find", "cat", "sed", "awk"] {
            assert!(
                prompt.contains(&format!("`{command}`")),
                "{command} should be shown as an exec_command.cmd example"
            );
        }
        assert!(prompt.contains("build tools"));
        assert!(prompt.contains("test tools"));
        assert!(!prompt.contains("command_session"));
        assert!(!prompt.contains("file_operation"));
        assert!(!prompt.contains("search_dispatch"));
        assert!(!prompt.contains("read_file"));
        assert!(!prompt.contains("write_file"));
    }

    #[test]
    fn tool_usage_prompt_supports_powershell_profile() {
        let prompt = PromptTemplates::tool_usage_prompt_for_profile(ResolvedShellPromptProfile::PowerShell);

        assert!(prompt.contains("native PowerShell commands"));
        assert!(prompt.contains("`Get-ChildItem`"));
        assert!(prompt.contains("`Select-String`"));
        assert!(prompt.contains("WSL"));
        assert!(prompt.contains("does not translate Unix command flags to PowerShell"));
        assert!(prompt.contains("write_stdin"));
        assert!(prompt.contains("apply_patch"));
        assert!(!prompt.contains("command_session"));
    }
}
