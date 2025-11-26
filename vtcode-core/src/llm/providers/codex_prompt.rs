use std::collections::HashSet;

const BASE_PROMPT: &str = r#"You are Codex, based on GPT-5, running as the VTCode coding agent on a user's computer.

General
- Execute terminal commands via the shell tool using execvp semantics; default to [\"bash\", \"-lc\"] and always set the workdir.
- Prefer `rg` / `rg --files` for search and gather only the context you need.

Editing Constraints
- Default to ASCII; add comments only when they clarify non-obvious logic.
- Never revert edits you did not make; stop and ask if the workspace changes unexpectedly.

Planning
- Use the planning tool only for substantial work; never produce single-step plans and update the plan after completing a step.

Approvals
- Follow approval rules; request escalation only when a command truly requires it.

Tools
- Use the shell tool and apply_patch; keep tool descriptions concise and remove unnecessary details.

Responses
- No preambles. Return concise, actionable updates in plain text.
- Summarize command output instead of dumping logs; reference file paths instead of pasting entire files.
- Offer next steps only when they exist, otherwise stop after the solution.

Less is more—trust the model's defaults and avoid redundant prompting."#;

const DEFAULT_PROMPT_PREFIXES: &[&str] = &[
    "You are a coding agent running in VTCode",
    "You are a specialized coding agent running in VTCode",
    "You are Codex, based on GPT-5, running as the VTCode coding agent",
];

fn should_skip_guidance(text: &str) -> bool {
    DEFAULT_PROMPT_PREFIXES
        .iter()
        .any(|prefix| text.starts_with(prefix))
}

/// Construct the developer prompt for GPT-5-Codex requests following Codex CLI guidance.
///
/// Additional system instructions provided by the caller are appended under an
/// `Additional Guidance` section after filtering duplicates and default VTCode prompts.
pub(crate) fn gpt5_codex_developer_prompt(additional_guidance: &[String]) -> String {
    let mut seen = HashSet::new();
    let mut extras = Vec::new();

    for guidance in additional_guidance {
        let trimmed = guidance.trim();
        if trimmed.is_empty() || should_skip_guidance(trimmed) {
            continue;
        }

        let trimmed_owned = trimmed.to_string();
        if seen.insert(trimmed_owned) {
            extras.push(trimmed.to_string());
        }
    }

    if extras.is_empty() {
        BASE_PROMPT.to_string()
    } else {
        format!(
            "{base}\n\n## Additional Guidance\n{guidance}",
            base = BASE_PROMPT.trim_end(),
            guidance = extras.join("\n\n"),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn returns_base_prompt_when_no_guidance() {
        let result = gpt5_codex_developer_prompt(&[]);
        assert_eq!(result, BASE_PROMPT);
    }

    #[test]
    fn appends_unique_guidance_and_skips_defaults() {
        let inputs = vec![
            "You are Codex, based on GPT-5, running as the VTCode coding agent on a user's computer.".to_string(),
            "  Always summarize complex diffs before finalizing.".to_string(),
            "Always summarize complex diffs before finalizing.".to_string(),
            "Respect existing TODO comments; clarify before editing.".to_string(),
        ];

        let prompt = gpt5_codex_developer_prompt(&inputs);

        assert!(prompt.contains("## Additional Guidance"));
        assert!(prompt.contains("Always summarize complex diffs before finalizing."));
        assert!(prompt.contains("Respect existing TODO comments; clarify before editing."));
        assert_eq!(
            prompt
                .matches("Always summarize complex diffs before finalizing.")
                .count(),
            1
        );
    }
}
