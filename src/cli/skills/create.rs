use anyhow::{Context, Result, bail};
use std::path::Path;
use vtcode_core::skills::manifest::generate_skill_template;
use vtcode_core::utils::colors::style;
use vtcode_core::utils::file_utils::{ensure_dir_exists_sync, write_file_with_context_sync};

use crate::cli::messages;

/// Create skill template.
pub async fn handle_skills_create(skill_path: &Path) -> Result<()> {
    if skill_path.exists() {
        bail!(
            "{}\n{}",
            messages::error(&format!(
                "Skill path already exists: {}",
                skill_path.display()
            )),
            messages::hint("Choose a different name or remove the existing skill first.")
        );
    }

    let skill_name = skill_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("my-skill");

    ensure_dir_exists_sync(skill_path).context("Failed to create skill directory")?;

    let template = generate_skill_template(skill_name, "Brief description of what this skill does");
    let skill_md = skill_path.join("SKILL.md");
    write_file_with_context_sync(&skill_md, &template, "SKILL.md")?;

    let _ = tokio::fs::create_dir(skill_path.join("scripts")).await;

    println!(
        "{}",
        messages::ok(&format!(
            "Created skill template at {}",
            skill_path.display()
        ))
    );
    println!(
        "  {} {}",
        style("SKILL.md").bold(),
        "Skill metadata and instructions"
    );
    println!(
        "  {} {}",
        style("scripts/").bold(),
        "Optional: executable scripts"
    );
    println!();
    println!("{}", style("Next steps:").bold());
    println!(
        "{}",
        messages::hint("Edit SKILL.md with your skill details.")
    );
    println!("{}", messages::hint("Add scripts to scripts/ if needed."));
    println!(
        "{}",
        messages::hint(&format!("Load with: vtcode skills load {}", skill_name))
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn creates_skill_template_directory() {
        let temp_dir = TempDir::new().expect("temp dir");
        let skill_path = temp_dir.path().join("test-skill");

        let result = handle_skills_create(&skill_path).await;

        result.unwrap();
        assert!(skill_path.join("SKILL.md").exists());
    }
}
