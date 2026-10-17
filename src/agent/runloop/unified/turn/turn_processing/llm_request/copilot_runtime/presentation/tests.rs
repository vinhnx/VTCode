use std::time::Duration;

use anstyle::{AnsiColor, Color};
use tokio::sync::mpsc::{UnboundedReceiver, unbounded_channel};
use vtcode_ui::tui::app::{InlineCommand, InlineHandle, InlineMessageKind, InlineSegment};

use super::{CopilotPtyStream, ProgressReporter, PtyConfig};

async fn collect_commands_until_closed(receiver: &mut UnboundedReceiver<InlineCommand>) -> Vec<InlineCommand> {
    tokio::time::timeout(Duration::from_secs(3), async {
        let mut commands = Vec::new();
        while let Some(command) = receiver.recv().await {
            commands.push(command);
        }
        commands
    })
    .await
    .expect("presentation tasks must release their UI handles")
}

fn final_pty_block(commands: &[InlineCommand]) -> &[Vec<InlineSegment>] {
    commands
        .iter()
        .rev()
        .find_map(|command| match command {
            InlineCommand::ReplaceLast { kind: InlineMessageKind::Pty, lines, .. } => Some(lines.as_slice()),
            _ => None,
        })
        .expect("final PTY preview")
}

fn block_text(lines: &[Vec<InlineSegment>]) -> String {
    lines
        .iter()
        .map(|line| line.iter().map(|segment| segment.text.as_str()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

#[tokio::test]
async fn finish_drains_queued_output_and_preserves_status_colors() {
    for color in [AnsiColor::Green, AnsiColor::Red, AnsiColor::Yellow] {
        let (sender, mut receiver) = unbounded_channel();
        let handle = InlineHandle::new_for_tests(sender);
        let reporter = ProgressReporter::new();
        reporter.set_total(37).await;
        reporter.set_progress(11).await;
        let stream =
            CopilotPtyStream::start(&handle, reporter.clone(), 8, "printf ordered".to_string(), PtyConfig::default());
        assert_eq!(reporter.percentage().await, 30);
        assert!(!reporter.get_state().is_complete());
        stream.push_output("first界\n");
        stream.push_output("second尾\n");
        drop(handle);
        stream.finish(Color::Ansi(color));

        let commands = collect_commands_until_closed(&mut receiver).await;
        let block = final_pty_block(&commands);
        let text = block_text(block);
        assert!(text.starts_with("• Ran printf ordered\n"), "{text}");
        assert!(text.contains("first界\n"), "{text}");
        assert!(text.contains("second尾"), "{text}");
        assert!(text.find("first界").unwrap() < text.find("second尾").unwrap());
        assert_eq!(text.matches("first界").count(), 1);
        assert_eq!(text.matches("second尾").count(), 1);
        assert_eq!(block[0][0].style.color, Some(Color::Ansi(color)));
        assert!(reporter.get_state().is_complete());
        assert_eq!(reporter.percentage().await, 100);
        assert!(
            !commands
                .iter()
                .any(|command| matches!(command, InlineCommand::SetInputStatus { left: None, right: None }))
        );
    }
}

#[tokio::test]
async fn finish_retains_configured_tail_and_empty_output_header() {
    for output in ["", "discard-first\ndiscard-second\nkeep-third\nkeep-fourth\n"] {
        let (sender, mut receiver) = unbounded_channel();
        let handle = InlineHandle::new_for_tests(sender);
        let stream = CopilotPtyStream::start(
            &handle,
            ProgressReporter::new(),
            2,
            "printf bounded".to_string(),
            PtyConfig::default(),
        );
        stream.push_output(output);
        drop(handle);
        stream.finish(Color::Ansi(AnsiColor::Red));

        let commands = collect_commands_until_closed(&mut receiver).await;
        let block = final_pty_block(&commands);
        let text = block_text(block);
        assert!(text.starts_with("• Ran printf bounded"), "{text}");
        assert!(!text.contains("discard-"), "{text}");
        if output.is_empty() {
            assert_eq!(block.len(), 1);
        } else {
            assert!(text.contains("keep-third\n"), "{text}");
            assert!(text.contains("keep-fourth"), "{text}");
        }
        assert_eq!(block[0][0].style.color, Some(Color::Ansi(AnsiColor::Red)));
    }
}

#[tokio::test]
async fn dropping_active_presentation_releases_tasks_without_completing_progress() {
    let (sender, mut receiver) = unbounded_channel();
    let handle = InlineHandle::new_for_tests(sender);
    let reporter = ProgressReporter::new();
    let stream =
        CopilotPtyStream::start(&handle, reporter.clone(), 4, "printf cancelled".to_string(), PtyConfig::default());
    let mut commands = tokio::time::timeout(Duration::from_secs(3), async {
        let mut commands = Vec::new();
        loop {
            let command = receiver.recv().await.expect("active presentation");
            let has_preview = matches!(command, InlineCommand::ReplaceLast { kind: InlineMessageKind::Pty, .. });
            commands.push(command);
            if has_preview {
                return commands;
            }
        }
    })
    .await
    .expect("runtime must start before cancellation");
    assert!(block_text(final_pty_block(&commands)).contains("• Ran printf cancelled"));
    drop(handle);
    drop(stream);

    commands.extend(collect_commands_until_closed(&mut receiver).await);
    assert!(!reporter.get_state().is_complete());
    assert!(commands.iter().any(|command| matches!(
        command,
        InlineCommand::SetInputStatus { left: Some(label), .. } if label.contains("printf cancelled")
    )));
    assert!(
        !commands
            .iter()
            .any(|command| matches!(command, InlineCommand::SetInputStatus { left: None, right: None }))
    );
}
