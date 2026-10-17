use std::fmt::Write as _;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SectionBoundaryMode {
    BracketOnly,
    BracketOrMarkdown,
}

pub(crate) fn find_prompt_section_bounds(
    prompt: &str,
    section_header: &str,
    boundary_mode: SectionBoundaryMode,
) -> Option<(usize, usize)> {
    fn is_section_header_line(line: &str, boundary_mode: SectionBoundaryMode) -> bool {
        let trimmed = line.trim();
        (trimmed.starts_with('[') && trimmed.ends_with(']'))
            || matches!(boundary_mode, SectionBoundaryMode::BracketOrMarkdown)
                && (trimmed.starts_with("# ") || trimmed.starts_with("## "))
    }

    let mut offset = 0usize;
    let mut section_start = None;

    for line in prompt.split_inclusive('\n') {
        let trimmed = line.trim();
        if section_start.is_none() && trimmed == section_header {
            section_start = Some(offset);
            offset += line.len();
            continue;
        }

        if let Some(start) = section_start
            && is_section_header_line(line, boundary_mode)
        {
            return Some((start, offset));
        }

        offset += line.len();
    }

    section_start.map(|start| (start, prompt.len()))
}

/// Canonical `"\n\n{block}"` append shared by prompt assemblers.
///
/// `harness_limits.rs` keeps its own single-`\n` `writeln!` append on purpose:
/// its section is re-inserted after a full trim and must end with exactly one
/// newline, so merging the two append shapes would change output.
pub(crate) fn append_prompt_block(prompt: &mut String, block: &str) {
    if block.is_empty() {
        return;
    }

    if prompt.is_empty() {
        prompt.push_str(block);
    } else {
        let _ = write!(prompt, "\n\n{block}");
    }
}

/// Remove every section headed by `section_header`, stopping at the next
/// header per `boundary_mode`. Callers keep their own mode so bracket-only
/// sections (harness limits) and markdown-aware sections (active tools,
/// deferred tools, catalog) preserve their contracts.
pub(crate) fn remove_prompt_section(prompt: &mut String, section_header: &str, boundary_mode: SectionBoundaryMode) {
    while let Some((section_start, section_end)) = find_prompt_section_bounds(prompt, section_header, boundary_mode) {
        prompt.replace_range(section_start..section_end, "");
    }
}

/// Strip trailing newlines before re-inserting a rebuilt section.
pub(crate) fn trim_trailing_newlines(prompt: &mut String) {
    while prompt.ends_with('\n') {
        prompt.pop();
    }
}

#[cfg(test)]
mod tests {
    use super::{SectionBoundaryMode, append_prompt_block, find_prompt_section_bounds, remove_prompt_section};

    #[test]
    fn markdown_boundary_mode_stops_before_next_markdown_heading() {
        let prompt = "Base\n## Active Tools\n- a\n## Environment\n- b\n";
        let bounds = find_prompt_section_bounds(prompt, "## Active Tools", SectionBoundaryMode::BracketOrMarkdown)
            .expect("section bounds");

        assert_eq!(&prompt[bounds.0..bounds.1], "## Active Tools\n- a\n");
    }

    #[test]
    fn bracket_only_mode_ignores_markdown_headings() {
        let prompt = "Base\n[Harness Limits]\n- a\n# Planning\n- b\n## Environment\n- c\n[Next Section]\n- d\n";
        let bounds = find_prompt_section_bounds(prompt, "[Harness Limits]", SectionBoundaryMode::BracketOnly)
            .expect("section bounds");

        assert_eq!(&prompt[bounds.0..bounds.1], "[Harness Limits]\n- a\n# Planning\n- b\n## Environment\n- c\n");
    }

    #[test]
    fn active_tools_boundary_preserves_following_planning_contract() {
        let prompt = "Base\n## Active Tools\n- old\n# PLANNING WORKFLOW (READ-ONLY)\nKeep the plan.\n";
        let (start, end) =
            find_prompt_section_bounds(prompt, "## Active Tools", SectionBoundaryMode::BracketOrMarkdown)
                .expect("section bounds");
        let mut replaced = prompt.to_string();
        replaced.replace_range(start..end, "## Active Tools\n- new\n");
        assert_eq!(replaced, "Base\n## Active Tools\n- new\n# PLANNING WORKFLOW (READ-ONLY)\nKeep the plan.\n");
    }

    #[test]
    fn shared_section_helpers_stay_idempotent() {
        let mut prompt = String::new();
        append_prompt_block(&mut prompt, "");
        assert!(prompt.is_empty());

        append_prompt_block(&mut prompt, "## Active Tools\n- a");
        assert_eq!(prompt, "## Active Tools\n- a");

        append_prompt_block(&mut prompt, "[Deferred Tools]\n- b");
        assert_eq!(prompt, "## Active Tools\n- a\n\n[Deferred Tools]\n- b");

        remove_prompt_section(&mut prompt, "## Active Tools", SectionBoundaryMode::BracketOrMarkdown);
        assert_eq!(prompt, "[Deferred Tools]\n- b");

        prompt.push_str("\n\n");
        super::trim_trailing_newlines(&mut prompt);
        assert_eq!(prompt, "[Deferred Tools]\n- b");
    }
}
