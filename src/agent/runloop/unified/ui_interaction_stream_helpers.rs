use anyhow::Error;
use anyhow::Result;
use vtcode_commons::formatting::compact_reasoning_text;
use vtcode_core::llm::error_display;
use vtcode_core::llm::provider as uni;
use vtcode_core::llm::providers::clean_reasoning_text;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};

#[cold]
pub(super) fn map_render_error(provider_name: &str, err: Error) -> uni::LLMError {
    let formatted_error =
        error_display::format_llm_error(provider_name, &format!("Failed to render streaming output: {err}"));
    uni::LLMError::Provider { message: formatted_error, metadata: None }
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
    !cleaned_reasoning.is_empty() && !cleaned_content.is_empty() && cleaned_reasoning == cleaned_content
}

pub(crate) fn common_prefix_len(a: &str, b: &str) -> usize {
    let mut len = 0;
    for (left, right) in a.chars().zip(b.chars()) {
        if left != right {
            break;
        }
        len += left.len_utf8();
    }
    len
}

/// Render a complete reasoning block compactly, emitting each line with light
/// emphasis when it announces a decision or tool call.
///
/// Returns `true` if any line was rendered. Blank lines are preserved as single
/// paragraph breaks so structure is kept without blank-line spam.
pub(super) fn render_compact_reasoning_block(renderer: &mut AnsiRenderer, text: &str) -> Result<bool> {
    let compact = compact_reasoning_text(text);
    if compact.trim().is_empty() {
        return Ok(false);
    }
    for line in compact.split('\n') {
        let style = if line.trim().is_empty() {
            MessageStyle::Reasoning
        } else if super::reasoning::is_decision_or_tool_line(line) {
            MessageStyle::ReasoningEmphasis
        } else {
            MessageStyle::Reasoning
        };
        renderer.line(style, line)?;
    }
    Ok(true)
}
