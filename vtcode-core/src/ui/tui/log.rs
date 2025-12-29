use std::fmt;
use std::sync::{Arc, Mutex, RwLock};
use std::time::SystemTime;

use humantime::format_rfc3339_seconds;
use once_cell::sync::Lazy;
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
};
use tokio::sync::mpsc::UnboundedSender;
use tracing::warn;
use tracing::{Event, Level, Subscriber, field::Visit};
use tracing_subscriber::layer::{Context, Layer};
use tracing_subscriber::registry::LookupSpan;
use tui_syntax_highlight::{
    Highlighter,
    syntect::{
        highlighting::{Theme, ThemeSet},
        parsing::{SyntaxReference, SyntaxSet},
    },
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
        *self.sender.lock().unwrap() = Some(sender);
    }

    fn clear_sender(&self) {
        *self.sender.lock().unwrap() = None;
    }

    fn send(&self, entry: LogEntry) {
        if let Some(sender) = self.sender.lock().unwrap().as_ref() {
            let _ = sender.send(entry);
        }
    }
}

static LOG_FORWARDER: Lazy<Arc<LogForwarder>> = Lazy::new(|| Arc::new(LogForwarder::default()));

const DEFAULT_THEME_NAME: &str = "base16-ocean.dark";
static LOG_SYNTAX_SET: Lazy<SyntaxSet> = Lazy::new(|| {
    // Attempt to load default syntaxes, but provide empty fallback if it fails
    let syntax_set = SyntaxSet::load_defaults_newlines();
    if syntax_set.syntaxes().is_empty() {
        warn!("Failed to load syntax set for log highlighting; log syntax highlighting disabled");
    }
    syntax_set
});
static LOG_THEME_SET: Lazy<ThemeSet> = Lazy::new(|| match ThemeSet::load_defaults() {
    theme_set if !theme_set.themes.is_empty() => theme_set,
    _ => {
        warn!("Failed to load theme set for log highlighting; using empty theme set");
        ThemeSet::new()
    }
});
static LOG_THEME_NAME: Lazy<RwLock<Option<String>>> = Lazy::new(|| RwLock::new(None));
static LOG_HIGHLIGHTER_CACHE: Lazy<RwLock<Option<(String, Highlighter)>>> =
    Lazy::new(|| RwLock::new(None));

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
        if *event.metadata().level() < tracing::Level::INFO {
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

    malloc_filters.iter().any(|&filter| lower_message.contains(&filter.to_lowercase()))
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
    let mut slot = LOG_THEME_NAME.write().unwrap();
    *slot = theme
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty());
    // Clear cached highlighter so it reloads with the new theme
    let mut cache = LOG_HIGHLIGHTER_CACHE.write().unwrap();
    *cache = None;
}

fn resolve_theme(theme_name: Option<String>) -> Theme {
    let name = theme_name.unwrap_or_else(|| DEFAULT_THEME_NAME.to_string());

    // If themes are empty, return default theme early
    if LOG_THEME_SET.themes.is_empty() {
        warn!("No log highlighting themes available");
        return Theme::default();
    }

    if let Some(theme) = LOG_THEME_SET.themes.get(&name) {
        return theme.clone();
    }
    if let Some(theme) = LOG_THEME_SET.themes.get(DEFAULT_THEME_NAME) {
        return theme.clone();
    }
    LOG_THEME_SET
        .themes
        .values()
        .next()
        .cloned()
        .unwrap_or_else(|| {
            warn!("No themes available in LOG_THEME_SET despite non-empty check");
            Theme::default()
        })
}

fn highlighter_for_current_theme() -> Highlighter {
    let theme_name = {
        let slot = LOG_THEME_NAME.read().unwrap();
        slot.clone()
    };
    let resolved_name = theme_name
        .clone()
        .unwrap_or_else(|| DEFAULT_THEME_NAME.to_string());
    {
        let cache = LOG_HIGHLIGHTER_CACHE.read().unwrap();
        if let Some((cached_name, cached)) = &*cache
            && *cached_name == resolved_name
        {
            return cached.clone();
        }
    }

    let theme = resolve_theme(theme_name);
    let highlighter = Highlighter::new(theme).line_numbers(false);
    let mut cache = LOG_HIGHLIGHTER_CACHE.write().unwrap();
    *cache = Some((resolved_name, highlighter.clone()));
    highlighter
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
            && let Some(json) = LOG_SYNTAX_SET
                .find_syntax_by_name("JSON")
                .or_else(|| LOG_SYNTAX_SET.find_syntax_by_extension("json"))
        {
            return json;
        }

        if (trimmed.contains('$') || trimmed.contains(';'))
            && let Some(shell) = LOG_SYNTAX_SET
                .find_syntax_by_name("Bash")
                .or_else(|| LOG_SYNTAX_SET.find_syntax_by_extension("sh"))
        {
            return shell;
        }
    }

    LOG_SYNTAX_SET
        .find_syntax_by_name("Rust")
        .or_else(|| LOG_SYNTAX_SET.find_syntax_by_extension("rs"))
        .unwrap_or_else(|| LOG_SYNTAX_SET.find_syntax_plain_text())
}

pub fn highlight_log_entry(entry: &LogEntry) -> Text<'static> {
    // If syntax set is empty, skip highlighting entirely
    if LOG_SYNTAX_SET.syntaxes().is_empty() {
        let mut text = Text::raw(entry.formatted.as_ref().to_string());
        prepend_metadata(&mut text, entry);
        return text;
    }

    let syntax = select_syntax(entry.message.as_ref());
    let mut highlighted = highlighter_for_current_theme()
        .highlight_lines(entry.message.lines(), syntax, &LOG_SYNTAX_SET)
        .unwrap_or_else(|_| Text::raw(entry.formatted.as_ref().to_string()));
    prepend_metadata(&mut highlighted, entry);
    highlighted
}
