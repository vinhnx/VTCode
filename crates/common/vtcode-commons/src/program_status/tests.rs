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

#[test]
fn program_status_vte_gate_checks_both_sides_of_the_safe_boundary() {
    assert!(!vte_version_allows_unknown_osc(Some("5399")));
    assert!(vte_version_allows_unknown_osc(Some("5400")));
    assert!(!vte_version_allows_unknown_osc(Some("0")));
    assert!(vte_version_allows_unknown_osc(Some("7600")));
}

#[test]
fn program_status_vte_gate_treats_missing_or_garbled_versions_as_safe() {
    assert!(vte_version_allows_unknown_osc(None));
    for raw in ["", "  ", "not-a-version", "-1", "54.0", "99999999999999999999"] {
        assert!(vte_version_allows_unknown_osc(Some(raw)), "{raw:?} must allow emission");
    }
    assert!(vte_version_allows_unknown_osc(Some("  5400  ")));
    assert!(!vte_version_allows_unknown_osc(Some("  5399  ")));
}

#[test]
fn program_status_iterm21337_mapping_covers_every_state() {
    use ProgramState::{Blocked, Clear, Done, Error, Idle, Working};
    assert_eq!(iterm21337_status(Idle), ("Idle", "#00d75f"));
    assert_eq!(iterm21337_status(Working), ("Working", "#ff9500"));
    assert_eq!(iterm21337_status(Blocked), ("Waiting", "#5f87ff"));
    for finished in [Done, Error, Clear] {
        assert_eq!(iterm21337_status(finished), ("Idle", "#00d75f"), "{finished:?} reads as idle");
    }
}

#[test]
fn program_status_iterm21337_exact_wire_and_clear() {
    assert_eq!(
        encode_iterm21337(ProgramState::Working, "Running tools"),
        "\x1b]21337;status=Working;indicator=#ff9500;status-color=;detail=Running tools\x07"
    );
    assert_eq!(
        encode_iterm21337(ProgramState::Blocked, "Waiting for permission"),
        "\x1b]21337;status=Waiting;indicator=#5f87ff;status-color=;detail=Waiting for permission\x07"
    );
    assert_eq!(clear_iterm21337(), "\x1b]21337;status=;indicator=;status-color=;detail=\x07");
}

#[test]
fn program_status_iterm21337_detail_cannot_reshape_the_sequence() {
    let wire = encode_iterm21337(ProgramState::Working, "a;b\\c\x1bd\x7fe");
    assert!(wire.ends_with(";detail=a\\;b\\\\cde\x07"), "{wire:?}");
    let long = "x".repeat(300);
    let wire = encode_iterm21337(ProgramState::Idle, &long);
    let detail = wire.split(";detail=").nth(1).unwrap().strip_suffix('\x07').unwrap();
    assert_eq!(detail.chars().count(), 200);
}

#[test]
fn program_status_iterm21337_gate_needs_direct_iterm2_only() {
    assert!(should_emit_iterm21337(true, false, false));
    assert!(!should_emit_iterm21337(false, false, false));
    assert!(!should_emit_iterm21337(true, true, false));
    assert!(!should_emit_iterm21337(true, false, true));
    assert!(!should_emit_iterm21337(true, true, true));
}

#[test]
fn program_status_iterm21337_live_gate_follows_the_environment() {
    let env = crate::env_lock::lock();
    let previous_iterm = std::env::var_os("ITERM_SESSION_ID");
    let previous_tmux = std::env::var_os("TMUX");
    let previous_sty = std::env::var_os("STY");
    env.set_var("ITERM_SESSION_ID", "w0t0p0:AAAA");
    env.remove_var("TMUX");
    env.remove_var("STY");
    assert!(should_emit_iterm21337_now());
    env.set_var("TMUX", "/tmp/tmux-501/default,123,0");
    assert!(!should_emit_iterm21337_now());
    env.remove_var("TMUX");
    env.set_var("STY", "1234.pts-0.host");
    assert!(!should_emit_iterm21337_now());
    env.remove_var("STY");
    env.remove_var("ITERM_SESSION_ID");
    assert!(!should_emit_iterm21337_now());
    env.restore_var("ITERM_SESSION_ID", previous_iterm);
    env.restore_var("TMUX", previous_tmux);
    env.restore_var("STY", previous_sty);
}

#[test]
fn program_status_vte_live_gate_follows_the_environment() {
    let env = crate::env_lock::lock();
    let previous = std::env::var_os("VTE_VERSION");
    env.set_var("VTE_VERSION", "4600");
    assert!(!vte_allows_unknown_osc());
    env.set_var("VTE_VERSION", "7600");
    assert!(vte_allows_unknown_osc());
    env.remove_var("VTE_VERSION");
    assert!(vte_allows_unknown_osc());
    env.restore_var("VTE_VERSION", previous);
}
