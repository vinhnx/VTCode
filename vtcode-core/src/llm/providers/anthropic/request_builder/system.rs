use crate::llm::provider::{LLMRequest, MessageRole};
use crate::llm::providers::anthropic_types::CacheControl;
use serde_json::{Value, json};

pub(crate) struct SystemPromptBuildResult {
    pub system_value: Option<Value>,
    pub breakpoints_used: usize,
    pub has_uncached_runtime_context: bool,
}

const RUNTIME_CONTEXT_SECTION_HEADER: &str = "[Runtime Context]";
const HISTORY_DIRECTIVES_SECTION_HEADER: &str = "[History Directives]";

fn has_runtime_context_section(prompt: &str) -> bool {
    prompt.starts_with(&format!("{RUNTIME_CONTEXT_SECTION_HEADER}\n"))
        || prompt.contains(&format!("\n{RUNTIME_CONTEXT_SECTION_HEADER}\n"))
}

fn append_history_system_directives(final_system_prompt: &mut String, request: &LLMRequest) {
    let directives = request
        .messages
        .iter()
        .filter(|msg| msg.role == MessageRole::System)
        .map(|msg| msg.content.as_text().trim().to_string())
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>();

    if directives.is_empty() {
        return;
    }

    if !has_runtime_context_section(final_system_prompt) {
        if !final_system_prompt.is_empty() && !final_system_prompt.ends_with('\n') {
            final_system_prompt.push('\n');
        }
        final_system_prompt.push_str(RUNTIME_CONTEXT_SECTION_HEADER);
        final_system_prompt.push('\n');
    } else if !final_system_prompt.ends_with('\n') {
        final_system_prompt.push('\n');
    }

    final_system_prompt.push_str(HISTORY_DIRECTIVES_SECTION_HEADER);
    final_system_prompt.push('\n');
    for directive in directives {
        final_system_prompt.push_str("- ");
        final_system_prompt.push_str(&directive);
        final_system_prompt.push('\n');
    }
}

fn split_runtime_context_section(prompt: &str) -> Option<(String, String)> {
    let marker = format!("\n{RUNTIME_CONTEXT_SECTION_HEADER}\n");
    let (stable_prefix, runtime_section) = if let Some(split_at) = prompt.rfind(&marker) {
        let (stable_prefix, runtime_section) = prompt.split_at(split_at);
        (stable_prefix, runtime_section.trim_start_matches('\n'))
    } else if prompt.starts_with(&format!("{RUNTIME_CONTEXT_SECTION_HEADER}\n")) {
        ("", prompt)
    } else {
        return None;
    };
    let runtime_section = runtime_section.trim().to_string();
    if runtime_section.is_empty() {
        return None;
    }
    Some((stable_prefix.trim().to_string(), runtime_section))
}

pub(crate) fn build_system_prompt(
    request: &LLMRequest,
    cache_control: &Option<CacheControl>,
    breakpoints_remaining: usize,
) -> SystemPromptBuildResult {
    let mut final_system_prompt = request
        .system_prompt
        .as_ref()
        .map(|s| s.as_str())
        .unwrap_or_default()
        .to_string();

    if let Some(settings) = &request.coding_agent_settings {
        if let Some(role) = &settings.role_specialization {
            if final_system_prompt.is_empty() {
                final_system_prompt = format!("You are {}.", role);
            } else {
                final_system_prompt = format!("You are {}.\n{}", role, final_system_prompt);
            }
        }
        if settings.force_xml_tags {
            final_system_prompt
                .push_str("\nPlease use XML tags to structure your response for consistency.");
        }
        if settings.allow_uncertainty {
            final_system_prompt.push_str("\nIf you are unsure or the information is missing, explicitly state 'I don't know' or 'I am unsure'.");
        }
        if settings.strict_grounding {
            final_system_prompt.push_str("\nOnly use information strictly from the provided documents. Do not rely on external knowledge.");
        }
        if settings.force_quote_grounding {
            final_system_prompt.push_str("\nFind quotes from the provided documents that are relevant to the user request. Place these in <quotes> tags first, and then use them to justify your response.");
        }
        if settings.enforce_structured_thought {
            final_system_prompt.push_str("\nBefore providing your final answer, think through the problem in <thinking> tags. Then, provide your final response in <answer> tags.");
        }
    }

    append_history_system_directives(&mut final_system_prompt, request);

    if final_system_prompt.is_empty() {
        return SystemPromptBuildResult {
            system_value: None,
            breakpoints_used: 0,
            has_uncached_runtime_context: false,
        };
    }

    if let Some((stable_prefix, runtime_section)) =
        split_runtime_context_section(&final_system_prompt)
    {
        let should_cache_stable_prefix =
            cache_control.is_some() && breakpoints_remaining > 0 && !stable_prefix.is_empty();
        let mut blocks = Vec::new();

        if !stable_prefix.is_empty() {
            if should_cache_stable_prefix {
                if let Some(cc) = cache_control.as_ref() {
                    blocks.push(json!({
                        "type": "text",
                        "text": stable_prefix,
                        "cache_control": cc
                    }));
                }
            } else {
                blocks.push(json!({
                    "type": "text",
                    "text": stable_prefix
                }));
            }
        }

        blocks.push(json!({
            "type": "text",
            "text": runtime_section
        }));

        return SystemPromptBuildResult {
            system_value: Some(Value::Array(blocks)),
            breakpoints_used: usize::from(should_cache_stable_prefix),
            has_uncached_runtime_context: true,
        };
    }

    let should_cache = cache_control.is_some() && breakpoints_remaining > 0;

    if should_cache && let Some(cc) = cache_control.as_ref() {
        let block = json!({
            "type": "text",
            "text": final_system_prompt.trim(),
            "cache_control": cc
        });
        return SystemPromptBuildResult {
            system_value: Some(Value::Array(vec![block])),
            breakpoints_used: 1,
            has_uncached_runtime_context: false,
        };
    }

    SystemPromptBuildResult {
        system_value: Some(Value::String(final_system_prompt.trim().to_string())),
        breakpoints_used: 0,
        has_uncached_runtime_context: false,
    }
}
