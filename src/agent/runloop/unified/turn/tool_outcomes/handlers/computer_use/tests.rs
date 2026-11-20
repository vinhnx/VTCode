use super::*;
use vtcode_computer_use::{MouseButton, Point, ScrollDirection};

fn target() -> ComputerUseTarget {
    ComputerUseTarget::Browser { build_dir: PathBuf::from("/approved/build") }
}

#[test]
fn computer_use_approval_names_every_action_kind_without_payloads() {
    let actions = [
        Action::Screenshot,
        Action::Zoom {
            top_left: Point { x: 1, y: 2 },
            bottom_right: Point { x: 3, y: 4 },
        },
        Action::Click {
            at: Some(Point { x: 5, y: 6 }),
            button: MouseButton::Left,
            count: 2,
            modifiers: vec!["shift".into()],
        },
        Action::Drag {
            path: vec![Point { x: 1, y: 1 }, Point { x: 2, y: 2 }],
            modifiers: Vec::new(),
        },
        Action::Move { at: Point { x: 7, y: 8 }, modifiers: Vec::new() },
        Action::ScrollPixels {
            at: Point { x: 1, y: 1 },
            x: 9,
            y: 10,
            modifiers: Vec::new(),
        },
        Action::ScrollTicks {
            at: None,
            direction: ScrollDirection::Down,
            amount: 3,
            modifiers: Vec::new(),
        },
        Action::Key { keys: vec!["a".into()], repeat: 1 },
        Action::HoldKey {
            keys: vec!["shift".into()],
            duration: std::time::Duration::from_secs(1),
        },
        Action::LeftMouseDown,
        Action::LeftMouseUp,
        Action::CursorPosition,
        Action::Type("private-typed-text".into()),
        Action::Wait(std::time::Duration::from_secs(1)),
    ];
    let kinds: Vec<_> = actions.iter().map(action_kind).collect();
    assert_eq!(
        kinds,
        [
            "screenshot",
            "zoom",
            "click",
            "drag",
            "move",
            "scroll",
            "scroll",
            "key",
            "hold_key",
            "left_mouse_down",
            "left_mouse_up",
            "cursor_position",
            "type",
            "wait",
        ]
    );

    let request = ApprovalRequest {
        target: target(),
        recipient: "gpt-6-luna (openai)".to_string(),
        call_ids: vec!["call_17".to_string(), "call_18".to_string()],
        action_kinds: kinds.iter().map(ToString::to_string).collect(),
        action_count: actions.len(),
    };
    let args = approval_args(&request);
    let rendered = args.to_string();
    assert!(rendered.contains("/approved/build"));
    assert!(rendered.contains("gpt-6-luna (openai)"));
    assert!(rendered.contains("call_17"));
    assert!(!rendered.contains("private-typed-text"));

    let justification = approval_justification(&request);
    assert!(justification.contains("/approved/build"));
    assert!(justification.contains("14 actions proposed"));
}

#[test]
fn computer_use_approval_display_neutralizes_control_characters() {
    // Approval rows render config paths and model labels that a malicious
    // repo config can fill with ANSI escapes or \r line-spoofing. Display
    // text maps controls to spaces; the underlying values used for
    // validation and mounts stay untouched.
    let request = ApprovalRequest {
        target: ComputerUseTarget::Browser {
            build_dir: PathBuf::from("/good\u{1b}[2K\r/evil\nnext"),
        },
        recipient: "model\u{7f} (openai)".to_string(),
        call_ids: vec!["call_1".to_string()],
        action_kinds: vec!["screenshot".to_string()],
        action_count: 1,
    };
    let justification = approval_justification(&request);
    assert!(!justification.chars().any(|c| c.is_control()), "{justification}");
    assert!(justification.contains("/good"), "{justification}");
    let lines = computer_use_approval_lines("computer_use", Some(&approval_args(&request)));
    assert_eq!(lines.len(), 3);
    for line in &lines {
        assert!(!line.chars().any(|c| c.is_control()), "{line}");
    }
    assert!(lines.iter().any(|line| line.contains("/good") && line.contains("/evil")));
    // Raw values keep their bytes for validation and audit.
    let args = approval_args(&request);
    assert!(args.to_string().contains("\\u001b"));
}

#[test]
fn computer_use_approval_grants_only_on_explicit_approval() {
    assert!(approval_granted(&ToolPermissionFlow::Approved { updated_args: None }));
    assert!(approval_granted(&ToolPermissionFlow::Approved { updated_args: Some(Value::Null) }));
    assert!(!approval_granted(&ToolPermissionFlow::Denied));
    assert!(!approval_granted(&ToolPermissionFlow::Blocked { reason: "headless".into() }));
    assert!(!approval_granted(&ToolPermissionFlow::Exit));
    assert!(!approval_granted(&ToolPermissionFlow::Interrupted));
}
