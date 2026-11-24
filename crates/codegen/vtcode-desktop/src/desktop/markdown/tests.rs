use std::time::Duration;

use super::{external_web_url, prepare_markdown};
use gpui_kit::SharedString;

#[test]
fn prepared_markdown_is_content_keyed_and_bounded() {
    super::clear_prepared_markdown_cache();
    let source: SharedString = "# heading\n\n- item".into();
    let prepared = super::prepared_markdown(&source);
    assert!(prepared.has_list);
    assert_eq!(super::prepared_markdown_cache_len(), 1);

    // Render paths rebuild the SharedString from a &str each frame, so an
    // equal-but-distinct source must hit the same content key.
    let same_content: SharedString = "# heading\n\n- item".into();
    assert_eq!(super::prepared_markdown(&same_content), prepared);
    assert_eq!(super::prepared_markdown_cache_len(), 1, "equal content must not miss");

    super::clear_prepared_markdown_cache();
    for index in 0..super::PREPARED_MARKDOWN_CACHE_CAPACITY + 3 {
        let source: SharedString = format!("body {index}").into();
        let _ = super::prepared_markdown(&source);
    }
    assert_eq!(
        super::prepared_markdown_cache_len(),
        super::PREPARED_MARKDOWN_CACHE_CAPACITY,
        "cache must stay bounded",
    );
    super::clear_prepared_markdown_cache();
}
use gpui_kit::component::theme::{ActiveTheme, Theme, ThemeMode};
use gpui_kit::test::TestWindowExt;
use gpui_kit::{
    AppContext, Context, InteractiveElement, IntoElement, ParentElement, Render, Styled, TestAppContext,
    TestSupportExt, Window, WindowOptions,
};

#[test]
fn code_preview_fence_preserves_long_backtick_runs() {
    let command = "printf 'start'\n``````````\nprintf 'end'";
    let markdown = super::code_preview_markdown("command.sh", command);
    assert_eq!(markdown, format!("```````````bash\n{command}\n```````````"));
    assert_eq!(super::code_preview_markdown("file.txt", "plain"), "````````\nplain\n````````");
}

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
    assert_eq!(motion.stream_fade(), Duration::from_millis(super::STREAMING_FADE_MS));
    assert_eq!(motion.stream_fade_stagger(), Duration::ZERO);
    assert!(
        motion.stream_fade() <= Duration::from_millis(300),
        "stream fade must stay under the 300ms UI-animation ceiling"
    );
    // Ease-out shape: front-loaded progress reads as instant feedback,
    // while an ease-in curve would lag below linear at the midpoint.
    assert!(motion.stream_fade_easing().sample(0.5) > 0.5, "stream fade must ride an ease-out-shaped curve");
}

struct LeadingFixture;

impl Render for LeadingFixture {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // One paragraph with hard breaks: three lines and no
        // inter-paragraph gaps, so the height delta isolates line leading.
        let source: SharedString = "aaa  \nbbb  \nccc".into();
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
    let (window, _) = cx
        .update(|cx| gpui_kit::open_window(WindowOptions::default(), cx, |_, cx| cx.new(|_| LeadingFixture)).unwrap());
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
    let (window, _) =
        cx.update(|cx| gpui_kit::open_window(WindowOptions::default(), cx, |_, cx| cx.new(|_| ThemeFixture)).unwrap());
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
            // Table headers share the filled-button label rule: softened
            // ink (`#171C26`/`#CCCCCC`) at the luminance crossover.
            let header_foreground = crate::desktop::themes::filled_button_foreground(cx.theme().colors.table_head);
            assert_eq!(style.table_head().text.color, Some(header_foreground));
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
fn source_edits_apply_in_one_pass_across_many_spans() {
    let source: String = (0..64).map(|i| format!("trước {i} <b>bold {i}</b> sau ")).collect();
    let mut edits: Vec<super::SourceEdit> = ["<b>", "</b>"]
        .into_iter()
        .flat_map(|needle| {
            source.match_indices(needle).map(move |(start, _)| super::SourceEdit {
                start,
                end: start + needle.len(),
                replacement: String::new(),
            })
        })
        .collect();
    let out = super::apply_source_edits(&source, &mut edits).expect("parser offsets are valid");
    assert!(!out.contains("<b>"), "opening tags removed: {out}");
    assert!(!out.contains("</b>"), "closing tags removed: {out}");
    assert!(out.contains("trước 0"), "leading text and unicode preserved");
    assert!(out.contains("bold 63"), "trailing text preserved");
}

#[test]
fn source_edits_fail_closed_on_bad_ranges() {
    let mut overlapping = [
        super::SourceEdit { start: 1, end: 4, replacement: String::new() },
        super::SourceEdit { start: 3, end: 5, replacement: String::new() },
    ];
    assert!(super::apply_source_edits("abcdef", &mut overlapping).is_none());

    let mut outside = [super::SourceEdit { start: 2, end: 9, replacement: String::new() }];
    assert!(super::apply_source_edits("abcdef", &mut outside).is_none());

    // Byte offset inside a multi-byte character is not a char boundary.
    let mut mid_char = [super::SourceEdit { start: 2, end: 3, replacement: String::new() }];
    assert!(super::apply_source_edits("a界b", &mut mid_char).is_none());
}

#[test]
fn source_edits_apply_replacements_and_keep_surrounding_text() {
    let source = "start [x] middle [y] end";
    let mut edits = [
        super::SourceEdit { start: 6, end: 9, replacement: "[X]".into() },
        super::SourceEdit { start: 17, end: 20, replacement: String::new() },
    ];
    assert_eq!(super::apply_source_edits(source, &mut edits).as_deref(), Some("start [X] middle  end"),);
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
fn transcript_link_filter_draws_only_web_links_as_links() {
    use gpui_kit::base::text::Link;

    use super::transcript_link_target;

    let kept = transcript_link_target(&Link::new("https://example.test/docs")).expect("web link draws");
    assert_eq!(kept.url.to_string(), "https://example.test/docs");
    for raw in [
        "file:///etc/passwd",
        "javascript:alert(1)",
        "https://user@example.test/path",
        "ftp://example.test/x",
    ] {
        assert!(transcript_link_target(&Link::new(raw)).is_none(), "must drop {raw:?} to plain text");
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

#[test]
fn viewable_plan_markdown_needs_nonblank_content() {
    assert!(!super::has_viewable_plan_markdown(None));
    assert!(!super::has_viewable_plan_markdown(Some("")));
    assert!(!super::has_viewable_plan_markdown(Some("  \n ")));
    assert!(super::has_viewable_plan_markdown(Some("# Plan")));
}

#[test]
fn code_fences_pass_through_byte_identical() {
    for source in [
        "```rust\nfn main() {}\n```",
        "```\nplain\n```",
        "```mermaid\nflowchart TD\n  A --> B\n```",
        "```mermaid title here\nflowchart TD\n  A --> B\n```",
        "```Mermaid\nflowchart TD\n  A --> B\n```",
        "~~~mermaid\nflowchart TD\n  A --> B\n~~~",
        "```mermaid\nunclosed",
        "```mermaid\n  \n```",
        "    indented code",
    ] {
        let prepared = prepare_markdown(&source.into());
        assert_eq!(prepared.markdown.as_str(), source, "fence mangled: {source:?}");
    }
}

/// Reference-style images (`![alt][id]` + `[id]: url`) pass through the
/// preprocessor untouched; the image hook blocks every URL at render time.
#[test]
fn reference_style_images_pass_through_untouched() {
    let source = "Before ![logo][pic] after.\n\n[pic]: https://evil.example/x.png\n";
    let prepared = prepare_markdown(&source.into());
    assert!(prepared.markdown.contains("![logo][pic]"), "reference kept: {}", prepared.markdown);
    assert!(
        prepared.markdown.contains("[pic]: https://evil.example/x.png"),
        "definition kept: {}",
        prepared.markdown
    );
}

#[test]
fn html_strips_around_code_fences() {
    let source = "<div>banner</div>\n\n```mermaid\nflowchart TD\n  A --> B\n```\n\ntail <b>bold</b>";
    let prepared = prepare_markdown(&source.into());
    assert!(!prepared.markdown.contains("<div>"), "html stripped: {}", prepared.markdown);
    assert!(!prepared.markdown.contains("<b>"), "inline html stripped: {}", prepared.markdown);
    assert!(prepared.markdown.contains("```mermaid\nflowchart TD\n  A --> B\n```"), "fence intact");
    assert!(prepared.markdown.contains("tail bold"), "tail intact");
}

struct FenceFixture {
    source: SharedString,
}

impl Render for FenceFixture {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        gpui_kit::div().size_full().child(super::render_markdown(
            "fence-fixture-markdown".into(),
            self.source.clone(),
            false,
            cx,
        ))
    }
}

#[gpui_kit::test]
fn code_fences_carry_copy_action_only(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    for source in [
        "```mermaid\nflowchart TD\n  A --> B\n```",
        "```rust\nfn main() {}\n```",
        "```\nplain\n```",
    ] {
        let source: SharedString = source.into();
        let (window, _) = cx.update(|cx| {
            gpui_kit::open_window(WindowOptions::default(), cx, |_, cx| {
                cx.new(|_| FenceFixture { source: source.clone() })
            })
            .unwrap()
        });
        let (copy, view) = cx
            .update_window(window, |_, window, cx| {
                window.render_frame(cx);
                (window.try_find("copy-code").is_some(), window.try_find("view-diagram").is_some())
            })
            .unwrap();
        assert!(copy, "copy action missing for {source:?}");
        assert!(!view, "no view action for {source:?}");
    }
}

struct OutputSelectionFixture;

impl Render for OutputSelectionFixture {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        gpui_kit::div().size_full().child(
            gpui_kit::div()
                .id("output-selection-probe")
                .w(gpui_kit::px(400.))
                .h(gpui_kit::px(40.))
                .child(super::render_selectable_output_text(
                    "output-selection-text",
                    &SharedString::from("alpha beta gamma"),
                )),
        )
    }
}

#[gpui_kit::test]
fn captured_output_text_selects_and_copies_from_a_drag(cx: &mut TestAppContext) {
    use gpui_kit::base::TextSelection;

    cx.update(gpui_kit::init);
    let (window, _) = cx.update(|cx| {
        gpui_kit::open_window(WindowOptions::default(), cx, |_, cx| cx.new(|_| OutputSelectionFixture)).unwrap()
    });
    let _ = cx.update_window(window, |_, window, cx| {
        use gpui_kit::{InputEvent, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, point, px};

        window.render_frame(cx);
        assert!(!TextSelection::has_selection(window, cx), "no selection before the drag");

        let dispatch = |event: gpui_kit::PlatformInput, window: &mut Window, cx: &mut gpui_kit::App| {
            let _ = window.dispatch_event(event, cx);
        };
        dispatch(
            MouseDownEvent {
                position: point(px(1.), px(12.)),
                button: MouseButton::Left,
                modifiers: Modifiers::default(),
                click_count: 1,
                first_mouse: false,
            }
            .to_platform_input(),
            window,
            cx,
        );
        dispatch(
            MouseMoveEvent {
                position: point(px(390.), px(12.)),
                pressed_button: Some(MouseButton::Left),
                modifiers: Modifiers::default(),
            }
            .to_platform_input(),
            window,
            cx,
        );
        dispatch(
            MouseUpEvent {
                position: point(px(390.), px(12.)),
                button: MouseButton::Left,
                modifiers: Modifiers::default(),
                click_count: 1,
            }
            .to_platform_input(),
            window,
            cx,
        );

        window.render_frame(cx);
        assert_eq!(TextSelection::selected_text(window, cx), "alpha beta gamma");
    });
}
