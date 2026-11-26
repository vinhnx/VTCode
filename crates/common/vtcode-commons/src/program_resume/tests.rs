use super::*;

const SPEC_TOKEN: &str = "dummy-token-replace-me";
const SPEC_PID: u32 = 74877;
const SPEC_TIME: u64 = 1795543200;

fn argv(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| part.to_string()).collect()
}

#[test]
fn spec_nvim_vector_round_trips_exact_bytes() {
    let report = encode_report(
        SPEC_PID,
        SPEC_TIME,
        &argv(&["nvim", "-S", "/home/user/.local/state/nvim/sessions/8f2c1d.vim"]),
        None,
        SPEC_TOKEN,
    )
    .expect("spec vector must encode");
    assert_eq!(
        report,
        "\x1b]7577;pid=74877:time=1795543200:argv=bnZpbQ==,LVM=,L2hvbWUvdXNlci8ubG9jYWwvc3RhdGUvbnZpbS9zZXNzaW9ucy84ZjJjMWQudmlt:mac=9dbb99bfb856749a87563839d7ce9062818d78cf25909db93f309e0cd3b1909b\x1b\\"
    );
}

#[test]
fn spec_clear_vector_round_trips_exact_bytes() {
    let report = encode_clear(SPEC_PID, SPEC_TIME, SPEC_TOKEN).expect("spec vector must encode");
    assert_eq!(
        report,
        "\x1b]7577;pid=74877:time=1795543200:clear=1:mac=c30603369f98fef581a4098be889175b0c74f0641e49f8b25f4326cb38f97c93\x1b\\"
    );
}

#[test]
fn spec_lazygit_cwd_vector_round_trips_exact_bytes() {
    let report = encode_report(SPEC_PID, SPEC_TIME, &argv(&["lazygit"]), Some("/home/user/src/project"), SPEC_TOKEN)
        .expect("spec vector must encode");
    assert_eq!(
        report,
        "\x1b]7577;pid=74877:time=1795543200:argv=bGF6eWdpdA==:cwd=L2hvbWUvdXNlci9zcmMvcHJvamVjdA==:mac=35cb909805c380336efd3689a125acc774a9dec8422a1d21492ebec06ca758e1\x1b\\"
    );
}

#[test]
fn spec_wrapper_vector_round_trips_exact_bytes() {
    let report = encode_report(90412, SPEC_TIME, &argv(&["resume-wrap", "ssh", "devbox"]), None, SPEC_TOKEN)
        .expect("spec vector must encode");
    assert_eq!(
        report,
        "\x1b]7577;pid=90412:time=1795543200:argv=cmVzdW1lLXdyYXA=,c3No,ZGV2Ym94:mac=e76149d0ba54a83570ffab0583867dacdfa23928ba9d18604f0badea88b62b65\x1b\\"
    );
}

#[test]
fn mac_changes_with_token_body_and_time() {
    let base_argv = argv(&["vtcode", "--resume", "session-1"]);
    let base = encode_report(SPEC_PID, SPEC_TIME, &base_argv, None, SPEC_TOKEN).expect("base must encode");
    let other_token = encode_report(SPEC_PID, SPEC_TIME, &base_argv, None, "another-token").expect("must encode");
    let other_argv = encode_report(SPEC_PID, SPEC_TIME, &argv(&["vtcode", "--resume", "session-2"]), None, SPEC_TOKEN)
        .expect("must encode");
    let other_time = encode_report(SPEC_PID, SPEC_TIME + 1, &base_argv, None, SPEC_TOKEN).expect("must encode");
    assert_ne!(base, other_token);
    assert_ne!(base, other_argv);
    assert_ne!(base, other_time);
}

#[test]
fn reports_reject_relative_programs_and_cwds() {
    assert!(encode_report(SPEC_PID, SPEC_TIME, &argv(&["./run.sh"]), None, SPEC_TOKEN).is_err());
    assert!(encode_report(SPEC_PID, SPEC_TIME, &argv(&["bin/vtcode"]), None, SPEC_TOKEN).is_err());
    assert!(encode_report(SPEC_PID, SPEC_TIME, &argv(&["-vtcode"]), None, SPEC_TOKEN).is_err());
    assert!(encode_report(SPEC_PID, SPEC_TIME, &argv(&[]), None, SPEC_TOKEN).is_err());
    assert!(encode_report(SPEC_PID, SPEC_TIME, &argv(&[""]), None, SPEC_TOKEN).is_err());
    assert!(encode_report(SPEC_PID, SPEC_TIME, &argv(&["vtcode"]), Some("project"), SPEC_TOKEN).is_err());
    // Absolute programs (Unix and Windows) and empty non-first args are fine.
    assert!(encode_report(SPEC_PID, SPEC_TIME, &argv(&["/usr/local/bin/vtcode", ""]), None, SPEC_TOKEN).is_ok());
    assert!(encode_report(SPEC_PID, SPEC_TIME, &argv(&["C:\\tools\\vtcode.exe"]), None, SPEC_TOKEN).is_ok());
}

#[test]
fn reports_reject_controls_zero_ids_and_empty_token() {
    assert!(encode_report(SPEC_PID, SPEC_TIME, &argv(&["vtcode", "a\nb"]), None, SPEC_TOKEN).is_err());
    assert!(encode_report(SPEC_PID, SPEC_TIME, &argv(&["vtcode"]), Some("/tmp/a\x7fb"), SPEC_TOKEN).is_err());
    assert!(encode_report(0, SPEC_TIME, &argv(&["vtcode"]), None, SPEC_TOKEN).is_err());
    assert!(encode_report(SPEC_PID, 0, &argv(&["vtcode"]), None, SPEC_TOKEN).is_err());
    assert!(encode_report(SPEC_PID, SPEC_TIME, &argv(&["vtcode"]), None, "").is_err());
    assert!(encode_clear(0, SPEC_TIME, SPEC_TOKEN).is_err());
    assert!(encode_clear(SPEC_PID, 0, SPEC_TOKEN).is_err());
    assert!(encode_clear(SPEC_PID, SPEC_TIME, "").is_err());
}

#[test]
fn reports_enforce_protocol_limits() {
    let many = vec!["arg".to_string(); MAX_ARGV_ELEMENTS + 1];
    assert!(encode_report(SPEC_PID, SPEC_TIME, &many, None, SPEC_TOKEN).is_err());
    let huge = "x".repeat(MAX_REPORT_LEN);
    assert!(encode_report(SPEC_PID, SPEC_TIME, &argv(&["vtcode", &huge]), None, SPEC_TOKEN).is_err());
    // Boundary: exactly the element cap still encodes.
    let mut capped = vec!["arg".to_string(); MAX_ARGV_ELEMENTS - 1];
    capped.insert(0, "vtcode".to_string());
    assert!(encode_report(SPEC_PID, SPEC_TIME, &capped, None, SPEC_TOKEN).is_ok());
}

#[test]
fn resume_token_follows_environment_presence() {
    let env = crate::env_lock::lock();
    let previous = std::env::var_os(RESUME_TOKEN_ENV);
    env.set_var(RESUME_TOKEN_ENV, "token-value");
    assert_eq!(resume_token().as_deref(), Some("token-value"));
    env.set_var(RESUME_TOKEN_ENV, "");
    assert_eq!(resume_token(), None);
    env.set_var(RESUME_TOKEN_ENV, "x".repeat(1025));
    assert_eq!(resume_token(), None);
    env.set_var(RESUME_TOKEN_ENV, "x".repeat(1024));
    assert!(resume_token().is_some());
    env.remove_var(RESUME_TOKEN_ENV);
    assert_eq!(resume_token(), None);
    env.restore_var(RESUME_TOKEN_ENV, previous);
}

#[test]
fn validate_target_matches_encoder_acceptance() {
    // Boundary and wire agree on both sides: valid here encodes, invalid here fails.
    let valid = argv(&["vtcode", "--resume", "session-1"]);
    assert!(validate_target(&valid, Some("/tmp/ws")).is_ok());
    assert!(encode_report(SPEC_PID, SPEC_TIME, &valid, Some("/tmp/ws"), SPEC_TOKEN).is_ok());
    for (argv, cwd) in [
        (argv(&["./relative"]), None),
        (argv(&["vtcode"]), Some("relative")),
        (argv(&[]), None),
        (argv(&["vtcode", "bad\narg"]), None),
    ] {
        assert!(validate_target(&argv, cwd).is_err());
        assert!(encode_report(SPEC_PID, SPEC_TIME, &argv, cwd, SPEC_TOKEN).is_err());
    }
}
