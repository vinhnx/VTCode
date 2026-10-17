#![allow(
    missing_docs,
    reason = "Intentional compatibility, platform, or test-only suppression."
)]
use super::super::*;
use super::helpers::*;

fn make_list_item(title: &str, cmd: &str) -> InlineListItem {
    InlineListItem {
        title: title.to_string(),
        subtitle: None,
        badge: None,
        indent: 0,
        selection: Some(InlineListSelection::SlashCommand(cmd.to_string())),
        search_value: Some(title.to_string()),
        ..Default::default()
    }
}

fn show_list_modal(session: &mut AppSession, title: &str, lines: Vec<&str>, items: Vec<InlineListItem>) {
    session.handle_command(app_types::InlineCommand::ShowTransient {
        request: Box::new(app_types::TransientRequest::List(app_types::ListOverlayRequest {
            title: title.to_string(),
            lines: lines.into_iter().map(|s| s.to_string()).collect(),
            footer_hint: None,
            items,
            selected: None,
            search: None,
            hotkeys: Vec::new(),
            status: None,
        })),
    });
}

fn render_session_to_terminal(session: &mut AppSession, rows: u16) -> Terminal<TestBackend> {
    let backend = TestBackend::new(VIEW_WIDTH, rows);
    let mut terminal = Terminal::new(backend).expect("failed to create test terminal");
    terminal.draw(|frame| session.render(frame)).expect("failed to render session");
    terminal
}

fn render_session_to_terminal_app(session: &mut Session, rows: u16) -> Terminal<TestBackend> {
    let backend = TestBackend::new(VIEW_WIDTH, rows);
    let mut terminal = Terminal::new(backend).expect("failed to create test terminal");
    terminal.draw(|frame| session.render(frame)).expect("failed to render session");
    terminal
}

fn show_overlay(
    session: &mut Session,
    title: &str,
    lines: Vec<&str>,
    items: Vec<InlineListItem>,
    selected: Option<InlineListSelection>,
) {
    show_overlay_with_hint(session, title, lines, items, selected, None);
}

fn show_overlay_with_hint(
    session: &mut Session,
    title: &str,
    lines: Vec<&str>,
    items: Vec<InlineListItem>,
    selected: Option<InlineListSelection>,
    footer_hint: Option<&str>,
) {
    session.handle_command(InlineCommand::ShowOverlay {
        request: Box::new(OverlayRequest::List(ListOverlayRequest {
            title: title.to_string(),
            lines: lines.into_iter().map(|s| s.to_string()).collect(),
            footer_hint: footer_hint.map(str::to_string),
            items,
            selected,
            search: None,
            hotkeys: Vec::new(),
            status: None,
        })),
    });
}

fn show_list_modal_with_hint(
    session: &mut AppSession,
    title: &str,
    lines: Vec<&str>,
    items: Vec<InlineListItem>,
    footer_hint: Option<&str>,
) {
    session.handle_command(app_types::InlineCommand::ShowTransient {
        request: Box::new(app_types::TransientRequest::List(app_types::ListOverlayRequest {
            title: title.to_string(),
            lines: lines.into_iter().map(|s| s.to_string()).collect(),
            footer_hint: footer_hint.map(str::to_string),
            items,
            selected: None,
            search: None,
            hotkeys: Vec::new(),
            status: None,
        })),
    });
}

fn modal_selection(session: &Session) -> Option<InlineListSelection> {
    session
        .modal_state()
        .and_then(|modal| modal.list.as_ref())
        .and_then(|list| list.current_selection())
}

#[test]
fn show_list_modal_renders_as_floating_transient_without_bottom_panel() {
    let mut session = AppSession::new(InlineTheme::default(), None, 30);
    show_list_modal(&mut session, "Pick one", vec!["Choose an option"], vec![make_list_item("Option A", "a")]);

    let _terminal = render_session_to_terminal(&mut session, 30);

    assert!(session.has_active_overlay(), "floating list modal should remain active after rendering");
    assert!(
        session.core.bottom_panel_area().is_none(),
        "floating list modal should not render in the bottom panel"
    );
}

#[test]
fn skip_confirmations_does_not_dismiss_user_selection_modals() {
    let mut session = Session::new(InlineTheme::default(), None, 30);
    session.handle_command(InlineCommand::SetSkipConfirmations(true));
    show_overlay(&mut session, "Pick one", vec!["Choose an option"], vec![make_list_item("Option A", "a")], None);

    let _terminal = render_session_to_terminal_app(&mut session, 30);

    assert!(session.has_active_overlay(), "skip confirmations must not dismiss user selection modals");
}

#[test]
fn show_list_modal_uses_bottom_half_of_terminal() {
    let mut session = AppSession::new(InlineTheme::default(), None, 30);
    show_list_modal(&mut session, "Pick one", vec!["Choose an option"], vec![make_list_item("Option A", "a")]);

    let lines = rendered_app_session_lines(&mut session, 30);
    assert!(
        lines.get(15).is_some_and(|line| line.contains("Pick one")),
        "floating modal title should start at the halfway row"
    );

    let modal_area = session.core.modal_list_area().expect("modal list area");
    assert!(
        modal_area.y >= 17,
        "floating modal list should render below the title chrome, got y={}",
        modal_area.y
    );
}

#[test]
fn floating_modal_clips_transcript_and_closing_restores_full_viewport() {
    let marker = "transcript-visible-above-approval";
    let mut session = AppSession::new(InlineTheme::default(), None, 30);
    for index in 0..40 {
        session
            .core
            .push_line(InlineMessageKind::Agent, vec![make_segment(&format!("transcript line {index}"))]);
    }
    session.core.push_line(InlineMessageKind::Agent, vec![make_segment(marker)]);

    let mut terminal = render_session_to_terminal(&mut session, 30);
    let full_transcript_area = session.core.transcript_area().expect("full transcript area");
    let full_transcript_rows = session.core.transcript_rows;

    show_list_modal(&mut session, "Approval", vec!["Ready to continue?"], vec![make_list_item("Yes", "yes")]);
    terminal
        .draw(|frame| session.render(frame))
        .expect("failed to render floating modal");

    let clipped_transcript_area = session.core.transcript_area().expect("clipped transcript area");
    let modal_area = session.core.modal_list_area().expect("modal list area");
    assert_eq!(clipped_transcript_area.x, full_transcript_area.x);
    assert_eq!(clipped_transcript_area.width, full_transcript_area.width);
    assert!(clipped_transcript_area.height < full_transcript_area.height);
    assert!(clipped_transcript_area.y.saturating_add(clipped_transcript_area.height) <= modal_area.y);
    assert!(session.core.transcript_rows < full_transcript_rows);

    let marker_visible_above_modal = (0..modal_area.y).any(|row| {
        (0..VIEW_WIDTH)
            .filter_map(|column| terminal.backend().buffer().cell((column, row)))
            .map(|cell| cell.symbol())
            .collect::<String>()
            .contains(marker)
    });
    assert!(marker_visible_above_modal, "long transcript content should remain visible above the modal");

    session.close_transient();
    terminal
        .draw(|frame| session.render(frame))
        .expect("failed to render after closing modal");

    assert_eq!(session.core.transcript_area(), Some(full_transcript_area));
    assert_eq!(session.core.transcript_rows, full_transcript_rows);
}

#[test]
fn floating_modal_render_handles_single_row_terminal() {
    let mut session = AppSession::new(InlineTheme::default(), None, 30);
    show_list_modal(&mut session, "Approval", vec!["Ready?"], vec![make_list_item("Yes", "yes")]);

    let _terminal = render_session_to_terminal(&mut session, 1);

    assert!(session.core.transcript_area().is_none());
}

#[test]
fn titled_floating_modal_renders_matching_title_and_divider_chrome() {
    let mut session = AppSession::new(InlineTheme::default(), None, 30);
    show_list_modal(&mut session, "Pick one", vec!["Choose an option"], vec![make_list_item("Option A", "a")]);

    let terminal = render_session_to_terminal(&mut session, 30);

    let buffer = terminal.backend().buffer();
    let title_cell = buffer.cell((0, 15)).expect("title cell");
    let top_divider_cell = buffer.cell((0, 16)).expect("top divider cell");
    let bottom_divider_cell = buffer.cell((0, 29)).expect("bottom divider cell");

    assert_eq!(title_cell.symbol(), "P");
    assert_eq!(top_divider_cell.symbol(), ui::INLINE_BLOCK_HORIZONTAL);
    assert_eq!(bottom_divider_cell.symbol(), ui::INLINE_BLOCK_HORIZONTAL);
    assert_ne!(title_cell.style().bg, Some(Color::Indexed(ui::SAFE_ANSI_BRIGHT_CYAN)));
    assert_eq!(top_divider_cell.style().fg, Some(Color::Indexed(ui::SAFE_ANSI_BRIGHT_CYAN)));
    assert_eq!(bottom_divider_cell.style().fg, Some(Color::Indexed(ui::SAFE_ANSI_BRIGHT_CYAN)));
}

#[test]
fn floating_modal_clears_stale_buffer_content_before_painting() {
    let theme = InlineTheme {
        foreground: Some(AnsiColorEnum::Rgb(RgbColor(0x22, 0x22, 0x22))),
        background: Some(AnsiColorEnum::Rgb(RgbColor(0xF5, 0xF5, 0xF0))),
        primary: Some(AnsiColorEnum::Rgb(RgbColor(0x7A, 0x8F, 0xFF))),
        ..InlineTheme::default()
    };
    let mut session = AppSession::new(theme, None, 30);
    show_list_modal(
        &mut session,
        "Theme",
        vec!["Choose a theme"],
        vec![InlineListItem {
            title: "Clapre".to_string(),
            subtitle: None,
            badge: None,
            indent: 0,
            selection: Some(InlineListSelection::SlashCommand("theme".to_string())),
            search_value: Some("Clapre".to_string()),
            ..Default::default()
        }],
    );

    let backend = TestBackend::new(VIEW_WIDTH, 30);
    let mut terminal = Terminal::new(backend).expect("failed to create test terminal");
    terminal
        .draw(|frame| {
            let filler = (0..30).map(|_| Line::from("X".repeat(VIEW_WIDTH as usize))).collect::<Vec<_>>();
            frame.render_widget(ratatui::widgets::Paragraph::new(filler), frame.area());
        })
        .expect("failed to prefill terminal buffer");
    terminal
        .draw(|frame| session.render(frame))
        .expect("failed to render modal over stale buffer");

    let buffer = terminal.backend().buffer();
    let title_tail_cell = buffer.cell((VIEW_WIDTH.saturating_sub(1), 15)).expect("title tail cell");
    let body_blank_cell = buffer.cell((10, 25)).expect("body blank cell");

    assert_eq!(title_tail_cell.symbol(), " ");
    assert_eq!(body_blank_cell.symbol(), " ");
    assert_eq!(title_tail_cell.style().bg, Some(Color::Rgb(0xF5, 0xF5, 0xF0)));
    assert_eq!(body_blank_cell.style().bg, Some(Color::Rgb(0xF5, 0xF5, 0xF0)));
}

#[test]
fn selected_modal_row_uses_primary_foreground() {
    let theme = InlineTheme {
        foreground: Some(AnsiColorEnum::Rgb(RgbColor(0xEE, 0xEE, 0xEE))),
        primary: Some(AnsiColorEnum::Rgb(RgbColor(0x12, 0x34, 0x56))),
        ..InlineTheme::default()
    };
    let mut session = AppSession::new(theme, None, 30);
    let selection = InlineListSelection::SlashCommand("a".to_string());
    session.handle_command(app_types::InlineCommand::ShowTransient {
        request: Box::new(app_types::TransientRequest::List(app_types::ListOverlayRequest {
            title: "Pick one".to_string(),
            lines: vec!["Choose an option".to_string()],
            footer_hint: None,
            items: vec![InlineListItem {
                title: "Option A".to_string(),
                subtitle: None,
                badge: Some("Active".to_string()),
                indent: 0,
                selection: Some(selection.clone()),
                search_value: Some("Option A".to_string()),
                ..Default::default()
            }],
            selected: Some(selection),
            search: None,
            hotkeys: Vec::new(),
            status: None,
        })),
    });

    let terminal = render_session_to_terminal(&mut session, 30);

    let modal_area = session.core.modal_list_area().expect("modal list area");
    let title_cell = terminal
        .backend()
        .buffer()
        .cell((modal_area.x + 11, modal_area.y))
        .expect("selected row title cell");
    assert_eq!(title_cell.style().fg, Some(Color::Rgb(0x12, 0x34, 0x56)));
    assert!(title_cell.style().add_modifier.contains(Modifier::BOLD));
}

#[test]
fn modal_section_header_uses_foreground_contrast_on_light_theme() {
    let theme = InlineTheme {
        foreground: Some(AnsiColorEnum::Rgb(RgbColor(0x22, 0x22, 0x22))),
        background: Some(AnsiColorEnum::Rgb(RgbColor(0xF5, 0xF5, 0xF0))),
        primary: Some(AnsiColorEnum::Rgb(RgbColor(0x7A, 0x8F, 0xFF))),
        ..InlineTheme::default()
    };
    let mut session = AppSession::new(theme, None, 30);
    show_list_modal(
        &mut session,
        "Theme",
        vec!["Choose a theme"],
        vec![
            InlineListItem {
                title: "Built-in themes".to_string(),
                subtitle: None,
                badge: None,
                indent: 0,
                selection: None,
                search_value: Some("Built-in themes".to_string()),
                ..Default::default()
            },
            InlineListItem {
                title: "Clapre".to_string(),
                subtitle: None,
                badge: None,
                indent: 0,
                selection: Some(InlineListSelection::SlashCommand("theme".to_string())),
                search_value: Some("Clapre".to_string()),
                ..Default::default()
            },
        ],
    );

    let terminal = render_session_to_terminal(&mut session, 30);

    let lines = rendered_app_session_lines(&mut session, 30);
    let title_row = lines.iter().position(|line| line.trim() == "Theme").expect("title row");
    let modal_area = session.core.modal_list_area().expect("modal list area");
    let header_cell = terminal
        .backend()
        .buffer()
        .cell((modal_area.x + 2, modal_area.y))
        .expect("section header cell");
    let title_cell = terminal
        .backend()
        .buffer()
        .cell((modal_area.x, title_row as u16))
        .expect("title cell");

    assert_eq!(title_cell.symbol(), "T");
    assert_eq!(title_cell.style().bg, Some(Color::Rgb(0xF5, 0xF5, 0xF0)));
    assert_eq!(header_cell.symbol(), "B");
    assert_eq!(header_cell.style().fg, Some(Color::Rgb(0x7A, 0x8F, 0xFF)));
    assert_eq!(header_cell.style().bg, Some(Color::Rgb(0xF5, 0xF5, 0xF0)));
    assert!(header_cell.style().add_modifier.contains(Modifier::BOLD));
}

#[test]
fn floating_modal_renders_approval_text_without_dim_modifier() {
    // Regression: the modal background used to be painted with
    // `Modifier::DIM` (via `styles.selectable`). ratatui's `Cell::set_style`
    // only *inserts* modifiers, so that DIM stuck to every glyph drawn on
    // top of it — the whole HITL approval popup rendered dimmed and the
    // command under review was hard to read. Popup text must recede only
    // via explicit muted colors, never via DIM.
    let theme = InlineTheme {
        foreground: Some(AnsiColorEnum::Rgb(RgbColor(0xEE, 0xEE, 0xEE))),
        background: Some(AnsiColorEnum::Rgb(RgbColor(0x10, 0x14, 0x1A))),
        primary: Some(AnsiColorEnum::Rgb(RgbColor(0x12, 0x34, 0x56))),
        secondary: Some(AnsiColorEnum::Rgb(RgbColor(0x8A, 0x93, 0xA3))),
        ..InlineTheme::default()
    };
    let mut session = AppSession::new(theme, None, 30);
    let approval = |title: &str, subtitle: &str, badge: &str, selection: InlineListSelection| InlineListItem {
        title: title.to_string(),
        subtitle: Some(subtitle.to_string()),
        badge: Some(badge.to_string()),
        indent: 0,
        selection: Some(selection),
        search_value: Some(title.to_string()),
        ..Default::default()
    };
    session.handle_command(app_types::InlineCommand::ShowTransient {
        request: Box::new(app_types::TransientRequest::List(app_types::ListOverlayRequest {
            title: "Would you like to run the following command?".to_string(),
            lines: vec![
                "Environment: default policy".to_string(),
                "`$ cargo nextest run`".to_string(),
                "Choose how to handle this run:".to_string(),
            ],
            footer_hint: Some("Use ↑↓ or Tab to navigate • Enter to select • Esc to deny".to_string()),
            items: vec![
                approval("Approve Once", "Allow this time only", "Permanent", InlineListSelection::ToolApproval(true)),
                approval(
                    "Allow for Session",
                    "For the current session",
                    "Session",
                    InlineListSelection::ToolApprovalSession,
                ),
                approval("Deny Once", "Ask again next time", "Persistent", InlineListSelection::ToolApprovalDenyOnce),
            ],
            selected: Some(InlineListSelection::ToolApproval(true)),
            search: None,
            hotkeys: Vec::new(),
            status: None,
        })),
    });

    let terminal = render_session_to_terminal(&mut session, 30);
    let buffer = terminal.backend().buffer();

    let mut dimmed_popup_text = String::new();
    for row in 15..30u16 {
        for column in 0..VIEW_WIDTH {
            let cell = buffer.cell((column, row)).expect("modal cell");
            if cell.symbol() != " " && cell.style().add_modifier.contains(Modifier::DIM) {
                dimmed_popup_text.push_str(cell.symbol());
            }
        }
    }
    assert!(
        dimmed_popup_text.trim().is_empty(),
        "approval popup text must not render dimmed, got: {dimmed_popup_text:?}"
    );

    // Emphasis stays intact: the selected row is bold and accent-colored,
    // while unselected option titles keep the full theme foreground so every
    // choice is readable.
    let modal_area = session.core.modal_list_area().expect("modal list area");
    let row_text = |row: u16| -> String {
        (0..VIEW_WIDTH)
            .filter_map(|column| buffer.cell((column, row)))
            .map(|cell| cell.symbol().to_owned())
            .collect::<String>()
    };
    let text_column =
        |row: u16, needle: &str| -> u16 { row_text(row).find(needle).expect("approval text in row") as u16 };
    let find_row = |needle: &str| -> u16 {
        (modal_area.y..modal_area.y.saturating_add(modal_area.height))
            .find(|row| row_text(*row).contains(needle))
            .unwrap_or_else(|| panic!("{needle} row in modal area {modal_area:?}"))
    };

    let selected_row = find_row("Approve Once");
    let selected_cell = buffer
        .cell((text_column(selected_row, "Approve Once"), selected_row))
        .expect("selected row title cell");
    assert_eq!(selected_cell.style().fg, Some(Color::Rgb(0x12, 0x34, 0x56)));
    assert!(selected_cell.style().add_modifier.contains(Modifier::BOLD));
    assert!(!selected_cell.style().add_modifier.contains(Modifier::DIM));

    let unselected_row = find_row("Allow for Session");
    assert_ne!(selected_row, unselected_row, "selected and unselected rows must differ");
    let unselected_cell = buffer
        .cell((text_column(unselected_row, "Allow for Session"), unselected_row))
        .expect("unselected row title cell");
    assert_eq!(unselected_cell.style().fg, Some(Color::Rgb(0xEE, 0xEE, 0xEE)));
    assert!(!unselected_cell.style().add_modifier.contains(Modifier::DIM));
}

#[test]
fn untitled_floating_modal_skips_title_chrome_but_keeps_section_divider() {
    let mut session = AppSession::new(InlineTheme::default(), None, 30);
    show_list_modal(&mut session, "", vec!["Choose an option"], vec![make_list_item("Option A", "a")]);

    let lines = rendered_app_session_lines(&mut session, 30);
    assert!(
        lines.get(15).is_some_and(|line| line.contains("Choose an option")),
        "untitled modal body should begin at the floating modal origin"
    );

    // Title chrome (title row plus its surrounding dividers) is skipped, but
    // the intentional instructions→list section divider still renders exactly
    // once between the instructions and the list items.
    let rule_rows: Vec<usize> = (15..30)
        .filter(|row| lines.get(*row).is_some_and(|line| is_horizontal_rule(line)))
        .collect();
    assert_eq!(rule_rows, vec![16], "untitled modal should render only the section divider");
    let instructions_row = lines
        .iter()
        .position(|line| line.contains("Choose an option"))
        .expect("instructions row");
    let item_row = lines.iter().position(|line| line.contains("Option A")).expect("list item row");
    assert!(instructions_row < rule_rows[0] && rule_rows[0] < item_row);

    let modal_area = session.core.modal_list_area().expect("modal list area");
    assert_eq!(modal_area.y, 17);
}

#[test]
fn inline_modal_height_budgets_list_divider_without_search() {
    use super::super::render::split_inline_modal_area;

    let mut session = Session::new(InlineTheme::default(), None, 30);
    // Sixteen three-line items saturate the 20-row multiline cap, so the exact
    // height below is sensitive to the divider row: 1 instructions +
    // 20 list (16 items × title/subtitle/gap, capped) + 1 divider + 0 summary
    // + 3 title chrome = 25.
    let items = (0..16)
        .map(|index| InlineListItem {
            title: format!("Option {index}"),
            subtitle: Some(format!("detail {index}")),
            badge: None,
            indent: 0,
            selection: Some(InlineListSelection::SlashCommand(format!("cmd{index}"))),
            search_value: Some(format!("option {index}")),
            ..Default::default()
        })
        .collect::<Vec<_>>();
    show_overlay(&mut session, "Pick", vec!["Choose"], items, None);

    let area = Rect::new(0, 0, 80, 40);
    let (_transcript_area, modal_area) = split_inline_modal_area(&session, area);
    let modal_area = modal_area.expect("list modal should claim a bottom panel area");
    assert_eq!(modal_area.height, 25, "modal height must budget the always-rendered list divider");
}

#[test]
fn plan_approval_modal_height_hugs_wrapped_header_without_gap() {
    use super::super::render::split_inline_modal_area;

    let mut session = Session::new(InlineTheme::default(), None, 30);
    // Plan-approval header: 6 raw lines where the Summary wraps to 2 visual
    // rows at 80 columns, so the wrapped instruction estimate is 7 rows
    // (not the raw count of 6). Expected height: 7 instructions + 1 divider
    // + 12 list (4 items × title/subtitle/gap) + 1 footer-hint summary
    // + 3 title chrome = 24. Any blank gap or clipped summary changes this height.
    let lines = vec![
        "A plan is ready to execute. Would you like to proceed?",
        "Summary: Fix vtcode analyze so it runs non-interactively with auto-allowed tools",
        "1. Add tools::LIST_FILES to AUTO_ALLOW_TOOLS in analyze",
        "2. Fix step parsing for numbered steps in analyze.rs",
        "3. Add bounded verification across startup and update paths",
        "… and 2 more plan steps",
    ];
    let subtitles = [
        "Continue with the current context and confirmation policy.",
        "Fresh thread. Context: 7% used.",
        "Use the Auto policy; safety gates stay active.",
        "Return to planning and revise the plan.",
    ];
    let items = [
        "Yes, implement this plan",
        "Yes, clear context and implement",
        "Yes, switch to Auto and implement",
        "No, stay in Plan mode",
    ]
    .into_iter()
    .enumerate()
    .map(|(index, title)| InlineListItem {
        title: title.to_string(),
        subtitle: Some(subtitles[index].to_string()),
        badge: None,
        indent: 0,
        selection: Some(InlineListSelection::SlashCommand(format!("plan{index}"))),
        search_value: Some(title.to_string()),
        ..Default::default()
    })
    .collect::<Vec<_>>();
    show_overlay_with_hint(
        &mut session,
        "Ready to code?",
        lines,
        items,
        None,
        Some("ctrl-g to edit in VS Code · .vtcode/plans/analyze.md"),
    );

    let area = Rect::new(0, 0, 80, 40);
    let (_transcript_area, modal_area) = split_inline_modal_area(&session, area);
    let modal_area = modal_area.expect("plan modal should claim a bottom panel area");
    assert_eq!(modal_area.height, 24, "plan modal must hug wrapped content with no blank gap");
}

#[test]
fn closing_top_transient_restores_previous_bottom_panel() {
    let mut session = AppSession::new(InlineTheme::default(), None, 30);
    session.set_task_panel_visible(true);

    let mut terminal = render_session_to_terminal(&mut session, 30);
    assert!(session.core.bottom_panel_area().is_some(), "task panel should occupy the bottom panel when visible");

    show_list_modal(&mut session, "Pick one", vec!["Choose an option"], vec![make_list_item("Option A", "a")]);

    terminal
        .draw(|frame| session.render(frame))
        .expect("failed to render stacked transients");
    assert!(
        session.core.bottom_panel_area().is_none(),
        "floating transient should hide the lower bottom panel while it is on top"
    );

    session.close_transient();
    terminal
        .draw(|frame| session.render(frame))
        .expect("failed to render restored bottom panel");
    assert!(
        session.core.bottom_panel_area().is_some(),
        "closing the top transient should restore the previous bottom panel"
    );
}

#[test]
fn list_modal_keeps_last_selection_when_items_append() {
    let mut session = Session::new(InlineTheme::default(), None, 30);

    let selected = InlineListSelection::SlashCommand("second".to_string());
    show_overlay(
        &mut session,
        "Pick",
        vec!["Choose"],
        vec![make_list_item("First", "first"), make_list_item("Second", "second")],
        Some(selected.clone()),
    );
    session.handle_command(InlineCommand::CloseOverlay);

    show_overlay(
        &mut session,
        "Pick",
        vec!["Choose"],
        vec![
            make_list_item("First", "first"),
            make_list_item("Second", "second"),
            make_list_item("Third", "third"),
        ],
        Some(selected),
    );

    let selection = session
        .modal_state()
        .and_then(|modal| modal.list.as_ref())
        .and_then(|list| list.current_selection());
    assert_eq!(selection, Some(InlineListSelection::SlashCommand("third".to_string())));
}

#[test]
fn render_always_reserves_input_status_row() {
    let mut session = Session::new(InlineTheme::default(), None, 30);
    let input_width = VIEW_WIDTH.saturating_sub(2);
    let base_input_height = Session::input_block_height_for_lines(session.desired_input_lines(input_width));

    let _terminal = render_session_to_terminal_app(&mut session, 30);

    assert!(
        session.input_height >= base_input_height + ui::INLINE_INPUT_STATUS_HEIGHT,
        "input should always reserve persistent status row"
    );
}

#[test]
fn modal_click_on_summary_row_keeps_selection() {
    let mut session = Session::new(InlineTheme::default(), None, 30);
    show_overlay_with_hint(
        &mut session,
        "Pick one",
        vec!["Choose an option"],
        vec![
            make_list_item("First", "first"),
            make_list_item("Second", "second"),
            make_list_item("Third", "third"),
        ],
        None,
        Some("footer hint"),
    );

    let info_rows = session
        .modal_state()
        .and_then(|modal| {
            modal
                .list
                .as_ref()
                .map(|list| list.summary_line_rows(modal.footer_hint.as_deref(), modal.status.is_some()))
        })
        .unwrap_or(0);
    assert_eq!(info_rows, 1, "footer hint should render one summary row above the list");

    let _terminal = render_session_to_terminal_app(&mut session, 30);
    let area = session
        .modal_list_area()
        .expect("modal list area should be published after render");
    assert_eq!(
        modal_selection(&session),
        Some(InlineListSelection::SlashCommand("first".to_string())),
        "initial selection should be the first item"
    );

    let (tx, _rx) = mpsc::unbounded_channel();
    // The summary row is not an item: clicking it must neither move selection
    // nor submit (previously it submitted the already-selected first item).
    left_click_session(&mut session, &tx, area.x + 2, area.y, KeyModifiers::NONE);

    assert!(session.has_active_overlay(), "clicking the summary row must not submit and close the modal");
    assert_eq!(
        modal_selection(&session),
        Some(InlineListSelection::SlashCommand("first".to_string())),
        "clicking the summary row must not move selection"
    );
}

#[test]
fn modal_click_on_item_blank_row_maps_to_owning_item() {
    let mut session = Session::new(InlineTheme::default(), None, 30);
    show_overlay_with_hint(
        &mut session,
        "Pick one",
        vec!["Choose an option"],
        vec![
            make_list_item("First", "first"),
            make_list_item("Second", "second"),
            make_list_item("Third", "third"),
        ],
        None,
        Some("footer hint"),
    );

    let _terminal = render_session_to_terminal_app(&mut session, 30);
    let area = session
        .modal_list_area()
        .expect("modal list area should be published after render");

    // Layout is [summary, item0 title, item0 pad, item1 title, ...]: the blank
    // padding row belongs to the first item, so clicking it re-clicks the
    // already-selected first item and submits it (previously it selected the
    // second item instead).
    let (tx, mut rx) = mpsc::unbounded_channel();
    left_click_session(&mut session, &tx, area.x + 2, area.y + 2, KeyModifiers::NONE);

    assert!(!session.has_active_overlay(), "clicking the selected item must submit and close the modal");
    assert!(
        matches!(
            rx.try_recv(),
            Ok(InlineEvent::Overlay(OverlayEvent::Submitted(OverlaySubmission::Selection(
                InlineListSelection::SlashCommand(cmd)
            )))) if cmd == "first"
        ),
        "clicking the first item padding row should submit the first item"
    );
}

#[test]
fn wizard_click_below_inline_editor_selects_correct_item() {
    let mut session = Session::new(InlineTheme::default(), None, 30);
    session.handle_command(InlineCommand::ShowOverlay {
        request: Box::new(OverlayRequest::Wizard(WizardOverlayRequest {
            title: "Question".to_string(),
            steps: vec![WizardStep {
                title: "Choose".to_string(),
                question: "Pick one".to_string(),
                items: vec![
                    InlineListItem {
                        title: "Other".to_string(),
                        subtitle: None,
                        badge: None,
                        indent: 0,
                        selection: Some(InlineListSelection::RequestUserInputAnswer {
                            question_id: "q".to_string(),
                            selected: Vec::new(),
                            other: Some(String::new()),
                        }),
                        search_value: Some("other".to_string()),
                        ..Default::default()
                    },
                    make_list_item("Scope", "scope"),
                    make_list_item("Priority", "priority"),
                ],
                completed: false,
                answer: None,
                allow_freeform: true,
                freeform_label: Some("Other".to_string()),
                freeform_placeholder: Some("Type here...".to_string()),
                freeform_default: None,
            }],
            current_step: 0,
            search: None,
            mode: WizardModalMode::MultiStep,
        })),
    });

    let _terminal = render_session_to_terminal_app(&mut session, 30);
    let area = session
        .modal_list_area()
        .expect("modal list area should be published after render");

    // The selected custom-note item renders an extra editor row, so the second
    // item starts two rows lower than a plain row-count would suggest. Clicking
    // its title row must select it (previously the stale height mapped here to
    // the third item).
    let (tx, _rx) = mpsc::unbounded_channel();
    left_click_session(&mut session, &tx, area.x + 2, area.y + 4, KeyModifiers::NONE);

    assert!(session.has_active_overlay(), "selecting a new item must keep the wizard open");
    let wizard = session.wizard_overlay().expect("wizard should stay open");
    let step = wizard.steps.get(wizard.current_step).expect("current step should exist");
    assert_eq!(
        step.list.list_state.selected(),
        Some(1),
        "clicking the second item row should select the second item"
    );
    assert_eq!(
        step.list.current_selection(),
        Some(InlineListSelection::SlashCommand("scope".to_string())),
        "clicking the second item row should select Scope, not Priority"
    );
}

#[test]
fn queued_help_modal_does_not_clobber_active_list_modal() {
    let mut session = Session::new(InlineTheme::default(), None, 30);
    show_overlay(
        &mut session,
        "Pick one",
        vec!["Choose an option"],
        vec![make_list_item("First", "first"), make_list_item("Second", "second")],
        None,
    );

    session.show_help_modal();

    // The visible overlay must still be the list modal, not help.
    let modal = session.modal_state().expect("list modal should stay active");
    assert!(!modal.is_help_modal, "queued help must not flag the active list modal");
    assert!(modal.list.is_some(), "active overlay should still be the list modal");

    session.handle_command(InlineCommand::CloseOverlay);

    // The queued help modal activates with the help flag carried on the request.
    let modal = session.modal_state().expect("queued help modal should activate after close");
    assert!(modal.is_help_modal, "queued help request should render as help when activated");
    assert!(modal.list.is_none(), "help modal should not carry the previous list");
}

#[test]
fn app_modal_click_on_summary_row_keeps_selection() {
    let mut session = AppSession::new(InlineTheme::default(), None, 30);
    show_list_modal_with_hint(
        &mut session,
        "Pick one",
        vec!["Choose an option"],
        vec![make_list_item("First", "first"), make_list_item("Second", "second")],
        Some("footer hint"),
    );

    let _terminal = render_session_to_terminal(&mut session, 30);
    let area = session
        .core
        .modal_list_area()
        .expect("modal list area should be published after render");

    let (tx, _rx) = mpsc::unbounded_channel();
    left_click_app_session(&mut session, &tx, area.x + 2, area.y, KeyModifiers::NONE);

    assert!(session.has_active_overlay(), "clicking the summary row must not submit and close the modal");
    let selection = session
        .modal_state()
        .and_then(|modal| modal.list.as_ref())
        .and_then(|list| list.current_selection());
    assert_eq!(
        selection,
        Some(InlineListSelection::SlashCommand("first".to_string())),
        "clicking the summary row must not move selection"
    );
}
