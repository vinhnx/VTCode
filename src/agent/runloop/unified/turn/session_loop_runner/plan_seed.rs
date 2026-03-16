const MAX_PLAN_SEED_BYTES: usize = 16_000;

pub(super) async fn load_active_plan_seed(
    tool_registry: &vtcode_core::tools::registry::ToolRegistry,
) -> Option<String> {
    let plan_state = tool_registry.plan_mode_state();
    let plan_file = plan_state.get_plan_file().await?;
    let plan_content = tokio::fs::read_to_string(&plan_file).await.ok();
    let tracker_file = plan_file
        .file_stem()
        .and_then(|stem| stem.to_str())
        .map(|stem| plan_file.with_file_name(format!("{stem}.tasks.md")));
    let tracker_content = if let Some(path) = tracker_file {
        if path.exists() {
            tokio::fs::read_to_string(path).await.ok()
        } else {
            None
        }
    } else {
        None
    };

    let merged =
        vtcode_core::tools::handlers::plan_mode::merge_plan_content(plan_content, tracker_content)?;
    if merged.len() > MAX_PLAN_SEED_BYTES {
        let truncated = merged
            .char_indices()
            .nth(MAX_PLAN_SEED_BYTES)
            .map(|(idx, _)| merged[..idx].to_string())
            .unwrap_or(merged);
        return Some(format!("{truncated}\n\n[plan context truncated]"));
    }

    Some(merged)
}
