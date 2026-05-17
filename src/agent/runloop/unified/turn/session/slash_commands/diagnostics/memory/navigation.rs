use anyhow::Result;
use vtcode_core::persistent_memory::PersistentMemoryStatus;

use super::{SlashCommandContext, SlashCommandControl};

pub(super) async fn handle_memory_navigation_action(
    ctx: &mut SlashCommandContext<'_>,
    action_key: &str,
    memory_status: &PersistentMemoryStatus,
) -> Result<Option<SlashCommandControl>> {
    match action_key {
        "open_settings_section" => super::super::super::show_settings_at_path_from_context(
            ctx,
            Some("agent.persistent_memory"),
        )
        .await
        .map(Some),
        "open_summary" => super::super::super::apps::launch_editor_from_context(
            ctx,
            Some(memory_status.summary_file.display().to_string()),
        )
        .await
        .map(Some),
        "open_directory" => super::super::super::apps::launch_editor_from_context(
            ctx,
            Some(memory_status.directory.display().to_string()),
        )
        .await
        .map(Some),
        _ => Ok(None),
    }
}
