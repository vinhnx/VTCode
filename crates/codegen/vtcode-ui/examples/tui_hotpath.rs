//! Hotpath profiling harness for TUI render/input hot paths.
//!
//! Build and run (prints a hotpath timing+alloc report on exit):
//! ```bash
//! cargo run -p vtcode-ui --example tui_hotpath --features profiling
//! ```
//!
//! Workload mirrors the heavy cases from the FPS work: large transcript,
//! streaming appends, scroll, and keystrokes.

use std::sync::Arc;
use std::time::Instant;

use ratatui::{
    Terminal,
    backend::TestBackend,
    crossterm::event::{Event as CrosstermEvent, KeyCode, KeyEvent, KeyModifiers},
};
use tokio::sync::mpsc;
use vtcode_ui::tui::app::{InlineCommand, InlineEvent, InlineMessageKind, InlineSegment, InlineTextStyle, InlineTheme};
use vtcode_ui::tui::core_tui::app::AppSession;

fn segment(text: impl Into<String>) -> InlineSegment {
    InlineSegment {
        text: text.into(),
        style: Arc::new(InlineTextStyle::default()),
    }
}

#[hotpath::measure]
fn fill_transcript(session: &mut AppSession, count: usize) {
    for index in 0..count {
        let text = format!("line {index}: the quick brown fox jumps over the lazy dog while streaming tool output");
        session.handle_command(InlineCommand::AppendLine {
            kind: InlineMessageKind::Agent,
            segments: vec![segment(text)],
        });
    }
}

#[hotpath::measure]
fn stream_chunks(session: &mut AppSession, chunks: usize) {
    session.handle_command(InlineCommand::AppendLine {
        kind: InlineMessageKind::Agent,
        segments: vec![segment("stream-start")],
    });
    for index in 0..chunks {
        session.handle_command(InlineCommand::AppendLine {
            kind: InlineMessageKind::Agent,
            segments: vec![segment(format!(" chunk-{index}"))],
        });
    }
}

#[hotpath::measure]
fn fill_link_transcript(session: &mut AppSession, count: usize) {
    for index in 0..count {
        session.handle_command(InlineCommand::AppendLine {
            kind: InlineMessageKind::Agent,
            segments: vec![segment(format!(
                "line {index}: inspect /tmp/project/src/file_{index}.rs:42 or https://example.com/item/{index}"
            ))],
        });
    }
}

#[hotpath::measure]
fn render_frames(session: &mut AppSession, terminal: &mut Terminal<TestBackend>, frames: usize) {
    for _ in 0..frames {
        let _ = terminal.draw(|frame| session.render(frame)).expect("render fixture frame");
    }
}

#[hotpath::measure]
fn input_burst(session: &mut AppSession, events: usize) {
    let (tx, mut rx) = mpsc::unbounded_channel::<InlineEvent>();
    for index in 0..events {
        let key = if index % 7 == 0 {
            KeyEvent::new(KeyCode::Up, KeyModifiers::NONE)
        } else if index % 7 == 1 {
            KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)
        } else {
            KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE)
        };
        session.handle_event(CrosstermEvent::Key(key), &tx, None);
    }
    while rx.try_recv().is_ok() {}
}

/// Channel throughput demo (profiling guide: queue depth + send→receive latency).
///
/// Uses a locally-owned `u64` channel so the wrapper type never crosses a typed
/// API boundary (`AppSession::handle_event` takes a concrete tokio
/// `UnboundedSender`; wrapping that call site would change its signature under
/// `profiling` only). Bursts 100 messages then drains them, so Max queue reads
/// 100 by construction; the prod signal from the guide is Max queue *growth*
/// between runs under steady load (unbounded-queue OOM early warning).
#[hotpath::measure]
#[allow(
    unsafe_code,
    reason = "hotpath `label =` emits an `#[unsafe(export_name)]` marker so duplicate labels fail to link."
)]
fn channel_demo() {
    let (tx, mut rx) = hotpath::channel!(mpsc::unbounded_channel::<u64>(), label = "tui-hotpath-demo");
    for job in 0..100u64 {
        if tx.send(job).is_err() {
            break;
        }
    }
    let mut received = 0u64;
    while rx.try_recv().is_ok() {
        received += 1;
    }
    let _ = std::hint::black_box(received);
}

/// Lock contention demo (profiling guide: wait time vs hold/acquire time).
///
/// Short critical sections only — the healthy pattern. The article's antipattern
/// is a write guard held across slow work (network round trip, disk, `.await`);
/// it shows up as high acquire time on the holder and high wait time on everyone
/// else. Production locks stay unwrapped (the macro changes the lock type under
/// `profiling` only); see the audit note in `docs/development/performance.md`.
#[hotpath::measure]
#[allow(
    unsafe_code,
    reason = "hotpath `label =` emits an `#[unsafe(export_name)]` marker so duplicate labels fail to link."
)]
fn mutex_demo() {
    let counter = hotpath::mutex!(std::sync::Mutex::new(0u64), label = "tui-hotpath-demo-mutex");
    for _ in 0..10_000 {
        *counter.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) += 1;
    }
    let total = *counter.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let _ = std::hint::black_box(total);
}

/// I/O throughput demo (profiling guide: byte throughput per call site).
///
/// Writes then reads a spool-shaped burst (1_000 transcript lines) through
/// `io!`-wrapped in-memory cursors so the I/O report shape (bytes, rate)
/// stays covered. Hermetic by design: `io!` returns a wrapper type under
/// `profiling` (identity when off), so wrapping the production session
/// event-log / PTY-spool writers would change their types under `profiling`
/// only — those boundaries need a design first (see `performance.md`).
/// The cursor stands in for the spool file: the demo covers report
/// mechanics, not disk speed. `InstrumentedIo` has no `Seek` impl, so the
/// read phase wraps a fresh cursor over the written bytes (`io_unwrap` is
/// identity when off, so both modes stay type-clean).
#[hotpath::measure]
#[allow(
    unsafe_code,
    reason = "hotpath `label =` emits an `#[unsafe(export_name)]` marker so duplicate labels fail to link."
)]
fn io_demo() {
    use std::io::{Read, Write};
    const LINE: &[u8] = b"line: the quick brown fox jumps over the lazy dog while streaming tool output\n";

    let mut spool = hotpath::io!(std::io::Cursor::new(Vec::new()), label = "tui-hotpath-spool-write");
    let mut written = 0usize;
    for _ in 0..1_000 {
        if spool.write_all(LINE).is_err() {
            break;
        }
        written += LINE.len();
    }
    let bytes = hotpath::io_unwrap(spool).into_inner();

    let mut reader = hotpath::io!(std::io::Cursor::new(bytes), label = "tui-hotpath-spool-read");
    let mut read_back = Vec::new();
    if reader.read_to_end(&mut read_back).is_err() {
        read_back.clear();
    }
    let _ = std::hint::black_box((written, read_back.len()));
}

#[hotpath::measure]
fn main_workload() {
    let mut session = AppSession::new(InlineTheme::default(), None, 30);
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).expect("terminal");

    fill_transcript(&mut session, 2_000);
    render_frames(&mut session, &mut terminal, 120);
    stream_chunks(&mut session, 400);
    render_frames(&mut session, &mut terminal, 80);
    input_burst(&mut session, 300);
    render_frames(&mut session, &mut terminal, 60);

    // Eviction path: push past the transcript cap.
    fill_transcript(&mut session, 4_000);
    render_frames(&mut session, &mut terminal, 40);
    fill_link_transcript(&mut session, 1_000);
    render_frames(&mut session, &mut terminal, 40);
    channel_demo();
    mutex_demo();
    io_demo();
}

/// `functions_limit = 0` lifts the top-15 truncation so the full function list
/// (including the sub-millisecond demos) stays visible for audits.
#[hotpath::main(functions_limit = 0)]
fn main() {
    let started = Instant::now();
    main_workload();
    eprintln!("[tui_hotpath] workload finished in {:?}", started.elapsed());
}
