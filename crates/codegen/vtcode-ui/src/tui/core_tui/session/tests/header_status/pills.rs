use super::*;

#[test]
fn mode_pill_absent_without_primary_agent() {
    let mut session = fresh_session();
    session.handle_command(InlineCommand::SetPrimaryAgent { name: None, color: None });

    let rendered = session
        .render_input_status_line(VIEW_WIDTH)
        .map(|line| line.spans.iter().map(|span| span.content.as_ref()).collect::<String>())
        .unwrap_or_default();

    assert!(!rendered.contains("•"), "modeless status line should not show a mode pill, got: {rendered}");

    session.handle_command(InlineCommand::SetPrimaryAgent {
        name: Some("   ".to_string()),
        color: Some("build".to_string()),
    });
    let blank = session
        .render_input_status_line(VIEW_WIDTH)
        .map(|line| line.spans.iter().map(|span| span.content.as_ref()).collect::<String>())
        .unwrap_or_default();
    assert!(!blank.contains("•"), "blank mode name should not show a mode pill, got: {blank}");
}
#[test]
fn mode_pill_shows_build_and_plan_distinctly() {
    let mut session = fresh_session();
    session.handle_command(InlineCommand::SetPrimaryAgent {
        name: Some("build".to_string()),
        color: Some("build".to_string()),
    });
    let build_line = session.render_input_status_line(VIEW_WIDTH).expect("mode pill status line");
    let build_text: String = build_line.spans.iter().map(|span| span.content.as_ref()).collect();
    assert!(build_text.contains("• Build"), "unexpected status: {build_text}");
    let build_fg = build_line
        .spans
        .iter()
        .find(|span| span.content.as_ref().contains("• Build"))
        .expect("build pill span")
        .style
        .fg;

    session.handle_command(InlineCommand::SetPrimaryAgent {
        name: Some("plan".to_string()),
        color: Some("plan".to_string()),
    });
    let plan_line = session.render_input_status_line(VIEW_WIDTH).expect("mode pill status line");
    let plan_text: String = plan_line.spans.iter().map(|span| span.content.as_ref()).collect();
    assert!(plan_text.contains("• Plan"), "unexpected status: {plan_text}");
    assert!(!plan_text.contains("• Build"), "stale mode pill, got: {plan_text}");
    let plan_fg = plan_line
        .spans
        .iter()
        .find(|span| span.content.as_ref().contains("• Plan"))
        .expect("plan pill span")
        .style
        .fg;

    assert!(build_fg.is_some() && plan_fg.is_some(), "mode pill must carry a concrete color");
    assert_ne!(build_fg, plan_fg, "build and plan pills must be visually distinct");
}
#[test]
fn mode_pill_falls_back_for_unknown_color_token() {
    let mut session = fresh_session();
    session.handle_command(InlineCommand::SetPrimaryAgent {
        name: Some("custom".to_string()),
        color: Some("not-a-color".to_string()),
    });

    let line = session.render_input_status_line(VIEW_WIDTH).expect("mode pill status line");
    let text: String = line.spans.iter().map(|span| span.content.as_ref()).collect();
    assert!(text.contains("• Custom"), "unexpected status: {text}");
    let fg = line
        .spans
        .iter()
        .find(|span| span.content.as_ref().contains("• Custom"))
        .expect("custom pill span")
        .style
        .fg;
    assert!(fg.is_some(), "unknown color token must fall back to a concrete color");
}
#[test]
fn mode_pill_coexists_with_right_status() {
    let mut session = fresh_session();
    session.handle_command(InlineCommand::SetInputStatus {
        left: None,
        right: Some("84% context left".to_string()),
    });
    session.handle_command(InlineCommand::SetPrimaryAgent {
        name: Some("auto".to_string()),
        color: Some("auto".to_string()),
    });

    let rendered = session
        .render_input_status_line(VIEW_WIDTH)
        .expect("mode pill status line")
        .spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>();

    assert!(rendered.contains("• Auto"), "missing mode pill, got: {rendered}");
    assert!(rendered.contains("84% context left"), "mode pill must not clobber right status, got: {rendered}");
}
#[test]
fn mode_adds_no_input_border_chrome() {
    let mut session = fresh_session();
    session.handle_command(InlineCommand::SetPrimaryAgent {
        name: Some("build".to_string()),
        color: Some("build".to_string()),
    });

    // The composer stays borderless and compact in every mode: no titles, no
    // extra border rows. Only shell (`!`) and subagent badges add chrome.
    assert_eq!(session.shell_mode_border_title(), None);
    assert!(session.active_subagent_input_title().is_none());
    assert_eq!(session.input_block_extra_height(), 0);
}
#[test]
fn hidden_header_init_sequence_shows_agent_with_provider_model_and_effort() {
    // Regression for the missing top-right summary: init must push
    // `SetHeaderContext` (provider/model/effort) before `SetPrimaryAgent`.
    // Asymmetric values so a missing piece fails distinctly.
    let mut session = fresh_session();
    session.appearance.hide_header = true;
    session.apply_transcript_width(VIEW_WIDTH);

    let mut context = session.header_context.clone();
    context.provider = format!("{}moonshot", ui::HEADER_PROVIDER_PREFIX);
    context.model = format!("{}kimi-k2.6", ui::HEADER_MODEL_PREFIX);
    context.reasoning = format!("{}high", ui::HEADER_REASONING_PREFIX);
    context.primary_agent = None;
    session.handle_command(InlineCommand::SetHeaderContext { context: Box::new(context) });
    session.handle_command(InlineCommand::SetPrimaryAgent {
        name: Some("build".to_string()),
        color: Some("build".to_string()),
    });

    let line = header_line_text(&mut session);
    assert!(line.contains("Build"), "missing agent badge, got: {line}");
    assert!(line.contains("Moonshot"), "missing provider, got: {line}");
    assert!(line.contains("Kimi-K2"), "missing model, got: {line}");
    assert!(line.contains("high"), "missing reasoning effort, got: {line}");
}
#[test]
fn hidden_header_preserves_agent_when_context_refreshes_after_agent() {
    // Post-hydration path pushes agent first, then a context refresh with
    // `primary_agent: None` must not clobber it.
    let mut session = fresh_session();
    session.appearance.hide_header = true;
    session.apply_transcript_width(VIEW_WIDTH);
    session.handle_command(InlineCommand::SetPrimaryAgent { name: Some("duck".to_string()), color: None });

    let mut context = session.header_context.clone();
    context.provider = format!("{}mimo", ui::HEADER_PROVIDER_PREFIX);
    context.model = format!("{}mimo-v2.6-pro", ui::HEADER_MODEL_PREFIX);
    context.reasoning = format!("{}low", ui::HEADER_REASONING_PREFIX);
    context.primary_agent = None;
    session.handle_command(InlineCommand::SetHeaderContext { context: Box::new(context) });

    let line = header_line_text(&mut session);
    assert!(line.contains("Duck"), "agent must survive context refresh, got: {line}");
    assert!(line.contains("Mimo Mimo-V2.6-Pro"), "missing provider/model, got: {line}");
    assert!(line.contains("low"), "missing reasoning effort, got: {line}");
}
#[test]
fn ultrafast_header_badge_is_padded_premium_gold_in_both_layouts() {
    use anstyle::{Color as AnsiColor, RgbColor};
    use ratatui::{
        buffer::Buffer,
        layout::Rect,
        style::Style,
        widgets::{Paragraph, Widget},
    };

    for (speed, label) in [("UlTrAfAsT", " ULTRAFAST "), ("FaSt", " FAST ")] {
        for hidden in [false, true] {
            let amber = RgbColor(224, 176, 72);
            let background = RgbColor(18, 20, 28);
            let theme = InlineTheme {
                warning: Some(AnsiColor::Rgb(amber)),
                background: Some(AnsiColor::Rgb(background)),
                ..Default::default()
            };
            let mut session = Session::new(theme, None, VIEW_ROWS);
            session.appearance.hide_header = hidden;
            session.apply_transcript_width(VIEW_WIDTH);
            session.header_context.provider = format!("{}openai", ui::HEADER_PROVIDER_PREFIX);
            session.header_context.model = format!("{}gpt-6.1-sol", ui::HEADER_MODEL_PREFIX);
            session.header_context.service_tier = Some(format!("{}{speed}", ui::HEADER_SERVICE_TIER_PREFIX));
            let lines = session.header_lines();
            let badge = lines[0].spans.iter().find(|span| span.content == label).expect("premium badge");

            let colors_enabled = vtcode_commons::color_policy::color_output_enabled();
            let (foreground, background) = if colors_enabled {
                (Color::Rgb(0, 0, 0), Color::Rgb(0xD9, 0x9A, 0x4E))
            } else {
                (Color::Reset, Color::Reset)
            };
            assert_eq!(badge.style.fg, Some(foreground));
            assert_eq!(badge.style.bg, Some(background));
            assert_eq!(badge.style.add_modifier.contains(Modifier::REVERSED), !colors_enabled);
            assert!(badge.style.sub_modifier.contains(Modifier::DIM));
            assert_eq!(badge.style.add_modifier.contains(Modifier::BOLD), !crate::theme::is_bold_bright_mode());

            let area = Rect::new(0, 0, 20, 1);
            let mut buffer = Buffer::empty(area);
            Paragraph::new(Line::from(vec![Span::raw("tier "), badge.clone()]))
                .style(Style::default().add_modifier(Modifier::DIM))
                .render(area, &mut buffer);
            assert!(buffer[(0, 0)].modifier.contains(Modifier::DIM));
            assert!(!buffer[(6, 0)].modifier.contains(Modifier::DIM));
            assert_eq!(buffer[(6, 0)].fg, foreground);
            assert_eq!(buffer[(6, 0)].bg, background);
        }
    }
}
#[test]
fn ordinary_service_tiers_keep_their_existing_header_treatment() {
    let mut session = fresh_session();
    for (tier, label) in [("priority", "Priority"), ("flex", "Flex")] {
        session.header_context.service_tier = Some(format!("{}{tier}", ui::HEADER_SERVICE_TIER_PREFIX));
        let line = session.header_title_line();
        let span = line.spans.iter().find(|span| span.content == label).expect("ordinary tier");
        assert!(!span.style.add_modifier.contains(Modifier::REVERSED));
        assert!(span.style.add_modifier.contains(Modifier::DIM));
        assert!(!line_text(&line).contains("ULTRAFAST"));
    }
}
#[test]
fn ultrafast_header_badge_respects_narrow_terminal_widths_and_can_be_disabled() {
    for width in [1, 8, 16, 32, 64, 100] {
        let mut session = fresh_session();
        session.appearance.hide_header = true;
        session.apply_transcript_width(width);
        session.header_context.service_tier = Some(format!("{}ultrafast", ui::HEADER_SERVICE_TIER_PREFIX));
        let lines = session.header_lines();
        assert!(lines[0].width() <= usize::from(width), "width={width}");
    }
    let mut session = fresh_session();
    session.header_context.service_tier = Some(format!("{}ultrafast", ui::HEADER_SERVICE_TIER_PREFIX));
    assert!(header_line_text(&mut session).contains(" ULTRAFAST "));
    let mut context = session.header_context.clone();
    context.service_tier = None;
    session.handle_command(InlineCommand::SetHeaderContext { context: Box::new(context) });
    assert!(!header_line_text(&mut session).contains("ULTRAFAST"));
}
