use super::config::{AgentPersonality, ResponseStyle};

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
            AgentPersonality::Professional => {
                "Maintain a professional, focused approach to problem-solving."
            }
            AgentPersonality::Friendly => {
                "Be friendly and encouraging while helping with coding tasks."
            }
            AgentPersonality::Technical => {
                "Provide detailed technical explanations and focus on best practices."
            }
            AgentPersonality::Creative => {
                "Think creatively and suggest innovative solutions to problems."
            }
        }
    }

    /// Get response style prompt addition
    pub fn response_style_prompt(style: &ResponseStyle) -> &'static str {
        match style {
            ResponseStyle::Concise => "Keep responses concise and to the point.",
            ResponseStyle::Detailed => "Provide detailed explanations and comprehensive answers.",
            ResponseStyle::Conversational => {
                "Use a conversational tone and explain concepts clearly."
            }
            ResponseStyle::Technical => {
                "Focus on technical accuracy and include relevant implementation details."
            }
        }
    }

    /// Get tool usage prompt
    pub fn tool_usage_prompt() -> &'static str {
        "Tools: unified_search (grep/list/structural/tools/errors/agent/web/skill; default to structural for code search and grep for plain text), unified_file (read/write/edit/patch), unified_exec (run/write/poll/inspect/list/close/code), request_user_input (interactive chat when enabled; always available in Plan mode), and apply_patch (first-class patch tool when exposed by the model). Paths for unified_search and unified_file are relative to the workspace root. Use unified_search `action=list` for file discovery and unified_file `action=read` for file contents; avoid using unified_exec with `ls`, `find`, `cat`, or `sed` for ordinary repo browsing when the public tools can express the task. Treat read_file/write_file/edit_file/grep_file/PTy helpers as compatibility aliases or internal routes; prefer the canonical public tools, and prefer `rg` over shell `grep` when command search is required."
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
        "## Skills\nSpecialized capabilities from .agents/skills/. Use list_skills to discover skills by name, description, and routing hints, load_skill to activate, and load_skill_resource for deeper assets. For deterministic workflows, explicitly say `Use the <skill> skill`."
    }
}

#[cfg(test)]
mod tests {
    use super::PromptTemplates;

    #[test]
    fn skills_prompt_mentions_routing_hints() {
        let prompt = PromptTemplates::skills_available_prompt();
        assert!(prompt.contains("routing hints"));
        assert!(prompt.contains("Use the <skill> skill"));
    }

    #[test]
    fn tool_usage_prompt_prefers_public_repo_browsing_tools() {
        let prompt = PromptTemplates::tool_usage_prompt();
        assert!(prompt.contains("relative to the workspace root"));
        assert!(prompt.contains("action=list"));
        assert!(prompt.contains("action=read"));
        assert!(prompt.contains("avoid using unified_exec"));
    }
}
