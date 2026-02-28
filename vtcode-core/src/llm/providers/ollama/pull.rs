use ratatui::crossterm::{
    cursor::MoveToColumn,
    execute,
    style::{PrintStyledContent, Stylize},
    terminal::{Clear, ClearType},
};
use std::collections::HashMap;
use std::io;
use std::io::Write;

/// Ollama model pull functionality with progress reporting.
/// Adapted from OpenAI Codex's codex-ollama/src/pull.rs
/// Events emitted while pulling a model from Ollama.
#[derive(Debug, Clone)]
pub enum OllamaPullEvent {
    /// A human-readable status message (e.g., "verifying", "writing").
    Status(String),
    /// Byte-level progress update for a specific layer digest.
    ChunkProgress {
        digest: String,
        total: Option<u64>,
        completed: Option<u64>,
    },
    /// The pull finished successfully.
    Success,
    /// Error event with a message.
    Error(String),
}

/// A progress reporter for pull operations. Implementations decide how to render progress
/// (CLI, TUI, logs, etc.).
pub trait OllamaPullProgressReporter {
    fn on_event(&mut self, event: &OllamaPullEvent) -> io::Result<()>;
}

/// A minimal CLI reporter that writes inline progress to stderr.
pub struct CliPullProgressReporter {
    printed_header: bool,
    last_line_len: usize,
    last_completed_sum: u64,
    last_instant: std::time::Instant,
    totals_by_digest: HashMap<String, (u64, u64)>,
}

impl Default for CliPullProgressReporter {
    fn default() -> Self {
        Self::new()
    }
}

impl CliPullProgressReporter {
    pub fn new() -> Self {
        Self {
            printed_header: false,
            last_line_len: 0,
            last_completed_sum: 0,
            last_instant: std::time::Instant::now(),
            totals_by_digest: HashMap::new(),
        }
    }
}

impl OllamaPullProgressReporter for CliPullProgressReporter {
    fn on_event(&mut self, event: &OllamaPullEvent) -> io::Result<()> {
        let mut out = std::io::stderr();
        match event {
            OllamaPullEvent::Status(status) => {
                // Avoid noisy manifest messages; otherwise show status inline.
                if status.eq_ignore_ascii_case("pulling manifest") {
                    return Ok(());
                }
                let pad = self.last_line_len.saturating_sub(status.len());
                let line = format!("\r{status}{}", " ".repeat(pad));
                self.last_line_len = status.len();
                out.write_all(line.as_bytes())?;
                out.flush()
            }
            OllamaPullEvent::ChunkProgress {
                digest,
                total,
                completed,
            } => {
                if let Some(t) = total {
                    self.totals_by_digest
                        .entry(digest.clone())
                        .or_insert((0, 0))
                        .0 = *t;
                }
                if let Some(c) = completed {
                    self.totals_by_digest
                        .entry(digest.clone())
                        .or_insert((0, 0))
                        .1 = *c;
                }
                let (sum_total, sum_completed) = self
                    .totals_by_digest
                    .values()
                    .fold((0u64, 0u64), |acc, (t, c)| (acc.0 + t, acc.1 + c));

                if sum_total > 0 {
                    if !self.printed_header {
                        let gb = (sum_total as f64) / (1024.0 * 1024.0 * 1024.0);
                        let header = format!("Downloading model: total {gb:.2} GB\n");
                        execute!(out, MoveToColumn(0), Clear(ClearType::CurrentLine))?;
                        execute!(out, PrintStyledContent(header.bold().cyan()))?;
                        self.printed_header = true;
                    }
                    let now = std::time::Instant::now();
                    let dt = now
                        .duration_since(self.last_instant)
                        .as_secs_f64()
                        .max(0.001);
                    let dbytes = sum_completed.saturating_sub(self.last_completed_sum) as f64;
                    let speed_mb_s = dbytes / (1024.0 * 1024.0) / dt;
                    self.last_completed_sum = sum_completed;
                    self.last_instant = now;
                    let done_gb = (sum_completed as f64) / (1024.0 * 1024.0 * 1024.0);
                    let total_gb = (sum_total as f64) / (1024.0 * 1024.0 * 1024.0);
                    let pct = (sum_completed as f64) * 100.0 / (sum_total as f64);
                    let text =
                        format!("{done_gb:.2}/{total_gb:.2} GB ({pct:.1}%) {speed_mb_s:.1} MB/s");
                    let pad = self.last_line_len.saturating_sub(text.len());
                    let line = format!("\r{text}{}", " ".repeat(pad));
                    self.last_line_len = text.len();
                    out.write_all(line.as_bytes())?;
                    out.flush()
                } else {
                    Ok(())
                }
            }
            OllamaPullEvent::Error(_) => {
                // This will be handled by the caller, so we don't do anything
                // here or the error will be printed twice.
                Ok(())
            }
            OllamaPullEvent::Success => {
                out.write_all(b"\n")?;
                out.flush()
            }
        }
    }
}

/// For now the TUI reporter delegates to the CLI reporter. This keeps UI and
/// CLI behavior aligned until a dedicated TUI integration is implemented.
#[derive(Default)]
pub struct TuiPullProgressReporter(CliPullProgressReporter);

impl OllamaPullProgressReporter for TuiPullProgressReporter {
    fn on_event(&mut self, event: &OllamaPullEvent) -> io::Result<()> {
        self.0.on_event(event)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_reporter_formats_status_messages() {
        let mut reporter = CliPullProgressReporter::new();
        let event = OllamaPullEvent::Status("verifying".to_string());
        let result = reporter.on_event(&event);
        assert!(result.is_ok());
    }

    #[test]
    fn cli_reporter_tracks_download_progress() {
        let mut reporter = CliPullProgressReporter::new();
        let event = OllamaPullEvent::ChunkProgress {
            digest: "sha256:abc".to_string(),
            total: Some(1_000_000_000), // 1 GB
            completed: Some(500_000_000),
        };
        let result = reporter.on_event(&event);
        assert!(result.is_ok());
    }
}
