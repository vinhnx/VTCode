use super::*;
use std::ffi::OsStr;

fn manifest_dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_owned()
}

#[tokio::test]
async fn container_spec_requires_a_pinned_image() {
    let dir = manifest_dir();
    assert!(ContainerSpec::approved("registry.invalid/runner:1.64.1", &dir).await.is_ok());
    assert!(
        ContainerSpec::approved("registry.invalid/runner@sha256:abc", &dir)
            .await
            .is_ok()
    );
    for image in [
        "",
        "runner",
        "runner:latest",
        "registry.invalid/runner:1.64\n1",
        "run ner:1.0",
    ] {
        assert!(ContainerSpec::approved(image, &dir).await.is_err(), "accepted {image:?}");
    }
}

#[tokio::test]
async fn container_spec_requires_a_real_directory() {
    let missing = manifest_dir().join("does-not-exist");
    assert!(ContainerSpec::approved("registry.invalid/runner:1.0", &missing).await.is_err());
    let file = manifest_dir().join("Cargo.toml");
    assert!(ContainerSpec::approved("registry.invalid/runner:1.0", &file).await.is_err());
    assert!(
        ContainerSpec::approved("registry.invalid/runner:1.0", &manifest_dir())
            .await
            .is_ok()
    );
}

#[test]
fn container_mount_spec_rejects_separators_quotes_and_controls() {
    assert!(mount_spec_safe("/build/app"));
    for dir in [
        "",
        "/build,with,comma",
        "/build\"quoted",
        "/build\\escaped",
        "/build\nnewline",
        "/build\tindented",
    ] {
        assert!(!mount_spec_safe(dir), "accepted {dir:?}");
    }
}

#[tokio::test]
async fn container_spec_rejects_a_comma_directory() {
    // Comma is legal in directory names on every platform; the remaining
    // rejections are covered portably through `mount_spec_safe`.
    let dir = std::env::temp_dir().join(format!("vtcode-cu-mount-{},comma", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let contaminated = ContainerSpec::approved("registry.invalid/runner:1.0", &dir).await;
    assert!(contaminated.is_err());
    std::fs::remove_dir(&dir).unwrap();
}

#[tokio::test]
async fn container_launch_argv_locks_the_hardening_flags() {
    let spec = ContainerSpec::approved("registry.invalid/runner:1.64.1", &manifest_dir())
        .await
        .unwrap();
    let argv = docker_run_argv(&spec, "vtcode-cu-test");
    let flags: Vec<_> = argv.iter().map(|arg| arg.to_string_lossy()).collect();
    assert_eq!(
        flags[..13].join(" "),
        "run --rm --interactive --init --pull=never --network=none --read-only \
         --cap-drop=ALL --security-opt=no-new-privileges --pids-limit=256 --memory=1g \
         --tmpfs=/tmp:rw,nosuid,nodev,size=512m --env=HOME=/tmp"
    );
    assert_eq!(flags[13], "--name=vtcode-cu-test");
    assert!(flags[14].starts_with("--mount=type=bind,src="));
    assert!(flags[14].ends_with(",dst=/build,readonly"));
    assert_eq!(flags[15], "registry.invalid/runner:1.64.1");
    assert_eq!(flags.len(), 16);
    assert!(argv.iter().all(|arg| arg.to_str().is_some()));
    assert_ne!(OsStr::new(&spec.build_dir), OsStr::new(""));
}

#[test]
fn container_identity_must_be_a_full_hex_id() {
    let valid = "a".repeat(CONTAINER_ID_LEN);
    assert_eq!(parse_container_id(&format!("  {valid}\n")).unwrap(), valid);
    for raw in [
        "",
        "xyz",
        &"g".repeat(CONTAINER_ID_LEN),
        &"a".repeat(CONTAINER_ID_LEN - 1),
        &"a".repeat(CONTAINER_ID_LEN + 1),
    ] {
        assert!(parse_container_id(raw).is_err(), "accepted {raw:?}");
    }
}

#[test]
fn container_diagnostics_are_bounded_on_char_boundaries() {
    assert_eq!(diagnostic_text(b"gone"), "gone");
    let multibyte = "é".repeat(DIAGNOSTIC_MAX_BYTES + 8);
    let diagnostic = diagnostic_text(multibyte.as_bytes());
    assert!(diagnostic.len() <= DIAGNOSTIC_MAX_BYTES);
    assert!(diagnostic.chars().all(|c| c == 'é'));
    assert_eq!(diagnostic_text(b"\xff\xfe"), "��");
}
