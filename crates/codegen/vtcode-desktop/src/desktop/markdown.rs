use std::{sync::Arc, time::Duration};

use gpui_kit::Styled;
use gpui_kit::base::{TextView, TextViewMotion, TextViewStyle};
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::theme::{ActiveTheme, ThemeColor};
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, ClipboardItem, ImageCacheError, ImageSource, SharedString, TestSupportExt, div, relative};
use pulldown_cmark::{Event, Parser, Tag, TagEnd};

pub(super) struct PreparedMarkdown {
    pub markdown: SharedString,
    pub accessible_text: SharedString,
    /// Whether the source contains a Markdown list. Kit lays list-item text out
    /// in a `flex_1` (basis-zero) column, so a list reports marker-only
    /// intrinsic width and a shrink-to-fit bubble collapses around it. Callers
    /// give list content a definite bubble width instead.
    pub has_list: bool,
}

/// Transcript body leading. 1.6 matches Kit's own fenced-code default
/// (`gpui-base` code blocks use 1.6) while staying above the WCAG 1.4.8
/// 1.5 floor for body copy.
const TRANSCRIPT_LINE_HEIGHT: f32 = 1.6;
const _: () = assert!(TRANSCRIPT_LINE_HEIGHT >= 1.5, "WCAG 1.4.8 needs at least 1.5x body leading");

/// Preserve Markdown structure and derive readable text while dropping raw HTML.
pub(super) fn prepare_markdown(source: &SharedString) -> PreparedMarkdown {
    let mut sanitized = String::new();
    let mut accessible_text = String::new();
    let mut has_list = false;
    let mut retained_start = 0;
    for (event, range) in Parser::new(source).into_offset_iter() {
        append_accessible_text(&mut accessible_text, &event);
        if matches!(event, Event::Start(Tag::List(_))) {
            has_list = true;
        }
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
        has_list,
    }
}

fn invalid_markdown() -> PreparedMarkdown {
    let message = SharedString::from(rust_i18n::t!("app.markdown.render_error").to_string());
    PreparedMarkdown {
        markdown: message.clone(),
        accessible_text: message,
        has_list: false,
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

pub(super) fn markdown_style(cx: &gpui_kit::App) -> TextViewStyle {
    let mut code = div()
        .font_family(cx.theme().mono_font_family.clone())
        .text_size(cx.theme().mono_font_size);
    let code_style = code.style().clone();
    let colors = cx.theme().colors;
    let mut table_head = div()
        .bg(colors.table_head)
        .text_color(super::themes::filled_button_foreground(colors.table_head));
    TextViewStyle::from_theme(&gpui_kit::base::Theme::global(cx))
        .with_code_block(code_style)
        .with_code_background(colors.muted)
        .with_link(colors.link)
        .with_muted_foreground(super::themes::muted_text(&colors))
        .with_paragraph_gap(gpui_kit::rems(1.))
        .with_inline_code(gpui_kit::HighlightStyle {
            color: Some(colors.foreground),
            background_color: Some(colors.accent),
            ..Default::default()
        })
        .with_table_head(table_head.style().clone())
}

pub(super) fn render_markdown(id: String, source: SharedString, streaming: bool, cx: &gpui_kit::App) -> TextView {
    render_markdown_with_style(id, source, streaming, markdown_style(cx))
}

/// Transcript Markdown with a caller-supplied style, so the per-frame row
/// loop hoists one style build instead of rebuilding it per visible row.
pub(super) fn render_markdown_with_style(
    id: String,
    source: SharedString,
    streaming: bool,
    style: TextViewStyle,
) -> TextView {
    let text_view = harden_transcript_text_view(TextView::markdown(id, source).style(style));

    if streaming {
        text_view.motion(streaming_motion())
    } else {
        text_view
    }
}

/// Render server HTML as an inert static preview: no scripts, no fetches.
///
/// Shares the transcript hardening (blocked images, web-only links, themed
/// code blocks) with [`render_markdown`]. The Kit HTML parser keeps
/// script/style inert; this only ever renders bodies fetched through the
/// bounded `mcp/app/read` service call.
pub(super) fn render_html(id: String, source: SharedString, cx: &gpui_kit::App) -> TextView {
    harden_transcript_text_view(TextView::html(id, source).style(markdown_style(cx)))
}

/// Tool-output body: literal prose plus an optional highlighted
/// `Structured output:` JSON block.
///
/// Splits per render over the already-bounded tail, so marker-less output
/// pays one scan and structured output one extra JSON parse; memoize at the
/// call site if profiling ever implicates this. Callers reuse the unsplit
/// tail for accessibility labels and copies.
pub(super) fn render_output_sections(
    base_id: String,
    tail: &SharedString,
    colors: &ThemeColor,
    style: TextViewStyle,
) -> AnyElement {
    let sections = super::output_sections::split_output_sections(tail);
    let mut column = div().flex().flex_col().gap_2();
    if !sections.leading.is_empty() {
        column = column.child(sections.leading);
    }
    if let Some(structured) = sections.structured {
        column = column.child(render_structured_output_block(
            format!("{base_id}-structured"),
            structured.json,
            colors,
            style,
        ));
        if !structured.trailing.is_empty() {
            column = column.child(structured.trailing);
        }
    }
    column.into_any_element()
}

/// Highlighted `Structured output:` JSON block for tool-output surfaces.
///
/// Renders the localized section label plus the validated JSON through the
/// shared transcript Markdown path, so code-block theming, syntax
/// highlighting, and hardening (blocked images, web-only links, copy action)
/// match fenced code in conversation rows.
fn render_structured_output_block(
    base_id: String,
    json: SharedString,
    colors: &ThemeColor,
    style: TextViewStyle,
) -> AnyElement {
    let mut fenced = String::with_capacity(json.len().saturating_add(12));
    fenced.push_str("```json\n");
    fenced.push_str(&json);
    fenced.push_str("\n```");
    div()
        .id(base_id.clone())
        .test_support()
        .flex()
        .flex_col()
        .gap_1()
        .child(
            div()
                .text_xs()
                .text_color(super::themes::muted_text(colors))
                .child(rust_i18n::t!("app.inspector.structured_output_label").to_string()),
        )
        .child(render_markdown_with_style(format!("{base_id}-code"), fenced.into(), false, style))
        .into_any_element()
}

fn harden_transcript_text_view(text_view: TextView) -> TextView {
    text_view
        // Shared by the Markdown and HTML transcript paths so conversation
        // text and app previews keep the same vertical rhythm.
        .line_height(relative(TRANSCRIPT_LINE_HEIGHT))
        .scrollable(false)
        // A richer renderer must not fetch URLs or local files from agent output.
        .image_source(|_| blocked_image_source())
        .on_link_click(|url, event, _, cx| {
            if !event.is_right_click()
                && let Some(url) = external_web_url(url)
            {
                cx.open_url(url.as_str());
            }
        })
        .code_block_actions(|block, _, _| {
            let code = block.code();
            Button::new("copy-code")
                .tab_stop(true)
                .cursor_pointer()
                .label(rust_i18n::t!("app.markdown.copy_code").to_string())
                .ghost()
                .on_click(move |_, window, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(code.to_string()));
                    super::notifications::copied_to_clipboard(window, cx);
                })
        })
}

fn streaming_motion() -> TextViewMotion {
    TextViewMotion::default().with_stream_fade(Duration::from_millis(350))
}

fn blocked_image_source() -> ImageSource {
    ImageSource::Custom(Arc::new(|_, _| {
        Some(Err(ImageCacheError::Asset(rust_i18n::t!("app.markdown.images_blocked").to_string().into())))
    }))
}

fn external_web_url(raw_url: &str) -> Option<url::Url> {
    if raw_url.chars().any(char::is_control) {
        return None;
    }

    let url = url::Url::parse(raw_url).ok()?;
    let is_web_scheme = matches!(url.scheme(), "http" | "https");
    let has_host = url.host().is_some();
    let has_no_embedded_credentials = url.username().is_empty() && url.password().is_none();

    (is_web_scheme && has_host && has_no_embedded_credentials).then_some(url)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{external_web_url, prepare_markdown};
    use gpui_kit::component::theme::{ActiveTheme, Theme, ThemeMode};
    use gpui_kit::test::TestWindowExt;
    use gpui_kit::{
        AppContext, Context, InteractiveElement, IntoElement, ParentElement, Render, Styled, TestAppContext,
        TestSupportExt, Window, WindowOptions,
    };

    struct ThemeFixture;

    impl Render for ThemeFixture {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            gpui_kit::div()
                .size_full()
                .text_color(cx.theme().colors.foreground)
                .child(super::render_markdown(
                    "retained-theme-markdown".into(),
                    "Readable body with `inline code`.\n\n| Column | Value |\n| --- | --- |\n| Body | text |\n\n```rust\nfn main() {}\n```".into(),
                    false,
                    cx,
                ))
        }
    }

    #[test]
    fn streaming_markdown_fades_chunks_without_word_stagger() {
        let motion = super::streaming_motion();
        assert_eq!(motion.stream_fade(), Duration::from_millis(350));
        assert_eq!(motion.stream_fade_stagger(), Duration::ZERO);
    }

    struct LeadingFixture;

    impl Render for LeadingFixture {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            // One paragraph with hard breaks: three lines and no
            // inter-paragraph gaps, so the height delta isolates line leading.
            let source: gpui_kit::SharedString = "aaa  \nbbb  \nccc".into();
            gpui_kit::div()
                .flex()
                .flex_col()
                .w_full()
                .child(gpui_kit::div().id("leading-probe").test_support().child(super::render_markdown(
                    "leading-probe-md".to_string(),
                    source.clone(),
                    false,
                    cx,
                )))
                .child(
                    gpui_kit::div().id("leading-baseline").test_support().child(
                        gpui_kit::base::TextView::markdown("leading-baseline-md".to_string(), source)
                            .scrollable(false)
                            .line_height(gpui_kit::relative(1.5)),
                    ),
                )
        }
    }

    #[gpui_kit::test]
    fn transcript_body_leading_renders_roomier_than_baseline(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        cx.update(|cx| {
            Theme::update(cx, |theme| {
                theme.font_size = gpui_kit::px(16.);
            });
        });
        let (window, _) = cx.update(|cx| {
            gpui_kit::open_window(WindowOptions::default(), cx, |_, cx| cx.new(|_| LeadingFixture)).unwrap()
        });
        let (probe, baseline) = cx
            .update_window(window, |_, window, cx| {
                window.render_frame(cx);
                (
                    window.find("leading-probe").bounds().size.height,
                    window.find("leading-baseline").bounds().size.height,
                )
            })
            .unwrap();
        // Three 16px lines at 1.6 vs 1.5 leading differ by
        // 3 × 16 × 0.1 = 4.8px. Both views share Kit layout and font
        // metrics, so only the transcript leading choice moves this delta.
        assert!(probe > baseline, "transcript leading must render roomier: probe={probe:?} baseline={baseline:?}");
        let delta = probe - baseline;
        let expected = gpui_kit::px(4.8);
        let drift = if delta > expected {
            delta - expected
        } else {
            expected - delta
        };
        assert!(
            drift < gpui_kit::px(0.5),
            "transcript leading drifted: probe={probe:?} baseline={baseline:?} delta={delta:?}"
        );
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

    #[gpui_kit::test]
    fn markdown_copy_and_code_pills_stay_readable_in_every_theme(cx: &mut TestAppContext) {
        use super::super::themes::{SYSTEM_DARK_ID, SYSTEM_LIGHT_ID, all_themes, apply_by_id, contrast_ratio};

        cx.update(|cx| {
            gpui_kit::init(cx);
            let ids = [SYSTEM_LIGHT_ID, SYSTEM_DARK_ID]
                .into_iter()
                .chain(all_themes().iter().map(|theme| theme.id));
            for id in ids {
                let _mode = apply_by_id(id, None, cx);
                let colors = cx.theme().colors;
                let style = super::markdown_style(cx);
                assert_eq!(style.paragraph_gap(), gpui_kit::rems(1.), "{id}: paragraph gap");
                for (name, surface) in [
                    ("canvas", colors.background),
                    ("panel", colors.secondary),
                    ("popover", colors.popover),
                ] {
                    let ratio = contrast_ratio(style.muted_foreground(), surface);
                    assert!(ratio >= 4.5, "{id}: muted Markdown copy on {name} is {ratio:.2}");
                }
                let pill = style.inline_code();
                let (Some(foreground), Some(background)) = (pill.color, pill.background_color) else {
                    panic!("{id}: inline code pill needs explicit text and fill colors");
                };
                let pill_ratio = contrast_ratio(foreground, background);
                assert!(pill_ratio >= 4.5, "{id}: inline code pill contrast is {pill_ratio:.2}");
                // User bubbles share the panel fill, where the pill shape may
                // blend in; the code text itself must stay readable there.
                let bubble_ratio = contrast_ratio(foreground, colors.secondary);
                assert!(bubble_ratio >= 4.5, "{id}: inline code on user bubbles is {bubble_ratio:.2}");
            }
        });
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
    fn external_links_require_valid_credential_free_web_urls() {
        for url in [
            "https://example.test/a",
            "HTTP://example.test/b",
            "http://localhost:5173/?q=vtcode",
        ] {
            assert!(external_web_url(url).is_some());
        }
        assert_eq!(external_web_url("HTTP://example.test/b").unwrap().as_str(), "http://example.test/b");
        for url in [
            "file:///etc/passwd",
            "javascript:alert(1)",
            "mailto:user@example.test",
            "../file",
            "vtcode:run",
            "https://",
            "https://user@example.test/path",
            "https://user:password@example.test/path",
            "https://example.test/path\nnext",
        ] {
            assert!(external_web_url(url).is_none(), "unexpectedly accepted {url:?}");
        }
    }

    #[test]
    fn html_filter_preserves_unicode_and_inline_code() {
        let source = "Kế hoạch: <b>đúng</b> `x < y` — café";
        let prepared = prepare_markdown(&source.into());
        assert_eq!(prepared.markdown, "Kế hoạch: đúng `x < y` — café");
        assert_eq!(prepared.accessible_text, "Kế hoạch: đúng x < y — café");
    }

    #[test]
    fn list_detection_follows_markdown_structure_not_substrings() {
        for source in [
            "1. First\n2. Second\n3. Third",
            "- first\n- second",
            "- [ ] open\n- [x] done",
            "Intro\n\n1. First\n2. Second",
        ] {
            assert!(prepare_markdown(&source.into()).has_list, "list missed in {source:?}");
        }
        for source in [
            "A user message.",
            "First paragraph.\n\nSecond paragraph.",
            "Version 1.5 is out",
            "```\n1. not a list\n```",
            "`1. not a list`",
        ] {
            assert!(!prepare_markdown(&source.into()).has_list, "list misdetected in {source:?}");
        }
    }
}
