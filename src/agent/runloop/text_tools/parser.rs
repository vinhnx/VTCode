use serde_json::Value;

/// A parsed tool call before canonicalization.
#[derive(Debug, Clone)]
pub(crate) struct ParsedToolCall {
    pub name: String,
    pub args: Value,
}

/// Result of attempting to parse a textual tool call.
///
/// Rejection reasons are first-class so callers can distinguish "this text is
/// not shaped like a tool call" from "it matched the shape but failed a
/// schema/validation rule".
#[derive(Debug, Clone)]
pub(crate) enum ParseResult {
    Success(ParsedToolCall),
    Reject(&'static str),
}

/// A textual tool parser that extracts structured tool calls from unstructured text.
///
/// Implementors should return `ParseResult::Success` when they successfully parse
/// a tool call in their supported format, or `ParseResult::Reject` with a reason
/// when the text matches their shape but cannot be accepted. Returning
/// `ParseResult::Reject` allows the registry to continue trying other parsers
/// while preserving the reason for observability.
///
/// Parsers should use `tracing::debug!` to log rejection reasons for debugging,
/// but should not log at higher levels to avoid spam.
pub(crate) trait TextualToolParser: Send + Sync {
    /// Returns the name of this parser for debugging and logging.
    fn name(&self) -> &'static str;

    /// Attempts to parse a tool call from the given text.
    ///
    /// Returns `ParseResult::Success` if the text matches this parser's format
    /// and could be successfully parsed. Returns `ParseResult::Reject` if the
    /// text matches the shape but fails a parser-local rule (e.g. unmatched
    /// delimiters). The registry will continue to the next parser in both the
    /// `Reject` and default (no match) cases.
    ///
    /// The returned tool call should contain the raw tool name and arguments
    /// as parsed, without canonicalization or validation.
    fn try_parse(&self, text: &str) -> ParseResult;

    /// Whether this parser's results should be validated against the known-tool allowlist.
    ///
    /// Returns `true` (default) to validate, `false` to skip validation.
    fn should_validate_tool_name(&self) -> bool {
        true
    }

    /// Reports the byte spans this parser would consume in `text` for stripping
    /// purposes, independent of whether the payload is fully parseable.
    ///
    /// The default implementation returns an empty vector. Parsers that match
    /// textual tool-call regions should override this to return the exact
    /// `(start, end)` ranges they recognize, allowing `strip_textual_tool_call_regions`
    /// to avoid a separate per-region parse-validation loop.
    fn find_consumed_spans(&self, _text: &str) -> Vec<(usize, usize)> {
        Vec::new()
    }
}

/// A registry of textual tool parsers that tries each parser in sequence
/// until one succeeds.
pub(crate) struct TextualToolParserRegistry {
    parsers: Vec<Box<dyn TextualToolParser>>,
}

impl TextualToolParserRegistry {
    /// Creates an empty registry.
    pub(crate) fn new() -> Self {
        Self { parsers: Vec::new() }
    }

    /// Registers a parser with this registry.
    pub(crate) fn register(&mut self, parser: Box<dyn TextualToolParser>) {
        self.parsers.push(parser);
    }

    /// Tries to parse a tool call using each registered parser in sequence.
    ///
    /// Returns the first successful parse along with whether validation is required,
    /// or `None` if no parser matched.
    pub(crate) fn try_parse(&self, text: &str) -> Option<(ParsedToolCall, bool)> {
        for parser in &self.parsers {
            match parser.try_parse(text) {
                ParseResult::Success(call) => {
                    tracing::debug!(
                        parser = parser.name(),
                        tool_name = %call.name,
                        "Parser successfully extracted tool call"
                    );
                    return Some((call, parser.should_validate_tool_name()));
                }
                ParseResult::Reject(reason) => {
                    tracing::debug!(
                        parser = parser.name(),
                        reason,
                        "Parser rejected textual tool call"
                    );
                }
            }
        }
        None
    }

    /// Collects all textual tool-call regions recognized by registered parsers.
    pub(crate) fn consumed_spans(&self, text: &str) -> Vec<(usize, usize)> {
        let mut spans = Vec::new();
        for parser in &self.parsers {
            spans.extend(parser.find_consumed_spans(text));
        }
        spans
    }
}
