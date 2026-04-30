#[cfg(target_os = "macos")]
#[expect(
    unsafe_code,
    reason = "Rust 2024 requires unsafe process-environment mutation in build.rs, and this helper runs before any VT Code threads exist."
)]
fn remove_build_env_var(key: &str) {
    // SAFETY: build scripts run in a dedicated process before any application worker threads
    // exist, so mutating the process environment here cannot race with other VT Code code.
    unsafe {
        std::env::remove_var(key);
    }
}

fn main() {
    // Suppress macOS malloc warnings in build output
    // IMPORTANT: Only remove vars, never set them to "0" as that triggers
    // "can't turn off malloc stack logging" warnings from xcrun
    #[cfg(target_os = "macos")]
    {
        // Unset all malloc-related environment variables that might cause warnings
        for key in [
            "MallocStackLogging",
            "MallocStackLoggingDirectory",
            "MallocScribble",
            "MallocGuardEdges",
            "MallocCheckHeapStart",
            "MallocCheckHeapEach",
            "MallocCheckHeapAbort",
            "MallocCheckHeapSleep",
            "MallocErrorAbort",
            "MallocCorruptionAbort",
            "MallocHelpOptions",
            "MallocStackLoggingNoCompact",
        ] {
            remove_build_env_var(key);
        }
    }

    let git_output = std::process::Command::new("git")
        .args(["rev-parse", "--git-dir"])
        .output()
        .ok();
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
            println!("cargo:rerun-if-changed={}/HEAD", git_dir);
        }
        if git_path.join("packed-refs").exists() {
            println!("cargo:rerun-if-changed={}/packed-refs", git_dir);
        }
        if refs_path.join("heads").exists() {
            println!("cargo:rerun-if-changed={}/refs/heads", git_dir);
        }
        if refs_path.join("tags").exists() {
            println!("cargo:rerun-if-changed={}/refs/tags", git_dir);
        }
    }

    let git_output = std::process::Command::new("git")
        .args(["describe", "--always", "--tags", "--long", "--dirty"])
        .output()
        .ok();
    let git_info = git_output
        .as_ref()
        .and_then(|output| std::str::from_utf8(&output.stdout).ok().map(str::trim));
    let cargo_pkg_version = env!("CARGO_PKG_VERSION");

    // Default git_describe to cargo_pkg_version
    let mut git_describe = String::from(cargo_pkg_version);

    if let Some(git_info) = git_info {
        // If the `git_info` contains `CARGO_PKG_VERSION`, we simply use `git_info` as it is.
        // Otherwise, prepend `CARGO_PKG_VERSION` to `git_info`.
        if git_info.contains(cargo_pkg_version) {
            // Remove the 'g' before the commit sha
            let git_info = &git_info.replace('g', "");
            git_describe = git_info.to_string();
        } else {
            git_describe = format!("v{}-{}", cargo_pkg_version, git_info);
        }
    }

    println!("cargo:rustc-env=VT_CODE_GIT_INFO={}", git_describe);
}
