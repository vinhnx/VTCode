use anyhow::{Context, Result};
use iocraft::prelude::*;
use std::time::{Duration, Instant};
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};

const ESCAPE_DOUBLE_MS: u64 = 750;

#[derive(Clone, Default)]
pub struct IocraftTextStyle {
    pub color: Option<Color>,
    pub weight: Weight,
    pub italic: bool,
}

impl IocraftTextStyle {
    pub fn merge_color(mut self, fallback: Option<Color>) -> Self {
        if self.color.is_none() {
            self.color = fallback;
        }
        self
    }
}

#[derive(Clone, Default)]
pub struct IocraftSegment {
    pub text: String,
    pub style: IocraftTextStyle,
}

#[derive(Clone, Default)]
struct StyledLine {
    segments: Vec<IocraftSegment>,
}

impl StyledLine {
    fn push_segment(&mut self, segment: IocraftSegment) {
        if segment.text.is_empty() {
            return;
        }
        self.segments.push(segment);
    }
}

#[derive(Clone)]
pub struct IocraftTheme {
    pub background: Option<Color>,
    pub foreground: Option<Color>,
    pub primary: Option<Color>,
    pub secondary: Option<Color>,
}

impl Default for IocraftTheme {
    fn default() -> Self {
        Self {
            background: None,
            foreground: None,
            primary: None,
            secondary: None,
        }
    }
}

pub enum IocraftCommand {
    AppendLine {
        segments: Vec<IocraftSegment>,
    },
    Inline {
        segment: IocraftSegment,
    },
    SetPrompt {
        prefix: String,
        style: IocraftTextStyle,
    },
    SetPlaceholder {
        hint: Option<String>,
    },
    SetTheme {
        theme: IocraftTheme,
    },
    Shutdown,
}

#[derive(Debug, Clone)]
pub enum IocraftEvent {
    Submit(String),
    Cancel,
    Exit,
    Interrupt,
    ScrollLineUp,
    ScrollLineDown,
    ScrollPageUp,
    ScrollPageDown,
}

#[derive(Clone)]
pub struct IocraftHandle {
    sender: UnboundedSender<IocraftCommand>,
}

impl IocraftHandle {
    pub fn append_line(&self, segments: Vec<IocraftSegment>) {
        if segments.is_empty() {
            let _ = self.sender.send(IocraftCommand::AppendLine {
                segments: vec![IocraftSegment::default()],
            });
        } else {
            let _ = self.sender.send(IocraftCommand::AppendLine { segments });
        }
    }

    pub fn inline(&self, segment: IocraftSegment) {
        let _ = self.sender.send(IocraftCommand::Inline { segment });
    }

    pub fn set_prompt(&self, prefix: String, style: IocraftTextStyle) {
        let _ = self
            .sender
            .send(IocraftCommand::SetPrompt { prefix, style });
    }

    pub fn set_placeholder(&self, hint: Option<String>) {
        let _ = self.sender.send(IocraftCommand::SetPlaceholder { hint });
    }

    pub fn set_theme(&self, theme: IocraftTheme) {
        let _ = self.sender.send(IocraftCommand::SetTheme { theme });
    }

    pub fn shutdown(&self) {
        let _ = self.sender.send(IocraftCommand::Shutdown);
    }
}

pub struct IocraftSession {
    pub handle: IocraftHandle,
    pub events: UnboundedReceiver<IocraftEvent>,
}

pub fn spawn_session(theme: IocraftTheme, placeholder: Option<String>) -> Result<IocraftSession> {
    let (command_tx, command_rx) = mpsc::unbounded_channel();
    let (event_tx, event_rx) = mpsc::unbounded_channel();

    tokio::spawn(async move {
        if let Err(err) = run_iocraft(command_rx, event_tx, theme, placeholder).await {
            tracing::error!(error = ?err, "iocraft session terminated unexpectedly");
        }
    });

    Ok(IocraftSession {
        handle: IocraftHandle { sender: command_tx },
        events: event_rx,
    })
}

async fn run_iocraft(
    commands: UnboundedReceiver<IocraftCommand>,
    events: UnboundedSender<IocraftEvent>,
    theme: IocraftTheme,
    placeholder: Option<String>,
) -> Result<()> {
    element! {
        SessionRoot(
            commands: commands,
            events: events,
            theme: theme,
            placeholder: placeholder,
        )
    }
    .render_loop()
    .await
    .context("iocraft render loop failed")
}

#[derive(Default, Props)]
struct SessionRootProps {
    commands: Option<UnboundedReceiver<IocraftCommand>>,
    events: Option<UnboundedSender<IocraftEvent>>,
    theme: IocraftTheme,
    placeholder: Option<String>,
}

#[component]
fn SessionRoot(props: &mut SessionRootProps, mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
    let mut system = hooks.use_context_mut::<SystemContext>();
    let lines = hooks.use_state(Vec::<StyledLine>::default);
    let current_line = hooks.use_state(StyledLine::default);
    let current_active = hooks.use_state(|| false);
    let prompt_prefix = hooks.use_state(|| "❯ ".to_string());
    let prompt_style = hooks.use_state(IocraftTextStyle::default);
    let input_value = hooks.use_state(|| String::new());
    let placeholder_hint = hooks.use_state(|| props.placeholder.clone().unwrap_or_default());
    let show_placeholder = hooks.use_state(|| props.placeholder.is_some());
    let should_exit = hooks.use_state(|| false);
    let theme_state = hooks.use_state(|| props.theme.clone());

    hooks.use_future({
        let mut rx = props
            .commands
            .take()
            .expect("iocraft commands receiver missing");
        let mut lines_state = lines;
        let mut current_line_state = current_line;
        let mut current_active_state = current_active;
        let mut prompt_prefix_state = prompt_prefix;
        let mut prompt_style_state = prompt_style;
        let mut placeholder_state = placeholder_hint;
        let mut placeholder_visible_state = show_placeholder;
        let mut exit_state = should_exit;
        async move {
            while let Some(cmd) = rx.recv().await {
                match cmd {
                    IocraftCommand::AppendLine { segments } => {
                        let was_active = current_active_state.get();
                        flush_current_line(
                            &mut current_line_state,
                            &mut current_active_state,
                            &mut lines_state,
                            was_active,
                        );
                        if let Some(mut lines) = lines_state.try_write() {
                            lines.push(StyledLine { segments });
                        }
                    }
                    IocraftCommand::Inline { segment } => {
                        append_inline_segment(
                            &mut current_line_state,
                            &mut current_active_state,
                            &mut lines_state,
                            segment,
                        );
                    }
                    IocraftCommand::SetPrompt { prefix, style } => {
                        prompt_prefix_state.set(prefix);
                        prompt_style_state.set(style);
                    }
                    IocraftCommand::SetPlaceholder { hint } => {
                        placeholder_state.set(hint.clone().unwrap_or_default());
                        placeholder_visible_state.set(hint.is_some());
                    }
                    IocraftCommand::SetTheme { theme } => {
                        let mut theme_handle = theme_state;
                        theme_handle.set(theme);
                    }
                    IocraftCommand::Shutdown => {
                        exit_state.set(true);
                        break;
                    }
                }
            }
        }
    });

    if should_exit.get() {
        system.exit();
    }

    let events_tx = props.events.clone().expect("iocraft events sender missing");
    let mut last_escape = hooks.use_state(|| None::<Instant>);
    let mut placeholder_toggle = show_placeholder;

    hooks.use_terminal_events(move |event| {
        if let TerminalEvent::Key(KeyEvent {
            code,
            kind,
            modifiers,
            ..
        }) = event
        {
            if kind == KeyEventKind::Release {
                return;
            }

            match code {
                KeyCode::Enter => {
                    let text = input_value.to_string();
                    let mut input_handle = input_value;
                    input_handle.set(String::new());
                    last_escape.set(None);
                    placeholder_toggle.set(false);
                    let _ = events_tx.send(IocraftEvent::Submit(text));
                }
                KeyCode::Esc => {
                    let now = Instant::now();
                    if last_escape
                        .get()
                        .and_then(|prev| now.checked_duration_since(prev))
                        .map(|elapsed| elapsed <= Duration::from_millis(ESCAPE_DOUBLE_MS))
                        .unwrap_or(false)
                    {
                        let _ = events_tx.send(IocraftEvent::Exit);
                        let mut exit_flag = should_exit;
                        exit_flag.set(true);
                    } else {
                        last_escape.set(Some(now));
                        let _ = events_tx.send(IocraftEvent::Cancel);
                    }
                }
                KeyCode::Char('c') if modifiers.contains(KeyModifiers::CONTROL) => {
                    let _ = events_tx.send(IocraftEvent::Interrupt);
                    let mut exit_flag = should_exit;
                    exit_flag.set(true);
                }
                KeyCode::Up => {
                    let _ = events_tx.send(IocraftEvent::ScrollLineUp);
                }
                KeyCode::Down => {
                    let _ = events_tx.send(IocraftEvent::ScrollLineDown);
                }
                KeyCode::PageUp => {
                    let _ = events_tx.send(IocraftEvent::ScrollPageUp);
                }
                KeyCode::PageDown => {
                    let _ = events_tx.send(IocraftEvent::ScrollPageDown);
                }
                KeyCode::Char('k')
                    if modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                {
                    let _ = events_tx.send(IocraftEvent::ScrollLineUp);
                }
                KeyCode::Char('j')
                    if modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                {
                    let _ = events_tx.send(IocraftEvent::ScrollLineDown);
                }
                _ => {}
            }
        }
    });

    let mut transcript_lines = lines.read().clone();
    if let Some(current) = current_line.try_read() {
        if current_active.get() && (!current.segments.is_empty()) {
            transcript_lines.push(current.clone());
        }
    }

    let prompt_prefix_value = prompt_prefix.to_string();
    let prompt_style_value = prompt_style.read().clone();
    let input_value_string = input_value.to_string();
    let placeholder_text = placeholder_hint.to_string();
    let placeholder_visible = show_placeholder.get() && !placeholder_text.is_empty();

    let transcript_rows = transcript_lines.into_iter().map(|line| {
        element! {
            View(flex_direction: FlexDirection::Row) {
                #(line
                    .segments
                    .into_iter()
                    .map(|segment| element! {
                        Text(
                            content: segment.text,
                            color: segment.style.color,
                            weight: segment.style.weight,
                            italic: segment.style.italic,
                            wrap: TextWrap::NoWrap,
                        )
                    }))
            }
        }
    });

    let theme_value = theme_state.read().clone();

    let background = theme_value
        .background
        .unwrap_or(Color::Rgb { r: 0, g: 0, b: 0 });
    let foreground = theme_value.foreground.unwrap_or(Color::White);

    let placeholder_color = theme_value.secondary.or(Some(foreground));
    let placeholder_element = placeholder_visible.then(|| {
        element! {
            Text(
                content: placeholder_text.clone(),
                color: placeholder_color,
                italic: true,
            )
        }
    });
    let input_value_state = input_value;

    element! {
        View(
            flex_direction: FlexDirection::Column,
            padding: 1u16,
            gap: 1u16,
            background_color: background,
        ) {
            View(
                flex_direction: FlexDirection::Column,
                flex_grow: 1.0,
                gap: 0u16,
                overflow: Overflow::Hidden,
            ) {
                #(transcript_rows)
            }
            View(flex_direction: FlexDirection::Column, gap: 1u16) {
                View(
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    gap: 1u16,
                ) {
                    Text(
                        content: prompt_prefix_value.clone(),
                        color: prompt_style_value.color.or(theme_value.secondary),
                        weight: prompt_style_value.weight,
                        italic: prompt_style_value.italic,
                        wrap: TextWrap::NoWrap,
                    )
                    TextInput(
                        has_focus: true,
                        value: input_value_string.clone(),
                        on_change: move |value| {
                            let mut handle = input_value_state;
                            handle.set(value);
                        },
                        color: theme_value.foreground,
                    )
                }
                #(placeholder_element.into_iter())
            }
        }
    }
}

fn flush_current_line(
    current_line: &mut State<StyledLine>,
    current_active: &mut State<bool>,
    lines_state: &mut State<Vec<StyledLine>>,
    force: bool,
) {
    if !force && !current_active.get() {
        return;
    }

    if let Some(cur) = current_line.try_read() {
        if !cur.segments.is_empty() || force {
            if let Some(mut lines) = lines_state.try_write() {
                lines.push(cur.clone());
            }
        }
    }

    if let Some(mut cur) = current_line.try_write() {
        cur.segments.clear();
    }
    current_active.set(false);
}

fn append_inline_segment(
    current_line: &mut State<StyledLine>,
    current_active: &mut State<bool>,
    lines_state: &mut State<Vec<StyledLine>>,
    segment: IocraftSegment,
) {
    let text = segment.text;
    let style = segment.style;

    if text.is_empty() {
        return;
    }

    let mut parts = text.split('\n').peekable();
    let ends_with_newline = text.ends_with('\n');

    while let Some(part) = parts.next() {
        if !part.is_empty() {
            if let Some(mut cur) = current_line.try_write() {
                cur.push_segment(IocraftSegment {
                    text: part.to_string(),
                    style: style.clone(),
                });
            }
            current_active.set(true);
        }

        if parts.peek().is_some() {
            flush_current_line(current_line, current_active, lines_state, true);
        }
    }

    if ends_with_newline {
        flush_current_line(current_line, current_active, lines_state, true);
    }
}

pub fn convert_style(style: anstyle::Style) -> IocraftTextStyle {
    let color = style.get_fg_color().and_then(|color| convert_color(color));
    let effects = style.get_effects();
    let weight = if effects.contains(anstyle::Effects::BOLD) {
        Weight::Bold
    } else {
        Weight::Normal
    };
    let italic = effects.contains(anstyle::Effects::ITALIC);

    IocraftTextStyle {
        color,
        weight,
        italic,
    }
}

pub fn convert_color(color: anstyle::Color) -> Option<Color> {
    match color {
        anstyle::Color::Ansi(ansi) => Some(match ansi {
            anstyle::AnsiColor::Black => Color::Black,
            anstyle::AnsiColor::Red => Color::DarkRed,
            anstyle::AnsiColor::Green => Color::DarkGreen,
            anstyle::AnsiColor::Yellow => Color::DarkYellow,
            anstyle::AnsiColor::Blue => Color::DarkBlue,
            anstyle::AnsiColor::Magenta => Color::DarkMagenta,
            anstyle::AnsiColor::Cyan => Color::DarkCyan,
            anstyle::AnsiColor::White => Color::Grey,
            anstyle::AnsiColor::BrightBlack => Color::DarkGrey,
            anstyle::AnsiColor::BrightRed => Color::Red,
            anstyle::AnsiColor::BrightGreen => Color::Green,
            anstyle::AnsiColor::BrightYellow => Color::Yellow,
            anstyle::AnsiColor::BrightBlue => Color::Blue,
            anstyle::AnsiColor::BrightMagenta => Color::Magenta,
            anstyle::AnsiColor::BrightCyan => Color::Cyan,
            anstyle::AnsiColor::BrightWhite => Color::White,
        }),
        anstyle::Color::Ansi256(value) => Some(Color::AnsiValue(value.index())),
        anstyle::Color::Rgb(rgb) => Some(Color::Rgb {
            r: rgb.r(),
            g: rgb.g(),
            b: rgb.b(),
        }),
    }
}

pub fn theme_from_styles(styles: &crate::ui::theme::ThemeStyles) -> IocraftTheme {
    IocraftTheme {
        background: convert_color(styles.background),
        foreground: convert_style(styles.output).color,
        primary: convert_style(styles.primary).color,
        secondary: convert_style(styles.secondary).color,
    }
}
