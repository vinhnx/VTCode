use std::time::Instant;

use anyhow::Result;
use vtcode_core::ui::tui::InlineHandle;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};

use crate::hooks::lifecycle::{HookMessage, HookMessageLevel};

pub(super) fn strip_harmony_syntax(text: &str) -> String {
    let mut result = text.to_string();
    while let Some(start_pos) = result.find("<|start|>") {
        if let Some(call_pos) = result[start_pos..].find("<|call|>") {
            let end_pos = start_pos + call_pos + "<|call|>".len();
            result.replace_range(start_pos..end_pos, "");
        } else {
            result.replace_range(start_pos..start_pos + "<|start|>".len(), "");
        }
    }

    result = result.replace("<|channel|>", "");
    result = result.replace("<|constrain|>", "");
    result = result.replace("<|message|>", "");
    result = result.replace("<|call|>", "");

    result.trim().to_string()
}

pub(super) fn safe_force_redraw(handle: &InlineHandle, last_forced_redraw: &mut Instant) {
    if last_forced_redraw.elapsed() > std::time::Duration::from_millis(100) {
        handle.force_redraw();
        *last_forced_redraw = Instant::now();
    }
}

pub(super) fn render_hook_messages(
    renderer: &mut AnsiRenderer,
    messages: &[HookMessage],
) -> Result<()> {
    for message in messages {
        let text = message.text.trim();
        if text.is_empty() {
            continue;
        }

        let style = match message.level {
            HookMessageLevel::Info => MessageStyle::Info,
            HookMessageLevel::Warning => MessageStyle::Info,
            HookMessageLevel::Error => MessageStyle::Error,
        };

        renderer.line(style, text)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::strip_harmony_syntax;

    #[test]
    fn test_strip_harmony_syntax_basic() {
        let input = r#"<|start|>assistant<|channel|>commentary to=grep_file <|constrain|>json<|message|>{"path":"", "pattern":"TODO"}<|call|>"#;
        let result = strip_harmony_syntax(input);
        assert_eq!(result, "");
    }

    #[test]
    fn test_strip_harmony_syntax_with_text() {
        let input = r#"Here is some text <|start|>assistant<|channel|>commentary to=grep_file <|constrain|>json<|message|>{"path":"", "pattern":"TODO"}<|call|> and more text"#;
        let result = strip_harmony_syntax(input);
        assert_eq!(result, "Here is some text  and more text");
    }

    #[test]
    fn test_strip_harmony_syntax_multiple() {
        let input = r#"<|start|>assistant<|channel|>commentary to=tool1<|message|>{}<|call|> text <|start|>assistant<|channel|>commentary to=tool2<|message|>{}<|call|>"#;
        let result = strip_harmony_syntax(input);
        assert_eq!(result, "text");
    }

    #[test]
    fn test_strip_harmony_syntax_no_harmony() {
        let input = "This is normal text without harmony syntax";
        let result = strip_harmony_syntax(input);
        assert_eq!(result, input);
    }

    #[test]
    fn test_strip_harmony_syntax_partial() {
        let input = "Text with <|channel|> partial tags <|message|>";
        let result = strip_harmony_syntax(input);
        assert_eq!(result, "Text with  partial tags");
    }
}
