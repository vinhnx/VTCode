use std::fmt;
use std::sync::{Arc, Mutex, RwLock};
use std::time::SystemTime;

use humantime::format_rfc3339_seconds;
use once_cell::sync::Lazy;
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
};
use syntect::easy::HighlightLines;
use syntect::highlighting::Theme;
use syntect::parsing::SyntaxReference;
use tokio::sync::mpsc::UnboundedSender;
use tracing::{Event, Level, Subscriber, field::Visit};
use tracing_subscriber::layer::{Context, Layer};
use tracing_subscriber::registry::LookupSpan;

use crate::ui::syntax_highlight::{
    default_theme_name, find_syntax_by_extension, find_syntax_by_name, find_syntax_plain_text,
    load_theme, syntax_set,
};

#[derive(Debug, Clone)]
pub struct LogEntry {
    pub formatted: Arc<str>,
    pub timestamp: Arc<str>,
    pub level: Level,
    pub target: Arc<str>,
    pub message: Arc<str>,
}

#[derive(Default)]
struct LogForwarder {
    sender: Mutex<Option<UnboundedSender<LogEntry>>>,
}

impl LogForwarder {
    fn set_sender(&self, sender: UnboundedSender<LogEntry>) {
        match self.sender.lock() {
            Ok(mut guard) => {
                *guard = Some(sender);
            }
            Err(err) => {
                tracing::warn!("failed to set TUI log sender; lock poisoned: {err}");
            }
        }
    }

    fn clear_sender(&self) {
        match self.sender.lock() {
            Ok(mut guard) => {
                *guard = None;
            }
            Err(err) => {
                tracing::warn!("failed to clear TUI log sender; lock poisoned: {err}");
            }
        }
    }

    fn send(&self, entry: LogEntry) {
        match self.sender.lock() {
            Ok(guard) => {
                if let Some(sender) = guard.as_ref() {
                    let _ = sender.send(entry);
                }
            }
            Err(err) => {
                tracing::warn!("failed to forward TUI log entry; lock poisoned: {err}");
            }
        }
    }
}

static LOG_FORWARDER: Lazy<Arc<LogForwarder>> = Lazy::new(|| Arc::new(LogForwarder::default()));

static LOG_THEME_NAME: Lazy<RwLock<Option<String>>> = Lazy::new(|| RwLock::new(None));
static LOG_THEME_CACHE: Lazy<RwLock<Option<(String, Theme)>>> = Lazy::new(|| RwLock::new(None));

#[derive(Default)]
struct FieldVisitor {
    message: Option<String>,
    extras: Vec<(String, String)>,
}

impl Visit for FieldVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn fmt::Debug) {
        let rendered = format!("{:?}", value);
        if field.name() == "message" {
            self.message = Some(rendered);
        } else {
            self.extras.push((field.name().to_string(), rendered));
        }
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "message" {
            self.message = Some(value.to_string());
        } else {
            self.extras
                .push((field.name().to_string(), value.to_string()));
        }
    }
}

pub struct TuiLogLayer {
    forwarder: Arc<LogForwarder>,
}

impl TuiLogLayer {
    pub fn new() -> Self {
        Self {
            forwarder: LOG_FORWARDER.clone(),
        }
    }
}

impl Default for TuiLogLayer {
    fn default() -> Self {
        Self::new()
    }
}

impl TuiLogLayer {
    pub fn set_sender(sender: UnboundedSender<LogEntry>) {
        LOG_FORWARDER.set_sender(sender);
    }

    pub fn clear_sender() {
        LOG_FORWARDER.clear_sender();
    }
}

impl<S> Layer<S> for TuiLogLayer
where
    S: Subscriber + for<'span> LookupSpan<'span>,
{
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let level = *event.metadata().level();

        // Always filter out TRACE level
        if level == tracing::Level::TRACE {
            return;
        }

        // Only show DEBUG level logs when in debug mode
        if level == tracing::Level::DEBUG && !crate::ui::tui::panic_hook::is_debug_mode() {
            return;
        }

        // Only show ERROR level logs in TUI when explicitly allowed via
        // ui.show_diagnostics_in_transcript config
        if level == tracing::Level::ERROR && !crate::ui::tui::panic_hook::show_diagnostics() {
            return;
        }

        let mut visitor = FieldVisitor::default();
        event.record(&mut visitor);

        let message = visitor
            .message
            .unwrap_or_else(|| "(no message)".to_string());
        let extras = if visitor.extras.is_empty() {
            String::new()
        } else {
            let rendered = visitor
                .extras
                .into_iter()
                .map(|(key, value)| format!("{key}={value}"))
                .collect::<Vec<_>>()
                .join(" ");
            format!(" {rendered}")
        };

        // Filter out redundant system warnings that clutter the UI
        let combined_message = format!("{}{}", message, extras);
        if should_filter_log_message(&combined_message) {
            return;
        }

        let timestamp = format_rfc3339_seconds(SystemTime::now()).to_string();
        let line = format!(
            "{} {:<5} {} {}{}",
            timestamp,
            event.metadata().level(),
            event.metadata().target(),
            message,
            extras
        );

        let log_target = Arc::from(event.metadata().target().to_string().into_boxed_str());
        let message_with_extras = Arc::from(format!("{message}{extras}").into_boxed_str());
        self.forwarder.send(LogEntry {
            formatted: Arc::from(line.into_boxed_str()),
            timestamp: Arc::from(timestamp.into_boxed_str()),
            level: *event.metadata().level(),
            target: log_target,
            message: message_with_extras,
        });
    }
}

/// Check if a log message should be filtered out because it's redundant or unhelpful
fn should_filter_log_message(message: &str) -> bool {
    let lower_message = message.to_lowercase();

    // Filter out MallocStackLogging related messages
    let malloc_filters = [
        "mallocstacklogging",
        "malscollogging",
        "no such file or directory",
        "can't turn off malloc stack logging",
        "could not tag msl-related memory",
        "those pages will be included in process footprint",
        "process is not in a debuggable environment",
        "unsetting mallocstackloggingdirectory environment variable",
    ];

    malloc_filters
        .iter()
        .any(|&filter| lower_message.contains(&filter.to_lowercase()))
}

pub fn make_tui_log_layer() -> TuiLogLayer {
    TuiLogLayer::new()
}

pub fn register_tui_log_sender(sender: UnboundedSender<LogEntry>) {
    TuiLogLayer::set_sender(sender);
}

pub fn clear_tui_log_sender() {
    TuiLogLayer::clear_sender();
}

pub fn set_log_theme_name(theme: Option<String>) {
    let Ok(mut slot) = LOG_THEME_NAME.write() else {
        tracing::warn!("failed to set TUI log theme name; theme lock poisoned");
        return;
    };
    *slot = theme
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty());
    drop(slot);
    if let Ok(mut cache) = LOG_THEME_CACHE.write() {
        *cache = None;
    } else {
        tracing::warn!("failed to clear TUI log theme cache; cache lock poisoned");
    }
}

fn theme_for_current_config() -> Theme {
    let theme_name = {
        let Ok(slot) = LOG_THEME_NAME.read() else {
            tracing::warn!("failed to read TUI log theme name; falling back to default theme");
            return load_theme(&default_theme_name(), true);
        };
        slot.clone()
    };
    let resolved_name = theme_name.clone().unwrap_or_else(default_theme_name);
    {
        if let Ok(cache) = LOG_THEME_CACHE.read()
            && let Some((cached_name, cached)) = &*cache
            && *cached_name == resolved_name
        {
            return cached.clone();
        }
    }

    let theme = load_theme(&resolved_name, true);
    if let Ok(mut cache) = LOG_THEME_CACHE.write() {
        *cache = Some((resolved_name, theme.clone()));
    }
    theme
}

fn syntect_to_ratatui_style(style: syntect::highlighting::Style) -> Style {
    let fg = style.foreground;
    Style::default().fg(Color::Rgb(fg.r, fg.g, fg.b))
}

fn highlight_lines_to_text<'a>(
    lines: impl Iterator<Item = &'a str>,
    syntax: &SyntaxReference,
) -> Text<'static> {
    let theme = theme_for_current_config();
    let ss = syntax_set();
    let mut highlighter = HighlightLines::new(syntax, &theme);
    let mut result_lines = Vec::new();

    for line in lines {
        match highlighter.highlight_line(line, ss) {
            Ok(ranges) => {
                let spans: Vec<Span<'static>> = ranges
                    .into_iter()
                    .map(|(style, text)| {
                        Span::styled(text.to_owned(), syntect_to_ratatui_style(style))
                    })
                    .collect();
                result_lines.push(Line::from(spans));
            }
            Err(_) => {
                result_lines.push(Line::raw(line.to_owned()));
            }
        }
    }

    Text::from(result_lines)
}

fn log_level_style(level: &Level) -> Style {
    match *level {
        Level::ERROR => Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        Level::WARN => Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
        Level::INFO => Style::default().fg(Color::Green),
        Level::DEBUG => Style::default().fg(Color::Blue),
        Level::TRACE => Style::default().fg(Color::DarkGray),
    }
}

fn log_prefix(entry: &LogEntry) -> Vec<Span<'static>> {
    let timestamp_style = Style::default().fg(Color::DarkGray);
    vec![
        Span::styled(format!("[{}]", entry.timestamp), timestamp_style),
        Span::raw(" "),
        Span::styled(format!("{:<5}", entry.level), log_level_style(&entry.level)),
        Span::raw(" "),
        Span::styled(entry.target.to_string(), Style::default().fg(Color::Gray)),
        Span::raw(" "),
    ]
}

fn prepend_metadata(text: &mut Text<'static>, entry: &LogEntry) {
    let mut prefix = log_prefix(entry);
    if let Some(first) = text.lines.first_mut() {
        let mut merged = Vec::with_capacity(prefix.len() + first.spans.len());
        merged.append(&mut prefix);
        merged.append(&mut first.spans);
        first.spans = merged;
    } else {
        text.lines.push(Line::from(prefix));
    }
}

fn select_syntax(message: &str) -> &'static SyntaxReference {
    let trimmed = message.trim_start();
    if !trimmed.is_empty() {
        if (trimmed.starts_with('{') || trimmed.starts_with('['))
            && let Some(json) =
                find_syntax_by_name("JSON").or_else(|| find_syntax_by_extension("json"))
        {
            return json;
        }

        if (trimmed.contains('$') || trimmed.contains(';'))
            && let Some(shell) =
                find_syntax_by_name("Bash").or_else(|| find_syntax_by_extension("sh"))
        {
            return shell;
        }
    }

    find_syntax_by_name("Rust")
        .or_else(|| find_syntax_by_extension("rs"))
        .unwrap_or_else(find_syntax_plain_text)
}

pub fn highlight_log_entry(entry: &LogEntry) -> Text<'static> {
    let ss = syntax_set();
    if ss.syntaxes().is_empty() {
        let mut text = Text::raw(entry.formatted.as_ref().to_string());
        prepend_metadata(&mut text, entry);
        return text;
    }

    let syntax = select_syntax(entry.message.as_ref());
    let mut highlighted = highlight_lines_to_text(entry.message.lines(), syntax);
    prepend_metadata(&mut highlighted, entry);
    highlighted
}
