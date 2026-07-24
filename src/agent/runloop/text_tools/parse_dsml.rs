use serde_json::{Map, Value};

use crate::agent::runloop::text_tools::parser::{ParseResult, ParsedToolCall, TextualToolParser};

const DSML_TAG_PREFIX: &str = "<\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}";
const DSML_CLOSE_PREFIX: &str = "</\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}";
const DSML_INVOKE_OPEN: &str = "<\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}invoke name=\"";
const DSML_INVOKE_CLOSE: &str = "</\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}invoke>";
const DSML_PARAM_OPEN: &str = "<\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}parameter";
const DSML_PARAM_CLOSE: &str = "</\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}parameter>";

/// Strips DSML markup from text, removing all `<||DSML||...>` and `</||DSML||...>` tags
/// while preserving non-tag content (including parameter values).
pub(crate) fn strip_dsml_markup(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    let open_prefix = DSML_TAG_PREFIX;
    let close_prefix = DSML_CLOSE_PREFIX;
    let open_bytes = open_prefix.as_bytes();
    let close_bytes = close_prefix.as_bytes();

    while !rest.is_empty() {
        let rest_bytes = rest.as_bytes();
        if rest_bytes.starts_with(open_bytes) || rest_bytes.starts_with(close_bytes) {
            let Some(gt) = rest.find('>') else {
                break;
            };
            rest = &rest[gt + 1..];
        } else if let Some(ch) = rest.chars().next() {
            out.push(ch);
            rest = &rest[ch.len_utf8()..];
        }
    }

    out
}

/// Public wrapper for tests
#[cfg(test)]
fn parse_dsml_tool_call(text: &str) -> Option<(String, Value)> {
    parse_dsml_tool_call_raw(text)
}

#[hotpath::measure]
fn parse_dsml_tool_call_raw(text: &str) -> Option<(String, Value)> {
    let invoke_start = text.find(DSML_INVOKE_OPEN)?;
    let after_prefix = &text[invoke_start + DSML_INVOKE_OPEN.len()..];

    let name_end = after_prefix.find('"')?;
    let name = after_prefix[..name_end].trim().to_string();
    if name.is_empty() {
        return None;
    }

    let after_name = &after_prefix[name_end + 1..];
    let tag_close = after_name.find('>')?;
    let rest = &after_name[tag_close + 1..];

    let content_end = rest.find(DSML_INVOKE_CLOSE)?;
    let content = &rest[..content_end];

    let mut object = Map::new();
    let mut remaining = content;

    while let Some(param_start) = remaining.find(DSML_PARAM_OPEN) {
        let after_tag = &remaining[param_start + DSML_PARAM_OPEN.len()..];

        let name_keyword = after_tag.find("name=\"")?;
        let name_content = &after_tag[name_keyword + "name=\"".len()..];
        let name_end = name_content.find('"')?;
        let param_name = name_content[..name_end].trim().to_string();

        let after_param_name = &name_content[name_end + 1..];
        let gt_pos = after_param_name.find('>')?;
        let is_string = after_param_name[..gt_pos].contains("string=\"true\"");

        let after_gt = &after_param_name[gt_pos + 1..];
        let value_end = after_gt.find(DSML_PARAM_CLOSE)?;
        let raw_value = after_gt[..value_end].trim();

        let value = if is_string {
            Value::String(raw_value.to_string())
        } else {
            serde_json::from_str::<Value>(raw_value).unwrap_or_else(|_| Value::String(raw_value.to_string()))
        };

        object.insert(param_name, value);

        let consumed = param_start
            + DSML_PARAM_OPEN.len()
            + name_keyword
            + "name=\"".len()
            + name_end
            + 1
            + gt_pos
            + 1
            + value_end
            + DSML_PARAM_CLOSE.len();
        remaining = &remaining[consumed..];
    }

    if object.is_empty() {
        return None;
    }

    Some((name, Value::Object(object)))
}

/// Collects DSML invoke regions for stripping.
pub(super) fn collect_dsml_regions(text: &str, regions: &mut Vec<(usize, usize)>) {
    let mut search_start = 0usize;
    while let Some(relative_start) = text[search_start..].find(DSML_INVOKE_OPEN) {
        let start = search_start + relative_start;
        let content_start = start + DSML_INVOKE_OPEN.len();
        let end = text[content_start..]
            .find(DSML_INVOKE_CLOSE)
            .map(|idx| content_start + idx + DSML_INVOKE_CLOSE.len())
            .unwrap_or(text.len());
        if start < end && end <= text.len() {
            regions.push((start, end));
        }
        search_start = end.max(content_start);
    }
}

/// Parser for DeepSeek DSML v2 format tool calls.
pub(crate) struct DsmlToolParser;

impl TextualToolParser for DsmlToolParser {
    fn name(&self) -> &'static str {
        "dsml"
    }

    fn try_parse(&self, text: &str) -> ParseResult {
        match parse_dsml_tool_call_raw(text) {
            Some((name, args)) => ParseResult::Success(ParsedToolCall { name, args }),
            None => {
                tracing::debug!(parser = "dsml", reason = "no matching DSML v2 pattern", "Rejected textual tool call");
                ParseResult::Reject("no matching DSML v2 pattern")
            }
        }
    }

    fn find_consumed_spans(&self, text: &str) -> Vec<(usize, usize)> {
        let mut regions = Vec::new();
        collect_dsml_regions(text, &mut regions);
        regions
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[test]
    fn parses_single_dsml_invoke_with_string_params() {
        let text = concat!(
            "<\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}invoke name=\"code_search\">\n",
            "<\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}parameter name=\"query\" string=\"true\">Widget</\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}parameter>\n",
            "<\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}parameter name=\"path\" string=\"true\">/src</\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}parameter>\n",
            "<\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}parameter name=\"result_types\">[\"definition\"]</\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}parameter>\n",
            "</\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}invoke>",
        );

        let (name, args) = parse_dsml_tool_call(text).expect("should parse");
        assert_eq!(name, "code_search");
        assert_eq!(args["query"], Value::String("Widget".to_string()));
        assert_eq!(args["path"], Value::String("/src".to_string()));
        assert_eq!(args["result_types"], serde_json::json!(["definition"]));
    }

    #[test]
    fn parses_dsml_invoke_inside_tool_calls_wrapper() {
        let text = concat!(
            "<\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}tool_calls>\n",
            "  <\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}invoke name=\"read_file\">\n",
            "    <\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}parameter name=\"path\" string=\"true\">/tmp/foo.txt</\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}parameter>\n",
            "  </\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}invoke>\n",
            "</\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}tool_calls>",
        );

        let (name, args) = parse_dsml_tool_call(text).expect("should parse");
        assert_eq!(name, "read_file");
        assert_eq!(args["path"], Value::String("/tmp/foo.txt".to_string()));
    }

    #[test]
    fn parses_first_invoke_only_when_multiple_present() {
        let text = concat!(
            "<\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}invoke name=\"code_search\">\n",
            "  <\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}parameter name=\"a\" string=\"true\">1</\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}parameter>\n",
            "</\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}invoke>\n",
            "<\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}invoke name=\"read_file\">\n",
            "  <\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}parameter name=\"b\" string=\"true\">2</\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}parameter>\n",
            "</\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}invoke>",
        );

        let (name, args) = parse_dsml_tool_call(text).expect("should parse");
        assert_eq!(name, "code_search");
        assert_eq!(args["a"], Value::String("1".to_string()));
        assert!(args.get("b").is_none());
    }

    #[test]
    fn returns_none_for_non_dsml_text() {
        assert!(parse_dsml_tool_call("plain text without any dsml tags").is_none());
    }

    #[test]
    fn handles_json_value_params_without_string_true() {
        let text = concat!(
            "<\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}invoke name=\"code_search\">\n",
            "<\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}parameter name=\"count\">42</\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}parameter>\n",
            "</\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}invoke>",
        );

        let (name, args) = parse_dsml_tool_call(text).expect("should parse");
        assert_eq!(name, "code_search");
        assert_eq!(args["count"], Value::Number(serde_json::Number::from(42)));
    }

    #[test]
    fn returns_none_for_empty_invoke_name() {
        let text = concat!(
            "<\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}invoke name=\"\">\n",
            "</\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}invoke>",
        );

        assert!(parse_dsml_tool_call(text).is_none());
    }

    // --- strip_dsml_markup tests ---

    #[test]
    fn strip_dsml_preserves_plain_text() {
        let input = "This is plain text without any DSML tags.";
        let result = strip_dsml_markup(input);
        assert_eq!(result, input);
    }

    #[test]
    fn strip_dsml_removes_single_invoke_with_params() {
        let text = concat!(
            "<\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}tool_calls>\n",
            "<\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}invoke name=\"task_tracker\">\n",
            "<\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}parameter name=\"action\" string=\"true\">update</\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}parameter>\n",
            "<\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}parameter name=\"item_index\" string=\"false\">1</\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}parameter>\n",
            "</\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}invoke>\n",
            "</\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}tool_calls>",
        );
        let result = strip_dsml_markup(text);
        assert!(!result.contains("DSML"));
        assert!(!result.contains("\u{ff5c}"));
    }

    #[test]
    fn strip_dsml_preserves_text_around_tags() {
        let text = concat!(
            "Here is my synthesis.\n",
            "<\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}invoke name=\"read_file\">\n",
            "<\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}parameter name=\"path\" string=\"true\">/tmp/foo.txt</\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}parameter>\n",
            "</\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}invoke>\n",
            "The key finding is...",
        );
        let result = strip_dsml_markup(text);
        assert!(result.contains("Here is my synthesis."));
        assert!(result.contains("The key finding is..."));
        assert!(!result.contains("DSML"));
    }

    #[test]
    fn strip_dsml_empty_for_pure_tags() {
        let text = concat!(
            "<\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}tool_calls>\n",
            "<\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}invoke name=\"read_file\">\n",
            "<\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}parameter name=\"path\" string=\"true\">/tmp/foo.txt</\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}parameter>\n",
            "</\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}invoke>\n",
            "</\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}tool_calls>",
        );
        let result = strip_dsml_markup(text);
        let trimmed = result.trim();
        assert!(trimmed.is_empty() || !trimmed.contains("DSML"));
    }
}
