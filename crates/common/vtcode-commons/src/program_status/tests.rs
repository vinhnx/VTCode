use super::*;

#[test]
fn program_status_record_identity_is_opaque_stable_and_kind_scoped() {
    assert_eq!(record_segment("delegated", "private-task"), "t3069c7703ff123f3c802259e9e1987");
    assert_ne!(record_segment("background", "private-task"), record_segment("delegated", "private-task"));
}

#[test]
fn program_status_exact_wire_and_unicode() {
    assert_eq!(
        encode_report("vt-1/c2", ProgramState::Blocked, Some(InteractionKind::Permission), "Task", "Approve").unwrap(),
        "\x1b]7501;state=blocked:id=vt-1/c2:app=vtcode:kind=permission:title=VGFzaw==:msg=QXBwcm92ZQ==\x1b\\"
    );
    let wire = encode_report("vt-1", ProgramState::Working, Some(InteractionKind::Auth), "é", "好\u{202e}").unwrap();
    assert!(wire.ends_with(":title=w6k=:msg=5aW9\x1b\\"));
    assert!(!wire.contains("kind="));
}

#[test]
fn program_status_limits_and_injection() {
    for text in ["\x1b]0;injected", "a\n", "\u{0085}", "\x7f"] {
        assert!(encode_report("vt", ProgramState::Idle, None, "", text).is_err());
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
        assert!(encode_report(id, ProgramState::Clear, None, "", "").is_err());
    }
    assert!(
        encode_report("vt", ProgramState::Done, None, &"a".repeat(192), &"b".repeat(2048))
            .unwrap()
            .len()
            < 3300
    );
    assert!(encode_report("vt", ProgramState::Done, None, &"a".repeat(193), "").is_err());
    assert!(encode_report("vt", ProgramState::Done, None, "", &"b".repeat(2049)).is_err());
    let id128 = ["a".repeat(32), "b".repeat(32), "c".repeat(32), "d".repeat(29)].join("/");
    assert!(encode_report(&id128, ProgramState::Idle, None, "", "").is_ok());
    assert!(encode_report(&(id128 + "d"), ProgramState::Idle, None, "", "").is_err());
}
