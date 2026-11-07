use super::*;

fn sample_active_plan() -> ActivePlanEntry {
    ActivePlanEntry {
        file_path: ".vtcode/plans/task.md".to_string(),
        status: "approved -> build".to_string(),
        step_titles: vec!["Add sidebar panel".to_string(), "Wire approval handoff".to_string()],
        total_steps: 2,
    }
}

fn rendered_wide_session_text(session: &mut AppSession) -> String {
    let area = Rect::new(0, 0, 130, 30);
    let mut buffer = Buffer::empty(area);
    SessionWidget::new(&mut session.core)
        .layout_mode(LayoutMode::Wide)
        .render(area, &mut buffer);
    buffer.content().iter().map(|cell| cell.symbol()).collect::<String>()
}

#[test]
fn set_active_plan_command_renders_and_clears_plan_sidebar() {
    let mut session = AppSession::new(InlineTheme::default(), None, VIEW_ROWS);
    session.handle_command(app_types::InlineCommand::SetActivePlan { entry: Some(sample_active_plan()) });

    let rendered = rendered_wide_session_text(&mut session);
    assert!(rendered.contains("approved -> build"), "status badge missing: {rendered}");
    assert!(rendered.contains(".vtcode/plans/task.md"), "plan path missing: {rendered}");
    assert!(rendered.contains("Add sidebar panel"), "step title missing: {rendered}");

    session.handle_command(app_types::InlineCommand::SetActivePlan { entry: None });
    let cleared = rendered_wide_session_text(&mut session);
    assert!(!cleared.contains("approved -> build"), "cleared plan must disappear: {cleared}");
    assert!(!cleared.contains("Add sidebar panel"), "cleared steps must disappear: {cleared}");
}

#[test]
fn fresh_session_has_no_plan_sidebar() {
    let mut session = AppSession::new(InlineTheme::default(), None, VIEW_ROWS);

    let rendered = rendered_wide_session_text(&mut session);
    assert!(!rendered.contains("approved -> build"), "fresh session must show no plan: {rendered}");
}
