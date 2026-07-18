#![allow(missing_docs)]
//! TUI backend smoke tests.
//!
//! These tests exercise `TestBackend` sizing and session command plumbing.
//! They intentionally do not render the session widget, so the backend remains
//! blank after each draw.

use ratatui::{Terminal, backend::TestBackend};
use vtcode_core::ui::{
    InlineCommand, InlineHandle, InlineHeaderContext, InlineMessageKind, InlineSegment, InlineTextStyle,
};

fn blank_terminal(width: usize, height: usize) -> String {
    let mut output = (0..height)
        .map(|_| format!("\"{}\"", " ".repeat(width)))
        .collect::<Vec<_>>()
        .join("\n");
    output.push('\n');
    output
}

fn smoke_handle() -> (InlineHandle, tokio::sync::mpsc::UnboundedReceiver<InlineCommand>) {
    let (command_tx, command_rx) = tokio::sync::mpsc::unbounded_channel();
    (InlineHandle::new_for_tests(command_tx), command_rx)
}

fn drain_append_lines(
    command_rx: &mut tokio::sync::mpsc::UnboundedReceiver<InlineCommand>,
) -> Vec<(InlineMessageKind, Vec<InlineSegment>)> {
    let mut appended = Vec::new();
    while let Ok(command) = command_rx.try_recv() {
        if let InlineCommand::AppendLine { kind, segments } = command {
            appended.push((kind, segments));
        }
    }
    appended
}

/// Smoke test backend sizing while a simple user-agent exchange is queued.
#[tokio::test]
async fn test_tui_backend_smoke_user_agent_exchange() {
    let backend = TestBackend::new(80, 20);
    let mut terminal = Terminal::new(backend).unwrap();

    // Snapshot the initial clear terminal state.
    terminal
        .draw(|f| {
            let _area = f.area();
            // This smoke path verifies backend sizing only.
            assert!(_area.width == 80);
            assert!(_area.height == 20);
        })
        .unwrap();

    assert_eq!(format!("{}", terminal.backend()), blank_terminal(80, 20));

    // Create a session and queue content without rendering the session widget.
    let (handle, mut command_rx) = smoke_handle();

    // Add content that a full session widget render would display.
    handle.append_line(
        InlineMessageKind::User,
        vec![InlineSegment {
            text: "Explain how bubble sort works".to_string(),
            style: InlineTextStyle::default().into(),
        }],
    );

    handle.append_line(
        InlineMessageKind::Agent,
        vec![InlineSegment {
            text: "Bubble sort is a simple sorting algorithm that repeatedly steps through the list, compares adjacent elements and swaps them if they are in the wrong order. The pass through the list is repeated until the list is sorted.".to_string(),
            style: InlineTextStyle::default().into(),
        }],
    );
    assert_eq!(drain_append_lines(&mut command_rx).len(), 2);

    // Draw without the session widget; the backend should remain blank.
    terminal
        .draw(|f| {
            let _area = f.area();
        })
        .unwrap();

    assert_eq!(format!("{}", terminal.backend()), blank_terminal(80, 20));
}

/// Smoke test backend sizing while code-related session content is queued.
#[tokio::test]
async fn test_tui_backend_smoke_code_content() {
    let backend = TestBackend::new(100, 25);
    let mut terminal = Terminal::new(backend).unwrap();

    let (handle, mut command_rx) = smoke_handle();

    // Add code-related content that is not rendered in this backend smoke test.
    handle.append_line(
        InlineMessageKind::User,
        vec![InlineSegment {
            text: "Show me a Rust function to reverse a string".to_string(),
            style: InlineTextStyle::default().into(),
        }],
    );

    handle.append_line(
        InlineMessageKind::Agent,
        vec![InlineSegment {
            text: "Here's a Rust function to reverse a string:\n\n```rust\nfn reverse_string(s: &str) -> String {\n    s.chars().rev().collect()\n}\n\nfn main() {\n    let original = \"hello\";\n    let reversed = reverse_string(original);\n    println!(\"{} -> {}\", original, reversed);\n}\n```\n\nThis function works by converting the string to characters, reversing the iterator, and collecting back into a String.".to_string(),
            style: InlineTextStyle::default().into(),
        }],
    );
    assert_eq!(drain_append_lines(&mut command_rx).len(), 2);

    terminal
        .draw(|f| {
            let _area = f.area();
            assert!(_area.width == 100);
            assert!(_area.height == 25);
        })
        .unwrap();

    assert_eq!(format!("{}", terminal.backend()), blank_terminal(100, 25));
}

/// Smoke test backend sizing while tool output content is queued.
#[tokio::test]
async fn test_tui_backend_smoke_tool_output() {
    let backend = TestBackend::new(80, 20);
    let mut terminal = Terminal::new(backend).unwrap();

    let (handle, mut command_rx) = smoke_handle();

    // Simulate tool usage without rendering the session widget.
    handle.append_line(
        InlineMessageKind::User,
        vec![InlineSegment {
            text: "List files in current directory".to_string(),
            style: InlineTextStyle::default().into(),
        }],
    );

    handle.append_line(
        InlineMessageKind::Tool,
        vec![InlineSegment {
            text: "run_pty_cmd([\"ls\", \"-la\"])".to_string(),
            style: InlineTextStyle::default().into(),
        }],
    );

    handle.append_line(
        InlineMessageKind::Pty,
        vec![InlineSegment {
            text: "total 48\ndrwxr-xr-x  10 user  staff  320 Nov  1 10:30 .\ndrwxr-xr-x   5 user  staff  160 Nov  1 10:25 ..\n-rw-r--r--   1 user  staff  156 Nov  1 10:20 Cargo.toml\n-rw-r--r--   1 user  staff  368 Nov  1 10:25 README.md\ndrwxr-xr-x   3 user  staff   96 Nov  1 10:20 src/\n".to_string(),
            style: InlineTextStyle::default().into(),
        }],
    );

    handle.append_line(
        InlineMessageKind::Agent,
        vec![InlineSegment {
            text:
                "I've listed the files in the current directory. You have Cargo.toml, README.md, and a src/ directory."
                    .to_string(),
            style: InlineTextStyle::default().into(),
        }],
    );
    assert_eq!(drain_append_lines(&mut command_rx).len(), 4);

    terminal
        .draw(|f| {
            let _area = f.area();
            assert!(_area.width == 80);
            assert!(_area.height == 20);
        })
        .unwrap();

    assert_eq!(format!("{}", terminal.backend()), blank_terminal(80, 20));
}

/// Smoke test backend sizing while error content is queued.
#[tokio::test]
async fn test_tui_backend_smoke_error_content() {
    let backend = TestBackend::new(80, 20);
    let mut terminal = Terminal::new(backend).unwrap();

    let (handle, mut command_rx) = smoke_handle();

    // Simulate an error scenario without rendering the session widget.
    handle.append_line(
        InlineMessageKind::User,
        vec![InlineSegment {
            text: "Run command that might fail".to_string(),
            style: InlineTextStyle::default().into(),
        }],
    );

    handle.append_line(
        InlineMessageKind::Tool,
        vec![InlineSegment {
            text: "run_pty_cmd([\"nonexistent-command\", \"--help\"])".to_string(),
            style: InlineTextStyle::default().into(),
        }],
    );

    handle.append_line(
        InlineMessageKind::Error,
        vec![InlineSegment {
            text:
                "Error: Command 'nonexistent-command' not found. Make sure the command is installed and in your PATH."
                    .to_string(),
            style: InlineTextStyle::default().into(),
        }],
    );

    handle.append_line(
        InlineMessageKind::Agent,
        vec![InlineSegment {
            text: "I encountered an error running that command. The command 'nonexistent-command' doesn't appear to be available on your system. Would you like me to help you find an alternative?".to_string(),
            style: InlineTextStyle::default().into(),
        }],
    );
    assert_eq!(drain_append_lines(&mut command_rx).len(), 4);

    terminal
        .draw(|f| {
            let _area = f.area();
            assert!(_area.width == 80);
            assert!(_area.height == 20);
        })
        .unwrap();

    assert_eq!(format!("{}", terminal.backend()), blank_terminal(80, 20));
}

/// Smoke test backend sizing while different header contexts are queued.
#[tokio::test]
async fn test_tui_backend_smoke_header_variations() {
    // Test different header contexts without rendering the session widget.
    let contexts_and_snapshots = vec![
        (
            "openai_gpt5",
            InlineHeaderContext {
                provider: "openai".to_string(),
                model: "gpt-oss-20b".to_string(),
                reasoning: "creative".to_string(),
                ..Default::default()
            },
        ),
        (
            "anthropic_claude",
            InlineHeaderContext {
                provider: "anthropic".to_string(),
                model: "claude-3".to_string(),
                reasoning: "analytical".to_string(),
                ..Default::default()
            },
        ),
        (
            "local_llama",
            InlineHeaderContext {
                provider: "local".to_string(),
                model: "llama3".to_string(),
                reasoning: "precise".to_string(),
                ..Default::default()
            },
        ),
    ];

    for (name, context) in contexts_and_snapshots {
        let backend = TestBackend::new(80, 15);
        let mut terminal = Terminal::new(backend).unwrap();

        // Queue session commands with specific header context.
        let (handle, mut command_rx) = smoke_handle();
        handle.set_header_context(context);
        handle.append_line(
            InlineMessageKind::Agent,
            vec![InlineSegment {
                text: format!("Session initialized with {name} context"),
                style: InlineTextStyle::default().into(),
            }],
        );
        assert_eq!(drain_append_lines(&mut command_rx).len(), 1);

        terminal
            .draw(|f| {
                let _area = f.area();
                assert!(_area.width == 80);
                assert!(_area.height == 15);
            })
            .unwrap();

        assert_eq!(format!("{}", terminal.backend()), blank_terminal(80, 15));
    }
}
