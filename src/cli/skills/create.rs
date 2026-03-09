use anyhow::{Context, Result, bail};
use std::path::Path;
use vtcode_core::skills::manifest::generate_skill_template;
use vtcode_core::utils::file_utils::{ensure_dir_exists_sync, write_file_with_context_sync};

/// Create skill template.
pub async fn handle_skills_create(skill_path: &Path) -> Result<()> {
    if skill_path.exists() {
        bail!("Skill path already exists: {}", skill_path.display());
    }

    let skill_name = skill_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("my-skill");

    ensure_dir_exists_sync(skill_path).context("Failed to create skill directory")?;

    let template = generate_skill_template(skill_name, "Brief description of what this skill does");
    let skill_md = skill_path.join("SKILL.md");
    write_file_with_context_sync(&skill_md, &template, "SKILL.md")?;

    std::fs::create_dir(skill_path.join("scripts")).ok();

    println!("Created skill template at: {}", skill_path.display());
    println!("  • SKILL.md - Skill metadata and instructions");
    println!("  • scripts/ - Optional: executable scripts");
    println!("\nNext steps:");
    println!("  1. Edit SKILL.md with your skill details");
    println!("  2. Add scripts to scripts/ if needed");
    println!("  3. Load with: vtcode skills load {}", skill_name);

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

        assert!(result.is_ok());
        assert!(skill_path.join("SKILL.md").exists());
    }
}
