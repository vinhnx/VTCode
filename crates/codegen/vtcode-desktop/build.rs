//! Track translations, embedded license documents, and the build identity.

use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=locales");
    // The license viewer embeds these at compile time; rebuild when they change.
    for document in ["LICENSE", "LICENSE-MIT", "THIRD-PARTY-NOTICES"] {
        println!("cargo:rerun-if-changed=../../../{document}");
    }
    emit_build_identity();
}

/// Stamp the commit that produced this binary. The packaging script exports the
/// same value for `CFBundleVersion`, so the native About panel and the in-app
/// fallback card always agree.
fn emit_build_identity() {
    emit_git_rerun_triggers();
    println!("cargo:rerun-if-env-changed=VTCODE_DESKTOP_BUILD_VERSION");
    let build_version = std::env::var("VTCODE_DESKTOP_BUILD_VERSION")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(git_revision)
        .unwrap_or_else(|| String::from("unknown"));
    println!("cargo:rustc-env=VTCODE_DESKTOP_BUILD_VERSION={build_version}");
}

/// Tell cargo to rerun this script when the checked-out commit changes.
fn emit_git_rerun_triggers() {
    let git_dir = Command::new("git")
        .args(["rev-parse", "--git-dir"])
        .output()
        .ok()
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|path| path.trim().to_string())
        .filter(|path| !path.is_empty());
    let Some(git_dir) = git_dir else {
        return;
    };
    let git_path = std::path::Path::new(&git_dir);
    for candidate in ["HEAD", "packed-refs", "refs/heads", "refs/tags"] {
        let path = git_path.join(candidate);
        if path.exists() {
            println!("cargo:rerun-if-changed={}", path.display());
        }
    }
}

/// Short commit hash, with `.dirty` appended when the worktree has edits, so a
/// locally built binary is distinguishable from the release commit.
fn git_revision() -> Option<String> {
    let sha = Command::new("git")
        .args(["rev-parse", "--short=12", "HEAD"])
        .output()
        .ok()
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|sha| sha.trim().to_string())
        .filter(|sha| !sha.is_empty())?;
    let dirty = Command::new("git")
        .args(["status", "--porcelain"])
        .output()
        .ok()
        .is_some_and(|output| !output.stdout.iter().all(u8::is_ascii_whitespace));
    Some(if dirty { format!("{sha}.dirty") } else { sha })
}
