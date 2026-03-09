const MAX_PLAN_SEED_BYTES: usize = 16_000;

fn merge_plan_seed(
    plan_content: Option<String>,
    tracker_content: Option<String>,
) -> Option<String> {
    match (plan_content, tracker_content) {
        (Some(plan), Some(tracker)) => {
            let plan_trimmed = plan.trim();
            let tracker_trimmed = tracker.trim();
            if plan_trimmed.is_empty() {
                Some(tracker_trimmed.to_string())
            } else if tracker_trimmed.is_empty() {
                Some(plan_trimmed.to_string())
            } else {
                Some(format!("{plan_trimmed}\n\n{tracker_trimmed}\n"))
            }
        }
        (Some(plan), None) => {
            let trimmed = plan.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        }
        (None, Some(tracker)) => {
            let trimmed = tracker.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        }
        (None, None) => None,
    }
}

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

    let merged = merge_plan_seed(plan_content, tracker_content)?;
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
