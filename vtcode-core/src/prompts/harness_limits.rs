use std::fmt::Write as _;

fn find_prompt_section_bounds(prompt: &str, section_header: &str) -> Option<(usize, usize)> {
    fn is_section_header_line(line: &str) -> bool {
        let trimmed = line.trim();
        trimmed.starts_with('[') && trimmed.ends_with(']')
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
            && is_section_header_line(line)
        {
            return Some((start, offset));
        }

        offset += line.len();
    }

    section_start.map(|start| (start, prompt.len()))
}

/// Keep prompt guidance aligned with runtime harness enforcement limits.
///
/// This operation is idempotent: existing `[Harness Limits]` sections are removed before
/// inserting the current values.
pub fn upsert_harness_limits_section(
    prompt: &mut String,
    max_tool_calls_per_turn: usize,
    max_tool_wall_clock_secs: u64,
    max_tool_retries: u32,
) {
    while let Some((section_start, section_end)) =
        find_prompt_section_bounds(prompt, "[Harness Limits]")
    {
        prompt.replace_range(section_start..section_end, "");
    }

    while prompt.ends_with('\n') {
        prompt.pop();
    }

    if prompt.is_empty() {
        let _ = writeln!(
            prompt,
            "[Harness Limits]\n- max_tool_calls_per_turn: {}\n- max_tool_wall_clock_secs: {}\n- max_tool_retries: {}",
            max_tool_calls_per_turn, max_tool_wall_clock_secs, max_tool_retries
        );
    } else {
        let _ = writeln!(
            prompt,
            "\n[Harness Limits]\n- max_tool_calls_per_turn: {}\n- max_tool_wall_clock_secs: {}\n- max_tool_retries: {}",
            max_tool_calls_per_turn, max_tool_wall_clock_secs, max_tool_retries
        );
    }
}

#[cfg(test)]
mod tests {
    use super::upsert_harness_limits_section;

    #[test]
    fn upsert_harness_limits_adds_single_section() {
        let mut prompt = "Base prompt".to_string();

        upsert_harness_limits_section(&mut prompt, 12, 180, 2);

        assert_eq!(prompt.matches("[Harness Limits]").count(), 1);
        assert!(prompt.contains("- max_tool_calls_per_turn: 12"));
        assert!(prompt.contains("- max_tool_wall_clock_secs: 180"));
        assert!(prompt.contains("- max_tool_retries: 2"));
    }

    #[test]
    fn upsert_harness_limits_replaces_existing_values() {
        let mut prompt = "Base prompt\n[Harness Limits]\n- max_tool_calls_per_turn: 3\n- max_tool_wall_clock_secs: 60\n- max_tool_retries: 1\n".to_string();

        upsert_harness_limits_section(&mut prompt, 9, 240, 4);

        assert_eq!(prompt.matches("[Harness Limits]").count(), 1);
        assert!(prompt.contains("- max_tool_calls_per_turn: 9"));
        assert!(prompt.contains("- max_tool_wall_clock_secs: 240"));
        assert!(prompt.contains("- max_tool_retries: 4"));
        assert!(!prompt.contains("- max_tool_calls_per_turn: 3"));
    }

    #[test]
    fn upsert_harness_limits_preserves_trailing_prompt_sections() {
        let mut prompt = "Base prompt\n[Harness Limits]\n- max_tool_calls_per_turn: 3\n- max_tool_wall_clock_secs: 60\n- max_tool_retries: 1\n[Additional Context]\nKeep this section".to_string();

        upsert_harness_limits_section(&mut prompt, 11, 90, 3);

        assert_eq!(prompt.matches("[Harness Limits]").count(), 1);
        assert!(prompt.contains("[Additional Context]\nKeep this section"));
        assert!(prompt.ends_with("- max_tool_retries: 3\n"));
    }

    #[test]
    fn upsert_harness_limits_replaces_indented_section_header() {
        let mut prompt = "Base prompt\n  [Harness Limits]\n- max_tool_calls_per_turn: 1\n- max_tool_wall_clock_secs: 1\n- max_tool_retries: 1\n".to_string();

        upsert_harness_limits_section(&mut prompt, 5, 30, 2);

        assert_eq!(prompt.matches("[Harness Limits]").count(), 1);
        assert!(prompt.contains("- max_tool_calls_per_turn: 5"));
        assert!(!prompt.contains("- max_tool_calls_per_turn: 1"));
    }

    #[test]
    fn upsert_harness_limits_removes_duplicate_sections() {
        let mut prompt = "Base prompt\n[Harness Limits]\n- max_tool_calls_per_turn: 2\n- max_tool_wall_clock_secs: 10\n- max_tool_retries: 1\n[Other]\nkeep\n[Harness Limits]\n- max_tool_calls_per_turn: 3\n- max_tool_wall_clock_secs: 20\n- max_tool_retries: 2\n".to_string();

        upsert_harness_limits_section(&mut prompt, 7, 70, 3);

        assert_eq!(prompt.matches("[Harness Limits]").count(), 1);
        assert!(prompt.contains("- max_tool_calls_per_turn: 7"));
        assert!(prompt.contains("[Other]\nkeep"));
    }
}
