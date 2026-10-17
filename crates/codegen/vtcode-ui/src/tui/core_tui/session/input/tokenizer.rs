//! Input syntax-highlight tokenizer for the composer line.
//!
//! Extracted from input.rs so slash-command, reference, and inline-code
//! token shapes live in one place. No behavior change: scanning, token
//! merging, and char-index ranges are verbatim.

/// Token type for syntax highlighting in the input field.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum InputTokenKind {
    Normal,
    SlashCommand,
    AgentReference,
    FileReference,
    InlineCode,
}

/// A contiguous range of characters sharing the same token kind.
pub(super) struct InputToken {
    pub(super) kind: InputTokenKind,
    /// Start char index (inclusive).
    pub(super) start: usize,
    /// End char index (exclusive).
    pub(super) end: usize,
}

/// Tokenize input text into styled regions for syntax highlighting.
pub(super) fn tokenize_input(content: &str) -> Vec<InputToken> {
    let chars: Vec<char> = content.chars().collect();
    let len = chars.len();
    if len == 0 {
        return Vec::new();
    }

    // Assign a token kind to each character position.
    let mut kinds = vec![InputTokenKind::Normal; len];

    // 1. Slash commands: `/word` at the start or after whitespace.
    //    Mark the leading `/` and subsequent non-whitespace chars.
    {
        let mut i = 0;
        while i < len {
            if chars[i] == '/'
                && (i == 0 || chars[i - 1].is_whitespace())
                && i + 1 < len
                && chars[i + 1].is_alphanumeric()
            {
                let start = i;
                i += 1;
                while i < len && !chars[i].is_whitespace() {
                    i += 1;
                }
                for kind in &mut kinds[start..i] {
                    *kind = InputTokenKind::SlashCommand;
                }
                continue;
            }
            i += 1;
        }
    }

    // 2. @agent references: canonical `@agent-...` mentions.
    {
        let mut i = 0;
        while i < len {
            if chars[i] == '@'
                && (i == 0 || chars[i - 1].is_whitespace())
                && chars[i..].starts_with(&['@', 'a', 'g', 'e', 'n', 't', '-'])
            {
                let start = i;
                i += 1;
                while i < len && !chars[i].is_whitespace() {
                    i += 1;
                }
                for kind in &mut kinds[start..i] {
                    *kind = InputTokenKind::AgentReference;
                }
                continue;
            }
            i += 1;
        }
    }

    // 3. @file references: `@` at word boundary followed by non-whitespace.
    {
        let mut i = 0;
        while i < len {
            if chars[i] == '@'
                && (i == 0 || chars[i - 1].is_whitespace())
                && i + 1 < len
                && !chars[i + 1].is_whitespace()
            {
                let start = i;
                i += 1;
                while i < len && !chars[i].is_whitespace() {
                    i += 1;
                }
                if kinds[start..i].iter().all(|kind| *kind == InputTokenKind::Normal) {
                    for kind in &mut kinds[start..i] {
                        *kind = InputTokenKind::FileReference;
                    }
                }
                continue;
            }
            i += 1;
        }
    }

    // 4. Inline code: backtick-delimited spans (single or triple).
    {
        let mut i = 0;
        while i < len {
            if chars[i] == '`' {
                let tick_start = i;
                let mut tick_len = 0;
                while i < len && chars[i] == '`' {
                    tick_len += 1;
                    i += 1;
                }
                // Find matching closing backticks.
                let mut found = false;
                let content_start = i;
                while i <= len.saturating_sub(tick_len) {
                    if chars[i] == '`' {
                        let mut close_len = 0;
                        while i < len && chars[i] == '`' {
                            close_len += 1;
                            i += 1;
                        }
                        if close_len == tick_len {
                            for kind in &mut kinds[tick_start..i] {
                                *kind = InputTokenKind::InlineCode;
                            }
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
    }

    // Coalesce adjacent chars with the same kind into tokens.
    let mut tokens = Vec::new();
    let mut cur_kind = kinds[0];
    let mut cur_start = 0;
    for (i, kind) in kinds.iter().enumerate().skip(1) {
        if *kind != cur_kind {
            tokens.push(InputToken { kind: cur_kind, start: cur_start, end: i });
            cur_kind = *kind;
            cur_start = i;
        }
    }
    tokens.push(InputToken { kind: cur_kind, start: cur_start, end: len });
    tokens
}
