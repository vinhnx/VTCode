use serde_json::Value;

/// A parsed tool call before canonicalization.
#[derive(Debug, Clone)]
pub(crate) struct ParsedToolCall {
    pub name: String,
    pub args: Value,
}

/// A textual tool parser that extracts structured tool calls from unstructured text.
///
/// Implementors should return `Some(ParsedToolCall)` when they successfully parse
/// a tool call in their supported format, or `None` when the text does not match
/// their expected pattern.
///
/// Parsers should use `tracing::debug!` to log rejection reasons for debugging,
/// but should not log at higher levels to avoid spam.
pub(crate) trait TextualToolParser: Send + Sync {
    /// Returns the name of this parser for debugging and logging.
    fn name(&self) -> &'static str;

    /// Attempts to parse a tool call from the given text.
    ///
    /// Returns `Some` if the text matches this parser's format and could be
    /// successfully parsed, `None` otherwise.
    ///
    /// The returned tool call should contain the raw tool name and arguments
    /// as parsed, without canonicalization or validation.
    fn try_parse(&self, text: &str) -> Option<ParsedToolCall>;

    /// Whether this parser's results should be validated against the known-tool allowlist.
    ///
    /// Returns `true` (default) to validate, `false` to skip validation.
    fn should_validate_tool_name(&self) -> bool {
        true
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
        Self {
            parsers: Vec::new(),
        }
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
            if let Some(call) = parser.try_parse(text) {
                tracing::debug!(
                    parser = parser.name(),
                    tool_name = %call.name,
                    "Parser successfully extracted tool call"
                );
                return Some((call, parser.should_validate_tool_name()));
            }
        }
        None
    }
}
