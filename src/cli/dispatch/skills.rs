use crate::startup::StartupContext;
use anyhow::Result;
use vtcode_core::cli::args::{SkillsRefSubcommand, SkillsSubcommand};

use crate::cli::adapters::skills_options;
use crate::cli::{skills, skills_index, skills_ref};

pub(super) async fn dispatch_skills_command(
    startup: &StartupContext,
    skills_cmd: SkillsSubcommand,
) -> Result<()> {
    let skills_options = skills_options(startup);

    match skills_cmd {
        SkillsSubcommand::List { .. } => {
            skills::handle_skills_list(&skills_options).await?;
        }
        SkillsSubcommand::Load { name, path } => {
            skills::handle_skills_load(&skills_options, &name, path).await?;
        }
        SkillsSubcommand::Info { name } => {
            skills::handle_skills_info(&skills_options, &name).await?;
        }
        SkillsSubcommand::Create { path, .. } => {
            skills::handle_skills_create(&path).await?;
        }
        SkillsSubcommand::Validate { path, strict } => {
            skills::handle_skills_validate(&path, strict).await?;
        }
        SkillsSubcommand::CheckCompatibility => {
            skills::handle_skills_validate_all(&skills_options).await?;
        }
        SkillsSubcommand::Config => {
            skills::handle_skills_config(&skills_options).await?;
        }
        SkillsSubcommand::RegenerateIndex => {
            skills_index::handle_skills_regenerate_index(&skills_options).await?;
        }
        SkillsSubcommand::Unload { .. } => {
            println!("Skill unload not yet implemented");
        }
        SkillsSubcommand::SkillsRef(skills_ref_cmd) => match skills_ref_cmd {
            SkillsRefSubcommand::Validate { path } => {
                skills_ref::handle_skills_ref_validate(&path).await?;
            }
            SkillsRefSubcommand::ToPrompt { paths } => {
                skills_ref::handle_skills_ref_to_prompt(&paths).await?;
            }
            SkillsRefSubcommand::List { path } => {
                skills_ref::handle_skills_ref_list(path.as_deref()).await?;
            }
        },
    }

    Ok(())
}
