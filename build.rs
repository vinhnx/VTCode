fn main() {
    // Suppress macOS malloc warnings in build output
    #[cfg(target_os = "macos")]
    {
        // Unset all malloc-related environment variables that might cause warnings
        unsafe {
            std::env::remove_var("MallocStackLogging");
            std::env::remove_var("MallocStackLoggingDirectory");
            std::env::remove_var("MallocScribble");
            std::env::remove_var("MallocGuardEdges");
            std::env::remove_var("MallocCheckHeapStart");
            std::env::remove_var("MallocCheckHeapEach");
            std::env::remove_var("MallocCheckHeapAbort");
            std::env::remove_var("MallocCheckHeapSleep");
            std::env::remove_var("MallocErrorAbort");
            std::env::remove_var("MallocCorruptionAbort");
            std::env::remove_var("MallocHelpOptions");
            std::env::remove_var("MallocStackLoggingNoCompact");

            // Set environment to explicitly disable malloc debugging
            std::env::set_var("MallocStackLogging", "0");
            std::env::set_var("MallocStackLoggingDirectory", "");
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
