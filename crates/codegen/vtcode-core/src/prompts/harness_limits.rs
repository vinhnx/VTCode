use crate::core::loop_detector::MAX_TOTAL_READONLY_CALLS;
use crate::prompts::sections::{SectionBoundaryMode, remove_prompt_section, trim_trailing_newlines};
use std::fmt::Write as _;

/// Per-file-path read cap advertised in the system prompt. Must match
/// `guards::MAX_SAME_FILE_PATH_READ_CALLS` (pinned by
/// `read_guard.rs::advertised_per_path_read_cap_matches_enforcement`).
/// Reading the same file more than this many times in a turn triggers recovery.
const MAX_SAME_FILE_PATH_READS: usize = 6;

/// Keep prompt guidance aligned with the selected user-visible harness limits.
///
/// This operation is idempotent: existing `[Harness Limits]` sections are removed before
/// inserting the current values.
pub fn upsert_harness_limits_section(
    prompt: &mut String,
    max_tool_calls_per_turn: usize,
    max_tool_wall_clock_secs: u64,
    max_tool_retries: u32,
) {
    let max_tool_calls_label = if max_tool_calls_per_turn == 0 {
        "unlimited".to_string()
    } else {
        max_tool_calls_per_turn.to_string()
    };
    let max_tool_wall_clock_label = if max_tool_wall_clock_secs == 0 {
        "unlimited".to_string()
    } else {
        format!("{max_tool_wall_clock_secs} seconds")
    };

    remove_prompt_section(prompt, "[Harness Limits]", SectionBoundaryMode::BracketOnly);
    trim_trailing_newlines(prompt);

    let section = format!(
        "[Harness Limits]\n\
         - max_tool_calls_per_turn: {max_tool_calls_label}\n\
         - max_tool_wall_clock_secs: {max_tool_wall_clock_label} (per turn; run long builds with exec_command, then call write_stdin next_wait_args with action:\"wait\" verbatim)\n\
         - max_tool_retries: {max_tool_retries}\n\
         - max_readonly_tool_calls: {MAX_TOTAL_READONLY_CALLS} (global budget across all read-only tools; produce output before exhausting)\n\
         - max_same_file_path_reads: {MAX_SAME_FILE_PATH_READS} (per file path per turn; read a file once in full rather than paginating)\n\
         - exec wait/inspect are exempt from max_tool_calls_per_turn\n\
         - long commands: set yield_time_ms above 10000 on the initial run for a single-call long run (no outer timeout), or reuse next_wait_args in write_stdin to block until exit or deadline"
    );

    if prompt.is_empty() {
        let _ = writeln!(prompt, "{section}");
    } else {
        let _ = writeln!(prompt, "\n{section}");
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
        assert!(prompt.contains("- max_readonly_tool_calls: 30"));
        assert!(prompt.contains("- max_same_file_path_reads: 6"));
        assert!(prompt.contains("exec wait/inspect are exempt from max_tool_calls_per_turn"));
        assert!(prompt.contains("single-call long run"));
    }

    #[test]
    fn upsert_harness_limits_states_wait_inspect_exemption_once() {
        // The wait/inspect budget exemption is one directive: the long-commands
        // line must not restate it. Asymmetric pin: deleting the canonical
        // line drops the count to 0, re-adding the restatement raises it to 2.
        let mut prompt = "Base prompt".to_string();

        upsert_harness_limits_section(&mut prompt, 12, 180, 2);

        assert_eq!(prompt.matches("exempt").count(), 1);
        assert!(prompt.contains("exec wait/inspect are exempt from max_tool_calls_per_turn"));
        assert!(prompt.contains("single-call long run"));
        assert!(prompt.contains("write_stdin next_wait_args"));
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
        assert!(prompt.contains("- max_tool_retries: 3"));
        assert!(prompt.contains("- max_readonly_tool_calls: 30"));
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

    #[test]
    fn upsert_harness_limits_renders_unlimited_when_tool_cap_disabled() {
        let mut prompt = "Base prompt".to_string();

        upsert_harness_limits_section(&mut prompt, 0, 600, 2);

        assert!(prompt.contains("- max_tool_calls_per_turn: unlimited"));
        assert!(prompt.contains("- max_tool_wall_clock_secs: 600 seconds"));
        assert!(prompt.contains("- max_tool_retries: 2"));
        assert!(prompt.contains("- max_readonly_tool_calls: 30"));
        assert!(prompt.contains("- max_same_file_path_reads: 6"));
    }

    #[test]
    fn upsert_harness_limits_renders_unlimited_wall_clock() {
        let mut prompt = "Base prompt".to_string();

        upsert_harness_limits_section(&mut prompt, 4, 0, 2);

        assert!(prompt.contains("- max_tool_wall_clock_secs: unlimited"));
        assert!(prompt.contains("write_stdin next_wait_args"));
    }
}
