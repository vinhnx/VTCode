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
mod validate;

fn collect_enclosed_regions(text: &str, open_marker: &str, close_marker: &str, regions: &mut Vec<(usize, usize)>) {
    let mut search_start = 0usize;
    while let Some(relative_start) = text[search_start..].find(open_marker) {
        let start = search_start + relative_start;
        let content_start = start + open_marker.len();
        let end = text[content_start..]
            .find(close_marker)
            .map(|idx| content_start + idx + close_marker.len())
            .unwrap_or(text.len());
        if start < end && end <= text.len() {
            regions.push((start, end));
        }
        search_start = end.max(content_start);
    }
}

#[cfg(test)]
mod tests;

pub(crate) use code_fence::{CodeFenceBlock, extract_code_fence_blocks};
pub(crate) use detect::{contains_pseudo_tool_call_markers, detect_textual_tool_call, strip_textual_tool_call_regions};
pub(crate) use parse_dsml::{contains_dsml_markup, strip_dsml_markup};

pub(crate) use canonical::{canonicalize_shell_tool_alias, is_dispatchable_tool_name};
