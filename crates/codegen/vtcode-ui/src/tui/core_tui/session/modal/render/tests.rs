use super::*;
use crate::tui::ui::tui::InlineListItem;
use ratatui::{Terminal, backend::TestBackend};

fn line_text(line: &Line<'_>) -> String {
    line.spans
        .iter()
        .map(|span| span.content.clone().into_owned())
        .collect::<String>()
}

fn modal_render_styles() -> ModalRenderStyles {
    ModalRenderStyles {
        border: Style::default(),
        highlight: Style::default(),
        badge: Style::default(),
        header: Style::default(),
        selectable: Style::default(),
        detail: Style::default(),
        search_match: Style::default(),
        title: Style::default(),
        divider: Style::default(),
        background: Style::default(),
        instruction_border: Style::default(),
        instruction_title: Style::default(),
        instruction_bullet: Style::default(),
        instruction_body: Style::default(),
        hint: Style::default(),
        success: Style::default(),
        warning: Style::default(),
        danger: Style::default(),
        accent: Style::default(),
    }
}

#[test]
fn modal_instruction_lines_marks_sections_and_highlights_commands() {
    let styles = modal_render_styles();
    let lines = modal_instruction_lines(
        Rect::new(0, 0, 80, 6),
        &[
            "Tool: shell — Run command".to_string(),
            "## Command".to_string(),
            "`cargo nextest run`".to_string(),
            "## Context".to_string(),
            "Reason: build check".to_string(),
        ],
        &styles,
    );

    let rendered = lines.iter().map(line_text).collect::<Vec<_>>().join("\n");
    assert!(rendered.contains("COMMAND"));
    assert!(rendered.contains("CONTEXT"));
    assert!(rendered.contains("cargo nextest run"));
    assert!(lines.iter().any(|line| line.spans.len() > 1));
}

#[test]
fn modal_instruction_lines_separate_sections_with_blank_row() {
    let styles = modal_render_styles();
    let lines = modal_instruction_lines(
        Rect::new(0, 0, 80, 10),
        &[
            "Tool: exec_command".to_string(),
            "## Command".to_string(),
            "`cargo test`".to_string(),
            "## Why".to_string(),
            "Reason: verify build".to_string(),
        ],
        &styles,
    );

    let texts = lines.iter().map(line_text).collect::<Vec<_>>();
    let why_idx = texts.iter().position(|text| text == "WHY").expect("WHY header");
    assert_eq!(texts[why_idx - 1], "", "section header needs a blank separator row");
}

#[test]
fn modal_instruction_command_uses_indent_without_pipe_or_bullet() {
    let styles = modal_render_styles();
    let lines = modal_instruction_lines(
        Rect::new(0, 0, 80, 6),
        &[
            "The agent wants to run a shell command and needs your approval.".to_string(),
            "`cargo test`".to_string(),
        ],
        &styles,
    );

    let command_line = lines
        .iter()
        .find(|line| line_text(line).contains("cargo"))
        .expect("command row");
    let text = line_text(command_line);
    assert!(!text.contains('│'), "command row must not use pipe gutter, got: {text}");
    assert!(!text.contains('•'), "command row must not use prose bullet, got: {text}");
}

#[test]
fn modal_instruction_command_keeps_syntax_token_colors() {
    let styles = modal_render_styles();
    let lines = modal_instruction_lines(
        Rect::new(0, 0, 80, 6),
        &[
            "The agent wants to run a shell command and needs your approval.".to_string(),
            "`cargo test --locked`".to_string(),
        ],
        &styles,
    );

    let command_line = lines
        .iter()
        .find(|line| line_text(line).contains("cargo"))
        .expect("command row");
    // Gutter + at least command/args/option segments.
    assert!(command_line.spans.len() > 2, "command should be tokenized, got: {command_line:?}");
    let token_styles: std::collections::HashSet<String> = command_line
        .spans
        .iter()
        .skip(1)
        .map(|span| format!("{:?}", span.style))
        .collect();
    assert!(token_styles.len() > 1, "command tokens should keep distinct syntax colors, got: {token_styles:?}");
}

#[test]
fn modal_instruction_command_renders_shell_marker_as_own_span() {
    let styles = modal_render_styles();
    let lines = modal_instruction_lines(Rect::new(0, 0, 80, 6), &["`$ cargo test --locked`".to_string()], &styles);

    let command_line = lines
        .iter()
        .find(|line| line_text(line).contains("cargo"))
        .expect("command row");
    // Gutter + marker + at least command/option token segments.
    assert!(command_line.spans.len() > 3, "marker must not swallow body tokens, got: {command_line:?}");
    assert_eq!(command_line.spans[1].content.as_ref(), "$ ");
    assert_eq!(line_text(command_line).trim_start(), "$ cargo test --locked");
}

#[test]
fn modal_instruction_shell_marker_applies_only_to_first_code_row() {
    let styles = modal_render_styles();
    let lines = modal_instruction_lines(
        Rect::new(0, 0, 80, 8),
        &["`$ echo hi`".to_string(), "`$ echo bye`".to_string()],
        &styles,
    );

    let texts = lines.iter().map(line_text).collect::<Vec<_>>();
    assert_eq!(texts.len(), 2);
    let first = lines.iter().find(|line| line_text(line).contains("hi")).expect("first row");
    assert_eq!(first.spans[1].content.as_ref(), "$ ");
    // A continuation line that literally starts with `$ ` keeps its
    // tokenizer output instead of gaining a marker span.
    let second = lines.iter().find(|line| line_text(line).contains("bye")).expect("second row");
    assert_ne!(second.spans[1].content.as_ref(), "$ ", "got: {second:?}");
}

fn numbered_option(title: &str) -> InlineListItem {
    InlineListItem {
        title: title.to_string(),
        subtitle: None,
        badge: None,
        indent: 0,
        selection: Some(InlineListSelection::SlashCommand(title.to_string())),
        search_value: None,
        ..Default::default()
    }
}

fn separator_option() -> InlineListItem {
    InlineListItem {
        title: String::new(),
        subtitle: None,
        badge: None,
        indent: 0,
        selection: None,
        search_value: None,
        ..Default::default()
    }
}

#[test]
fn modal_list_item_numbers_skip_non_selectable_rows() {
    let styles = modal_render_styles();
    let list = ModalListState::new(
        vec![
            numbered_option("Approve once"),
            numbered_option("Allow for session"),
            separator_option(),
            numbered_option("Deny once"),
        ],
        None,
    );

    let first_lines: Vec<String> = (0..4)
        .map(|visible_index| {
            let item_index = list.visible_indices[visible_index];
            let number = list.shortcut_number(visible_index);
            let rows = modal_list_item_lines(&list, visible_index, item_index, &styles, 60, None, false, number);
            line_text(&rows[0])
        })
        .collect();
    assert!(
        first_lines[0].contains("1.") && first_lines[0].contains("Approve once"),
        "got: {:?}",
        first_lines[0]
    );
    assert_eq!(
        line_text(
            &modal_list_item_lines(
                &list,
                0,
                list.visible_indices[0],
                &styles,
                60,
                None,
                false,
                list.shortcut_number(0)
            )[0]
        )
        .trim_start(),
        "1. Approve once"
    );
    assert!(
        first_lines[1].contains("2.") && first_lines[1].contains("Allow for session"),
        "got: {:?}",
        first_lines[1]
    );
    assert!(!first_lines[2].contains("3."), "separator must carry no number, got: {:?}", first_lines[2]);
    assert!(first_lines[3].contains("3.") && first_lines[3].contains("Deny once"), "got: {:?}", first_lines[3]);
}

#[test]
fn modal_list_item_numbers_hidden_when_disabled() {
    let styles = modal_render_styles();
    let list = ModalListState::new(vec![numbered_option("Approve once"), numbered_option("Deny once")], None);

    for visible_index in 0..2 {
        let item_index = list.visible_indices[visible_index];
        let rows = modal_list_item_lines(&list, visible_index, item_index, &styles, 60, None, false, None);
        let text = line_text(&rows[0]);
        assert!(!text.contains("1.") && !text.contains("2."), "numbers must stay hidden, got: {text}");
    }
}

#[test]
fn modal_list_item_numbers_hidden_on_crowded_lists() {
    // Ten selectables fail the shared gate: no row may show a number,
    // or digits would advertise shortcuts that routing swallows.
    let styles = modal_render_styles();
    let items: Vec<InlineListItem> = (1..=10).map(|number| numbered_option(&format!("Option {number}"))).collect();
    let list = ModalListState::new(items, None);
    assert!(!list.numbered_shortcuts());

    for visible_index in 0..10 {
        let item_index = list.visible_indices[visible_index];
        let number = list.shortcut_number(visible_index);
        assert_eq!(number, None, "crowded row {visible_index} must have no number");
        let rows = modal_list_item_lines(&list, visible_index, item_index, &styles, 60, None, false, number);
        let text = line_text(&rows[0]);
        assert!(!text.contains("1."), "dead number leaked on crowded row, got: {text}");
    }
}

#[test]
fn narrow_modal_keeps_command_tail_and_omission_evidence() {
    let styles = modal_render_styles();
    let lines = modal_instruction_lines(
        Rect::new(0, 0, 32, 16),
        &[
            "Tool: exec_command".to_string(),
            "## Command".to_string(),
            "`python3 -c 'print(1)' … --output ../../critical.txt`".to_string(),
            "… +5 more lines (full command runs on approval)".to_string(),
            "`rm -- target/last-line`".to_string(),
        ],
        &styles,
    );

    let rendered = lines.iter().map(line_text).collect::<Vec<_>>().join("\n");
    assert!(rendered.contains("../../critical.txt"), "trailing destination must remain visible: {rendered}");
    assert!(rendered.contains("+5 more lines"), "omission count must remain visible: {rendered}");
    assert!(rendered.contains("target/last-line"), "multiline tail must remain visible: {rendered}");
}

#[test]
fn modal_instruction_context_row_splits_label_and_value() {
    let styles = modal_render_styles();
    let lines = modal_instruction_lines(
        Rect::new(0, 0, 80, 6),
        &[
            "The agent wants to run a shell command and needs your approval.".to_string(),
            "Risk: High".to_string(),
        ],
        &styles,
    );

    let risk_line = lines.iter().find(|line| line_text(line).contains("High")).expect("risk row");
    let text = line_text(risk_line);
    assert!(!text.contains('•'), "context row must not use bullet, got: {text}");
    assert!(risk_line.spans.len() > 1, "label and value should be separate spans");
}

#[test]
fn modal_instruction_environment_row_splits_label_and_value() {
    let styles = modal_render_styles();
    let lines = modal_instruction_lines(
        Rect::new(0, 0, 80, 6),
        &["Environment: default policy + extra grants".to_string()],
        &styles,
    );

    let env_line = lines
        .iter()
        .find(|line| line_text(line).contains("extra grants"))
        .expect("env row");
    let text = line_text(env_line);
    assert!(!text.contains('•'), "env row must not use bullet, got: {text}");
    assert!(env_line.spans.len() > 1, "label and value should be separate spans");
    assert!(env_line.spans.iter().any(|span| span.content.as_ref() == "Environment:"));
}

#[test]
fn modal_instruction_permission_labels_render_without_bullets() {
    let styles = modal_render_styles();
    let lines = modal_instruction_lines(
        Rect::new(0, 0, 80, 8),
        &[
            "The agent wants to run a shell command and needs your approval.".to_string(),
            "`cargo test`".to_string(),
            "What the agent is trying to do: verify build".to_string(),
            "Requested from: agent-1".to_string(),
        ],
        &styles,
    );

    let texts = lines.iter().map(line_text).collect::<Vec<_>>();
    let goal = texts.iter().find(|text| text.contains("verify build")).expect("goal row");
    assert!(!goal.contains('•'), "goal row must not use bullet, got: {goal}");
    let source = texts.iter().find(|text| text.contains("agent-1")).expect("source row");
    assert!(!source.contains('•'), "source row must not use bullet, got: {source}");
    assert!(!texts.iter().any(|text| text.contains('│')), "no pipe gutter expected, got: {texts:?}");
}

#[test]
fn plan_approval_header_renders_without_bullets() {
    let styles = modal_render_styles();
    let lines = modal_instruction_lines(
        Rect::new(0, 0, 80, 10),
        &[
            "A plan is ready to execute. Would you like to proceed?".to_string(),
            "Summary: Fix vtcode analyze so it runs non-interactively".to_string(),
            "1. Add tools::LIST_FILES to AUTO_ALLOW_TOOLS".to_string(),
            "2. Fix step parsing in analyze.rs".to_string(),
            "… and 2 more plan steps".to_string(),
        ],
        &styles,
    );

    let texts = lines.iter().map(line_text).collect::<Vec<_>>();
    assert!(texts.iter().all(|text| !text.contains('•')), "plan header must not use bullets, got: {texts:?}");
    assert!(texts.iter().any(|text| text.contains("Summary:")), "summary overview must remain");
    assert!(texts.iter().any(|text| text.contains("1. Add")), "numbered steps must remain");
    assert!(texts.iter().any(|text| text.contains("more plan steps")), "overflow evidence must remain");
}

#[test]
fn plan_approval_long_header_wraps_without_bullets_or_elision() {
    let styles = modal_render_styles();
    let summary = "Summary: Fix vtcode analyze so it runs non-interactively with auto-allowed tools, correct step parsing, and bounded verification";
    let step = "1. Make TurnDiffTracker bounded and deterministic: sort paths for stable output ordering";
    let lines = modal_instruction_lines(
        Rect::new(0, 0, 40, 10),
        &[
            "A plan is ready to execute. Would you like to proceed?".to_string(),
            summary.to_string(),
            step.to_string(),
            "… and 2 more plan steps".to_string(),
        ],
        &styles,
    );

    let texts = lines.iter().map(line_text).collect::<Vec<_>>();
    assert!(
        texts.iter().all(|text| !text.contains('•')),
        "wrapped plan header must not use bullets, got: {texts:?}"
    );
    let normalized = texts.join(" ").split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(normalized.contains("non-interactively"), "full summary text must survive wrapping: {normalized}");
    assert!(
        normalized.contains("stable output ordering"),
        "full step text must survive wrapping without elision: {normalized}"
    );
    assert!(
        texts
            .iter()
            .filter(|text| text.trim_end().ends_with('…'))
            .all(|text| text.contains("more plan steps")),
        "only the overflow row may end with an ellipsis, got: {texts:?}"
    );
}

fn render_modal_lines(search: ModalSearchState) -> Vec<String> {
    let styles = modal_render_styles();
    let mut list = ModalListState::new(
        vec![InlineListItem {
            title: "Alpha".to_string(),
            subtitle: Some("First item".to_string()),
            badge: Some("OpenAI".to_string()),
            indent: 0,
            selection: Some(InlineListSelection::Model(0)),
            search_value: Some("alpha".to_string()),
            ..Default::default()
        }],
        None,
    );
    let instructions = vec!["Choose a model".to_string()];
    let backend = TestBackend::new(80, 8);
    let mut terminal = Terminal::new(backend).expect("test terminal");

    terminal
        .draw(|frame| {
            render_modal_body(
                frame,
                Rect::new(0, 0, 80, 8),
                ModalBodyContext {
                    instructions: &instructions,
                    status: None,
                    footer_hint: None,
                    list: Some(&mut list),
                    styles: &styles,
                    secure_prompt: None,
                    search: Some(&search),
                    input: "",
                    cursor: 0,
                    input_styles: &InputStyles::default(),
                },
                None,
                None,
                Style::default(),
                Style::default(),
            );
        })
        .expect("modal render should succeed");

    let buffer = terminal.backend().buffer();
    (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .filter_map(|x| buffer.cell((x, y)).map(|cell| cell.symbol().to_string()))
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect()
}

#[test]
fn render_markdown_lines_for_modal_wraps_long_questions() {
    let lines = render_markdown_lines_for_modal(
        "What user-visible outcome should this change deliver, and what constraints or non-goals must remain unchanged?",
        40,
        Style::default(),
    );

    assert!(lines.len() > 1, "long question should wrap across lines");
    for line in &lines {
        let text = line_text(line);
        assert!(UnicodeWidthStr::width(text.as_str()) <= 40, "line exceeded modal width: {text}");
    }
}

#[test]
fn render_markdown_lines_for_modal_renders_markdown_headings() {
    let lines = render_markdown_lines_for_modal("### Goal\n- Reduce prompt size", 80, Style::default());

    let rendered = lines.iter().map(line_text).collect::<Vec<_>>().join("\n");
    assert!(rendered.contains("Goal"));
    assert!(!rendered.contains("### Goal"));
    assert!(rendered.contains("Reduce prompt size"));
}

#[test]
fn config_list_summary_uses_navigation_hint_instead_of_density() {
    let list = ModalListState::new(
        vec![InlineListItem {
            title: "Permission default".to_string(),
            subtitle: Some("permissions.default = ask".to_string()),
            badge: Some("Toggle".to_string()),
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction("permissions.default:cycle".to_string())),
            search_value: None,
            ..Default::default()
        }],
        None,
    );

    let styles = ModalRenderStyles {
        border: Style::default(),
        highlight: Style::default(),
        badge: Style::default(),
        header: Style::default(),
        selectable: Style::default(),
        detail: Style::default(),
        search_match: Style::default(),
        title: Style::default(),
        divider: Style::default(),
        background: Style::default(),
        instruction_border: Style::default(),
        instruction_title: Style::default(),
        instruction_bullet: Style::default(),
        instruction_body: Style::default(),
        hint: Style::default(),
        success: Style::default(),
        warning: Style::default(),
        danger: Style::default(),
        accent: Style::default(),
    };

    let summary = modal_list_summary_line(&list, &styles, None)
        .into_iter()
        .next()
        .expect("expected summary line for config list");
    let text = line_text(&summary);
    assert!(text.contains("↑↓ select"), "hint: {text}");
    assert!(!text.contains("Alt+D"));
    assert!(!text.contains("Density:"));
}

#[test]
fn config_list_summary_ignores_explicit_footer_hint() {
    // Regression: callers must not pass a footer to a list containing
    // `ConfigAction` items. Such lists are `FixedComfortable` and render
    // the shared navigation hint, so an explicit footer is silently
    // dropped. Pin that behavior so dead footer copy cannot be reintroduced.
    let list = ModalListState::new(
        vec![InlineListItem {
            title: "Permission default".to_string(),
            subtitle: Some("permissions.default = ask".to_string()),
            badge: Some("Toggle".to_string()),
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction("permissions.default:cycle".to_string())),
            search_value: None,
            ..Default::default()
        }],
        None,
    );

    let styles = ModalRenderStyles {
        border: Style::default(),
        highlight: Style::default(),
        badge: Style::default(),
        header: Style::default(),
        selectable: Style::default(),
        detail: Style::default(),
        search_match: Style::default(),
        title: Style::default(),
        divider: Style::default(),
        background: Style::default(),
        instruction_border: Style::default(),
        instruction_title: Style::default(),
        instruction_bullet: Style::default(),
        instruction_body: Style::default(),
        hint: Style::default(),
        success: Style::default(),
        warning: Style::default(),
        danger: Style::default(),
        accent: Style::default(),
    };

    let summary = modal_list_summary_line(&list, &styles, Some("Esc to go back"))
        .into_iter()
        .next()
        .expect("summary line");
    let text = line_text(&summary);
    assert!(text.contains("↑↓ select"), "config lists render the shared navigation hint: {text}");
    assert!(!text.contains("Esc to go back"), "explicit footer must be dropped for config lists: {text}");
}

#[test]
fn non_config_list_summary_omits_density_hint() {
    let list = ModalListState::new(
        vec![InlineListItem {
            title: "gpt-5".to_string(),
            subtitle: Some("General reasoning".to_string()),
            badge: None,
            indent: 0,
            selection: Some(InlineListSelection::Model(0)),
            search_value: Some("gpt-5".to_string()),
            ..Default::default()
        }],
        None,
    );

    let styles = ModalRenderStyles {
        border: Style::default(),
        highlight: Style::default(),
        badge: Style::default(),
        header: Style::default(),
        selectable: Style::default(),
        detail: Style::default(),
        search_match: Style::default(),
        title: Style::default(),
        divider: Style::default(),
        background: Style::default(),
        instruction_border: Style::default(),
        instruction_title: Style::default(),
        instruction_bullet: Style::default(),
        instruction_body: Style::default(),
        hint: Style::default(),
        success: Style::default(),
        warning: Style::default(),
        danger: Style::default(),
        accent: Style::default(),
    };

    let summary = modal_list_summary_line(&list, &styles, None);
    assert!(summary.is_empty(), "density summary should be hidden");
}

#[test]
fn modal_text_area_alignment_reserves_selection_gutter() {
    let area = Rect::new(10, 3, 20, 4);
    let aligned = modal_text_area_aligned_with_list(area);
    let gutter = selection_padding_width() as u16;

    assert_eq!(aligned.x, area.x + gutter);
    assert_eq!(aligned.width, area.width - gutter);
    assert_eq!(aligned.y, area.y);
    assert_eq!(aligned.height, area.height);
}

#[test]
fn modal_text_area_alignment_keeps_narrow_areas_unchanged() {
    let gutter = selection_padding_width() as u16;
    let area = Rect::new(2, 1, gutter, 2);
    let aligned = modal_text_area_aligned_with_list(area);
    assert_eq!(aligned, area);
}

#[test]
fn modal_search_field_renders_placeholder_inside_brackets() {
    let lines = render_modal_lines(ModalSearchState {
        label: "Search models".to_string(),
        placeholder: Some("provider, name, id".to_string()),
        query: String::new(),
        fuzzy: false,
    });

    let has_title = lines.iter().any(|line| line.contains("Search models"));
    assert!(has_title, "search title should render");
    let has_placeholder = lines.iter().any(|line| line.contains("provider, name, id"));
    assert!(has_placeholder, "search placeholder should render");
}

#[test]
fn modal_search_field_renders_query_above_list() {
    let lines = render_modal_lines(ModalSearchState {
        label: "Search models".to_string(),
        placeholder: Some("provider, name, id".to_string()),
        query: "openrouter".to_string(),
        fuzzy: false,
    });

    let search_index = lines
        .iter()
        .position(|line| line.contains("openrouter"))
        .expect("search query should render");
    let item_index = lines
        .iter()
        .position(|line| line.contains("Alpha"))
        .expect("list item should render");

    assert!(search_index < item_index);
}

#[test]
fn modal_search_field_without_label_omits_title_row() {
    // An empty label collapses the search field to a single row (prompt +
    // placeholder), removing the redundant "Search …" title that restates
    // the placeholder.
    let lines = render_modal_lines(ModalSearchState {
        label: String::new(),
        placeholder: Some("provider, name, id".to_string()),
        query: String::new(),
        fuzzy: false,
    });

    let placeholder_row = lines
        .iter()
        .position(|line| line.contains("provider, name, id"))
        .expect("placeholder should render on the single search row");
    // The prompt indicator marks the search row.
    assert!(lines[placeholder_row].contains('>'), "search row should render the prompt indicator");
}

#[test]
fn filtered_modal_summary_shows_matches_without_repeating_query() {
    let list = ModalListState::new(
        vec![InlineListItem {
            title: "gpt-5".to_string(),
            subtitle: Some("General reasoning".to_string()),
            badge: None,
            indent: 0,
            selection: Some(InlineListSelection::Model(0)),
            search_value: Some("gpt-5".to_string()),
            ..Default::default()
        }],
        None,
    );
    let styles = modal_render_styles();
    let mut list = list;
    list.apply_search("gpt", false);

    let summary = modal_list_summary_line(&list, &styles, None)
        .into_iter()
        .next()
        .expect("summary should exist");
    let text = line_text(&summary);

    assert!(text.contains("1 / 1"), "quiet match counter: {text}");
    assert!(!text.contains("gpt"));
    assert!(!text.contains("Filter:"));
}

#[test]
fn instruction_highlight_markup_strips_bold_markers() {
    let styles = modal_render_styles();
    let mut list = ModalListState::new(Vec::new(), None);
    let instructions = vec!["Header".to_string(), "**ABCD-EFGH**".to_string()];
    let backend = TestBackend::new(40, 8);
    let mut terminal = Terminal::new(backend).expect("test terminal");

    terminal
        .draw(|frame| {
            render_modal_body(
                frame,
                Rect::new(0, 0, 40, 8),
                ModalBodyContext {
                    instructions: &instructions,
                    status: None,
                    footer_hint: None,
                    list: Some(&mut list),
                    styles: &styles,
                    secure_prompt: None,
                    search: None,
                    input: "",
                    cursor: 0,
                    input_styles: &InputStyles::default(),
                },
                None,
                None,
                Style::default(),
                Style::default(),
            );
        })
        .expect("modal render should succeed");

    let buffer = terminal.backend().buffer();
    let rendered = (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .filter_map(|x| buffer.cell((x, y)).map(|cell| cell.symbol().to_string()))
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    assert!(rendered.contains("ABCD-EFGH"));
    assert!(!rendered.contains("**ABCD-EFGH**"));
}

#[test]
fn setting_row_renders_accent_value_and_dimmed_subtitle() {
    let styles = modal_render_styles();
    let list = ModalListState::new(
        vec![InlineListItem {
            title: "Fullscreen copy".to_string(),
            value: Some("On".to_string()),
            subtitle: Some("Copy selection to clipboard".to_string()),
            badge: Some("On".to_string()),
            selection: Some(InlineListSelection::ConfigAction("settings:set:x:toggle".to_string())),
            badge_tone: InlineTone::Success,
            kind: InlineItemKind::Setting,
            ..Default::default()
        }],
        None,
    );
    let lines = modal_list_item_lines(&list, 0, 0, &styles, 60, None, false, None);
    let text: String = lines
        .iter()
        .flat_map(|line| line.spans.iter().map(|span| span.content.to_string()))
        .collect();
    assert!(text.contains("Fullscreen copy"), "title rendered: {text}");
    assert!(text.contains("On"), "value rendered: {text}");
    assert!(text.contains("Copy selection to clipboard"), "subtitle rendered: {text}");
}

#[test]
fn status_strip_uses_tone_and_sits_above_hint() {
    let styles = modal_render_styles();
    let mut list = ModalListState::new(
        vec![InlineListItem {
            title: "Item".to_string(),
            selection: Some(InlineListSelection::ConfigAction("x".to_string())),
            ..Default::default()
        }],
        None,
    );
    let status = InlineStatus::success("Enabled Todo Planning");
    let area = Rect::new(0, 0, 60, 6);
    let mut terminal = Terminal::new(TestBackend::new(60, 6)).expect("terminal");
    terminal
        .draw(|frame| {
            render_modal_list(frame, area, &mut list, &styles, Some("Esc close"), None, false, Some(&status));
        })
        .expect("render");
    let buffer = terminal.backend().buffer();
    let rendered = (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .filter_map(|x| buffer.cell((x, y)).map(|cell| cell.symbol().to_string()))
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(rendered.contains("Enabled Todo Planning"), "status visible: {rendered}");
    assert!(rendered.contains("•"), "status bullet: {rendered}");
}

#[test]
fn badge_tone_maps_current_to_accent_bold() {
    let styles = modal_render_styles();
    let current = modal_badge_style("Current", InlineTone::Current, &styles);
    let danger = modal_badge_style("Destructive", InlineTone::Danger, &styles);
    assert_ne!(current, danger, "tones must be distinguishable");
    assert_eq!(current, styles.accent.add_modifier(Modifier::BOLD));
    assert_eq!(danger, styles.danger);
}

#[test]
fn group_headers_add_spacing_above_and_below() {
    let styles = modal_render_styles();
    let list = ModalListState::new(
        vec![
            InlineListItem::group_header("Anthropic"),
            InlineListItem {
                title: "Claude".to_string(),
                subtitle: Some("desc".to_string()),
                selection: Some(InlineListSelection::Model(0)),
                ..Default::default()
            },
            InlineListItem::group_header("OpenAI"),
            InlineListItem {
                title: "GPT".to_string(),
                subtitle: Some("desc".to_string()),
                selection: Some(InlineListSelection::Model(1)),
                ..Default::default()
            },
        ],
        None,
    );
    // First header: no leading blank, but trailing blank before items.
    let h0 = modal_list_item_lines(&list, 0, 0, &styles, 60, None, false, None);
    assert_eq!(h0.len(), 2, "first header is title + trailing gap: {h0:?}");
    // Later header: blank above and below.
    let h1 = modal_list_item_lines(&list, 2, 2, &styles, 60, None, false, None);
    assert_eq!(h1.len(), 3, "later header is gap + title + gap: {h1:?}");
    // Item rows are title + subtitle + blank separator gap.
    let item = modal_list_item_lines(&list, 1, 1, &styles, 60, None, false, None);
    assert_eq!(item.len(), 3, "title + subtitle + gap: {item:?}");
    assert!(item.last().is_some_and(|line| line.spans.is_empty()), "last row is the blank gap: {item:?}");
}

#[test]
fn all_subtitle_lists_keep_comfortable_spacing_between_items() {
    // Shared rhythm across modal subclasses: config rows and plain model
    // rows alike end with a blank separator gap.
    let styles = modal_render_styles();
    let list = ModalListState::new(
        vec![
            InlineListItem {
                title: "Temperature".to_string(),
                value: Some("0.7".to_string()),
                subtitle: Some("Sampling temperature (0.0 precise to 1.0 creative).".to_string()),
                selection: Some(InlineListSelection::ConfigAction("settings:set:agent.temperature:inc".to_string())),
                ..Default::default()
            },
            InlineListItem {
                title: "Claude".to_string(),
                subtitle: Some("desc".to_string()),
                selection: Some(InlineListSelection::Model(0)),
                ..Default::default()
            },
        ],
        None,
    );
    for (visible_index, item_index) in [0usize, 1usize].into_iter().enumerate() {
        let item = modal_list_item_lines(&list, visible_index, item_index, &styles, 60, None, false, None);
        assert_eq!(item.len(), 3, "title + subtitle + gap: {item:?}");
        assert!(item.last().is_some_and(|line| line.spans.is_empty()), "last row is the blank gap: {item:?}");
    }
}
