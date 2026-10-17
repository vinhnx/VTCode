use super::*;

#[test]
fn program_status_record_identity_is_opaque_stable_and_kind_scoped() {
    assert_eq!(record_segment("delegated", "private-task"), "t3069c7703ff123f3c802259e9e1987");
    assert_ne!(record_segment("background", "private-task"), record_segment("delegated", "private-task"));
}

#[test]
fn program_status_exact_wire_and_unicode() {
    assert_eq!(
        encode_report("vt-1/c2", ProgramState::Blocked, Some(InteractionKind::Permission), None, "Task", "Approve")
            .unwrap(),
        "\x1b]7501;state=blocked:id=vt-1/c2:app=vtcode:kind=permission:title=VGFzaw==:msg=QXBwcm92ZQ==\x1b\\"
    );
    let wire =
        encode_report("vt-1", ProgramState::Working, Some(InteractionKind::Auth), None, "é", "好\u{202e}").unwrap();
    assert!(wire.ends_with(":title=w6k=:msg=5aW9\x1b\\"));
    assert!(!wire.contains("kind="));
}

#[test]
fn program_status_progress_only_with_working_or_blocked() {
    let working = encode_report("vt", ProgramState::Working, None, Some(40), "VT Code", "Working").unwrap();
    assert!(working.contains(":progress=40:"));
    let blocked =
        encode_report("vt", ProgramState::Blocked, Some(InteractionKind::Question), Some(0), "VT Code", "Waiting")
            .unwrap();
    assert!(blocked.contains(":progress=0:"));
    // Ignored otherwise: idle/done/error/clear never carry progress.
    for state in [
        ProgramState::Idle,
        ProgramState::Done,
        ProgramState::Error,
        ProgramState::Clear,
    ] {
        let wire = encode_report("vt", state, None, Some(50), "", "").unwrap();
        assert!(!wire.contains("progress="), "{state:?} must omit progress");
    }
    // Out-of-range is treated as absent.
    let clamped = encode_report("vt", ProgramState::Working, None, Some(101), "", "").unwrap();
    assert!(!clamped.contains("progress="));
    let full = encode_report("vt", ProgramState::Working, None, Some(100), "", "").unwrap();
    assert!(full.contains(":progress=100:"));
}

#[test]
fn program_status_limits_and_injection() {
    for text in ["\x1b]0;injected", "a\n", "\u{0085}", "\x7f"] {
        assert!(encode_report("vt", ProgramState::Idle, None, None, "", text).is_err());
    }
    for id in [
        "",
        "/vt",
        "vt/",
        "vt//c",
        "vt:x",
        "vt;state=clear",
        "é",
        &"x".repeat(33),
        "a/b/c/d/e/f/g/h/i",
    ] {
        assert!(encode_report(id, ProgramState::Clear, None, None, "", "").is_err());
    }
    assert!(
        encode_report("vt", ProgramState::Done, None, None, &"a".repeat(192), &"b".repeat(2048))
            .unwrap()
            .len()
            < 3300
    );
    assert!(encode_report("vt", ProgramState::Done, None, None, &"a".repeat(193), "").is_err());
    assert!(encode_report("vt", ProgramState::Done, None, None, "", &"b".repeat(2049)).is_err());
    let id128 = ["a".repeat(32), "b".repeat(32), "c".repeat(32), "d".repeat(29)].join("/");
    assert!(encode_report(&id128, ProgramState::Idle, None, None, "", "").is_ok());
    assert!(encode_report(&(id128 + "d"), ProgramState::Idle, None, None, "", "").is_err());
}
