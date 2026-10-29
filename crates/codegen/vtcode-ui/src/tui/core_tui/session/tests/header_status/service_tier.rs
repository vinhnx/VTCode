use super::*;

#[test]
fn header_title_line_omits_service_tier_when_unset() {
    let mut session = fresh_session();
    session.header_context.provider = format!("{}openai", ui::HEADER_PROVIDER_PREFIX);
    session.header_context.model = format!("{}gpt-6-astra", ui::HEADER_MODEL_PREFIX);
    session.header_context.service_tier = None;

    let summary = line_text(&session.header_title_line());
    assert!(
        !summary.contains("ULTRAFAST") && !summary.contains("Priority") && !summary.contains("Tier:"),
        "unset tier must stay hidden, got: {summary}"
    );
}
#[test]
fn hidden_header_summary_shows_service_tier_on_top_right() {
    let mut session = fresh_session();
    session.appearance.hide_header = true;
    session.apply_transcript_width(VIEW_WIDTH);
    session.header_context.provider = format!("{}openai", ui::HEADER_PROVIDER_PREFIX);
    session.header_context.model = format!("{}gpt-6-astra", ui::HEADER_MODEL_PREFIX);
    session.header_context.reasoning = format!("{}medium", ui::HEADER_REASONING_PREFIX);
    session.header_context.service_tier = Some(format!("{}ultrafast", ui::HEADER_SERVICE_TIER_PREFIX));

    let lines = session.header_lines();
    assert_eq!(lines.len(), 1);

    let summary = line_text(&lines[0]);
    assert!(summary.contains("ULTRAFAST"), "tier should appear in top-right summary, got: {summary}");
}
#[test]
fn hidden_header_summary_live_reloads_service_tier_changes() {
    let mut session = fresh_session();
    session.appearance.hide_header = true;
    session.apply_transcript_width(VIEW_WIDTH);
    session.header_context.provider = format!("{}openai", ui::HEADER_PROVIDER_PREFIX);
    session.header_context.model = format!("{}gpt-6-astra", ui::HEADER_MODEL_PREFIX);
    session.header_context.reasoning = format!("{}medium", ui::HEADER_REASONING_PREFIX);

    let initial = header_line_text(&mut session);
    assert!(!initial.contains("ULTRAFAST"));

    let mut next_context = session.header_context.clone();
    next_context.service_tier = Some(format!("{}ultrafast", ui::HEADER_SERVICE_TIER_PREFIX));
    session.handle_command(InlineCommand::SetHeaderContext { context: Box::new(next_context) });

    let updated = header_line_text(&mut session);
    assert!(updated.contains("ULTRAFAST"), "tier should appear after context update, got: {updated}");
}
#[test]
fn hidden_header_summary_combines_provider_model_and_styles_effort_label() {
    let mut session = fresh_session();
    session.appearance.hide_header = true;
    session.apply_transcript_width(VIEW_WIDTH);
    session.header_context.provider = format!("{}mimo", ui::HEADER_PROVIDER_PREFIX);
    session.header_context.model = format!("{}mimo-v2.6-pro", ui::HEADER_MODEL_PREFIX);
    session.header_context.reasoning = format!("{}medium", ui::HEADER_REASONING_PREFIX);

    let lines = session.header_lines();
    assert_eq!(lines.len(), 1);

    let line = &lines[0];
    let summary = line_text(line);
    assert!(summary.contains("Mimo Mimo-V2.6-Pro"));
    assert!(!summary.contains("Mimo · Mimo-V2.6-Pro"));
    assert!(summary.contains("medium"));
}
#[test]
fn hidden_header_summary_live_reloads_model_changes() {
    let mut session = fresh_session();
    session.appearance.hide_header = true;
    session.apply_transcript_width(VIEW_WIDTH);
    session.header_context.provider = format!("{}mimo", ui::HEADER_PROVIDER_PREFIX);
    session.header_context.model = format!("{}mimo-v2.6-pro", ui::HEADER_MODEL_PREFIX);
    session.header_context.reasoning = format!("{}medium", ui::HEADER_REASONING_PREFIX);

    let initial = header_line_text(&mut session);
    assert!(initial.contains("Mimo Mimo-V2.6-Pro"));

    let mut next_context = session.header_context.clone();
    next_context.provider = format!("{}moonshot", ui::HEADER_PROVIDER_PREFIX);
    next_context.model = format!("{}kimi-k2.6", ui::HEADER_MODEL_PREFIX);
    next_context.reasoning = format!("{}high", ui::HEADER_REASONING_PREFIX);
    session.handle_command(InlineCommand::SetHeaderContext { context: Box::new(next_context) });

    let updated = header_line_text(&mut session);
    assert!(updated.contains("Moonshot Kimi-K2"));
    assert!(updated.contains("high"));
    assert!(!updated.contains("Mimo Mimo-V2.6-Pro"));
}
#[test]
fn hidden_header_summary_appears_after_width_set_post_measure() {
    // Reproduces the startup ordering bug: `measure_frame` reads `header_lines()`
    // while `transcript_width` is still 0, caching a header line WITHOUT the
    // right-aligned mode/model summary (it only renders when width > 0). The
    // transcript widget then sets the real width via `apply_transcript_width`.
    // The summary must appear on the next read without waiting for a keypress.
    let mut session = fresh_session();
    session.appearance.hide_header = true;
    session.header_context.provider = format!("{}mimo", ui::HEADER_PROVIDER_PREFIX);
    session.header_context.model = format!("{}mimo-v2.6-pro", ui::HEADER_MODEL_PREFIX);
    session.header_context.reasoning = format!("{}medium", ui::HEADER_REASONING_PREFIX);

    // First frame's measure step: width is still 0, so the summary is omitted
    // and the line is cached.
    let first = header_line_text(&mut session);
    assert!(!first.contains("Mimo Mimo-V2.6-Pro"), "width=0 header must omit the summary, got: {first}");

    // Transcript widget sets the real width during render.
    session.apply_transcript_width(VIEW_WIDTH);

    // The next read must reflect the now-known width without any keypress.
    let second = header_line_text(&mut session);
    assert!(
        second.contains("Mimo Mimo-V2.6-Pro"),
        "header summary should appear once transcript width is known, got: {second}"
    );
}
#[test]
fn hidden_header_summary_renders_active_primary_agent() {
    let mut session = fresh_session();
    session.appearance.hide_header = true;
    session.apply_transcript_width(VIEW_WIDTH);
    session.header_context.primary_agent = Some("duck".to_string());

    let line = header_line_text(&mut session);

    assert!(line.contains("Duck"));
    assert!(!line.contains("Edit"));
    assert!(!line.contains("Auto"));
    assert!(!line.contains("Plan"));
}
#[test]
fn header_context_updates_preserve_active_primary_agent_without_mode_badges() {
    let mut session = fresh_session();
    session.appearance.hide_header = true;
    session.apply_transcript_width(VIEW_WIDTH);
    session.header_context.primary_agent = Some("duck".to_string());

    let mut replacement = session.header_context.clone();
    replacement.provider = format!("{}mimo", ui::HEADER_PROVIDER_PREFIX);
    replacement.model = format!("{}mimo-v2.6-pro", ui::HEADER_MODEL_PREFIX);
    replacement.reasoning = format!("{}low", ui::HEADER_REASONING_PREFIX);
    session.handle_command(InlineCommand::SetHeaderContext { context: Box::new(replacement) });

    let line = header_line_text(&mut session);
    assert!(line.contains("Duck"));
    assert!(!line.contains("Edit"));
    assert!(!line.contains("Auto"));
    assert!(!line.contains("Plan"));
}
