use crate::skills::command_skills::is_model_catalog_eligible;
use crate::skills::model::SkillMetadata;

pub fn render_skills_section(skills: &[SkillMetadata]) -> Option<String> {
    if skills.is_empty() {
        return None;
    }

    let (mut lines, overflow) = render_skill_index(
        skills,
        "These skills are discovered at startup from multiple local sources. Each entry includes a description and file path so you can open the source for full instructions.",
    );

    if overflow > 0 {
        lines.push(format!("(+{} more skills available)", overflow));
    }

    lines.push(render_skills_usage_rules().to_string());

    Some(lines.join("\n"))
}

pub fn render_prompt_skills_section(skills: &[SkillMetadata]) -> Option<String> {
    let visible_skills = skills
        .iter()
        .filter(|skill| is_model_catalog_eligible(skill))
        .collect::<Vec<_>>();
    if visible_skills.is_empty() {
        return None;
    }

    let mut lines = Vec::new();
    lines.push("## Skills".to_string());
    lines.push(
        "Indexed for routing. Open only the relevant `SKILL.md` when the task or user request calls for it."
            .to_string(),
    );

    let mut sorted_skills = visible_skills;
    sorted_skills.sort_by(|left, right| left.name.cmp(&right.name));
    let overflow = sorted_skills.len().saturating_sub(10);
    if overflow > 0 {
        sorted_skills.truncate(10);
    }

    for skill in sorted_skills {
        lines.push(render_prompt_skill_line(skill));
    }

    if overflow > 0 {
        lines.push(format!("(+{} more skills available)", overflow));
    }

    lines.push("- Routing: Use a skill when the user names it or the task clearly matches the description. Load only the relevant `SKILL.md` on demand.".to_string());

    Some(lines.join("\n"))
}

/// Returns the standard skill usage rules (Codex-compatible).
/// These rules guide the agent on when and how to use skills.
fn render_skills_usage_rules() -> &'static str {
    r###"- Discovery: Available skills are listed in project docs and may also appear in a runtime "## Skills" section (name + description + file path). Skill bodies live on disk at the listed paths.
- Trigger rules: If the user names a skill (with `$SkillName` or plain text) OR the task clearly matches a skill's description, you must use that skill for that turn. Multiple mentions mean use them all. Do not carry skills across turns unless re-mentioned.
- Missing/blocked: If a named skill isn't in the list or the path can't be read, say so briefly and continue with the best fallback.
- How to use a skill (progressive disclosure):
  1) After deciding to use a skill, open its `SKILL.md`. Read only enough to follow the workflow.
  2) If `SKILL.md` points to extra folders such as `references/`, load only the specific files needed for the request; don't bulk-load everything.
  3) If `scripts/` exist, prefer running or patching them instead of retyping large code blocks.
  4) If `assets/` or templates exist, reuse them instead of recreating from scratch.
- Description as trigger: The YAML `description` in `SKILL.md` is the primary trigger signal. If unsure, ask a brief clarification before proceeding.
- Coordination and sequencing:
  - If multiple skills apply, choose the minimal set that covers the request and state the order you'll use them.
  - Announce which skill(s) you're using and why (one short line). If you skip an obvious skill, say why.
- Context hygiene:
  - Keep context small: summarize long sections instead of pasting them; only load extra files when needed.
  - Avoid deeply nested references; prefer one-hop files explicitly linked from `SKILL.md`.
  - When variants exist (frameworks, providers, domains), pick only the relevant reference file(s) and note that choice.
- Safety and fallback: If a skill can't be applied cleanly (missing files, unclear instructions), state the issue, pick the next-best approach, and continue."###
}

fn render_skill_index(skills: &[SkillMetadata], intro: &str) -> (Vec<String>, usize) {
    let mut lines = Vec::new();
    lines.push("## Skills".to_string());
    lines.push(intro.to_string());

    let mut sorted_skills = skills.iter().collect::<Vec<_>>();
    sorted_skills.sort_by(|left, right| left.name.cmp(&right.name));
    let overflow = sorted_skills.len().saturating_sub(10);
    if overflow > 0 {
        sorted_skills.truncate(10);
    }

    for skill in sorted_skills {
        lines.push(render_skill_line(skill));
    }

    (lines, overflow)
}

fn render_skill_line(skill: &SkillMetadata) -> String {
    let path_str = skill.path.to_string_lossy().replace('\\', "/");
    let name = skill.name.as_str();
    let description = skill.description.as_str();
    let scope = match skill.scope {
        crate::skills::model::SkillScope::User => "user",
        crate::skills::model::SkillScope::Repo => "repo",
        crate::skills::model::SkillScope::System => "system",
        crate::skills::model::SkillScope::Admin => "admin",
    };
    format!("- {name}: {description} (file: {path_str}, scope: {scope})")
}

fn render_prompt_skill_line(skill: &SkillMetadata) -> String {
    let path_str = skill.path.to_string_lossy().replace('\\', "/");
    format!(
        "- {}: {} (file: {})",
        skill.name, skill.description, path_str
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_render_skills_section_empty() {
        let skills: Vec<SkillMetadata> = vec![];
        let result = render_skills_section(&skills);
        assert_eq!(result, None);
    }

    #[test]
    fn test_render_skills_section_single() {
        let skill = SkillMetadata {
            name: "test-skill".to_string(),
            description: "A test skill".to_string(),
            short_description: None,
            path: PathBuf::from("/path/to/skill"),
            scope: crate::skills::model::SkillScope::User,
            manifest: None,
        };
        let skills = vec![skill];
        let result = render_skills_section(&skills);

        assert!(result.is_some());
        let output = result.unwrap();
        assert!(output.contains("## Skills"));
        assert!(output.contains("- test-skill: A test skill (file: /path/to/skill, scope: user)"));
        // Check for Codex-style usage rules
        assert!(output.contains("Discovery: Available skills are listed"));
        assert!(output.contains("Description as trigger"));
    }

    #[test]
    fn test_render_skills_section_multiple() {
        let skill1 = SkillMetadata {
            name: "skill-one".to_string(),
            description: "First skill".to_string(),
            short_description: None,
            path: PathBuf::from("/path/to/skill1"),
            scope: crate::skills::model::SkillScope::User,
            manifest: None,
        };
        let skill2 = SkillMetadata {
            name: "skill-two".to_string(),
            description: "Second skill".to_string(),
            short_description: None,
            path: PathBuf::from("\\path\\to\\skill2"), // Test path separator replacement
            scope: crate::skills::model::SkillScope::Repo,
            manifest: None,
        };
        let skills = vec![skill1, skill2];
        let result = render_skills_section(&skills);

        assert!(result.is_some());
        let output = result.unwrap();
        assert!(output.contains("## Skills"));
        assert!(output.contains("- skill-one: First skill (file: /path/to/skill1, scope: user)"));
        assert!(output.contains("- skill-two: Second skill (file: /path/to/skill2, scope: repo)")); // Path separator replaced
        assert!(output.contains("Context hygiene"));
    }

    #[test]
    fn test_render_prompt_skills_section_stays_lean() {
        let skill = SkillMetadata {
            name: "test-skill".to_string(),
            description: "A test skill".to_string(),
            short_description: None,
            path: PathBuf::from("/path/to/skill"),
            scope: crate::skills::model::SkillScope::User,
            manifest: None,
        };
        let output = render_prompt_skills_section(&[skill]).expect("prompt skills section");

        assert!(output.contains("## Skills"));
        assert!(output.contains("Indexed for routing"));
        assert!(output.contains("- test-skill: A test skill (file: /path/to/skill)"));
        assert!(output.contains("- Routing: Use a skill"));
        assert!(!output.contains("Discovery: Available skills are listed"));
        assert!(!output.contains("Description as trigger"));
        assert!(!output.contains("scope:"));
        assert!(!output.contains("; use:"));
    }

    #[test]
    fn test_render_prompt_skills_section_hides_command_skills() {
        let hidden_skill = SkillMetadata {
            name: "hidden-skill".to_string(),
            description: "Hidden from model activation".to_string(),
            short_description: None,
            path: PathBuf::from("/path/to/hidden-skill"),
            scope: crate::skills::model::SkillScope::System,
            manifest: Some(crate::skills::types::SkillManifest {
                name: "hidden-skill".to_string(),
                description: "Hidden from model activation".to_string(),
                disable_model_invocation: Some(true),
                ..Default::default()
            }),
        };
        let normal_skill = SkillMetadata {
            name: "repo-skill".to_string(),
            description: "A repo skill".to_string(),
            short_description: None,
            path: PathBuf::from("/path/to/repo-skill"),
            scope: crate::skills::model::SkillScope::Repo,
            manifest: None,
        };

        let output = render_prompt_skills_section(&[hidden_skill, normal_skill])
            .expect("prompt skills section");

        assert!(output.contains("repo-skill"));
        assert!(!output.contains("hidden-skill"));
    }

    #[test]
    fn test_render_skills_usage_rules() {
        let rules = render_skills_usage_rules();
        assert!(rules.contains("Description as trigger"));
        assert!(rules.contains("progressive disclosure"));
        assert!(rules.contains("Coordination and sequencing"));
        assert!(rules.contains("Context hygiene"));
        assert!(rules.contains("Safety and fallback"));
    }
}
