use super::*;

use vtcode_utility_tool_specs::{code_search_parameters, exec_command_parameters, write_stdin_parameters};

/// Collect every parameter description a tool schema declares.
fn schema_descriptions(parameters: &serde_json::Value) -> Vec<String> {
    let mut out = Vec::new();
    let Some(properties) = parameters.get("properties").and_then(serde_json::Value::as_object) else {
        return out;
    };
    for property in properties.values() {
        if let Some(description) = property.get("description").and_then(serde_json::Value::as_str) {
            out.push(description.to_string());
        }
    }
    out
}

/// The prompt must not restate a tool schema's own parameter documentation.
///
/// The tool definitions reach the model with every request, so a prompt line
/// that repeats a parameter's documented meaning is a second home for a rule
/// that will drift. This test derives the forbidden text from the schemas
/// themselves rather than hardcoding it, so a new schema is covered the day
/// it is added.
///
/// The check is a phrase overlap, not an exact match: the prompt states
/// behavior in its own words, so only a shared run of words counts.
#[test]
fn tool_schemas_own_their_parameter_documentation() {
    let mut forbidden: Vec<(String, String)> = Vec::new();
    for (tool, parameters) in [
        ("exec_command", exec_command_parameters()),
        ("write_stdin", write_stdin_parameters()),
        ("code_search", code_search_parameters()),
        (
            "task_tracker",
            crate::tools::handlers::task_tracker::task_tracker_parameter_schema_for_workflow(false),
        ),
    ] {
        for description in schema_descriptions(&parameters) {
            forbidden.push((tool.to_string(), description));
        }
    }
    assert!(!forbidden.is_empty(), "the schemas must expose descriptions to check");

    let tools = vec![
        TOOL_EXEC_COMMAND.to_string(),
        TOOL_WRITE_STDIN.to_string(),
        TOOL_APPLY_PATCH.to_string(),
        TOOL_CODE_SEARCH.to_string(),
        TOOL_TASK_TRACKER.to_string(),
    ];
    // Check every density and shell profile, so the rule holds for each
    // variant the runtime can render rather than one representative string.
    let variants = [
        (
            "default unix",
            generate_tool_guidelines_for_profile(&tools, None, ResolvedShellPromptProfile::UnixLike, true),
        ),
        (
            "default powershell",
            generate_tool_guidelines_for_profile(&tools, None, ResolvedShellPromptProfile::PowerShell, true),
        ),
        (
            "default planning",
            generate_runtime_tool_guidelines_for_profile(&tools, true, ResolvedShellPromptProfile::UnixLike),
        ),
        (
            "minimal",
            generate_tool_guidelines_with_capabilities(
                &tools,
                None,
                ResolvedShellPromptProfile::UnixLike,
                ToolGuidanceProfile::Minimal,
                true,
            ),
        ),
    ];

    for (variant, guidance) in &variants {
        for (tool, description) in &forbidden {
            // Only the leading sentence of a description is the fact a prompt
            // would restate; the tail often carries unrelated prose.
            let fact = description.split(". ").next().unwrap_or(description).trim();
            if fact.len() < 16 {
                continue;
            }
            let words: Vec<String> = fact
                .split_whitespace()
                .filter(|w| w.len() > 2 && w.chars().all(|c| c.is_alphanumeric()))
                .map(str::to_ascii_lowercase)
                .collect();
            if words.len() < 3 {
                continue;
            }
            // A restatement repeats the description's content words in a row.
            // The threshold is the whole phrase, not half of it: ordinary
            // vocabulary ("search", "file") overlaps everywhere, but a
            // verbatim phrase does not.
            let haystack = guidance.to_ascii_lowercase();
            let mut run = 0usize;
            let mut best = 0usize;
            for word in &words {
                if haystack.contains(word.as_str()) {
                    run += 1;
                    best = best.max(run);
                } else {
                    run = 0;
                }
            }
            assert!(
                best < words.len(),
                "the {variant} prompt restates the {tool} schema's own documentation ({fact:?}); the tool schema is its one home"
            );
        }
    }
}
