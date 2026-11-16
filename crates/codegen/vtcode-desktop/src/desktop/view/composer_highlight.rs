//! Token decoration for the desktop chat composer.
//!
//! Port of the terminal composer tokenizer (`vtcode-ui::...::input::tokenizer`)
//! to the GPUI input highlighter seam. Token shapes stay identical so the two
//! surfaces agree on what counts as a slash command, an `@` reference, or an
//! inline code span. Themes supply the colors; this module never hard-codes a
//! palette.

use std::{cell::RefCell, ops::Range, rc::Rc};

use gpui_kit::component::input::{
    EditorState, FoldRange, HighlightStyleResolver, InputEdit, InputHighlighter, InputHighlighterFactory, Rope,
};
use gpui_kit::component::theme::ThemeColor;
use gpui_kit::gpui::{Context, FontWeight, HighlightStyle, Hsla, SharedString, Window};

/// Language name that selects this highlighter on the composer `EditorState`.
pub(super) const PROMPT_LANGUAGE: &str = "vtcode-prompt";

/// Token kinds we decorate. `Normal` is represented by the absence of a token
/// so the highlighter can return a default style for the gaps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PromptTokenKind {
    SlashCommand,
    AgentReference,
    FileReference,
    InlineCode,
}

/// A decorated span, in UTF-8 byte offsets into the composer text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct PromptToken {
    pub(super) kind: PromptTokenKind,
    pub(super) range: Range<usize>,
}

/// Split composer text into decorated spans. Mirrors the TUI tokenizer:
/// `/word` at a word boundary, `@agent-…` and `@file` references, and
/// single/triple backtick spans. Later rules never overwrite an earlier, more
/// specific token kind.
pub(super) fn tokenize_prompt(text: &str) -> Vec<PromptToken> {
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();
    if len == 0 {
        return Vec::new();
    }
    let at = |index: usize| chars.get(index).copied();
    let boundary = |index: usize| index == 0 || at(index.saturating_sub(1)).is_none_or(char::is_whitespace);
    let mark = |kinds: &mut [Option<PromptTokenKind>], start: usize, end: usize, kind: PromptTokenKind| {
        if let Some(slice) = kinds.get_mut(start..end) {
            for slot in slice {
                *slot = Some(kind);
            }
        }
    };

    // One kind per character, then coalesce into ranges at the end. `None`
    // means undecorated text.
    let mut kinds = vec![None; len];

    // Slash commands: `/word` at the start or after whitespace.
    let mut i = 0;
    while i < len {
        if at(i) == Some('/') && boundary(i) && at(i + 1).is_some_and(|ch| ch.is_alphanumeric()) {
            let start = i;
            i += 1;
            while at(i).is_some_and(|ch| !ch.is_whitespace()) {
                i += 1;
            }
            mark(&mut kinds, start, i, PromptTokenKind::SlashCommand);
            continue;
        }
        i += 1;
    }

    // Canonical agent mentions: `@agent-…`.
    i = 0;
    while i < len {
        if at(i) == Some('@')
            && boundary(i)
            && chars
                .get(i..)
                .is_some_and(|rest| rest.starts_with(&['@', 'a', 'g', 'e', 'n', 't', '-']))
        {
            let start = i;
            i += 1;
            while at(i).is_some_and(|ch| !ch.is_whitespace()) {
                i += 1;
            }
            mark(&mut kinds, start, i, PromptTokenKind::AgentReference);
            continue;
        }
        i += 1;
    }

    // File references: `@` at a word boundary followed by non-whitespace,
    // only where no more specific rule already claimed the span.
    i = 0;
    while i < len {
        if at(i) == Some('@') && boundary(i) && at(i + 1).is_some_and(|ch| !ch.is_whitespace()) {
            let start = i;
            i += 1;
            while at(i).is_some_and(|ch| !ch.is_whitespace()) {
                i += 1;
            }
            if kinds.get(start..i).is_some_and(|slice| slice.iter().all(|kind| kind.is_none())) {
                mark(&mut kinds, start, i, PromptTokenKind::FileReference);
            }
            continue;
        }
        i += 1;
    }

    // Inline code: backtick-delimited spans (matching delimiter lengths).
    i = 0;
    while i < len {
        if at(i) == Some('`') {
            let tick_start = i;
            let mut tick_len = 0;
            while at(i) == Some('`') {
                tick_len += 1;
                i += 1;
            }
            let content_start = i;
            let mut found = false;
            while i.saturating_add(tick_len) <= len {
                if at(i) == Some('`') {
                    let mut close_len = 0;
                    while at(i) == Some('`') {
                        close_len += 1;
                        i += 1;
                    }
                    if close_len == tick_len {
                        mark(&mut kinds, tick_start, i, PromptTokenKind::InlineCode);
                        found = true;
                        break;
                    }
                } else {
                    i += 1;
                }
            }
            if !found {
                i = content_start;
            }
            continue;
        }
        i += 1;
    }

    // Coalesce adjacent characters with the same kind and convert char indices
    // to UTF-8 byte offsets, which is what GPUI highlight styles use.
    let byte_offsets: Vec<usize> = text
        .char_indices()
        .map(|(offset, _)| offset)
        .chain(std::iter::once(text.len()))
        .collect();
    let mut tokens = Vec::new();
    let mut start = 0;
    let push_token = |tokens: &mut Vec<PromptToken>, start: usize, end: usize| {
        if let (Some(kind), Some(&from), Some(&to)) =
            (kinds.get(start).copied().flatten(), byte_offsets.get(start), byte_offsets.get(end))
        {
            tokens.push(PromptToken { kind, range: from..to });
        }
    };
    for (index, kind) in kinds.iter().enumerate().skip(1) {
        if kinds.get(start) != Some(kind) {
            push_token(&mut tokens, start, index);
            start = index;
        }
    }
    push_token(&mut tokens, start, len);
    tokens
}

/// Theme-derived styles for each token kind. Kept behind `Rc<RefCell<_>>` so a
/// theme change can update the colors without rebuilding the highlighter.
#[derive(Clone, Copy)]
pub(super) struct ComposerHighlightStyles {
    slash: HighlightStyle,
    agent: HighlightStyle,
    file: HighlightStyle,
    code: HighlightStyle,
}

impl ComposerHighlightStyles {
    pub(super) fn from_colors(colors: &ThemeColor) -> Self {
        Self {
            slash: token_style(colors.accent_foreground, true, false),
            agent: token_style(colors.info_foreground, false, false),
            file: token_style(colors.success_foreground, false, true),
            code: token_style(colors.warning_foreground, false, false),
        }
    }

    fn for_kind(&self, kind: PromptTokenKind) -> HighlightStyle {
        match kind {
            PromptTokenKind::SlashCommand => self.slash,
            PromptTokenKind::AgentReference => self.agent,
            PromptTokenKind::FileReference => self.file,
            PromptTokenKind::InlineCode => self.code,
        }
    }
}

/// GPUI's `HighlightStyle` background is a flat rectangle with no corner-radius
/// support (and range decorations don't expose one either). To avoid drawing
/// square pills we decorate with readable foreground colors plus weight and an
/// underline instead of a background.
fn token_style(foreground: Hsla, bold: bool, underline: bool) -> HighlightStyle {
    HighlightStyle {
        color: Some(foreground),
        font_weight: bold.then_some(FontWeight::BOLD),
        underline: underline.then_some(gpui_kit::gpui::UnderlineStyle {
            thickness: gpui_kit::gpui::px(1.),
            color: Some(foreground),
            wavy: false,
        }),
        ..Default::default()
    }
}

pub(super) type SharedComposerHighlightStyles = Rc<RefCell<ComposerHighlightStyles>>;

/// Build the factory installed on the composer's `EditorState`. Only our
/// language name produces a highlighter, so the Kit tree-sitter adapter still
/// handles any other language an editor might ask for.
pub(super) fn prompt_highlighter_factory(styles: SharedComposerHighlightStyles) -> InputHighlighterFactory {
    Rc::new(move |language| {
        (language == PROMPT_LANGUAGE)
            .then(|| Box::new(PromptHighlighter::new(styles.clone())) as Box<dyn InputHighlighter>)
    })
}

struct PromptHighlighter {
    styles: SharedComposerHighlightStyles,
    tokens: Vec<PromptToken>,
}

impl PromptHighlighter {
    fn new(styles: SharedComposerHighlightStyles) -> Self {
        Self { styles, tokens: Vec::new() }
    }
}

impl InputHighlighter for PromptHighlighter {
    fn language(&self) -> SharedString {
        PROMPT_LANGUAGE.into()
    }

    fn update(
        &mut self,
        _edit: Option<InputEdit>,
        text: &Rope,
        _folding: bool,
        _window: &mut Window,
        _cx: &mut Context<EditorState>,
    ) {
        self.tokens = tokenize_prompt(&text.to_string());
    }

    fn styles(
        &self,
        range: &Range<usize>,
        _resolver: &dyn HighlightStyleResolver,
    ) -> Vec<(Range<usize>, HighlightStyle)> {
        let styles = self.styles.borrow();
        let mut runs = Vec::new();
        let mut cursor = range.start;
        for token in &self.tokens {
            if token.range.end <= range.start {
                continue;
            }
            if token.range.start >= range.end {
                break;
            }
            let start = token.range.start.max(range.start);
            let end = token.range.end.min(range.end);
            if start > cursor {
                runs.push((cursor..start, HighlightStyle::default()));
            }
            runs.push((start..end, styles.for_kind(token.kind)));
            cursor = end;
        }
        if cursor < range.end {
            runs.push((cursor..range.end, HighlightStyle::default()));
        }
        runs
    }

    fn fold_ranges(&self, _text: &Rope) -> Vec<FoldRange> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenizes_slash_agent_file_and_code() {
        let tokens = tokenize_prompt("/model @agent-explorer @README.md `code`");
        let kinds: Vec<_> = tokens.iter().map(|token| token.kind).collect();
        assert_eq!(
            kinds,
            vec![
                PromptTokenKind::SlashCommand,
                PromptTokenKind::AgentReference,
                PromptTokenKind::FileReference,
                PromptTokenKind::InlineCode,
            ]
        );
        let text = "/model @agent-explorer @README.md `code`";
        assert_eq!(text.get(tokens[0].range.clone()), Some("/model"));
        assert_eq!(text.get(tokens[3].range.clone()), Some("`code`"));
    }

    #[test]
    fn byte_ranges_survive_multibyte_text() {
        let text = "héllo @file.rs";
        let tokens = tokenize_prompt(text);
        assert_eq!(tokens.len(), 1);
        assert_eq!(text.get(tokens[0].range.clone()), Some("@file.rs"));
    }

    #[test]
    fn agent_reference_wins_over_file_reference() {
        let tokens = tokenize_prompt("@agent-explorer");
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].kind, PromptTokenKind::AgentReference);
    }

    struct NoResolver;

    impl HighlightStyleResolver for NoResolver {
        fn style(&self, _name: &str) -> Option<HighlightStyle> {
            None
        }
    }

    #[test]
    fn runs_cover_the_requested_range() {
        let styles = Rc::new(RefCell::new(ComposerHighlightStyles::from_colors(&ThemeColor::default())));
        let mut highlighter = PromptHighlighter::new(styles);
        let text = "/model hello @README.md";
        highlighter.tokens = tokenize_prompt(text);
        let runs = highlighter.styles(&(0..text.len()), &NoResolver);
        assert_eq!(runs.first().map(|run| run.0.start), Some(0));
        assert_eq!(runs.last().map(|run| run.0.end), Some(text.len()));
        for pair in runs.windows(2) {
            assert_eq!(pair[0].0.end, pair[1].0.start);
        }
        // The slash-command run carries a theme color, not the default style.
        assert_eq!(runs[0].1.color, Some(ThemeColor::default().accent_foreground));
        assert!(runs.iter().any(|run| run.1 == HighlightStyle::default()));
    }

    #[test]
    fn theme_change_updates_styles_without_rebuilding() {
        let styles = Rc::new(RefCell::new(ComposerHighlightStyles::from_colors(&ThemeColor::default())));
        let mut highlighter = PromptHighlighter::new(styles.clone());
        highlighter.tokens = tokenize_prompt("/model");
        let before = highlighter.styles(&(0..6), &NoResolver)[0].1;
        *styles.borrow_mut() = ComposerHighlightStyles::from_colors(&ThemeColor::light());
        let after = highlighter.styles(&(0..6), &NoResolver)[0].1;
        assert_ne!(before, after);
    }
}
