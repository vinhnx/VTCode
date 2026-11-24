use std::{cell::RefCell, collections::VecDeque, sync::Arc, time::Duration};

use gpui_kit::Styled;
use gpui_kit::base::{Easing, TextView, TextViewMotion, TextViewStyle};
use gpui_kit::component::theme::ActiveTheme;
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, ClipboardItem, ImageCacheError, ImageSource, SharedString, TestSupportExt, div, relative};
use pulldown_cmark::{Event, Parser, Tag, TagEnd};

#[derive(Clone, Debug, PartialEq, Eq)]
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

/// Source edit: replace `source[start..end]` with `replacement`.
/// Insertions use `start == end`.
struct SourceEdit {
    start: usize,
    end: usize,
    replacement: String,
}

/// Apply collected edits in one forward pass. `None` when a range falls
/// outside the source or overlaps another range (never for parser ranges);
/// callers fail closed to [`invalid_markdown`].
///
/// Parser offsets refer to the immutable source, so walking them in ascending
/// order keeps the next offset valid. The previous back-to-front `format!`
/// loop re-copied the whole buffer once per edit — quadratic for a body with
/// many raw-HTML spans. A single pass is O(source + edits) and matches the
/// content-keyed cache's goal of keeping render-time work cheap.
fn apply_source_edits(source: &str, edits: &mut [SourceEdit]) -> Option<String> {
    edits.sort_by_key(|edit| edit.start);
    let mut out = String::with_capacity(source.len());
    let mut cursor = 0;
    for edit in edits {
        if edit.start < cursor || edit.end < edit.start || edit.end > source.len() {
            return None;
        }
        out.push_str(source.get(cursor..edit.start)?);
        out.push_str(&edit.replacement);
        cursor = edit.end;
    }
    out.push_str(source.get(cursor..)?);
    Some(out)
}

/// Preserve Markdown structure and derive readable text while dropping raw
/// HTML. Code fences of every language pass through untouched.
pub(super) fn prepare_markdown(source: &SharedString) -> PreparedMarkdown {
    let mut accessible_text = String::new();
    let mut has_list = false;
    let mut edits: Vec<SourceEdit> = Vec::new();
    for (event, range) in Parser::new(source).into_offset_iter() {
        append_accessible_text(&mut accessible_text, &event);
        if matches!(event, Event::Start(Tag::List(_))) {
            has_list = true;
        }
        if matches!(event, Event::Html(_) | Event::InlineHtml(_)) {
            edits.push(SourceEdit {
                start: range.start,
                end: range.end,
                replacement: String::new(),
            });
        }
    }
    let markdown = if edits.is_empty() {
        source.clone()
    } else {
        let Some(rewritten) = apply_source_edits(source, &mut edits) else {
            return invalid_markdown();
        };
        rewritten.into()
    };
    PreparedMarkdown {
        markdown,
        accessible_text: accessible_text.trim().to_string().into(),
        has_list,
    }
}

/// How many distinct Markdown sources retain their prepared projection between
/// frames. Bounded so a long session streaming many distinct bodies cannot grow
/// the cache without limit; most views revisit only a handful of sources.
const PREPARED_MARKDOWN_CACHE_CAPACITY: usize = 16;

thread_local! {
    /// Content-keyed cache of parsed Markdown projections (most-recent first).
    ///
    /// The immutable source text *is* the key, so there is nothing to
    /// invalidate: a changed source is simply a different key. Render paths ask
    /// for a source each frame and pay the parse only on a miss, which keeps the
    /// per-frame formatter cheap without any sync state. Transcript rows already
    /// prepare once at store time ([`super::transcript::TranscriptItem`]); this
    /// cache serves the render-time paths (interaction details, plan/inspector
    /// viewers) that only have a borrowed source.
    static PREPARED_MARKDOWN_CACHE: RefCell<VecDeque<(SharedString, PreparedMarkdown)>> =
        const { RefCell::new(VecDeque::new()) };
}

/// Prepare a Markdown source, reusing a previously prepared projection for the
/// same content. Call this from render paths instead of [`prepare_markdown`] so
/// repeated frames over an unchanged source skip the parse.
pub(super) fn prepared_markdown(source: &SharedString) -> PreparedMarkdown {
    PREPARED_MARKDOWN_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if let Some(index) = cache.iter().position(|(key, _)| key == source)
            && let Some((_, prepared)) = cache.remove(index)
        {
            let result = prepared.clone();
            cache.push_front((source.clone(), prepared));
            return result;
        }
        let prepared = prepare_markdown(source);
        if cache.len() == PREPARED_MARKDOWN_CACHE_CAPACITY {
            let _ = cache.pop_back();
        }
        cache.push_front((source.clone(), prepared.clone()));
        prepared
    })
}

#[cfg(test)]
pub(super) fn clear_prepared_markdown_cache() {
    PREPARED_MARKDOWN_CACHE.with(|cache| cache.borrow_mut().clear());
}

#[cfg(test)]
pub(super) fn prepared_markdown_cache_len() -> usize {
    PREPARED_MARKDOWN_CACHE.with(|cache| cache.borrow().len())
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

/// Maximum height of one fenced code block. Agent output can emit a whole file
/// in a single fence; without a cap that block becomes an unbounded row inside
/// the virtualized transcript, inflating measured list geometry and burying the
/// rest of the conversation. Kit's scroll mode gives the block its own
/// scrollbar and consumes wheel input until its edge, so the enclosing list
/// only scrolls once the code has reached the end.
const CODE_BLOCK_MAX_HEIGHT_REMS: f32 = 24.0;

/// The single source of truth for code-block box styling across every
/// transcript Markdown/HTML surface. Callers needing extra refinements (e.g.
/// wrapping a long shell command) must refine on top of this; replacing the
/// `code_block` style wholesale silently drops the height cap and scroll mode.
pub(super) fn code_block_style(cx: &gpui_kit::App) -> gpui_kit::StyleRefinement {
    let mut style = div()
        .font_family(cx.theme().mono_font_family.clone())
        .text_size(cx.theme().mono_font_size)
        .max_h(gpui_kit::rems(CODE_BLOCK_MAX_HEIGHT_REMS))
        .style()
        .clone();
    // Kit opts a code block into its own scroll region when `overflow.y` is
    // Scroll: the block gains a scrollbar and consumes the wheel until its
    // edge, so it cannot inflate a row inside the virtualized transcript.
    style.overflow.y = Some(gpui_kit::Overflow::Scroll);
    style
}

pub(super) fn markdown_style(cx: &gpui_kit::App) -> TextViewStyle {
    let colors = cx.theme().colors;
    let mut table_head = div()
        .bg(colors.table_head)
        .text_color(super::themes::filled_button_foreground(colors.table_head));
    TextViewStyle::from_theme(&gpui_kit::base::Theme::global(cx))
        .with_code_block(code_block_style(cx))
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

/// Read-only code preview for one non-markdown text file: the bounded text
/// renders as a single fenced code block through the shared Markdown
/// pipeline, so Kit's syntax highlighter paints it with the app theme while
/// the transcript hardening keeps links inert. The fence is longer than any
/// backtick run in the source, so even nested fences remain in one block.
pub(super) fn render_code_preview(id: String, rel_path: &str, text: SharedString, cx: &gpui_kit::App) -> TextView {
    render_markdown_with_style(id, code_preview_markdown(rel_path, &text).into(), false, markdown_style(cx))
}

fn code_preview_markdown(rel_path: &str, text: &str) -> String {
    let longest_run = text.split(|character| character != '`').map(str::len).max().unwrap_or(0);
    let fence = "`".repeat(longest_run.saturating_add(1).max(8));
    match code_fence_language(rel_path) {
        Some(language) => format!("{fence}{language}\n{text}\n{fence}"),
        None => format!("{fence}\n{text}\n{fence}"),
    }
}

/// Fence language token for a file extension. Kit's highlighter resolves
/// these tokens to grammars; unmapped extensions render unhighlighted
/// rather than guessing from the path shape.
fn code_fence_language(rel_path: &str) -> Option<&'static str> {
    let extension = rel_path.rsplit_once('.')?.1.to_ascii_lowercase();
    let extension: &str = &extension;
    Some(match extension {
        "rs" => "rust",
        "ts" | "tsx" | "mts" | "cts" => "typescript",
        "js" | "mjs" | "cjs" | "jsx" => "javascript",
        "py" | "pyi" => "python",
        "go" => "go",
        "c" | "h" => "c",
        "cc" | "cpp" | "cxx" | "hpp" | "hh" => "cpp",
        "java" => "java",
        "kt" | "kts" => "kotlin",
        "swift" => "swift",
        "rb" => "ruby",
        "php" => "php",
        "sh" | "bash" | "zsh" => "bash",
        "ps1" => "powershell",
        "lua" => "lua",
        "dart" => "dart",
        "zig" => "zig",
        "sql" => "sql",
        "json" => "json",
        "yaml" | "yml" => "yaml",
        "toml" => "toml",
        "html" | "htm" => "html",
        "xml" => "xml",
        "css" | "scss" | "less" => "css",
        _ => return None,
    })
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

/// Fixed height of the inset markdown viewer. Kit's scrollable TextView
/// virtualizes through `gpui::list` and requires a fixed-height parent;
/// 16rem shows roughly ten lines while the surrounding scroller yields
/// the space.
const MARKDOWN_VIEW_PANEL_HEIGHT_REMS: f32 = 16.0;

/// Whether an interaction carries plan markdown worth showing. `None` and
/// blank share one hiding rule so the panel toggle and the inspector tab
/// cannot disagree.
pub(super) fn has_viewable_plan_markdown(plan_markdown: Option<&str>) -> bool {
    plan_markdown.is_some_and(|markdown| !markdown.trim().is_empty())
}

/// Geometry for [`render_markdown_view`]: inset panels own a fixed
/// scrollport, while pane-filling hosts supply a definite height.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum MarkdownViewHeight {
    /// Fixed-height inset box.
    Panel,
    /// Fill the host pane; the host must supply a definite height.
    Fill,
}

/// Framed scrollable markdown viewer: the shared hardened Kit TextView.
/// Used by the plan-proposal expander, the inspector tab, and markdown file
/// previews. Parsing goes through [`prepared_markdown`], so repeated frames over
/// an unchanged source reuse the projection instead of re-parsing it.
pub(super) fn render_markdown_view(
    id: String,
    source: SharedString,
    height: MarkdownViewHeight,
    cx: &gpui_kit::App,
) -> AnyElement {
    let colors = cx.theme().colors;
    let prepared = prepared_markdown(&source);
    let frame = div()
        .id(id.clone())
        .test_support()
        .role(gpui_kit::Role::Label)
        .aria_label(prepared.accessible_text.to_string())
        .w_full()
        .min_w_0();
    let frame = match height {
        MarkdownViewHeight::Panel => frame
            .h(gpui_kit::rems(MARKDOWN_VIEW_PANEL_HEIGHT_REMS))
            .rounded_md()
            .border_1()
            .border_color(super::themes::soft_edge_color(&colors))
            .bg(colors.background)
            .overflow_hidden()
            .p_3(),
        MarkdownViewHeight::Fill => frame.flex_1().min_h_0().px_3().py_2(),
    };
    frame
        .child(
            render_markdown_with_style(format!("{id}-md"), prepared.markdown, false, markdown_style(cx))
                .scrollable(true),
        )
        .into_any_element()
}

/// Tool-output body: literal text only.
///
/// Captured output is projected to its human form at store time (see
/// `output_sections::human_output_text`), so renderers never see the
/// `Structured output:` metadata envelope — there is no JSON branch to keep
/// in sync here. Call sites reuse this same tail for accessibility labels
/// and copies.
pub(super) fn render_output_text(tail: &SharedString) -> AnyElement {
    div().child(tail.clone()).into_any_element()
}

/// Window-selectable output for the inspector body.
///
/// `SelectableText` registers one stable participant per render and projects
/// the laid-out text back to UTF-8 byte ranges, so a drag selects and copies
/// exactly the captured output on screen. The compact transcript excerpt
/// keeps the plain renderer: its row already owns the click-to-expand gesture
/// and a full-output copy button.
pub(super) fn render_selectable_output_text(id: impl Into<gpui_kit::ElementId>, tail: &SharedString) -> AnyElement {
    gpui_kit::base::SelectableText::new(id, tail.clone()).into_any_element()
}

fn harden_transcript_text_view(text_view: TextView) -> TextView {
    text_view
        // Shared by the Markdown and HTML transcript paths so conversation
        // text and app previews keep the same vertical rhythm.
        .line_height(relative(TRANSCRIPT_LINE_HEIGHT))
        .scrollable(false)
        // A richer renderer must not fetch URLs or local files from agent output:
        // every image stays blocked.
        .image_source(|_| blocked_image_source())
        // Links that fail the gate never paint as links at all (no link
        // color or underline, not clickable) instead of rendering as links
        // that refuse to open. The click handler below stays as the opener
        // for the surviving web links.
        .link(|link, _| transcript_link_target(link))
        .on_link_click(|url, event, _, cx| {
            if !event.is_right_click()
                && let Some(url) = external_web_url(url)
            {
                cx.open_url(url.as_str());
            }
        })
        .code_block_actions(|block, _, _| {
            let code = block.code();
            super::view::desktop_ghost_button("copy-code")
                .label(rust_i18n::t!("app.markdown.copy_code").to_string())
                .on_click(move |_, window, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(code.to_string()));
                    super::notifications::copied_to_clipboard(window, cx);
                })
                .into_any_element()
        })
}

/// Fade-in length for streamed transcript chunks. Matches Kit's own
/// streaming preset (280ms) and stays under the 300ms UI-animation
/// ceiling; chunks fade as one (zero stagger) for calm streaming text.
const STREAMING_FADE_MS: u64 = 280;
const _: () = assert!(STREAMING_FADE_MS <= 300, "stream fade must stay under the 300ms UI-animation ceiling");

fn streaming_motion() -> TextViewMotion {
    TextViewMotion::default()
        .with_stream_fade(Duration::from_millis(STREAMING_FADE_MS))
        .with_stream_fade_easing(Easing::EaseOut)
}

fn blocked_image_source() -> ImageSource {
    ImageSource::Custom(Arc::new(|_, _| {
        Some(Err(ImageCacheError::Asset(rust_i18n::t!("app.markdown.images_blocked").to_string().into())))
    }))
}

/// Fail-closed gate for opening a remote URL in the browser: http(s) with a
/// host and no embedded credentials or control characters. Shared by
/// transcript links and the update changelog viewer.
pub(super) fn external_web_url(raw_url: &str) -> Option<url::Url> {
    if raw_url.chars().any(char::is_control) {
        return None;
    }

    let url = url::Url::parse(raw_url).ok()?;
    let is_web_scheme = matches!(url.scheme(), "http" | "https");
    let has_host = url.host().is_some();
    let has_no_embedded_credentials = url.username().is_empty() && url.password().is_none();

    (is_web_scheme && has_host && has_no_embedded_credentials).then_some(url)
}

/// Draw-time link filter for [`harden_transcript_text_view`]: keep links
/// that pass [`external_web_url`], drop the rest to plain text. Runs per
/// draw per link, so it stays a pure URL check with no I/O.
fn transcript_link_target(link: &gpui_kit::base::text::Link) -> Option<gpui_kit::base::text::Link> {
    external_web_url(&link.url).map(|_| link.clone())
}

#[cfg(test)]
mod tests;
