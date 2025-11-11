//! Snapshot tests for the Ratatui TUI components
//!
//! These tests use the `insta` crate to capture snapshots of the UI components and logic.
//! Since direct TUI rendering requires internal access, we test the UI logic and components
//! that feed into the TUI rendering pipeline.
//!
//! To update snapshots, run: `cargo insta review`

use insta::assert_snapshot;
use vtcode_core::ui::tui::{
    InlineHeaderContext, InlineMessageKind, InlineSegment, InlineTextStyle,
};

/// Test message kind string representation
#[test]
fn test_message_kind_snapshot() {
    let agent_kind = format!("{:?}", InlineMessageKind::Agent);
    let user_kind = format!("{:?}", InlineMessageKind::User);
    let error_kind = format!("{:?}", InlineMessageKind::Error);

    assert_snapshot!("message_kind_agent", agent_kind);
    assert_snapshot!("message_kind_user", user_kind);
    assert_snapshot!("message_kind_error", error_kind);
}

/// Test message segment representation
#[test]
fn test_inline_segment_snapshot() {
    let segment = InlineSegment {
        text: "Hello, world!".to_string(),
        style: InlineTextStyle {
            color: None,
            bg_color: None,
            effects: Effects::new().bold(),
        },
    };
    assert_snapshot!("styled_segment", format!("{:?}", segment));
}

/// Test header context representation
#[test]
fn test_header_context_snapshot() {
    let context = InlineHeaderContext {
        provider: "openai".to_string(),
        model: "gpt-4".to_string(),
        reasoning: "creative".to_string(),
        mode: "inline".to_string(),
        workspace_trust: "trusted".to_string(),
        tools: "enabled".to_string(),
        git: "clean".to_string(),
        mcp: "disabled".to_string(),
        highlights: vec![],
        version: "0.37.1".to_string(),
    };
    assert_snapshot!("header_context", format!("{:?}", context));
}

/// Test inline command debugging output
#[test]
fn test_inline_command_debug() {
    let segment = InlineSegment {
        text: "Hello! I'm your AI assistant.".to_string(),
        style: InlineTextStyle {
            color: None,
            bg_color: None,
            effects: Effects::new().bold(),
        },
    };

    // Test string representation
    let debug_output = format!("{:?}", InlineMessageKind::Agent);
    assert_snapshot!("message_kind_debug", debug_output);

    let segment_debug = format!("{:?}", segment);
    assert_snapshot!("segment_debug", segment_debug);
}

/// Test UI component combinations
#[test]
fn test_ui_component_combinations() {
    let message_segment = InlineSegment {
        text: "This is a test message with styling".to_string(),
        style: InlineTextStyle {
            color: None,
            bg_color: None,
            effects: Effects::new().bold().italic(),
        },
    };

    let context = InlineHeaderContext {
        provider: "anthropic".to_string(),
        model: "claude-3".to_string(),
        mode: "alternate".to_string(),
        ..Default::default()
    };

    let combined_repr = format!("Message: {:?}\nContext: {:?}", message_segment, context);

    assert_snapshot!("ui_component_combinations", combined_repr);
}
