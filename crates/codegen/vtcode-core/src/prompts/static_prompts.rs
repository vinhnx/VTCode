//! Static prompt text, constants, and cached static prompt builders.

use std::sync::OnceLock;

use crate::config::types::SystemPromptMode;
use crate::prompts::runtime_guidance::{RUNTIME_GUIDANCE_MAX_ESTIMATED_TOKENS, RUNTIME_GUIDANCE_SECTION};
use crate::prompts::system::{
    CONTRACT_HEADER, DEFAULT_OPERATING_PROFILE_DELTA, DEFAULT_SPECIFIC_LINES, LIGHTWEIGHT_OPERATING_PROFILE_DELTA,
    MINIMAL_OPERATING_PROFILE_DELTA, MINIMAL_SPECIFIC_LINES, PROMPT_INTRO, PROMPT_ROLE_PARAGRAPH, PROMPT_TITLE,
    SHARED_CONTRACT_LINES, SPECIALIZED_OPERATING_PROFILE_DELTA,
};
use vtcode_commons::estimate_tokens;

/// Agent identity labels for the system prompt.
/// Maps agent names to human-readable identity strings that combine VT Code
/// with the active agent mode, so the LLM knows its role.
pub fn agent_identity_label(agent_name: &str) -> String {
    match agent_name {
        "build" => "VT Code (Build mode)".to_string(),
        "auto" => "VT Code (Auto mode)".to_string(),
        "coordinator" => "VT Code (Coordinator mode)".to_string(),
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

fn append_runtime_guidance(prompt: &mut String) {
    assert!(estimate_tokens(RUNTIME_GUIDANCE_SECTION) <= RUNTIME_GUIDANCE_MAX_ESTIMATED_TOKENS);
    prompt.push_str(RUNTIME_GUIDANCE_SECTION);
    prompt.push('\n');
}

fn append_contract_block(prompt: &mut String, specific_lines: &[&str]) {
    for line in SHARED_CONTRACT_LINES {
        prompt.push_str("- ");
        prompt.push_str(line);
        prompt.push('\n');
    }
    for line in specific_lines {
        prompt.push_str("- ");
        prompt.push_str(line);
        prompt.push('\n');
    }
    prompt.pop();
    prompt.push('\n');
    prompt.push('\n');
}

fn build_static_prompt(include_role: bool, specific_lines: &[&str], operating_delta: &str) -> String {
    let mut prompt = String::new();
    prompt.push_str(PROMPT_TITLE);
    prompt.push_str("\n\n");
    prompt.push_str(PROMPT_INTRO);
    prompt.push_str("\n\n");
    if include_role {
        prompt.push_str(PROMPT_ROLE_PARAGRAPH);
        prompt.push_str("\n\n");
    }
    append_runtime_guidance(&mut prompt);
    prompt.push_str(CONTRACT_HEADER);
    prompt.push_str("\n\n");
    append_contract_block(&mut prompt, specific_lines);
    prompt.push_str(operating_delta);
    prompt
}

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
        SystemPromptMode::Default => DEFAULT_SYSTEM_PROMPT
            .get_or_init(|| build_static_prompt(true, DEFAULT_SPECIFIC_LINES, DEFAULT_OPERATING_PROFILE_DELTA)),
        SystemPromptMode::Minimal => MINIMAL_SYSTEM_PROMPT
            .get_or_init(|| build_static_prompt(false, MINIMAL_SPECIFIC_LINES, MINIMAL_OPERATING_PROFILE_DELTA)),
        SystemPromptMode::Lightweight => DEFAULT_LIGHTWEIGHT_PROMPT
            .get_or_init(|| build_static_prompt(false, DEFAULT_SPECIFIC_LINES, LIGHTWEIGHT_OPERATING_PROFILE_DELTA)),
        SystemPromptMode::Specialized => DEFAULT_SPECIALIZED_PROMPT
            .get_or_init(|| build_static_prompt(true, DEFAULT_SPECIFIC_LINES, SPECIALIZED_OPERATING_PROFILE_DELTA)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::constants::prompt_budget;
    use crate::prompts::runtime_guidance::RUNTIME_GUIDANCE_SECTION;

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
            assert_eq!(
                prompt.matches(RUNTIME_GUIDANCE_SECTION).count(),
                1,
                "{mode:?} should include runtime guidance exactly once"
            );
            assert!(
                prompt.find(RUNTIME_GUIDANCE_SECTION) < prompt.find(CONTRACT_HEADER),
                "{mode:?} should place runtime guidance before the contract"
            );
            assert!(
                (estimate_tokens(prompt) as u64) <= prompt_budget::DEFAULT_MAX_SYSTEM_PROMPT_TOKENS,
                "{mode:?} static prompt should fit the default budget"
            );
        }
    }

    #[test]
    fn shared_contract_lines_and_runtime_bullets_appear_exactly_once_per_profile() {
        let runtime_bullets = RUNTIME_GUIDANCE_SECTION
            .lines()
            .filter(|line| line.starts_with("- "))
            .collect::<Vec<_>>();
        assert!(runtime_bullets.len() >= 10, "runtime guidance bullets were not found");
        let profiles = [
            (SystemPromptMode::Default, DEFAULT_SPECIFIC_LINES),
            (SystemPromptMode::Minimal, MINIMAL_SPECIFIC_LINES),
            (SystemPromptMode::Lightweight, DEFAULT_SPECIFIC_LINES),
            (SystemPromptMode::Specialized, DEFAULT_SPECIFIC_LINES),
        ];

        for (mode, specific_lines) in profiles {
            let prompt = static_profile_prompt(mode);
            for line in SHARED_CONTRACT_LINES.iter().chain(specific_lines) {
                let bullet = format!("- {line}\n");
                assert_eq!(
                    prompt.matches(line).count(),
                    1,
                    "{mode:?} should state contract line {line:?} exactly once"
                );
                assert_eq!(
                    prompt.matches(&bullet).count(),
                    1,
                    "{mode:?} should render {line:?} as one contract bullet"
                );
            }
            for bullet in &runtime_bullets {
                assert_eq!(
                    prompt.matches(bullet).count(),
                    1,
                    "{mode:?} should state runtime guidance bullet {bullet:?} exactly once"
                );
            }
        }
    }

    #[test]
    fn static_prompts_do_not_embed_maintainer_files() {
        let prompts = [
            default_system_prompt(),
            minimal_system_prompt(),
            default_lightweight_prompt(),
            specialized_system_prompt(),
        ];
        let maintainer_only_markers = [
            "Keep this file concise and under 150 lines",
            "vtcode-exec-events::ThreadEvent",
            "RUSTFLAGS: \"-D warnings\"",
            "Cargo workspace, ~30 crates",
            "Root summary",
        ];

        for prompt in prompts {
            for marker in maintainer_only_markers {
                assert!(!prompt.contains(marker), "static prompt unexpectedly contains {marker:?}");
            }
        }
    }
}
