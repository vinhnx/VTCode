use serde_json::{Map, Value};

use crate::agent::runloop::text_tools::canonical::canonicalize_tool_result;

const DSML_TAG_PREFIX: &str = "<\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}";
const DSML_CLOSE_PREFIX: &str = "</\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}";

pub(super) fn parse_dsml_tool_call(text: &str) -> Option<(String, Value)> {
    let invoke_open = format!("{}invoke name=\"", DSML_TAG_PREFIX);
    let invoke_close = format!("{}invoke>", DSML_CLOSE_PREFIX);
    let param_open = format!("{}parameter", DSML_TAG_PREFIX);
    let param_close = format!("{}parameter>", DSML_CLOSE_PREFIX);

    let invoke_start = text.find(&invoke_open)?;
    let after_prefix = &text[invoke_start + invoke_open.len()..];

    let name_end = after_prefix.find('"')?;
    let name = after_prefix[..name_end].trim().to_string();
    if name.is_empty() {
        return None;
    }

    let after_name = &after_prefix[name_end + 1..];
    let tag_close = after_name.find('>')?;
    let rest = &after_name[tag_close + 1..];

    let content_end = rest.find(&invoke_close)?;
    let content = &rest[..content_end];

    let mut object = Map::new();
    let mut remaining = content;

    while let Some(param_start) = remaining.find(&param_open) {
        let after_tag = &remaining[param_start + param_open.len()..];

        let name_keyword = after_tag.find("name=\"")?;
        let name_content = &after_tag[name_keyword + "name=\"".len()..];
        let name_end = name_content.find('"')?;
        let param_name = name_content[..name_end].trim().to_string();

        let after_param_name = &name_content[name_end + 1..];
        let gt_pos = after_param_name.find('>')?;
        let is_string = after_param_name[..gt_pos].contains("string=\"true\"");

        let after_gt = &after_param_name[gt_pos + 1..];
        let value_end = after_gt.find(&param_close)?;
        let raw_value = after_gt[..value_end].trim();

        let value = if is_string {
            Value::String(raw_value.to_string())
        } else {
            serde_json::from_str::<Value>(raw_value)
                .unwrap_or_else(|_| Value::String(raw_value.to_string()))
        };

        object.insert(param_name, value);

        let consumed = param_start
            + param_open.len()
            + name_keyword
            + "name=\"".len()
            + name_end
            + 1
            + gt_pos
            + 1
            + value_end
            + param_close.len();
        remaining = &remaining[consumed..];
    }

    if object.is_empty() {
        return None;
    }

    canonicalize_tool_result(name, Value::Object(object))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[test]
    fn parses_single_dsml_invoke_with_string_params() {
        let text = concat!(
            "<\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}invoke name=\"unified_search\">\n",
            "<\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}parameter name=\"action\" string=\"true\">list</\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}parameter>\n",
            "<\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}parameter name=\"path\" string=\"true\">/src</\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}parameter>\n",
            "<\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}parameter name=\"scope\" string=\"true\">full</\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}parameter>\n",
            "</\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}invoke>",
        );

        let (name, args) = parse_dsml_tool_call(text).expect("should parse");
        assert_eq!(name, "unified_search");
        assert_eq!(args["action"], Value::String("list".to_string()));
        assert_eq!(args["path"], Value::String("/src".to_string()));
        assert_eq!(args["scope"], Value::String("full".to_string()));
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
            "<\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}invoke name=\"unified_search\">\n",
            "  <\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}parameter name=\"a\" string=\"true\">1</\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}parameter>\n",
            "</\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}invoke>\n",
            "<\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}invoke name=\"read_file\">\n",
            "  <\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}parameter name=\"b\" string=\"true\">2</\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}parameter>\n",
            "</\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}invoke>",
        );

        let (name, args) = parse_dsml_tool_call(text).expect("should parse");
        assert_eq!(name, "unified_search");
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
            "<\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}invoke name=\"unified_search\">\n",
            "<\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}parameter name=\"count\">42</\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}parameter>\n",
            "</\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}invoke>",
        );

        let (name, args) = parse_dsml_tool_call(text).expect("should parse");
        assert_eq!(name, "unified_search");
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
}
