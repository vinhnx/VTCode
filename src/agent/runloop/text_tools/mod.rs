mod canonical;
mod code_fence;
mod detect;
mod parse_args;
mod parse_bracketed;
mod parse_channel;
mod parse_dsml;
mod parse_structured;
mod parse_tagged;
mod parse_yaml;
mod parser;

#[cfg(test)]
mod tests;

pub(crate) use code_fence::{CodeFenceBlock, extract_code_fence_blocks};
pub(crate) use detect::{
    contains_pseudo_tool_call_markers, detect_textual_tool_call, strip_textual_tool_call_regions,
};
pub(crate) use parse_dsml::strip_dsml_markup;
