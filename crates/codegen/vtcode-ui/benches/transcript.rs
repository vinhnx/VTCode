#![allow(
    missing_docs,
    reason = "Intentional compatibility, platform, or test-only suppression."
)]
#![expect(
    unused_results,
    reason = "Benchmark rendering and criterion group methods are invoked for their measurement side effects."
)]

use std::hint::black_box;
use std::sync::Arc;
use std::time::Duration;

use criterion::{BatchSize, BenchmarkId, Criterion, criterion_group, criterion_main};
use ratatui::{
    Terminal,
    backend::TestBackend,
    buffer::Buffer,
    crossterm::event::{
        Event as CrosstermEvent, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
    },
    layout::Rect,
    widgets::Widget,
};
use tokio::sync::mpsc;
use vtcode_ui::tui::{
    app::{
        FullscreenInteractionSettings, InlineCommand as AppInlineCommand, InlineEvent, InlineMessageKind,
        InlineSegment, InlineTextStyle, InlineTheme,
    },
    core_tui::{
        InlineCommand as CoreInlineCommand, app::AppSession, session::Session as CoreSession, widgets::TranscriptWidget,
    },
};

const VIEWPORT: Rect = Rect { x: 0, y: 0, width: 100, height: 24 };
const NARROW_VIEWPORT: Rect = Rect { x: 0, y: 0, width: 48, height: 24 };
const APP_WIDTH: u16 = 100;
const APP_HEIGHT: u16 = 30;
const MESSAGE_COUNT: usize = 500;
// Stay below the bounded live transcript capacity so an append measures
// incremental reflow rather than eviction work.
const LARGE_MESSAGE_COUNT: usize = 4_000;

fn segment(text: impl Into<String>) -> InlineSegment {
    InlineSegment {
        text: text.into(),
        style: Arc::new(InlineTextStyle::default()),
    }
}

fn build_core_session(message_count: usize, link_heavy: bool) -> CoreSession {
    let mut session = CoreSession::new(InlineTheme::default(), None, 40);
    for index in 0..message_count {
        let text = if link_heavy {
            format!("line {index}: inspect /tmp/project/src/file_{index}.rs:42 or https://example.com/item/{index}")
        } else {
            format!("plain transcript line {index} with steady-state viewport rendering")
        };
        session.handle_command(CoreInlineCommand::AppendLine {
            kind: InlineMessageKind::Agent,
            segments: vec![segment(text)],
        });
    }
    session
}

fn build_app_session(message_count: usize, match_heavy: bool) -> AppSession {
    let mut session = AppSession::new(InlineTheme::default(), None, 30);
    for index in 0..message_count {
        let text = if match_heavy {
            format!("match target line {index} with alpha needle for search navigation")
        } else {
            format!("review transcript line {index} for refresh benchmarking")
        };
        session.handle_command(AppInlineCommand::AppendLine {
            kind: InlineMessageKind::Agent,
            segments: vec![segment(text)],
        });
    }
    session
}

fn open_review(session: &mut AppSession, tx: &mpsc::UnboundedSender<InlineEvent>) {
    session.handle_event(CrosstermEvent::Key(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL)), tx, None);
}

fn set_review_search(session: &mut AppSession, tx: &mpsc::UnboundedSender<InlineEvent>, query: &str) {
    session.handle_event(CrosstermEvent::Key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE)), tx, None);
    for ch in query.chars() {
        session.handle_event(CrosstermEvent::Key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE)), tx, None);
    }
    session.handle_event(CrosstermEvent::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)), tx, None);
}

fn draw_app_session(session: &mut AppSession, terminal: &mut Terminal<TestBackend>) {
    terminal.draw(|frame| session.render(frame)).expect("app session should render");
}

fn transcript_widget_benchmark(c: &mut Criterion) {
    let cases = [("plain", false), ("link_heavy", true)];
    let mut group = c.benchmark_group("transcript_viewport_render");

    for (name, link_heavy) in cases {
        let mut session = build_core_session(MESSAGE_COUNT, link_heavy);
        let mut warmup = Buffer::empty(VIEWPORT);
        TranscriptWidget::new(&mut session).render(VIEWPORT, &mut warmup);

        group.bench_function(BenchmarkId::from_parameter(name), |b| {
            b.iter_batched(
                || Buffer::empty(VIEWPORT),
                |mut buf| {
                    TranscriptWidget::new(&mut session).render(VIEWPORT, &mut buf);
                    black_box(&buf);
                },
                BatchSize::SmallInput,
            );
        });
    }

    group.finish();
}

fn transcript_reflow_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("transcript_reflow");
    group.sample_size(20);
    group.measurement_time(Duration::from_secs(6));

    group.bench_function("large_append_tool_boundary", |b| {
        b.iter_batched(
            || {
                let mut session = build_core_session(LARGE_MESSAGE_COUNT, false);
                let mut warmup = Buffer::empty(VIEWPORT);
                TranscriptWidget::new(&mut session).render(VIEWPORT, &mut warmup);
                (session, Buffer::empty(VIEWPORT))
            },
            |(mut session, mut buffer)| {
                session.handle_command(CoreInlineCommand::AppendLine {
                    kind: InlineMessageKind::Tool,
                    segments: vec![segment("• Ran cargo nextest run -p vtcode-ui")],
                });
                TranscriptWidget::new(&mut session).render(VIEWPORT, &mut buffer);
                black_box((&session, &buffer));
            },
            BatchSize::LargeInput,
        );
    });

    group.bench_function("large_width_change", |b| {
        b.iter_batched(
            || {
                let mut session = build_core_session(LARGE_MESSAGE_COUNT, true);
                let mut warmup = Buffer::empty(VIEWPORT);
                TranscriptWidget::new(&mut session).render(VIEWPORT, &mut warmup);
                (session, Buffer::empty(NARROW_VIEWPORT))
            },
            |(mut session, mut buffer)| {
                TranscriptWidget::new(&mut session).render(NARROW_VIEWPORT, &mut buffer);
                black_box((&session, &buffer));
            },
            BatchSize::LargeInput,
        );
    });

    group.finish();
}

fn transcript_review_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("transcript_review");

    group.bench_function("refresh_append", |b| {
        b.iter_batched(
            || {
                let (tx, _rx) = mpsc::unbounded_channel();
                let mut session = build_app_session(MESSAGE_COUNT, false);
                open_review(&mut session, &tx);
                let mut terminal = Terminal::new(TestBackend::new(APP_WIDTH, APP_HEIGHT)).expect("test backend");
                draw_app_session(&mut session, &mut terminal);
                (session, terminal, tx)
            },
            |(mut session, mut terminal, tx)| {
                session.handle_command(AppInlineCommand::AppendLine {
                    kind: InlineMessageKind::Agent,
                    segments: vec![segment("fresh review append")],
                });
                draw_app_session(&mut session, &mut terminal);
                black_box((&session, &terminal, &tx));
            },
            BatchSize::SmallInput,
        );
    });

    group.bench_function("refresh_mid_edit", |b| {
        b.iter_batched(
            || {
                let (tx, _rx) = mpsc::unbounded_channel();
                let mut session = build_app_session(MESSAGE_COUNT, false);
                open_review(&mut session, &tx);
                let mut terminal = Terminal::new(TestBackend::new(APP_WIDTH, APP_HEIGHT)).expect("test backend");
                draw_app_session(&mut session, &mut terminal);
                (session, terminal, tx)
            },
            |(mut session, mut terminal, tx)| {
                let start = MESSAGE_COUNT / 2;
                let replacement_lines = (start..MESSAGE_COUNT)
                    .map(|index| {
                        vec![segment(format!(
                            "edited review line {index} with more wrapped content for refresh"
                        ))]
                    })
                    .collect();
                session.handle_command(AppInlineCommand::ReplaceLast {
                    count: MESSAGE_COUNT - start,
                    kind: InlineMessageKind::Agent,
                    lines: replacement_lines,
                    link_ranges: None,
                });
                draw_app_session(&mut session, &mut terminal);
                black_box((&session, &terminal, &tx));
            },
            BatchSize::SmallInput,
        );
    });

    group.bench_function("search_navigation", |b| {
        b.iter_batched(
            || {
                let (tx, _rx) = mpsc::unbounded_channel();
                let mut session = build_app_session(MESSAGE_COUNT, true);
                open_review(&mut session, &tx);
                set_review_search(&mut session, &tx, "alpha");
                let mut terminal = Terminal::new(TestBackend::new(APP_WIDTH, APP_HEIGHT)).expect("test backend");
                draw_app_session(&mut session, &mut terminal);
                (session, terminal, tx)
            },
            |(mut session, mut terminal, tx)| {
                session.handle_event(
                    CrosstermEvent::Key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE)),
                    &tx,
                    None,
                );
                draw_app_session(&mut session, &mut terminal);
                black_box((&session, &terminal));
            },
            BatchSize::SmallInput,
        );
    });

    group.finish();
}

#[derive(Clone, Copy)]
enum TranscriptInteraction {
    Idle,
    Streaming,
    Scroll,
    Selection,
}

struct InteractionFixture {
    session: AppSession,
    terminal: Terminal<TestBackend>,
    tx: mpsc::UnboundedSender<InlineEvent>,
    rx: mpsc::UnboundedReceiver<InlineEvent>,
    before: Buffer,
}

fn transcript_interaction_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("transcript_interaction");
    for action in [
        TranscriptInteraction::Idle,
        TranscriptInteraction::Streaming,
        TranscriptInteraction::Scroll,
        TranscriptInteraction::Selection,
    ] {
        let name = match action {
            TranscriptInteraction::Idle => "idle_frame",
            TranscriptInteraction::Streaming => "streaming_frame",
            TranscriptInteraction::Scroll => "scroll_frame",
            TranscriptInteraction::Selection => "selection_frame",
        };
        group.bench_function(name, |b| {
            b.iter_batched(
                || {
                    let (tx, rx) = mpsc::unbounded_channel();
                    let mut session = build_app_session(MESSAGE_COUNT, false);
                    // Bench hygiene: auto-copy spawns a real clipboard subprocess
                    // per finished selection (`pbcopy` ≈ 15–19 ms on macOS, timed
                    // locally), which would measure process spawn — and overwrite
                    // the developer's clipboard every iteration — instead of
                    // selection rendering. Disable via the product's own public
                    // fullscreen-interaction command (same surface as the
                    // `ui.fullscreen.copy_on_select` settings toggle); explicit
                    // Ctrl+C copying is unaffected.
                    session.handle_command(AppInlineCommand::SetFullscreenInteraction {
                        interaction: FullscreenInteractionSettings {
                            copy_on_select: false,
                            ..FullscreenInteractionSettings::default()
                        },
                    });
                    let mut terminal = Terminal::new(TestBackend::new(APP_WIDTH, APP_HEIGHT)).expect("test backend");
                    draw_app_session(&mut session, &mut terminal);
                    let before = terminal.backend().buffer().clone();
                    InteractionFixture { session, terminal, tx, rx, before }
                },
                |InteractionFixture { mut session, mut terminal, tx, mut rx, before }| {
                    match action {
                        TranscriptInteraction::Idle => {}
                        TranscriptInteraction::Streaming => session.handle_command(AppInlineCommand::AppendLine {
                            kind: InlineMessageKind::Agent,
                            segments: vec![segment("asymmetric fresh streaming output")],
                        }),
                        TranscriptInteraction::Scroll | TranscriptInteraction::Selection => {
                            let events = if matches!(action, TranscriptInteraction::Scroll) {
                                vec![(MouseEventKind::ScrollUp, 5, 5)]
                            } else {
                                vec![
                                    (MouseEventKind::Down(MouseButton::Left), 5, 5),
                                    (MouseEventKind::Drag(MouseButton::Left), 20, 7),
                                    (MouseEventKind::Up(MouseButton::Left), 20, 7),
                                ]
                            };
                            for (kind, column, row) in events {
                                session.handle_event(
                                    CrosstermEvent::Mouse(MouseEvent {
                                        kind,
                                        column,
                                        row,
                                        modifiers: KeyModifiers::NONE,
                                    }),
                                    &tx,
                                    None,
                                );
                            }
                        }
                    }
                    draw_app_session(&mut session, &mut terminal);
                    if !matches!(action, TranscriptInteraction::Idle) {
                        assert_ne!(terminal.backend().buffer(), &before, "{name} must change the rendered frame");
                    }
                    assert!(rx.try_recv().is_err(), "local interaction must not emit an external action");
                    black_box((&session, &terminal));
                },
                BatchSize::SmallInput,
            );
        });
    }
    group.finish();
}

criterion_group!(
    benches,
    transcript_widget_benchmark,
    transcript_reflow_benchmark,
    transcript_review_benchmark,
    transcript_interaction_benchmark
);
criterion_main!(benches);
