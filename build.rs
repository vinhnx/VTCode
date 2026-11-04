#![allow(
    missing_docs,
    reason = "Intentional compatibility, platform, or test-only suppression."
)]

fn main() {
    let git_output = std::process::Command::new("git").args(["rev-parse", "--git-dir"]).output().ok();
    let git_dir = git_output.as_ref().and_then(|output| {
        std::str::from_utf8(&output.stdout)
            .ok()
            .and_then(|s| s.strip_suffix('\n').or_else(|| s.strip_suffix("\r\n")))
    });

    // Tell cargo to rebuild if the head or any relevant refs change.
    if let Some(git_dir) = git_dir {
        let git_path = std::path::Path::new(git_dir);
        let refs_path = git_path.join("refs");
        if git_path.join("HEAD").exists() {
            println!("cargo:rerun-if-changed={git_dir}/HEAD");
        }
        if git_path.join("packed-refs").exists() {
            println!("cargo:rerun-if-changed={git_dir}/packed-refs");
        }
        if refs_path.join("heads").exists() {
            println!("cargo:rerun-if-changed={git_dir}/refs/heads");
        }
        if refs_path.join("tags").exists() {
            println!("cargo:rerun-if-changed={git_dir}/refs/tags");
        }
    }

    let cargo_pkg_version = env!("CARGO_PKG_VERSION");

    // Build stamp: `{version}+{short_sha}[.dirty]` (semver build metadata).
    // Build metadata is ignored in precedence, so the desktop updater's
    // `installed >= release` check treats a stamped local build exactly like
    // its bare release — while `--version` triage still shows which commit
    // produced the binary. Single token, always: the updater's helper-version
    // parse rejects anything but `name <semver>` on the first line.
    let sha = std::process::Command::new("git")
        .args(["rev-parse", "--short=12", "HEAD"])
        .output()
        .ok()
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|sha| sha.trim().to_string())
        .filter(|sha| !sha.is_empty());
    let dirty = std::process::Command::new("git")
        .args(["status", "--porcelain"])
        .output()
        .ok()
        .is_some_and(|output| !output.stdout.iter().all(u8::is_ascii_whitespace));
    let mut git_describe = String::from(cargo_pkg_version);
    if let Some(sha) = sha {
        git_describe = format!("{cargo_pkg_version}+{sha}");
        if dirty {
            git_describe.push_str(".dirty");
        }
    }

    println!("cargo:rustc-env=VT_CODE_GIT_INFO={git_describe}");

    // macOS: the `vtcode` binary's `__eh_frame` exceeds ld64's 16 MiB
    // compact-unwind limit, so every dev link warns `__eh_frame section too
    // large ... performance of exception handling might be affected`
    // (rust-lang/rust#159105). That warning is cosmetic — do NOT silence it
    // with `-Wl,-no_compact_unwind`: forcing the DWARF fallback breaks panic
    // unwinding entirely for this binary (any panicking test aborts with
    // `fatal runtime error: failed to initiate panic, error 5`, SIGABRT;
    // cf. rust-lang/rust#156812). Release builds use `panic = "abort"` and
    // strip symbols, so unwind tables are unaffected there.
    //
    // Instead, the warning is silenced at the lint level:
    // `#![cfg_attr(target_os = "macos", allow(linker_messages, ...))]` in
    // `src/main.rs` (the lint can only be controlled at a linked crate root).
    //
    // (A `-no_compact_unwind` workaround lived here until 2026-09-25, when it
    // was found to be the root cause of the SIGABRT failures in
    // `from_validated_debug_asserts_on_non_ready_report` and
    // `planning_preview_budget_keeps_midsize_payload_exec_strips_it`.)
}
