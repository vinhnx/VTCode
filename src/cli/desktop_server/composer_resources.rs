//! Metadata only: reuse the same discovery paths as `/skills` and `/plugin`.
use anyhow::{Context, Result};
use std::path::PathBuf;
use vtcode_desktop_protocol::{ComposerResource, ComposerResourceKind};

pub(super) async fn discover(workspace: PathBuf) -> Result<Vec<ComposerResource>> {
    // Both CLI discovery APIs perform filesystem traversal. Run them on blocking
    // workers, never the native foreground or the service's async I/O worker.
    let runtime = tokio::runtime::Handle::current();
    tokio::task::spawn_blocking(move || {
        let mut loader = vtcode_core::skills::loader::EnhancedSkillLoader::new(workspace.clone());
        let discovered = runtime.block_on(loader.discover_all_skills())?;
        let mut resources = discovered
            .skills
            .into_iter()
            .filter_map(|skill| {
                let manifest = skill.manifest();
                ComposerResource::is_valid_name(&manifest.name).then(|| ComposerResource {
                    name: manifest.name.clone(),
                    description: manifest.description.chars().take(400).collect(),
                    kind: ComposerResourceKind::Skill,
                })
            })
            .take(128)
            .collect::<Vec<_>>();
        resources.extend(
            crate::agent::runloop::discover_installed_plugins(&workspace)
                .into_iter()
                .filter(|plugin| ComposerResource::is_valid_name(&plugin.name))
                .take(64)
                .map(|plugin| ComposerResource {
                    name: plugin.name,
                    description: plugin.description.chars().take(400).collect(),
                    kind: ComposerResourceKind::Plugin,
                }),
        );
        resources.sort_by(|a, b| a.name.cmp(&b.name));
        resources.dedup_by(|a, b| a.name == b.name && a.kind == b.kind);
        Ok(resources)
    })
    .await
    .context("composer discovery task failed")?
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn composer_resource_names_cannot_inject_draft_commands() {
        assert!(ComposerResource::is_valid_name("rust-skills.v2"));
        for rejected in [
            "",
            "skill name",
            "skill\n/run",
            "../escape",
            "$override",
            "name;command",
        ] {
            assert!(!ComposerResource::is_valid_name(rejected), "{rejected:?}");
        }
        assert!(!ComposerResource::is_valid_name(&"a".repeat(129)));
        assert!(ComposerResource::is_valid_name(&"a".repeat(128)));
    }
}
