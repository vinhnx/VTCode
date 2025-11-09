//! Example demonstrating unified styling with anstyle and ratatui
//!
//! This example shows how to define styles once using anstyle and use them
//! in both CLI output and TUI widgets.

use anstyle::{AnsiColor, Color, Effects, Style};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Alignment, Constraint, Direction, Layout};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Terminal;
use std::io;
use vtcode_core::utils::ratatui_styles::anstyle_to_ratatui;

/// Define application theme using anstyle
struct AppTheme {
    title: Style,
    success: Style,
    warning: Style,
    error: Style,
    info: Style,
    highlight: Style,
}

impl AppTheme {
    fn new() -> Self {
        Self {
            title: Style::new()
                .fg_color(Some(Color::Ansi(AnsiColor::Cyan)))
                .effects(Effects::BOLD),
            success: Style::new()
                .fg_color(Some(Color::Ansi(AnsiColor::Green)))
                .effects(Effects::BOLD),
            warning: Style::new()
                .fg_color(Some(Color::Ansi(AnsiColor::Yellow)))
                .effects(Effects::BOLD),
            error: Style::new()
                .fg_color(Some(Color::Ansi(AnsiColor::Red)))
                .effects(Effects::BOLD),
            info: Style::new()
                .fg_color(Some(Color::Ansi(AnsiColor::Blue))),
            highlight: Style::new()
                .fg_color(Some(Color::Ansi(AnsiColor::Magenta)))
                .effects(Effects::UNDERLINE),
        }
    }

    /// Render text for CLI using ANSI escape codes
    fn render_cli(&self, text: &str, style: &Style) -> String {
        format!(
            "{}{}{}",
            style.render(),
            text,
            style.render_reset()
        )
    }

    /// Render text for TUI using ratatui styles
    fn render_tui_span<'a>(&self, text: &'a str, style: &Style) -> Span<'a> {
        Span::styled(text, anstyle_to_ratatui(*style))
    }
}

fn main() -> io::Result<()> {
    // Create theme
    let theme = AppTheme::new();

    // Demonstrate CLI rendering
    println!("\n=== CLI Output Example ===\n");
    println!("{}", theme.render_cli("Welcome to VTCode Styling!", &theme.title));
    println!("{}", theme.render_cli("✓ Operation successful", &theme.success));
    println!("{}", theme.render_cli("⚠ Warning: Check configuration", &theme.warning));
    println!("{}", theme.render_cli("✗ Error encountered", &theme.error));
    println!("{}", theme.render_cli("ℹ Info: System ready", &theme.info));

    // Setup terminal for TUI
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    crossterm::execute!(stdout, EnterAlternateScreen)?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Render TUI demo
    terminal.draw(|f| {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(2)
            .constraints([
                Constraint::Length(5),
                Constraint::Length(8),
                Constraint::Min(0),
            ])
            .split(f.area());

        // Title section
        let title_block = Block::default()
            .borders(Borders::ALL)
            .style(anstyle_to_ratatui(theme.title));

        let title_text = Line::from(vec![
            theme.render_tui_span("VTCode ", &theme.title),
            theme.render_tui_span("Styling Example", &theme.highlight),
        ]);

        let title = Paragraph::new(title_text)
            .block(title_block)
            .alignment(Alignment::Center);

        f.render_widget(title, chunks[0]);

        // Status section
        let status_lines = vec![
            Line::from(vec![
                Span::raw("Status: "),
                theme.render_tui_span("Connected", &theme.success),
            ]),
            Line::from(vec![
                Span::raw("Mode: "),
                theme.render_tui_span("Interactive", &theme.info),
            ]),
            Line::from(vec![
                Span::raw("Warnings: "),
                theme.render_tui_span("2 pending", &theme.warning),
            ]),
            Line::from(vec![
                Span::raw("Last Error: "),
                theme.render_tui_span("None", &theme.success),
            ]),
        ];

        let status_block = Block::default()
            .title(" Status ")
            .borders(Borders::ALL);

        let status = Paragraph::new(status_lines)
            .block(status_block);

        f.render_widget(status, chunks[1]);

        // Help section
        let help_text = Line::from(vec![
            Span::raw("Press "),
            theme.render_tui_span("Q", &theme.highlight),
            Span::raw(" to quit • "),
            theme.render_tui_span("Colors", &theme.success),
            Span::raw(" are unified across CLI and TUI"),
        ]);

        let help = Paragraph::new(help_text)
            .alignment(Alignment::Center);

        f.render_widget(help, chunks[2]);
    })?;

    // Wait for input
    use crossterm::event::{read, Event, KeyCode};
    loop {
        if let Event::Key(key) = read()? {
            if key.code == KeyCode::Char('q') {
                break;
            }
        }
    }

    // Cleanup
    disable_raw_mode()?;
    crossterm::execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen
    )?;
    terminal.show_cursor()?;

    println!("\n=== Demonstration Complete ===\n");
    println!(
        "{}",
        theme.render_cli(
            "Styling integration working seamlessly!",
            &theme.success
        )
    );

    Ok(())
}
