use anyhow::Error;
use vtcode_core::llm::error_display;
use vtcode_core::llm::provider as uni;
use vtcode_core::llm::providers::clean_reasoning_text;

pub(super) fn map_render_error(provider_name: &str, err: Error) -> uni::LLMError {
    let formatted_error = error_display::format_llm_error(
        provider_name,
        &format!("Failed to render streaming output: {}", err),
    );
    uni::LLMError::Provider {
        message: formatted_error,
        metadata: None,
    }
}

pub(super) fn reasoning_matches_content(reasoning: &str, content: &str) -> bool {
    fn normalize_for_compare(text: &str) -> String {
        clean_reasoning_text(text)
            .chars()
            .filter(|ch| !ch.is_whitespace())
            .collect::<String>()
    }

    let cleaned_reasoning = normalize_for_compare(reasoning);
    let cleaned_content = normalize_for_compare(content);
    !cleaned_reasoning.is_empty()
        && !cleaned_content.is_empty()
        && cleaned_reasoning == cleaned_content
}

pub(super) fn common_prefix_len(a: &str, b: &str) -> usize {
    let mut len = 0;
    for (left, right) in a.chars().zip(b.chars()) {
        if left != right {
            break;
        }
        len += left.len_utf8();
    }
    len
}
