use super::*;
pub use super::super::Session;
use super::super::TranscriptLine;
pub use crate::config::constants::ui;
pub use crate::core_tui::app::session::AppSession;
pub use crate::core_tui::app::types as app_types;
pub use crate::core_tui::session::terminal_capabilities;
pub use crate::core_tui::types::{
    InlineHeaderBadge, InlineHeaderStatusBadge, InlineHeaderStatusTone, InlineLinkTarget,
    InlineListItem, InlineListSearchConfig, InlineListSelection, InlineSegment, InlineTextStyle,
    InlineTheme, ListOverlayRequest, LocalAgentEntry, OverlayEvent, OverlayHotkey,
    OverlayHotkeyAction, OverlayHotkeyKey, OverlayRequest, OverlaySubmission, WizardModalMode,
    WizardOverlayRequest, WizardStep,
};
pub use crate::core_tui::types::{InlineCommand, InlineEvent};
pub use crate::ui::tui::session::message::RenderedTranscriptLink;
use crate::core_tui::session::transcript_links::{TranscriptFileLinkTarget, TranscriptLinkTarget};
pub use crate::ui::tui::style::ratatui_style_from_inline;
pub use crate::core_tui::widgets::TranscriptWidget;
pub use vtcode_commons::ui_protocol::InlineMessageKind;
pub use anstyle::Color as AnsiColorEnum;
pub use anstyle::RgbColor;
pub use ratatui::text::Text;
pub use ratatui::crossterm::event::{
    Event as CrosstermEvent, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent,
    MouseEventKind,
};
#[cfg(target_os = "macos")]
pub use ratatui::crossterm::event::{KeyEventKind, ModifierKeyCode};
pub use ratatui::{
    Terminal,
    backend::TestBackend,
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier},
    text::{Line, Span},
    widgets::Widget,
};
pub use std::fs;
pub use std::path::PathBuf;
pub use std::sync::{
    LazyLock, Mutex,
    atomic::{AtomicUsize, Ordering},
};
pub use std::sync::Arc;
pub use std::time::{Duration, Instant};
pub use tokio::sync::mpsc;
pub use tokio::sync::mpsc::UnboundedSender;

pub const VIEW_ROWS: u16 = 14;
pub const VIEW_WIDTH: u16 = 100;
pub const LINE_COUNT: usize = 10;
pub const LABEL_PREFIX: &str = "line";
pub const EXTRA_SEGMENT: &str = "\nextra-line";
pub static TRANSCRIPT_TEST_FILE_COUNTER: AtomicUsize = AtomicUsize::new(0);
pub static CLIPBOARD_TEST_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));
pub static TERMINAL_ENV_TEST_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

pub fn with_terminal_env<R>(tmux: Option<&str>, term: Option<&str>, f: impl FnOnce() -> R) -> R {
    let _guard = TERMINAL_ENV_TEST_LOCK
        .lock()
        .expect("terminal env test lock");
    let original_tmux = std::env::var("TMUX").ok();
    let original_term = std::env::var("TERM").ok();

    terminal_capabilities::set_test_env_override("TMUX", tmux);
    terminal_capabilities::set_test_env_override("TERM", term);

    let result = f();

    match original_tmux {
        Some(value) => terminal_capabilities::set_test_env_override("TMUX", Some(&value)),
        None => terminal_capabilities::clear_test_env_override("TMUX"),
    }
    match original_term {
        Some(value) => terminal_capabilities::set_test_env_override("TERM", Some(&value)),
        None => terminal_capabilities::clear_test_env_override("TERM"),
    }

    result
}

pub fn make_segment(text: &str) -> InlineSegment {
    InlineSegment {
        text: text.to_string(),
        style: Arc::new(InlineTextStyle::default()),
    }
}

pub fn themed_inline_colors() -> InlineTheme {
    InlineTheme {
        foreground: Some(AnsiColorEnum::Rgb(RgbColor(0xEE, 0xEE, 0xEE))),
        tool_accent: Some(AnsiColorEnum::Rgb(RgbColor(0xBF, 0x45, 0x45))),
        tool_body: Some(AnsiColorEnum::Rgb(RgbColor(0xAA, 0x88, 0x88))),
        primary: Some(AnsiColorEnum::Rgb(RgbColor(0x88, 0x88, 0x88))),
        secondary: Some(AnsiColorEnum::Rgb(RgbColor(0x77, 0x99, 0xAA))),
        ..Default::default()
    }
}

pub fn session_with_input(input: &str, cursor: usize) -> Session {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS * 2);
    session.set_input(input.to_string());
    session.set_cursor(cursor);
    session
}

pub fn app_session_with_input(input: &str, cursor: usize) -> AppSession {
    let mut session = AppSession::new(InlineTheme::default(), None, VIEW_ROWS);
    session.core.set_input(input.to_string());
    session.core.set_cursor(cursor);
    session
}

pub fn left_click_session(
    session: &mut Session,
    events: &UnboundedSender<InlineEvent>,
    column: u16,
    row: u16,
    modifiers: KeyModifiers,
) {
    session.handle_event(
        CrosstermEvent::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column,
            row,
            modifiers,
        }),
        events,
        None,
    );
    session.handle_event(
        CrosstermEvent::Mouse(MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column,
            row,
            modifiers,
        }),
        events,
        None,
    );
}

pub fn left_click_app_session(
    session: &mut AppSession,
    events: &UnboundedSender<app_types::InlineEvent>,
    column: u16,
    row: u16,
    modifiers: KeyModifiers,
) {
    session.handle_event(
        CrosstermEvent::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column,
            row,
            modifiers,
        }),
        events,
        None,
    );
    session.handle_event(
        CrosstermEvent::Mouse(MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column,
            row,
            modifiers,
        }),
        events,
        None,
    );
}

pub fn sample_local_agent_entry(kind: app_types::LocalAgentKind) -> LocalAgentEntry {
    sample_local_agent_entry_with_id("agent-1", "rust-engineer", kind)
}

pub fn sample_local_agent_entry_with_id(
    id: &str,
    display_label: &str,
    kind: app_types::LocalAgentKind,
) -> LocalAgentEntry {
    LocalAgentEntry {
        id: id.to_string(),
        display_label: display_label.to_string(),
        agent_name: display_label.to_string(),
        color: Some("cyan".to_string()),
        kind,
        status: "running".to_string(),
        summary: Some("Reviewing the workspace".to_string()),
        preview: "assistant: reviewing the workspace".to_string(),
        transcript_path: None,
    }
}

pub fn load_app_file_palette(session: &mut AppSession, files: Vec<String>, workspace: PathBuf) {
    session.handle_command(app_types::InlineCommand::ShowTransient {
        request: Box::new(app_types::TransientRequest::FilePalette(
            app_types::FilePaletteTransientRequest {
                files,
                workspace,
                visible: None,
            },
        )),
    });
}

pub fn session_with_slash_palette_commands() -> AppSession {
    AppSession::new_with_logs(
        InlineTheme::default(),
        None,
        VIEW_ROWS,
        true,
        None,
        vec![
            app_types::SlashCommandItem::new("new", "Start a new session"),
            app_types::SlashCommandItem::new("review", "Review current diff"),
            app_types::SlashCommandItem::new("doctor", "Run diagnostics"),
            app_types::SlashCommandItem::new("command", "Run a terminal command"),
            app_types::SlashCommandItem::new("files", "Browse files"),
        ],
        "Agent TUI".to_string(),
    )
}

pub fn enable_vim_normal_mode(session: &mut Session) {
    session.vim_state.set_enabled(true);
    assert!(session.handle_vim_key(&KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)));
}

pub fn enable_vim_normal_mode_app(session: &mut AppSession) {
    session.core.vim_state.set_enabled(true);
    assert!(
        session
            .core
            .handle_vim_key(&KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
    );
}

pub fn show_basic_list_overlay(session: &mut Session) {
    session.handle_command(InlineCommand::ShowOverlay {
        request: Box::new(OverlayRequest::List(ListOverlayRequest {
            title: "Pick one".to_string(),
            lines: vec!["Choose an option".to_string()],
            footer_hint: None,
            items: vec![
                InlineListItem {
                    title: "Option A".to_string(),
                    subtitle: None,
                    badge: None,
                    indent: 0,
                    selection: Some(InlineListSelection::SlashCommand("a".to_string())),
                    search_value: None,
                },
                InlineListItem {
                    title: "Option B".to_string(),
                    subtitle: None,
                    badge: None,
                    indent: 0,
                    selection: Some(InlineListSelection::SlashCommand("b".to_string())),
                    search_value: None,
                },
            ],
            selected: Some(InlineListSelection::SlashCommand("a".to_string())),
            search: None,
            hotkeys: Vec::new(),
        })),
    });
}

pub fn show_diff_overlay(session: &mut AppSession, mode: app_types::DiffPreviewMode) {
    session.show_diff_overlay(app_types::DiffOverlayRequest {
        file_path: "src/main.rs".to_string(),
        before: "fn old() {}\n".to_string(),
        after: "fn new() {}\n".to_string(),
        hunks: vec![app_types::DiffHunk {
            old_start: 0,
            new_start: 0,
            old_lines: 1,
            new_lines: 1,
            display: "@@ -1 +1 @@".to_string(),
        }],
        current_hunk: 0,
        mode,
    });
}

pub fn visible_transcript(session: &mut Session) -> Vec<String> {
    let backend = TestBackend::new(VIEW_WIDTH, VIEW_ROWS);
    let mut terminal = Terminal::new(backend).expect("failed to create test terminal");
    terminal
        .draw(|frame| session.render(frame))
        .expect("failed to render test session");

    current_visible_transcript(session)
}

pub fn current_visible_transcript(session: &mut Session) -> Vec<String> {
    let width = session.transcript_width;
    let viewport = session.viewport_height();
    let offset = session.transcript_view_top;
    let lines = session.reflow_transcript_lines(width);

    let start = offset.min(lines.len());
    let mut visible: Vec<TranscriptLine> = lines
        .into_iter()
        .skip(start)
        .take(viewport)
        .map(|line| TranscriptLine {
            line,
            explicit_links: Vec::new(),
        })
        .collect();
    let filler = viewport.saturating_sub(visible.len());
    if filler > 0 {
        visible.extend((0..filler).map(|_| TranscriptLine::default()));
    }
    if !session.queued_inputs.is_empty() {
        session.overlay_queue_lines(&mut visible, width);
    }
    visible
        .into_iter()
        .map(|line| {
            line.line
                .spans
                .into_iter()
                .map(|span| span.content.into_owned())
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect()
}

pub fn rendered_session_lines(session: &mut Session, rows: u16) -> Vec<String> {
    session.apply_view_rows(rows);
    let backend = TestBackend::new(VIEW_WIDTH, rows);
    let mut terminal = Terminal::new(backend).expect("failed to create test terminal");
    let completed = terminal
        .draw(|frame| session.render(frame))
        .expect("failed to render session");

    let buffer = completed.buffer;
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

pub fn rendered_transcript_widget_lines(session: &mut Session, width: u16, height: u16) -> Vec<String> {
    let area = Rect::new(0, 0, width, height);
    let mut buf = Buffer::empty(area);
    TranscriptWidget::new(session).render(area, &mut buf);

    (0..area.height)
        .map(|y| {
            (0..area.width)
                .map(|x| buf[(x, y)].symbol())
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect()
}

pub fn rendered_transcript_lines(session: &mut Session, rows: u16) -> (Rect, Vec<String>) {
    session.apply_view_rows(rows);
    let backend = TestBackend::new(VIEW_WIDTH, rows);
    let mut terminal = Terminal::new(backend).expect("failed to create test terminal");
    let _ = terminal
        .draw(|frame| session.render(frame))
        .expect("failed to render session");

    let area = session.transcript_area().expect("expected transcript area");
    (area, current_visible_transcript(session))
}

pub fn rendered_app_session_lines(session: &mut AppSession, rows: u16) -> Vec<String> {
    session.core.apply_view_rows(rows);
    let backend = TestBackend::new(VIEW_WIDTH, rows);
    let mut terminal = Terminal::new(backend).expect("failed to create test terminal");
    let completed = terminal
        .draw(|frame| session.render(frame))
        .expect("failed to render session");

    let buffer = completed.buffer;
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

pub fn is_horizontal_rule(line: &str) -> bool {
    let glyph = ui::INLINE_BLOCK_HORIZONTAL
        .chars()
        .next()
        .expect("horizontal rule glyph");
    !line.is_empty() && line.chars().all(|ch| ch == glyph)
}

pub fn line_text(line: &Line<'_>) -> String {
    line.spans
        .iter()
        .map(|span| span.content.clone().into_owned())
        .collect()
}

pub fn transcript_line(text: impl Into<String>) -> TranscriptLine {
    TranscriptLine {
        line: Line::from(text.into()),
        explicit_links: Vec::new(),
    }
}

pub fn text_content(text: &Text<'static>) -> String {
    text.lines
        .iter()
        .map(line_text)
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn vtcode_tui_workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

pub fn transcript_file_fixture_relative_path() -> &'static str {
    "src/core_tui/session.rs"
}

pub fn transcript_file_fixture_absolute_path() -> String {
    vtcode_tui_workspace_root()
        .join(transcript_file_fixture_relative_path())
        .display()
        .to_string()
}

pub fn quoted_transcript_temp_file_path() -> PathBuf {
    let unique = TRANSCRIPT_TEST_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "vtcode transcript quoted path {} {unique}.txt",
        std::process::id()
    ))
}

#[cfg(target_os = "macos")]
pub fn open_file_click_modifiers() -> KeyModifiers {
    KeyModifiers::SUPER
}

#[cfg(not(target_os = "macos"))]
pub fn open_file_click_modifiers() -> KeyModifiers {
    KeyModifiers::CONTROL
}

#[cfg(target_os = "macos")]
pub fn command_modifier_press_event() -> KeyEvent {
    KeyEvent::new_with_kind(
        KeyCode::Modifier(ModifierKeyCode::LeftSuper),
        KeyModifiers::SUPER,
        KeyEventKind::Press,
    )
}

#[cfg(target_os = "macos")]
pub fn command_modifier_release_event() -> KeyEvent {
    KeyEvent::new_with_kind(
        KeyCode::Modifier(ModifierKeyCode::LeftSuper),
        KeyModifiers::SUPER,
        KeyEventKind::Release,
    )
}

#[cfg(target_os = "macos")]
pub fn meta_modifier_press_event() -> KeyEvent {
    KeyEvent::new_with_kind(
        KeyCode::Modifier(ModifierKeyCode::LeftMeta),
        KeyModifiers::META,
        KeyEventKind::Press,
    )
}

#[cfg(target_os = "macos")]

#[cfg(target_os = "macos")]

#[cfg(target_os = "macos")]

#[cfg(target_os = "macos")]

#[cfg(target_os = "macos")]

#[cfg(unix)]

#[cfg(unix)]

#[cfg(unix)]

pub fn request_user_input_step(question_id: &str, label: &str) -> WizardStep {
    WizardStep {
        title: format!("Question {question_id}"),
        question: format!("Select {question_id}"),
        items: vec![InlineListItem {
            title: label.to_string(),
            subtitle: Some("Option".to_string()),
            badge: None,
            indent: 0,
            selection: Some(InlineListSelection::RequestUserInputAnswer {
                question_id: question_id.to_string(),
                selected: vec![label.to_string()],
                other: None,
            }),
            search_value: Some(label.to_string()),
        }],
        completed: false,
        answer: None,
        allow_freeform: true,
        freeform_label: None,
        freeform_placeholder: None,
        freeform_default: None,
    }
}

pub fn request_user_input_custom_step(question_id: &str, label: &str, default: &str) -> WizardStep {
    WizardStep {
        title: format!("Question {question_id}"),
        question: format!("Enter {question_id}"),
        items: vec![InlineListItem {
            title: label.to_string(),
            subtitle: Some("Input".to_string()),
            badge: None,
            indent: 0,
            selection: Some(InlineListSelection::RequestUserInputAnswer {
                question_id: question_id.to_string(),
                selected: vec![],
                other: Some(String::new()),
            }),
            search_value: Some(label.to_string()),
        }],
        completed: false,
        answer: None,
        allow_freeform: true,
        freeform_label: Some(label.to_string()),
        freeform_placeholder: Some(default.to_string()),
        freeform_default: Some(default.to_string()),
    }
}

pub fn show_plan_confirmation_overlay(session: &mut Session, plan: app_types::PlanContent) {
    let mut lines: Vec<String> = plan
        .raw_content
        .lines()
        .map(|line| line.to_string())
        .collect();
    if lines.is_empty() && !plan.summary.is_empty() {
        lines.push(plan.summary.clone());
    }
    lines.insert(
        0,
        "A plan is ready to execute. Would you like to proceed?".to_string(),
    );

    session.handle_command(InlineCommand::ShowOverlay {
        request: Box::new(OverlayRequest::List(ListOverlayRequest {
            title: "Ready to code?".to_string(),
            lines,
            footer_hint: plan
                .file_path
                .as_ref()
                .map(|path| format!("ctrl-g to edit in VS Code · {path}")),
            items: vec![
                InlineListItem {
                    title: "Yes, auto-accept edits".to_string(),
                    subtitle: Some("Execute with auto-approval.".to_string()),
                    badge: Some("Recommended".to_string()),
                    indent: 0,
                    selection: Some(InlineListSelection::PlanApprovalAutoAccept),
                    search_value: None,
                },
                InlineListItem {
                    title: "Yes, manually approve edits".to_string(),
                    subtitle: Some(
                        "Keep context and confirm each edit before applying.".to_string(),
                    ),
                    badge: None,
                    indent: 0,
                    selection: Some(InlineListSelection::PlanApprovalExecute),
                    search_value: None,
                },
                InlineListItem {
                    title: "Type feedback to revise the plan".to_string(),
                    subtitle: Some("Return to plan mode and refine the plan.".to_string()),
                    badge: None,
                    indent: 0,
                    selection: Some(InlineListSelection::PlanApprovalEditPlan),
                    search_value: None,
                },
            ],
            selected: Some(InlineListSelection::PlanApprovalAutoAccept),
            search: None,
            hotkeys: vec![OverlayHotkey {
                key: OverlayHotkeyKey::CtrlChar('g'),
                action: OverlayHotkeyAction::LaunchEditor,
            }],
        })),
    });
}

// Helper structs and functions for transcript_links tests

pub struct SessionWithFileLink {
    pub session: Session,
    pub path: String,
}

impl SessionWithFileLink {
    pub fn new() -> Self {
        let path = transcript_file_fixture_absolute_path();
        let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
        session.push_line(
            InlineMessageKind::Agent,
            vec![make_segment(&format!("Open {}", path))],
        );
        let _ = visible_transcript(&mut session);
        Self { session, path }
    }

    pub fn target_area(&self) -> Rect {
        self.session
            .transcript_file_link_targets
            .first()
            .expect("expected transcript file target")
            .area
    }
}

pub struct AppSessionWithFileLink {
    pub session: AppSession,
    pub path: String,
}

impl AppSessionWithFileLink {
    pub fn new() -> Self {
        let path = transcript_file_fixture_absolute_path();
        let mut session = AppSession::new(InlineTheme::default(), None, VIEW_ROWS);
        session.push_line(
            InlineMessageKind::Agent,
            vec![make_segment(&format!("Open {}", path))],
        );
        let _ = rendered_app_session_lines(&mut session, VIEW_ROWS);
        Self { session, path }
    }

    pub fn target_area(&self) -> Rect {
        self.session
            .core
            .transcript_file_link_targets
            .first()
            .expect("expected transcript file target")
            .area
    }
}

pub fn wizard_auth_transient(url: &str) -> app_types::TransientRequest {
    app_types::TransientRequest::Wizard(app_types::WizardOverlayRequest {
        title: "OpenAI manual callback".to_string(),
        steps: vec![WizardStep {
            title: "Callback".to_string(),
            question: format!("Open this URL in your browser:\n\n{url}"),
            items: vec![InlineListItem {
                title: "Submit".to_string(),
                subtitle: Some("Press Enter to continue.".to_string()),
                badge: None,
                indent: 0,
                selection: Some(InlineListSelection::ConfigAction("submit".to_string())),
                search_value: None,
            }],
            completed: false,
            answer: None,
            allow_freeform: false,
            freeform_label: None,
            freeform_placeholder: None,
            freeform_default: None,
        }],
        current_step: 0,
        search: None,
        mode: WizardModalMode::MultiStep,
    })
}

pub fn list_auth_overlay(url: &str) -> OverlayRequest {
    OverlayRequest::List(ListOverlayRequest {
        title: format!("Authorize with {url}"),
        lines: vec![format!("Open this URL in your browser:\n{url}")],
        footer_hint: None,
        items: vec![InlineListItem {
            title: "Continue".to_string(),
            subtitle: None,
            badge: None,
            indent: 0,
            selection: Some(InlineListSelection::SlashCommand("continue".to_string())),
            search_value: None,
        }],
        selected: Some(InlineListSelection::SlashCommand("continue".to_string())),
        search: None,
        hotkeys: Vec::new(),
    })
}

// Helpers for queue_inputs tests

pub fn set_busy_status(session: &mut Session) {
    session.handle_command(InlineCommand::SetInputStatus {
        left: Some("Running tool: unified_search".to_string()),
        right: None,
    });
}

pub fn set_app_session_busy_status(session: &mut AppSession) {
    session.handle_command(app_types::InlineCommand::SetInputStatus {
        left: Some("Running tool: unified_search".to_string()),
        right: None,
    });
}

pub fn set_queued_inputs(session: &mut Session, entries: Vec<String>) {
    session.handle_command(InlineCommand::SetQueuedInputs { entries });
}

pub fn set_app_session_queued_inputs(session: &mut AppSession, entries: Vec<String>) {
    session.handle_command(app_types::InlineCommand::SetQueuedInputs { entries });
}

pub fn assert_footer_contains(session: &mut Session, take_from_bottom: u16, needle: &str) {
    let view = visible_transcript(session);
    let footer: Vec<String> = view.iter().rev().take(take_from_bottom as usize).cloned().collect();
    assert!(
        footer.iter().any(|line| line.contains(needle)),
        "expected footer to contain: {needle}"
    );
}

pub fn assert_footer_not_contains(session: &mut Session, take_from_bottom: u16, needle: &str) {
    let view = visible_transcript(session);
    let footer: Vec<String> = view.iter().rev().take(take_from_bottom as usize).cloned().collect();
    assert!(
        !footer.iter().any(|line| line.contains(needle)),
        "expected footer NOT to contain: {needle}"
    );
}
