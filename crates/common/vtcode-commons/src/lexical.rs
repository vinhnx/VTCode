//! Lightweight lexical classification of source text.
//!
//! The search tools need to tell an identifier occurrence apart from the same
//! text inside a string literal or a comment. A full parser is not needed for
//! that, and parsing every file is expensive, so this module scans the text
//! once and records which bytes are *code*.
//!
//! The scanner is deliberately conservative: it understands line comments,
//! block comments, and string literals, which is what distinguishes a real
//! identifier occurrence from a mention in a comment or a string. When it
//! meets syntax it does not understand, it keeps scanning as code, so an
//! unrecognised construct can only cause a *text* result to be reported as a
//! usage, never a hit to be lost.

/// The comment and string syntax of one language.
///
/// Empty delimiters are ignored: a comment prefix or a block-comment pair with
/// an empty side would otherwise match at every position (or panic while
/// scanning for the closing delimiter).
#[derive(Debug, Clone, Copy)]
pub struct LanguageSyntax {
    /// Prefixes that start a comment running to the end of the line.
    pub line_comments: &'static [&'static str],
    /// Delimiter pairs that start and end a block comment.
    pub block_comments: &'static [(&'static str, &'static str)],
    /// Delimiters that start and end a string literal.
    ///
    /// Longer delimiters are matched first, so a language can list `"""`
    /// alongside `"` to support multi-line strings.
    pub string_delimiters: &'static [&'static str],
    /// Whether a backslash escapes the next character inside a string.
    pub backslash_escapes: bool,
}

impl LanguageSyntax {
    /// Returns the delimiter that starts a string at `bytes[index]`, if any.
    ///
    /// The longest matching delimiter wins, so a multi-line delimiter is
    /// preferred over a single-character one.
    fn string_delimiter_at(&self, bytes: &[u8], index: usize) -> Option<&'static str> {
        let rest = bytes.get(index..)?;
        self.string_delimiters
            .iter()
            .filter(|delimiter| !delimiter.is_empty() && rest.starts_with(delimiter.as_bytes()))
            .max_by_key(|delimiter| delimiter.len())
            .copied()
    }
}

impl Default for LanguageSyntax {
    fn default() -> Self {
        Self {
            line_comments: &[],
            block_comments: &[],
            string_delimiters: &["\""],
            backslash_escapes: true,
        }
    }
}

/// Marks which bytes of `source` are code rather than string or comment text.
///
/// The returned vector has one entry per byte of `source`; `true` means the
/// byte is part of code.
#[must_use]
pub fn code_mask(source: &str, syntax: &LanguageSyntax) -> Vec<bool> {
    let bytes = source.as_bytes();
    let mut mask = vec![true; bytes.len()];
    let mut index = 0usize;

    while index < bytes.len() {
        if let Some(end) = match_line_comment(bytes, index, syntax) {
            mark(&mut mask, index, end);
            index = end;
            continue;
        }
        if let Some(end) = match_block_comment(bytes, index, syntax) {
            mark(&mut mask, index, end);
            index = end;
            continue;
        }
        if let Some(end) = match_string(bytes, index, syntax) {
            mark(&mut mask, index, end);
            index = end;
            continue;
        }
        index += 1;
    }

    mask
}

/// Marks a byte range as not being code.
fn mark(mask: &mut [bool], start: usize, end: usize) {
    let Some(range) = mask.get_mut(start..end.min(mask.len())) else {
        return;
    };
    for slot in range {
        *slot = false;
    }
}

/// Returns the end of a line comment starting at `index`, if there is one.
fn match_line_comment(bytes: &[u8], index: usize, syntax: &LanguageSyntax) -> Option<usize> {
    let rest = bytes.get(index..)?;
    for prefix in syntax.line_comments {
        // An empty prefix matches everywhere, so it would comment out the whole
        // file. Treat it as "no comment syntax" instead.
        if prefix.is_empty() {
            continue;
        }
        if rest.starts_with(prefix.as_bytes()) {
            return Some(
                rest.iter()
                    .position(|byte| *byte == b'\n')
                    .map_or(bytes.len(), |offset| index + offset),
            );
        }
    }
    None
}

/// Returns the end of a block comment starting at `index`, if there is one.
fn match_block_comment(bytes: &[u8], index: usize, syntax: &LanguageSyntax) -> Option<usize> {
    let rest = bytes.get(index..)?;
    for (open, close) in syntax.block_comments {
        // Both delimiters must be non-empty: an empty `open` matches at every
        // position, and an empty `close` has no valid window to scan for.
        if open.is_empty() || close.is_empty() {
            continue;
        }
        if !rest.starts_with(open.as_bytes()) {
            continue;
        }
        let body_start = index + open.len();
        return Some(
            match bytes
                .get(body_start..)
                .unwrap_or_default()
                .windows(close.len())
                .position(|window| window == close.as_bytes())
            {
                Some(offset) => body_start + offset + close.len(),
                None => bytes.len(),
            },
        );
    }
    None
}

/// Returns the end of a string literal starting at `index`, if there is one.
fn match_string(bytes: &[u8], index: usize, syntax: &LanguageSyntax) -> Option<usize> {
    let delimiter = syntax.string_delimiter_at(bytes, index)?;
    let body_start = index + delimiter.len();
    let mut cursor = body_start;

    while let Some(byte) = bytes.get(cursor) {
        if syntax.backslash_escapes && *byte == b'\\' {
            cursor = (cursor + 2).min(bytes.len());
            continue;
        }
        if bytes.get(cursor..).unwrap_or_default().starts_with(delimiter.as_bytes()) {
            return Some(cursor + delimiter.len());
        }
        // A single-line string ends at the newline unless the delimiter spans
        // lines, so an unterminated literal cannot swallow the rest of file.
        if delimiter.len() == 1 && *byte == b'\n' {
            return Some(cursor);
        }
        cursor += 1;
    }

    Some(bytes.len())
}

#[cfg(test)]
mod tests {
    use super::{LanguageSyntax, code_mask};

    const RUST: LanguageSyntax = LanguageSyntax {
        line_comments: &["//"],
        block_comments: &[("/*", "*/")],
        string_delimiters: &["\""],
        backslash_escapes: true,
    };

    fn code_at(source: &str, needle: &str) -> bool {
        let mask = code_mask(source, &RUST);
        let start = source.find(needle).expect("needle present");
        mask[start]
    }

    #[test]
    fn identifier_occurrences_are_code() {
        assert!(code_at("let Widget = 1;", "Widget"));
    }

    #[test]
    fn string_contents_are_not_code() {
        assert!(!code_at("let label = \"Widget\";", "Widget"));
    }

    #[test]
    fn comment_contents_are_not_code() {
        assert!(!code_at("// Widget comment", "Widget"));
        assert!(!code_at("/* Widget */", "Widget"));
    }

    #[test]
    fn escaped_quotes_do_not_end_the_string_early() {
        let source = "let label = \"a\\\"Widget\"; let x = Widget;";
        assert!(!code_at(source, "Widget\""));
        assert!(code_at(source, "Widget;"));
    }

    #[test]
    fn block_comments_span_lines() {
        let source = "/* start\nWidget\nend */ let y = Widget;";
        let mask = code_mask(source, &RUST);
        let widget = source.find("Widget").expect("needle present");
        assert!(!mask[widget], "the commented occurrence is not code");
        let last = source.rfind("Widget").expect("second occurrence");
        assert!(mask[last], "the code occurrence is code");
    }

    #[test]
    fn languages_without_comments_treat_everything_as_code() {
        let syntax = LanguageSyntax { line_comments: &[], ..LanguageSyntax::default() };
        let source = "// Widget";
        assert!(code_mask(source, &syntax)[source.find("Widget").expect("needle present")]);
    }

    #[test]
    fn python_style_comments_are_recognised() {
        let syntax = LanguageSyntax {
            line_comments: &["#"],
            block_comments: &[],
            string_delimiters: &["\"", "'"],
            backslash_escapes: true,
        };
        let source = "# Widget\nx = 'Widget'\ny = Widget";
        let mask = code_mask(source, &syntax);
        let comment = source.find("Widget").expect("needle present");
        let string = source.find("'Widget'").expect("string present") + 1;
        let code = source.rfind("Widget").expect("needle present");
        assert!(!mask[comment], "a comment mention is not code");
        assert!(!mask[string], "a string literal is not code");
        assert!(mask[code], "a bare identifier is code");
    }
}
