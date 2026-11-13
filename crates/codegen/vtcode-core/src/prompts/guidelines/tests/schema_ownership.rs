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

/// The longest run of consecutive words copied verbatim from `fact` into `hay`.
///
/// A restatement copies the schema's wording, so a whole phrase appears in
/// the prompt. Ordinary wording overlap ("shell", "command", "write") does
/// not, because those words are scattered rather than adjacent. Requiring a
/// contiguous run separates the two, and it lets a genuine paraphrase pass:
/// restating a rule in different words is fine, copying the schema's
/// documentation is not.
fn longest_copied_run(fact: &str, hay: &str) -> usize {
    let words: Vec<&str> = fact.split_whitespace().collect();
    let hay: Vec<String> = hay.split_whitespace().map(str::to_ascii_lowercase).collect();
    let mut best = 0usize;
    for start in 0..words.len() {
        for end in (start + 4..=words.len()).rev() {
            if end - start <= best {
                break;
            }
            let phrase: Vec<String> = words[start..end].iter().map(|w| w.to_ascii_lowercase()).collect();
            let copied = hay.windows(phrase.len()).any(|window| window == phrase.as_slice());
            if copied {
                best = end - start;
                break;
            }
        }
    }
    best
}

/// The share of a description that the prompt copies verbatim.
///
/// The share, not a raw word count, keeps a short description from tripping
/// on three words and lets a long one be caught at the same relative
/// severity. A restatement lands near 1.0; ordinary wording overlap lands
/// near 0.0.
fn copied_share(fact: &str, hay: &str) -> f64 {
    let words = fact.split_whitespace().count();
    if words == 0 {
        return 0.0;
    }
    longest_copied_run(fact, hay) as f64 / words as f64
}

/// The prompt must not restate a tool schema's own parameter documentation.
///
/// The tool definitions reach the model with every request, so a prompt line
/// that copies a parameter's documented meaning is a second home for a rule
/// that will drift. This test derives the forbidden text from the schemas
/// themselves rather than hardcoding it, so a new schema is covered the day
/// it is added.
///
/// The check looks for copied wording, not for the exact description: the
/// prompt states behavior in its own words, so only a shared phrase counts.
#[test]
fn tool_schemas_own_their_parameter_documentation() {
    // The tracker has two schemas, one per workflow, and the planning one
    // documents facts the standard one does not (the positive-index rule).
    // Both are checked, because a planning prompt must not restate either.
    let mut forbidden: Vec<(String, String)> = Vec::new();
    for (tool, parameters) in [
        ("exec_command", exec_command_parameters()),
        ("write_stdin", write_stdin_parameters()),
        ("code_search", code_search_parameters()),
        (
            "task_tracker",
            crate::tools::handlers::task_tracker::task_tracker_parameter_schema_for_workflow(false),
        ),
        (
            "task_tracker planning",
            crate::tools::handlers::task_tracker::task_tracker_parameter_schema_for_workflow(true),
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
            let copied = copied_share(fact, guidance);
            assert!(
                copied < 0.6,
                "the {variant} prompt restates the {tool} schema's own documentation \
                 ({:.0}% of {fact:?} copied verbatim); the tool schema is its one home",
                copied * 100.0
            );
        }
    }
}
