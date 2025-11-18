use std::borrow::Cow;

/// Use direct tool name without alias resolution.
pub fn canonical_tool_name<'a>(name: &'a str) -> Cow<'a, str> {
    Cow::Borrowed(name)
}
