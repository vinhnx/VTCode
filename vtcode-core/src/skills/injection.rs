use crate::skills::model::SkillLoadOutcome;
use std::path::PathBuf;
use tokio::fs;

#[derive(Debug, Default)]
pub struct SkillInjections {
    pub items: Vec<SkillInstructions>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct SkillInstructions {
    pub name: String,
    pub path: PathBuf,
    pub contents: String,
}

/// Builds injections for the specified skills.
///
/// `skill_names` is a list of skill names that should be loaded (e.g. detected from user input).
pub async fn build_skill_injections(
    skill_names: &[String],
    skills: Option<&SkillLoadOutcome>,
) -> SkillInjections {
    if skill_names.is_empty() {
        return SkillInjections::default();
    }

    let Some(outcome) = skills else {
        return SkillInjections::default();
    };

    let mut result = SkillInjections {
        items: Vec::with_capacity(skill_names.len()),
        warnings: Vec::new(),
    };

    for name in skill_names {
        if let Some(skill) = outcome.skills.iter().find(|s| s.name == *name) {
            match fs::read_to_string(&skill.path).await {
                Ok(contents) => {
                    result.items.push(SkillInstructions {
                        name: skill.name.clone(),
                        path: skill.path.clone(),
                        contents,
                    });
                }
                Err(err) => {
                    let message = format!(
                        "Failed to load skill {} at {}: {err:#}",
                        skill.name,
                        skill.path.display()
                    );
                    result.warnings.push(message);
                }
            }
        }
    }

    result
}
