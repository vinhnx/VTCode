use std::fmt;
use std::io::{self, IsTerminal};

use anyhow::{Context, Result, anyhow};
use crossterm::cursor::Show;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, BorderType, Borders, List, ListDirection, ListItem, ListState, Paragraph, Wrap,
};

const CONTROLS_HINT: &str =
    "Use ↑/↓ or j/k to move • Home/End to jump • Enter to confirm • Esc to cancel";
const NUMBER_JUMP_HINT: &str = "Tip: type a number to jump directly to an option.";

#[derive(Debug, Clone)]
pub struct SelectionEntry {
    pub title: String,
    pub description: Option<String>,
}

impl SelectionEntry {
    pub fn new(title: impl Into<String>, description: Option<String>) -> Self {
        Self {
            title: title.into(),
            description,
        }
    }
}

#[derive(Debug)]
pub struct SelectionInterrupted;

impl fmt::Display for SelectionInterrupted {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("selection interrupted by Ctrl+C")
    }
}

impl std::error::Error for SelectionInterrupted {}

pub fn run_interactive_selection(
    title: &str,
    instructions: &str,
    entries: &[SelectionEntry],
    default_index: usize,
) -> Result<Option<usize>> {
    if entries.is_empty() {
        return Err(anyhow!("No options available for selection"));
    }

    if !io::stdout().is_terminal() {
        return Err(anyhow!("Terminal UI is unavailable"));
    }

    let mut stdout = io::stdout();
    let mut terminal_guard = TerminalModeGuard::new(title);
    terminal_guard.enable_raw_mode()?;
    terminal_guard.enter_alternate_screen(&mut stdout)?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)
        .with_context(|| format!("Failed to initialize Ratatui terminal for {title} selector"))?;
    terminal_guard.hide_cursor(&mut terminal)?;

    let selection_result = (|| -> Result<Option<usize>> {
        let total = entries.len();
        let mut selected_index = default_index.min(total.saturating_sub(1));
        let mut number_buffer = String::new();
        let mut list_state = ListState::default();
        list_state.select(Some(selected_index));

        loop {
            list_state.select(Some(selected_index));
            terminal
                .draw(|frame| {
                    let area = frame.area();
                    let instruction_lines = instructions.lines().count().max(1) as u16;
                    let instruction_height = instruction_lines.saturating_add(2);
                    let footer_height: u16 = 4;
                    let layout = Layout::default()
                        .direction(Direction::Vertical)
                        .margin(1)
                        .constraints([
                            Constraint::Length(
                                instruction_height
                                    .min(area.height.saturating_sub(footer_height + 3)),
                            ),
                            Constraint::Min(5),
                            Constraint::Length(footer_height),
                        ])
                        .split(area);

                    let instructions_widget = Paragraph::new(instructions)
                        .block(
                            Block::default()
                                .title("Instructions")
                                .borders(Borders::ALL)
                                .border_type(BorderType::Rounded),
                        )
                        .wrap(Wrap { trim: true });
                    frame.render_widget(instructions_widget, layout[0]);

                    let items: Vec<ListItem> = entries
                        .iter()
                        .enumerate()
                        .map(|(idx, entry)| {
                            let mut lines = vec![Line::from(vec![
                                Span::styled(
                                    format!("{:>2}. ", idx + 1),
                                    Style::default()
                                        .fg(Color::LightBlue)
                                        .add_modifier(Modifier::BOLD),
                                ),
                                Span::raw(entry.title.clone()),
                            ])];
                            if let Some(description) = entry.description.as_ref()
                                && !description.is_empty()
                                && description != &entry.title
                            {
                                lines.push(Line::from(Span::styled(
                                    description.clone(),
                                    Style::default().fg(Color::Gray),
                                )));
                            }
                            ListItem::new(lines)
                        })
                        .collect();

                    let list = List::new(items)
                        .block(
                            Block::default()
                                .title(title)
                                .borders(Borders::ALL)
                                .border_type(BorderType::Rounded),
                        )
                        .style(Style::default().fg(Color::White))
                        .highlight_style(
                            Style::default()
                                .fg(Color::Cyan)
                                .add_modifier(Modifier::BOLD | Modifier::REVERSED),
                        )
                        .highlight_symbol("> ")
                        .repeat_highlight_symbol(true)
                        .direction(ListDirection::TopToBottom)
                        .scroll_padding(1);

                    frame.render_stateful_widget(list, layout[1], &mut list_state);

                    let current = &entries[selected_index];
                    let mut summary_lines = vec![Line::from(Span::styled(
                        format!("Selected: {}", current.title),
                        Style::default().add_modifier(Modifier::BOLD),
                    ))];
                    if let Some(description) = current.description.as_ref()
                        && !description.is_empty()
                        && description != &current.title
                    {
                        summary_lines.push(Line::from(Span::styled(
                            description.clone(),
                            Style::default().fg(Color::Gray),
                        )));
                    }
                    summary_lines.push(Line::from(Span::raw(CONTROLS_HINT)));
                    summary_lines.push(Line::from(Span::styled(
                        NUMBER_JUMP_HINT,
                        Style::default().fg(Color::Gray),
                    )));

                    let footer = Paragraph::new(summary_lines)
                        .block(
                            Block::default()
                                .title("Selection")
                                .borders(Borders::ALL)
                                .border_type(BorderType::Rounded),
                        )
                        .wrap(Wrap { trim: true });
                    frame.render_widget(footer, layout[2]);
                })
                .with_context(|| format!("Failed to draw {title} selector UI"))?;

            match event::read()
                .with_context(|| format!("Failed to read terminal input for {title} selector"))?
            {
                Event::Key(key) if key.kind == KeyEventKind::Press => match key.code {
                    KeyCode::Up | KeyCode::Char('k') => {
                        if selected_index == 0 {
                            selected_index = total - 1;
                        } else {
                            selected_index -= 1;
                        }
                        number_buffer.clear();
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        selected_index = (selected_index + 1) % total;
                        number_buffer.clear();
                    }
                    KeyCode::Home => {
                        selected_index = 0;
                        number_buffer.clear();
                    }
                    KeyCode::End => {
                        selected_index = total - 1;
                        number_buffer.clear();
                    }
                    KeyCode::PageUp => {
                        let step = 5.min(total - 1);
                        if selected_index < step {
                            selected_index = 0;
                        } else {
                            selected_index -= step;
                        }
                        number_buffer.clear();
                    }
                    KeyCode::PageDown => {
                        let step = 5.min(total - 1);
                        selected_index = (selected_index + step).min(total - 1);
                        number_buffer.clear();
                    }
                    KeyCode::Enter => return Ok(Some(selected_index)),
                    KeyCode::Esc => return Ok(None),
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        return Err(SelectionInterrupted.into());
                    }
                    KeyCode::Char(c) if c.is_ascii_digit() => {
                        number_buffer.push(c);
                        if let Ok(index) = number_buffer.parse::<usize>()
                            && (1..=total).contains(&index)
                        {
                            selected_index = index - 1;
                        }
                        if number_buffer.len() >= total.to_string().len() {
                            number_buffer.clear();
                        }
                    }
                    KeyCode::Backspace => {
                        number_buffer.pop();
                    }
                    _ => {}
                },
                Event::Resize(_, _) => {
                    number_buffer.clear();
                }
                _ => {}
            }
        }
    })();

    let cleanup_result = terminal_guard.restore_with_terminal(&mut terminal);
    cleanup_result?;
    selection_result
}

struct TerminalModeGuard {
    label: String,
    raw_mode_enabled: bool,
    alternate_screen: bool,
    cursor_hidden: bool,
}

impl TerminalModeGuard {
    fn new(label: &str) -> Self {
        Self {
            label: label.to_string(),
            raw_mode_enabled: false,
            alternate_screen: false,
            cursor_hidden: false,
        }
    }

    fn enable_raw_mode(&mut self) -> Result<()> {
        enable_raw_mode()
            .with_context(|| format!("Failed to enable raw mode for {} selector", self.label))?;
        self.raw_mode_enabled = true;
        Ok(())
    }

    fn enter_alternate_screen(&mut self, stdout: &mut io::Stdout) -> Result<()> {
        execute!(stdout, EnterAlternateScreen).with_context(|| {
            format!(
                "Failed to enter alternate screen for {} selector",
                self.label
            )
        })?;
        self.alternate_screen = true;
        Ok(())
    }

    fn hide_cursor(&mut self, terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
        terminal
            .hide_cursor()
            .with_context(|| format!("Failed to hide cursor for {} selector", self.label))?;
        self.cursor_hidden = true;
        Ok(())
    }

    fn restore_with_terminal(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    ) -> Result<()> {
        if self.raw_mode_enabled {
            disable_raw_mode().with_context(|| {
                format!("Failed to disable raw mode after {} selector", self.label)
            })?;
            self.raw_mode_enabled = false;
        }

        if self.alternate_screen {
            execute!(terminal.backend_mut(), LeaveAlternateScreen).with_context(|| {
                format!(
                    "Failed to leave alternate screen after {} selector",
                    self.label
                )
            })?;
            self.alternate_screen = false;
        }

        if self.cursor_hidden {
            terminal
                .show_cursor()
                .with_context(|| format!("Failed to show cursor after {} selector", self.label))?;
            self.cursor_hidden = false;
        }

        Ok(())
    }
}

impl Drop for TerminalModeGuard {
    fn drop(&mut self) {
        if self.raw_mode_enabled {
            let _ = disable_raw_mode();
            self.raw_mode_enabled = false;
        }

        if self.alternate_screen {
            let mut stdout = io::stdout();
            let _ = execute!(stdout, LeaveAlternateScreen);
            self.alternate_screen = false;
        }

        if self.cursor_hidden {
            let mut stdout = io::stdout();
            let _ = execute!(stdout, Show);
            self.cursor_hidden = false;
        }
    }
}
