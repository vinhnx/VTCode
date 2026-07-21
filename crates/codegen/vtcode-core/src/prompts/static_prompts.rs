//! Static prompt text, constants, and cached static prompt builders.

use std::sync::OnceLock;

use crate::config::types::SystemPromptMode;
use crate::prompts::system::{
    CONTRACT_HEADER, DEFAULT_OPERATING_PROFILE_DELTA, DEFAULT_SPECIFIC_LINES, LIGHTWEIGHT_OPERATING_PROFILE_DELTA,
    MINIMAL_OPERATING_PROFILE_DELTA, MINIMAL_SPECIFIC_LINES, PROMPT_INTRO, PROMPT_TITLE, SHARED_CONTRACT_LINES,
    SPECIALIZED_OPERATING_PROFILE_DELTA,
};

/// Agent identity labels for the system prompt.
/// Maps agent names to human-readable identity strings that combine VT Code
/// with the active agent mode, so the LLM knows its role.
pub fn agent_identity_label(agent_name: &str) -> String {
    match agent_name {
        "build" => "VT Code (Build mode)".to_string(),
        "auto" => "VT Code (Auto mode)".to_string(),
        "duck" => "VT Code (Duck mode)".to_string(),
        "plan" => "VT Code (Plan mode)".to_string(),
        "explorer" => "VT Code (Explorer mode)".to_string(),
        "worker" => "VT Code (Worker mode)".to_string(),
        other => format!("VT Code ({other})"),
    }
}

static DEFAULT_SYSTEM_PROMPT: OnceLock<String> = OnceLock::new();
static MINIMAL_SYSTEM_PROMPT: OnceLock<String> = OnceLock::new();
static DEFAULT_LIGHTWEIGHT_PROMPT: OnceLock<String> = OnceLock::new();
static DEFAULT_SPECIALIZED_PROMPT: OnceLock<String> = OnceLock::new();

pub fn default_system_prompt() -> &'static str {
    static_profile_prompt(SystemPromptMode::Default)
}

pub fn minimal_system_prompt() -> &'static str {
    static_profile_prompt(SystemPromptMode::Minimal)
}

pub fn default_lightweight_prompt() -> &'static str {
    static_profile_prompt(SystemPromptMode::Lightweight)
}

pub fn specialized_system_prompt() -> &'static str {
    static_profile_prompt(SystemPromptMode::Specialized)
}

pub fn minimal_instruction_text() -> String {
    minimal_system_prompt().to_string()
}

pub fn lightweight_instruction_text() -> String {
    default_lightweight_prompt().to_string()
}

pub fn specialized_instruction_text() -> String {
    specialized_system_prompt().to_string()
}

pub fn static_profile_prompt(prompt_mode: SystemPromptMode) -> &'static str {
    match prompt_mode {
        SystemPromptMode::Default => DEFAULT_SYSTEM_PROMPT.get_or_init(|| {
            let mut prompt = String::new();
            prompt.push_str(PROMPT_TITLE);
            prompt.push_str("\n\n");
            prompt.push_str(PROMPT_INTRO);
            prompt.push_str("\n\n");
            prompt.push_str(CONTRACT_HEADER);
            prompt.push_str("\n\n");
            for line in SHARED_CONTRACT_LINES {
                prompt.push_str("- ");
                prompt.push_str(line);
                prompt.push('\n');
            }
            prompt.pop();
            prompt.push('\n');
            for line in DEFAULT_SPECIFIC_LINES {
                prompt.push_str("- ");
                prompt.push_str(line);
                prompt.push('\n');
            }
            prompt.pop();
            prompt.push('\n');
            prompt.push('\n');
            prompt.push_str(DEFAULT_OPERATING_PROFILE_DELTA);
            prompt
        }),
        SystemPromptMode::Minimal => MINIMAL_SYSTEM_PROMPT.get_or_init(|| {
            let mut prompt = String::new();
            prompt.push_str(PROMPT_TITLE);
            prompt.push_str("\n\n");
            prompt.push_str(PROMPT_INTRO);
            prompt.push_str("\n\n");
            prompt.push_str(CONTRACT_HEADER);
            prompt.push_str("\n\n");
            for line in SHARED_CONTRACT_LINES {
                prompt.push_str("- ");
                prompt.push_str(line);
                prompt.push('\n');
            }
            prompt.pop();
            prompt.push('\n');
            for line in MINIMAL_SPECIFIC_LINES {
                prompt.push_str("- ");
                prompt.push_str(line);
                prompt.push('\n');
            }
            prompt.pop();
            prompt.push('\n');
            prompt.push('\n');
            prompt.push_str(MINIMAL_OPERATING_PROFILE_DELTA);
            prompt
        }),
        SystemPromptMode::Lightweight => DEFAULT_LIGHTWEIGHT_PROMPT.get_or_init(|| {
            let mut prompt = String::new();
            prompt.push_str(PROMPT_TITLE);
            prompt.push_str("\n\n");
            prompt.push_str(PROMPT_INTRO);
            prompt.push_str("\n\n");
            prompt.push_str(CONTRACT_HEADER);
            prompt.push_str("\n\n");
            for line in SHARED_CONTRACT_LINES {
                prompt.push_str("- ");
                prompt.push_str(line);
                prompt.push('\n');
            }
            prompt.pop();
            prompt.push('\n');
            for line in DEFAULT_SPECIFIC_LINES {
                prompt.push_str("- ");
                prompt.push_str(line);
                prompt.push('\n');
            }
            prompt.pop();
            prompt.push('\n');
            prompt.push('\n');
            prompt.push_str(LIGHTWEIGHT_OPERATING_PROFILE_DELTA);
            prompt
        }),
        SystemPromptMode::Specialized => DEFAULT_SPECIALIZED_PROMPT.get_or_init(|| {
            let mut prompt = String::new();
            prompt.push_str(PROMPT_TITLE);
            prompt.push_str("\n\n");
            prompt.push_str(PROMPT_INTRO);
            prompt.push_str("\n\n");
            prompt.push_str(CONTRACT_HEADER);
            prompt.push_str("\n\n");
            for line in SHARED_CONTRACT_LINES {
                prompt.push_str("- ");
                prompt.push_str(line);
                prompt.push('\n');
            }
            prompt.pop();
            prompt.push('\n');
            for line in DEFAULT_SPECIFIC_LINES {
                prompt.push_str("- ");
                prompt.push_str(line);
                prompt.push('\n');
            }
            prompt.pop();
            prompt.push('\n');
            prompt.push('\n');
            prompt.push_str(SPECIALIZED_OPERATING_PROFILE_DELTA);
            prompt
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn static_prompts_include_required_sections() {
        let modes = [
            (SystemPromptMode::Default, default_system_prompt()),
            (SystemPromptMode::Minimal, minimal_system_prompt()),
            (SystemPromptMode::Lightweight, default_lightweight_prompt()),
            (SystemPromptMode::Specialized, specialized_system_prompt()),
        ];
        for (mode, prompt) in modes {
            assert!(prompt.contains(PROMPT_TITLE), "{mode:?} missing title");
            assert!(prompt.contains(CONTRACT_HEADER), "{mode:?} missing contract");
            assert!(prompt.contains("AGENTS.md"), "{mode:?} missing AGENTS.md ref");
        }
    }
}
