use vtcode_core::config::constants::tools;
use vtcode_core::utils::ansi::MessageStyle;
use vtcode_ui::tui::app::PlanContent;

pub(super) async fn auto_create_task_tracker_from_plan(
    tool_registry: &vtcode_core::tools::registry::ToolRegistry,
    renderer: &mut vtcode_core::utils::ansi::AnsiRenderer,
) -> anyhow::Result<()> {
    let plan_state = tool_registry.planning_workflow_state();
    let plan_file = match plan_state.get_plan_file().await {
        Some(path) => path,
        None => return Ok(()),
    };

    let plan_content = match tokio::fs::read_to_string(&plan_file).await {
        Ok(content) => content,
        Err(_) => return Ok(()),
    };

    if plan_content.trim().is_empty() {
        return Ok(());
    }

    let plan = PlanContent::from_markdown(
        plan_file
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Implementation Plan")
            .to_string(),
        &plan_content,
        plan_file.to_str().map(|s| s.to_string()),
    );

    let mut items = Vec::new();
    for phase in &plan.phases {
        for step in &phase.steps {
            if step.description.trim().is_empty() {
                continue;
            }
            let mut obj = serde_json::json!({
                "description": step.description,
                "status": if step.completed { "completed" } else { "pending" },
            });
            if !step.files.is_empty() {
                obj["files"] = serde_json::to_value(step.files.clone()).unwrap();
            }
            items.push(obj);
        }
    }

    if items.is_empty() {
        return Ok(());
    }

    let tool = match tool_registry.get_tool(tools::TASK_TRACKER) {
        Some(tool) => tool,
        None => return Ok(()),
    };

    let args = serde_json::json!({
        "action": "create",
        "title": plan.title,
        "items": items,
    });

    match tool.execute(args).await {
        Ok(result) => {
            let msg = result.get("message").and_then(|m| m.as_str()).unwrap_or("Task tracker created");
            let _ = renderer.line(MessageStyle::Info, &format!("📋 {msg}"));
        }
        Err(err) => {
            tracing::warn!("Failed to auto-create task tracker from plan: {}", err);
        }
    }

    Ok(())
}
