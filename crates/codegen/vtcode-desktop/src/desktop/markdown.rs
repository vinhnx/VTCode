use std::sync::Arc;

use gpui_kit::Styled;
use gpui_kit::base::{TextView, TextViewStyle};
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::theme::ActiveTheme;
use gpui_kit::{ClipboardItem, ImageCacheError, ImageSource, SharedString};
use pulldown_cmark::{Event, Parser, Tag, TagEnd};

pub(super) struct PreparedMarkdown {
    pub markdown: SharedString,
    pub accessible_text: SharedString,
}

/// Preserve Markdown structure and derive readable text while dropping raw HTML.
pub(super) fn prepare_markdown(source: &SharedString) -> PreparedMarkdown {
    let mut sanitized = String::new();
    let mut accessible_text = String::new();
    let mut retained_start = 0;
    for (event, range) in Parser::new(source).into_offset_iter() {
        append_accessible_text(&mut accessible_text, &event);
        if matches!(event, Event::Html(_) | Event::InlineHtml(_)) {
            let Some(retained) = source.get(retained_start..range.start) else {
                return invalid_markdown();
            };
            sanitized.push_str(retained);
            retained_start = range.end;
        }
    }
    let markdown = if retained_start == 0 {
        source.clone()
    } else {
        let Some(retained) = source.get(retained_start..) else {
            return invalid_markdown();
        };
        sanitized.push_str(retained);
        sanitized.into()
    };
    PreparedMarkdown {
        markdown,
        accessible_text: accessible_text.trim().to_string().into(),
    }
}

fn invalid_markdown() -> PreparedMarkdown {
    let message = SharedString::from("Unable to render Markdown: invalid source span.");
    PreparedMarkdown {
        markdown: message.clone(),
        accessible_text: message,
    }
}

fn append_accessible_text(output: &mut String, event: &Event<'_>) {
    match event {
        Event::Text(text) | Event::Code(text) | Event::InlineMath(text) | Event::DisplayMath(text) => {
            output.push_str(text);
        }
        Event::SoftBreak | Event::HardBreak => append_separator(output, ' '),
        Event::TaskListMarker(checked) => {
            append_separator(output, ' ');
            output.push_str(if *checked { "checked " } else { "unchecked " });
        }
        Event::Start(tag) if is_block_start(tag) => append_block_separator(output),
        Event::End(tag) if is_block_end(*tag) => append_block_separator(output),
        Event::End(TagEnd::TableCell) => append_separator(output, ' '),
        Event::Rule => append_block_separator(output),
        Event::FootnoteReference(reference) => {
            append_separator(output, ' ');
            output.push_str(reference);
        }
        _ => {}
    }
}

fn is_block_start(tag: &Tag<'_>) -> bool {
    matches!(
        tag,
        Tag::Paragraph
            | Tag::Heading { .. }
            | Tag::BlockQuote(_)
            | Tag::CodeBlock(_)
            | Tag::HtmlBlock
            | Tag::List(_)
            | Tag::Item
            | Tag::FootnoteDefinition(_)
            | Tag::DefinitionList
            | Tag::DefinitionListTitle
            | Tag::DefinitionListDefinition
            | Tag::Table(_)
            | Tag::TableHead
            | Tag::TableRow
    )
}

fn is_block_end(tag: TagEnd) -> bool {
    matches!(
        tag,
        TagEnd::Paragraph
            | TagEnd::Heading(_)
            | TagEnd::BlockQuote(_)
            | TagEnd::CodeBlock
            | TagEnd::HtmlBlock
            | TagEnd::List(_)
            | TagEnd::Item
            | TagEnd::FootnoteDefinition
            | TagEnd::DefinitionList
            | TagEnd::DefinitionListTitle
            | TagEnd::DefinitionListDefinition
            | TagEnd::Table
            | TagEnd::TableHead
            | TagEnd::TableRow
    )
}

fn append_block_separator(output: &mut String) {
    while output.ends_with(' ') {
        let _ = output.pop();
    }
    if !output.is_empty() && !output.ends_with('\n') {
        output.push('\n');
    }
}

fn append_separator(output: &mut String, separator: char) {
    if !output.is_empty() && !output.chars().last().is_some_and(char::is_whitespace) {
        output.push(separator);
    }
}

fn markdown_style(cx: &gpui_kit::App) -> TextViewStyle {
    let mut code = gpui_kit::div()
        .font_family(cx.theme().mono_font_family.clone())
        .text_size(cx.theme().mono_font_size);
    let code_style = code.style().clone();
    let colors = cx.theme().colors;
    let mut table_head = gpui_kit::div()
        .bg(colors.table_head)
        .text_color(super::themes::filled_button_foreground(colors.table_head));
    TextViewStyle::from_theme(&gpui_kit::base::Theme::global(cx))
        .with_code_block(code_style)
        .with_code_background(colors.muted)
        .with_link(colors.link)
        .with_table_head(table_head.style().clone())
}

pub(super) fn render_markdown(id: String, source: SharedString, cx: &gpui_kit::App) -> TextView {
    TextView::markdown(id, source)
        .style(markdown_style(cx))
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
                .tab_stop(true)
                .cursor_pointer()
                .label("Copy code")
                .ghost()
                .on_click(move |_, window, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(code.to_string()));
                    super::notifications::copied_to_clipboard(window, cx);
                })
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
    use gpui_kit::component::theme::{ActiveTheme, Theme, ThemeMode};
    use gpui_kit::test::TestWindowExt;
    use gpui_kit::{
        AppContext, Context, IntoElement, ParentElement, Render, Styled, TestAppContext, Window, WindowOptions,
    };

    struct ThemeFixture;

    impl Render for ThemeFixture {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            gpui_kit::div().size_full().text_color(cx.theme().colors.foreground).child(super::render_markdown(
                "retained-theme-markdown".into(),
                "Readable body with `inline code`.\n\n| Column | Value |\n| --- | --- |\n| Body | text |\n\n```rust\nfn main() {}\n```".into(), cx,
            ))
        }
    }

    #[gpui_kit::test]
    fn retained_markdown_repaints_inline_code_and_tables_in_both_theme_directions(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let (window, _) = cx.update(|cx| {
            gpui_kit::open_window(WindowOptions::default(), cx, |_, cx| cx.new(|_| ThemeFixture)).unwrap()
        });
        let mut previous = None;
        for (mode, accent, header) in [
            (ThemeMode::Light, 0xc8d6e4, 0xe4d6c8),
            (ThemeMode::Dark, 0x28394a, 0x4a3928),
            (ThemeMode::Light, 0xc8d6e4, 0xe4d6c8),
        ] {
            cx.update_window(window, |_, window, cx| {
                Theme::change(mode, Some(window), cx);
                Theme::update(cx, |theme| {
                    theme.colors.accent = gpui_kit::rgb(accent).into();
                    theme.colors.table_head = gpui_kit::rgb(header).into();
                });
                assert!(
                    gpui_kit::base::TextViewDefaults::global(cx).has_code_block_highlighter(),
                    "fenced code needs Kit's syntax highlighter after every theme change"
                );
                let style = super::markdown_style(cx);
                assert_eq!(style.foreground(), cx.theme().colors.foreground);
                assert_eq!(style.inline_code().background_color, Some(gpui_kit::rgb(accent).into()));
                let header_foreground = if mode == ThemeMode::Dark { 0xffffff } else { 0x000000 };
                assert_eq!(style.table_head().text.color, Some(gpui_kit::rgb(header_foreground).into()));
                window.render_frame(cx);
                let quads = window.painted_quads();
                for color in [accent, header] {
                    let background: gpui_kit::Background = gpui_kit::rgb(color).into();
                    assert!(
                        quads.iter().any(|quad| quad.background == background),
                        "current theme color {color:x} was not painted"
                    );
                }
                if let Some((old_accent, old_header)) = previous {
                    for color in [old_accent, old_header] {
                        let background: gpui_kit::Background = gpui_kit::rgb(color).into();
                        assert!(
                            !quads.iter().any(|quad| quad.background == background),
                            "stale theme color remained painted"
                        );
                    }
                }
            })
            .unwrap();
            previous = Some((accent, header));
        }
    }

    #[test]
    fn preserves_markdown_structure_and_code_but_removes_raw_html() {
        let source = "# Plan\n\n**Review** [the diff](https://example.test)\n\n- first\n- second\n\n<div>hidden</div>\n\n```html\n<script>sample</script>\n```\n\nvisible <b>label</b>";
        let prepared = prepare_markdown(&source.into());
        let rendered = prepared.markdown;
        assert!(rendered.starts_with("# Plan\n\n**Review** [the diff](https://example.test)"));
        assert!(rendered.contains("- first\n- second"));
        assert!(!rendered.contains("<div>hidden</div>"));
        assert!(rendered.contains("```html\n<script>sample</script>\n```"));
        assert!(rendered.ends_with("visible label"));
        assert_eq!(
            prepared.accessible_text,
            "Plan\nReview the diff\nfirst\nsecond\n<script>sample</script>\nvisible label"
        );
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
        let prepared = prepare_markdown(&source.into());
        assert_eq!(prepared.markdown, "Kế hoạch: đúng `x < y` — café");
        assert_eq!(prepared.accessible_text, "Kế hoạch: đúng x < y — café");
    }
}
