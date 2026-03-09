use anstyle::Style as AnsiStyle;
use anyhow::Result;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};

#[derive(Clone)]
pub(crate) struct PanelContentLine {
    pub(crate) rendered: String,
    pub(crate) style: MessageStyle,
    pub(crate) override_style: Option<AnsiStyle>,
}

impl PanelContentLine {
    pub(crate) fn new(text: impl Into<String>, style: MessageStyle) -> Self {
        Self {
            rendered: text.into(),
            style,
            override_style: None,
        }
    }
}

pub(crate) fn render_left_border_panel(
    renderer: &mut AnsiRenderer,
    lines: Vec<PanelContentLine>,
) -> Result<()> {
    for line in lines {
        if let Some(override_style) = line.override_style {
            renderer.line_with_override_style(
                line.style,
                override_style,
                line.rendered.as_str(),
            )?;
        } else {
            renderer.line(line.style, line.rendered.as_str())?;
        }
    }

    Ok(())
}

#[cfg(test)]
pub(crate) fn wrap_text(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![text.to_string()];
    }

    let mut lines = Vec::new();
    let mut current_line = String::new();

    for word in text.split_whitespace() {
        let word_len = word.chars().count();

        if !current_line.is_empty() {
            let current_visible = current_line
                .chars()
                .filter(|ch| !ch.is_whitespace())
                .count();

            if current_visible + word_len > width {
                lines.push(current_line);
                current_line = String::new();
            }
        }

        if word_len > width {
            if !current_line.is_empty() {
                lines.push(current_line);
                current_line = String::new();
            }

            let mut remaining = word;
            while !remaining.is_empty() {
                let mut byte_len = remaining.len();

                for (chars_taken, (idx, ch)) in remaining.char_indices().enumerate() {
                    if chars_taken == width {
                        byte_len = idx;
                        break;
                    }
                    byte_len = idx + ch.len_utf8();
                }

                let chunk = &remaining[..byte_len];
                lines.push(chunk.to_string());
                remaining = &remaining[byte_len..];
            }
        } else {
            if !current_line.is_empty() {
                current_line.push(' ');
            }
            current_line.push_str(word);
        }
    }

    if !current_line.is_empty() {
        lines.push(current_line);
    }

    if lines.is_empty() {
        lines.push(text.to_string());
    }

    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wrap_text() {
        let result = wrap_text("Hello world this is a long text", 10);
        assert_eq!(result, vec!["Hello world", "this is a", "long text"]);

        let result = wrap_text("supercalifragilisticexpialidocious", 10);
        assert_eq!(
            result,
            vec!["supercalif", "ragilistic", "expialidoc", "ious",]
        );

        let result = wrap_text("", 10);
        assert_eq!(result, vec!["".to_string()]);

        let result = wrap_text("Hello", 10);
        assert_eq!(result, vec!["Hello"]);
    }
}
