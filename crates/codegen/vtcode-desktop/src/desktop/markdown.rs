use std::sync::Arc;

use gpui_kit::base::TextView;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::{ClipboardItem, ImageCacheError, ImageSource, SharedString};
use pulldown_cmark::{Event, Parser};

/// Preserve Markdown structure while keeping raw HTML out of the transcript.
pub(super) fn prepare_markdown(source: &SharedString) -> SharedString {
    let mut sanitized = String::new();
    let mut retained_start = 0;
    for (event, range) in Parser::new(source).into_offset_iter() {
        if matches!(event, Event::Html(_) | Event::InlineHtml(_)) {
            let Some(retained) = source.get(retained_start..range.start) else {
                return "Unable to render Markdown: invalid source span.".into();
            };
            sanitized.push_str(retained);
            retained_start = range.end;
        }
    }
    if retained_start == 0 {
        return source.clone();
    }
    let Some(retained) = source.get(retained_start..) else {
        return "Unable to render Markdown: invalid source span.".into();
    };
    sanitized.push_str(retained);
    sanitized.into()
}

pub(super) fn render_markdown(id: String, source: SharedString) -> TextView {
    TextView::markdown(id, source)
        .scrollable(false)
        // A richer renderer must not fetch URLs or local files from agent output.
        .image_source(|_| blocked_image_source())
        .on_link_click(|url, event, _, cx| {
            if !event.is_right_click() && is_web_link(url) {
                cx.open_url(url);
            }
        })
        .code_block_actions(|block, _, _| {
            let code = block.code();
            Button::new("copy-code")
                .label("Copy code")
                .ghost()
                .on_click(move |_, _, cx| cx.write_to_clipboard(ClipboardItem::new_string(code.to_string())))
        })
}

fn blocked_image_source() -> ImageSource {
    ImageSource::Custom(Arc::new(|_, _| {
        Some(Err(ImageCacheError::Asset("Transcript images are not loaded automatically".into())))
    }))
}

fn is_web_link(url: &str) -> bool {
    url.split_once(':')
        .is_some_and(|(scheme, _)| scheme.eq_ignore_ascii_case("https") || scheme.eq_ignore_ascii_case("http"))
}

#[cfg(test)]
mod tests {
    use super::{is_web_link, prepare_markdown};

    #[test]
    fn preserves_markdown_structure_and_code_but_removes_raw_html() {
        let source = "# Plan\n\n**Review** [the diff](https://example.test)\n\n- first\n- second\n\n<div>hidden</div>\n\n```html\n<script>sample</script>\n```\n\nvisible <b>label</b>";
        let rendered = prepare_markdown(&source.into());
        assert!(rendered.starts_with("# Plan\n\n**Review** [the diff](https://example.test)"));
        assert!(rendered.contains("- first\n- second"));
        assert!(!rendered.contains("<div>hidden</div>"));
        assert!(rendered.contains("```html\n<script>sample</script>\n```"));
        assert!(rendered.ends_with("visible label"));
    }

    #[test]
    fn external_links_only_allow_web_schemes() {
        for url in ["https://example.test/a", "HTTP://example.test/b"] {
            assert!(is_web_link(url));
        }
        for url in [
            "file:///etc/passwd",
            "javascript:alert(1)",
            "mailto:user@example.test",
            "../file",
            "vtcode:run",
        ] {
            assert!(!is_web_link(url));
        }
    }

    #[test]
    fn html_filter_preserves_unicode_and_inline_code() {
        let source = "Kế hoạch: <b>đúng</b> `x < y` — café";
        assert_eq!(prepare_markdown(&source.into()), "Kế hoạch: đúng `x < y` — café");
    }
}
