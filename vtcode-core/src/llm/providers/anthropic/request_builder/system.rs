use crate::llm::provider::LLMRequest;
use crate::llm::providers::anthropic_types::CacheControl;
use serde_json::{Value, json};

pub(crate) fn build_system_prompt(
    request: &LLMRequest,
    cache_control: &Option<CacheControl>,
    breakpoints_remaining: usize,
) -> (Option<Value>, usize) {
    let mut final_system_prompt = request.system_prompt.clone().unwrap_or_default();

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

    if final_system_prompt.is_empty() {
        return (None, 0);
    }

    let should_cache = cache_control.is_some() && breakpoints_remaining > 0;

    if should_cache {
        if let Some(cc) = cache_control.as_ref() {
            let block = json!({
                "type": "text",
                "text": final_system_prompt.trim(),
                "cache_control": cc
            });
            return (Some(Value::Array(vec![block])), 1);
        }
    }

    (
        Some(Value::String(final_system_prompt.trim().to_string())),
        0,
    )
}
