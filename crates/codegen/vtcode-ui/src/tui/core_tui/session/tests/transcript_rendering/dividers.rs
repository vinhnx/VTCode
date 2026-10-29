use super::*;

#[test]
fn tool_followed_by_agent_has_section_divider() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.push_line(InlineMessageKind::Tool, vec![make_segment("• Ran cargo check")]);
    session.push_line(InlineMessageKind::Agent, vec![make_segment("synthesis of the check")]);

    let rendered = session.reflow_transcript_lines(80);
    let texts: Vec<String> = rendered.iter().map(line_text).collect();
    let tool = texts
        .iter()
        .position(|text| text.contains("Ran cargo check"))
        .expect("tool header");
    let answer = texts.iter().position(|text| text.contains("synthesis")).expect("agent answer");
    let dividers = divider_positions(&texts);
    assert_eq!(dividers.len(), 1, "expected one section divider, got {texts:?}");
    let divider = dividers[0];
    assert!(divider > tool && divider < answer, "divider should sit between tool and agent, got {texts:?}");
    let gap = &texts[tool + 1..divider];
    assert_eq!(gap.len(), 1, "expected exactly one blank row before the divider, got {texts:?}");
    assert!(gap[0].trim().is_empty());
}
#[test]
fn pty_followed_by_agent_has_section_divider() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    push_pty_line(&mut session, "• Ran git status");
    push_pty_line(&mut session, "M src/main.rs");
    session.push_line(InlineMessageKind::Agent, vec![make_segment("working tree is dirty")]);

    let rendered = session.reflow_transcript_lines(80);
    let texts: Vec<String> = rendered.iter().map(line_text).collect();
    let dividers = divider_positions(&texts);
    assert_eq!(dividers.len(), 1, "pty burst should close with one divider, got {texts:?}");
    let answer = texts.iter().position(|text| text.contains("dirty")).expect("agent answer");
    assert!(dividers[0] < answer);
}
#[test]
fn info_summary_followed_by_agent_has_section_divider() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.push_line(InlineMessageKind::Tool, vec![make_segment("• Search code")]);
    session.push_line(InlineMessageKind::Info, vec![make_segment("  └ Path: src/")]);
    session.push_line(InlineMessageKind::Agent, vec![make_segment("found the loop")]);

    let rendered = session.reflow_transcript_lines(80);
    let texts: Vec<String> = rendered.iter().map(line_text).collect();
    let dividers = divider_positions(&texts);
    assert_eq!(dividers.len(), 1, "tool detail tail should still break the section, got {texts:?}");
}
#[test]
fn agent_separates_from_tool_and_pty_blocks() {
    for kind in [InlineMessageKind::Tool, InlineMessageKind::Pty] {
        let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
        session.push_line(InlineMessageKind::Agent, vec![make_segment("lead-in prose")]);
        let header = if kind == InlineMessageKind::Tool {
            "• Ran cargo check"
        } else {
            "• Ran git status"
        };
        if kind == InlineMessageKind::Tool {
            session.push_line(kind, vec![make_segment(header)]);
        } else {
            push_pty_line(&mut session, header);
        }

        let rendered = session.reflow_transcript_lines(80);
        let texts: Vec<String> = rendered.iter().map(line_text).collect();
        let prose = texts
            .iter()
            .position(|text| text.contains("lead-in prose"))
            .expect("agent prose");
        let block = texts.iter().position(|text| text.contains("Ran")).expect("tool header");
        assert_eq!(block, prose + 2, "prose must keep one blank row before {kind:?} block, got {texts:?}");
        assert!(texts[prose + 1].trim().is_empty(), "gap row should be blank, got {texts:?}");
    }
}
#[test]
fn agent_prose_separates_from_work_log_with_rule_section_break() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.push_line(InlineMessageKind::Agent, vec![make_segment("Links all resolve.")]);
    session.push_line(InlineMessageKind::Info, vec![make_segment("• Ran 5 commands")]);
    session.push_line(InlineMessageKind::Agent, vec![make_segment("The provider list is richer.")]);

    let rendered = session.reflow_transcript_lines(80);
    let texts: Vec<String> = rendered.iter().map(line_text).collect();
    let prose = texts
        .iter()
        .position(|text| text.contains("Links all resolve."))
        .expect("agent prose");
    let hint = texts
        .iter()
        .position(|text| text.contains("Ran 5 commands"))
        .expect("work-log hint");
    assert_eq!(hint, prose + 2, "prose must keep one blank row before its work log, got {texts:?}");
    assert!(texts[prose + 1].trim().is_empty(), "gap row should be blank, got {texts:?}");
    let dividers = divider_positions(&texts);
    assert_eq!(dividers.len(), 1, "one rule separates the sections, got {texts:?}");
    let follow = texts
        .iter()
        .position(|text| text.contains("provider list"))
        .expect("next prose");
    assert_eq!(follow, dividers[0] + 1, "next prose must hug the rule, got {texts:?}");
}
#[test]
fn agent_followed_by_agent_has_no_section_divider() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.push_line(InlineMessageKind::Agent, vec![make_segment("first paragraph")]);
    session.push_line(InlineMessageKind::Agent, vec![make_segment("second paragraph")]);

    let rendered = session.reflow_transcript_lines(80);
    let texts: Vec<String> = rendered.iter().map(line_text).collect();
    assert!(divider_positions(&texts).is_empty(), "consecutive agent lines must not divide, got {texts:?}");
}
#[test]
fn empty_agent_after_tool_has_no_section_divider() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.push_line(InlineMessageKind::Tool, vec![make_segment("• Ran cargo check")]);
    session.push_line(InlineMessageKind::Agent, vec![make_segment("   ")]);

    let rendered = session.reflow_transcript_lines(80);
    let texts: Vec<String> = rendered.iter().map(line_text).collect();
    assert!(divider_positions(&texts).is_empty(), "empty agent rows must not add chrome, got {texts:?}");
}
#[test]
fn policy_followed_by_agent_has_no_section_divider() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.push_line(InlineMessageKind::Policy, vec![make_segment("thinking aloud")]);
    session.push_line(InlineMessageKind::Agent, vec![make_segment("the answer")]);

    let rendered = session.reflow_transcript_lines(80);
    let texts: Vec<String> = rendered.iter().map(line_text).collect();
    assert!(divider_positions(&texts).is_empty(), "reasoning -> content flow must stay undivided, got {texts:?}");
}
#[test]
fn user_followed_by_agent_has_no_section_divider() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.push_line(InlineMessageKind::User, vec![make_segment("run the check")]);
    session.push_line(InlineMessageKind::Agent, vec![make_segment("on it")]);

    let rendered = session.reflow_transcript_lines(80);
    let texts: Vec<String> = rendered.iter().map(line_text).collect();
    // The single divider belongs to the User turn itself; no extra section
    // divider may appear between User and the following Agent.
    let dividers = divider_positions(&texts);
    assert_eq!(dividers.len(), 1, "only the user turn divider should exist, got {texts:?}");
    let user = texts.iter().position(|text| text.contains("run the check")).expect("user text");
    let agent = texts.iter().position(|text| text.contains("on it")).expect("agent text");
    assert!(dividers[0] < user, "user divider should precede the user text, got {texts:?}");
    assert!(
        !dividers.iter().any(|divider| *divider > user && *divider < agent),
        "no section divider between user and agent, got {texts:?}"
    );
}
#[test]
fn section_divider_uses_muted_dim_style() {
    use anstyle::{Color as AnsiColorEnum, RgbColor};

    let muted = AnsiColorEnum::Rgb(RgbColor(0x77, 0x99, 0xAA));
    let theme = InlineTheme {
        secondary: Some(muted),
        primary: Some(AnsiColorEnum::Rgb(RgbColor(0x12, 0x34, 0x56))),
        ..Default::default()
    };
    let mut session = Session::new(theme, None, VIEW_ROWS);
    session.push_line(InlineMessageKind::Tool, vec![make_segment("• Ran cargo check")]);
    session.push_line(InlineMessageKind::Agent, vec![make_segment("synthesis")]);

    let agent_index = 1;
    let rows = session.reflow_message_lines(agent_index, 40, false);
    let divider = rows
        .iter()
        .find(|row| {
            let text: String = row.line.spans.iter().map(|span| span.content.as_ref()).collect();
            !text.is_empty() && text.chars().all(|ch| ch == '─')
        })
        .expect("section divider row");
    let expected_fg = Some(ratatui_color_from_ansi(muted));
    assert_eq!(divider.line.style.fg, expected_fg);
    assert!(divider.line.style.add_modifier.contains(Modifier::DIM));
    assert!(!divider.line.style.add_modifier.contains(Modifier::BOLD));
}
