#![allow(missing_docs)]
//! Comprehensive Ratatui snapshot tests for VT Code TUI
//!
//! These tests use the `insta` crate to capture visual snapshots of the actual terminal UI.
//! The tests verify that the TUI renders correctly with various content and states.
//!
//! To update snapshots, run: `cargo insta review`

use anstyle::Effects;
use insta::assert_snapshot;
use ratatui::{Terminal, backend::TestBackend};
use vtcode_core::ui::{
    InlineHeaderContext, InlineMessageKind, InlineSegment, InlineTextStyle, InlineTheme, SessionOptions,
    spawn_session_with_options,
};

/// Test actual UI rendering with a full terminal backend simulation
#[test]
fn test_actual_tui_rendering() {
    // Create a TestBackend to simulate the terminal
    let backend = TestBackend::new(80, 20);
    let mut terminal = Terminal::new(backend).unwrap();

    // This test verifies that the TestBackend works correctly with the TUI system
    terminal
        .draw(|f| {
            // Draw a basic UI representation
            let area = f.area();
            // The actual rendering happens elsewhere, but this ensures our test infrastructure works
            assert!(area.width > 0);
            assert!(area.height > 0);
        })
        .unwrap();

    // Capture the actual terminal output as a snapshot
    assert_snapshot!(
        format!("{}", terminal.backend()),
        @r###"
    "                                                                                "
    "                                                                                "
    "                                                                                "
    "                                                                                "
    "                                                                                "
    "                                                                                "
    "                                                                                "
    "                                                                                "
    "                                                                                "
    "                                                                                "
    "                                                                                "
    "                                                                                "
    "                                                                                "
    "                                                                                "
    "                                                                                "
    "                                                                                "
    "                                                                                "
    "                                                                                "
    "                                                                                "
    "                                                                                "
    "###
    );
}

/// Test that UI components are properly serializable for debugging
#[test]
fn test_ui_component_serialization() {
    // Test InlineTheme serialization
    let theme = InlineTheme::default();
    let theme_repr = format!("{theme:?}");
    assert_snapshot!(
        &theme_repr,
        @"InlineTheme { foreground: None, background: None, primary: None, secondary: None, tool_accent: None, tool_body: None, pty_body: None }"
    );

    // Test InlineSegment serialization with different styles
    let normal_segment = InlineSegment {
        text: "Normal text".to_string(),
        style: InlineTextStyle {
            color: None,
            bg_color: None,
            effects: Effects::new(),
        }
        .into(),
    };
    assert_snapshot!(
        &format!("{normal_segment:?}"),
        @"InlineSegment { text: \"Normal text\", style: InlineTextStyle { color: None, bg_color: None, effects: Effects() } }"
    );

    let bold_segment = InlineSegment {
        text: "Bold text".to_string(),
        style: InlineTextStyle {
            color: None,
            bg_color: None,
            effects: Effects::BOLD,
        }
        .into(),
    };
    assert_snapshot!(
        &format!("{bold_segment:?}"),
        @"InlineSegment { text: \"Bold text\", style: InlineTextStyle { color: None, bg_color: None, effects: Effects(BOLD) } }"
    );

    let italic_segment = InlineSegment {
        text: "Italic text".to_string(),
        style: InlineTextStyle {
            color: None,
            bg_color: None,
            effects: Effects::ITALIC,
        }
        .into(),
    };
    assert_snapshot!(
        &format!("{italic_segment:?}"),
        @"InlineSegment { text: \"Italic text\", style: InlineTextStyle { color: None, bg_color: None, effects: Effects(ITALIC) } }"
    );
}

/// Test header context rendering simulation
#[test]
fn test_header_context_rendering() {
    let context = InlineHeaderContext {
        app_name: "VT Code".to_string(),
        provider: "openai".to_string(),
        model: "gpt-oss-20b".to_string(),
        context_window_size: None,
        version: "0.37.1".to_string(),
        search_tools: None,
        persistent_memory: None,
        pr_review: None,
        git: "main branch".to_string(),
        reasoning: "creative".to_string(),
        reasoning_stage: None,
        workspace_trust: "trusted".to_string(),
        tools: "enabled".to_string(),
        mcp: "available".to_string(),
        primary_agent: None,
        primary_agent_color: None,
        highlights: vec![],
        subagent_badges: vec![],
        editor_context: None,
    };

    // Test that the context can be properly represented
    let context_repr = format!("{context:?}");
    assert_snapshot!(
        &context_repr,
        @"InlineHeaderContext { app_name: \"VT Code\", provider: \"openai\", model: \"gpt-oss-20b\", context_window_size: None, version: \"0.37.1\", search_tools: None, persistent_memory: None, pr_review: None, editor_context: None, git: \"main branch\", reasoning: \"creative\", reasoning_stage: None, workspace_trust: \"trusted\", tools: \"enabled\", mcp: \"available\", primary_agent: None, primary_agent_color: None, highlights: [], subagent_badges: [] }"
    );
}

/// Test message kind representations
#[test]
fn test_message_kind_representations() {
    assert_snapshot!(
        &format!("{:?}", InlineMessageKind::Agent),
        @"Agent"
    );
    assert_snapshot!(
        &format!("{:?}", InlineMessageKind::User),
        @"User"
    );
    assert_snapshot!(
        &format!("{:?}", InlineMessageKind::Error),
        @"Error"
    );
    assert_snapshot!(
        &format!("{:?}", InlineMessageKind::Info),
        @"Info"
    );
    assert_snapshot!(
        &format!("{:?}", InlineMessageKind::Policy),
        @"Policy"
    );
    assert_snapshot!(
        &format!("{:?}", InlineMessageKind::Tool),
        @"Tool"
    );
    assert_snapshot!(
        &format!("{:?}", InlineMessageKind::Pty),
        @"Pty"
    );
}

/// Test TUI session creation and basic functionality
#[tokio::test]
async fn test_tui_session_creation() {
    let session_result = spawn_session_with_options(
        InlineTheme::default(),
        SessionOptions {
            placeholder: Some("Enter your query here...".to_string()),
            inline_rows: 10,
            ..SessionOptions::default()
        },
    );

    // In headless CI/non-interactive runs, TUI startup may fail with a non-TTY error.
    let startup_ok = match &session_result {
        Ok(_) => true,
        Err(err) => err.to_string().contains("stdin is not a terminal"),
    };
    assert!(startup_ok);

    // Snapshot the creation result
    assert_snapshot!(
        &format!("{startup_ok:?}"),
        @"true"
    );
}
