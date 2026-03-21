use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let workspace_root = manifest_dir.parent().expect("workspace root");
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("out dir"));
    let target = env::var("TARGET").expect("target");

    rerun_if_changed(&manifest_dir.join("csrc").join("ghostty_vt_host.c"));
    rerun_if_changed(&manifest_dir.join("ghostty-vt-manifest.toml"));
    rerun_if_changed(&workspace_root.join("dist").join("ghostty-vt"));
    println!("cargo:rerun-if-env-changed=VTCODE_GHOSTTY_VT_ASSET_DIR");
    println!("cargo:rustc-env=VTCODE_GHOSTTY_VT_HOST_BUILD=");

    let Some(asset_dir) = find_asset_dir(workspace_root, &target) else {
        return;
    };

    let include_dir = asset_dir.join("include");
    let lib_dir = asset_dir.join("lib");
    if !include_dir.join("ghostty").join("vt.h").is_file() {
        panic!(
            "Ghostty VT asset dir '{}' is missing include/ghostty/vt.h",
            asset_dir.display()
        );
    }

    if !lib_dir.is_dir() {
        panic!(
            "Ghostty VT asset dir '{}' is missing lib/ directory",
            asset_dir.display()
        );
    }

    let helper_name = if cfg!(windows) {
        "ghostty_vt_host.exe"
    } else {
        "ghostty_vt_host"
    };
    let helper_path = out_dir.join(helper_name);

    let mut build_helper = Command::new("zig");
    build_helper.arg("cc");
    build_helper.arg(manifest_dir.join("csrc").join("ghostty_vt_host.c"));
    build_helper.arg("-std=c11");
    build_helper.arg("-O2");
    build_helper.arg(format!("-I{}", include_dir.display()));
    build_helper.arg(format!("-L{}", lib_dir.display()));
    build_helper.arg("-lghostty-vt");
    if cfg!(target_os = "macos") || cfg!(target_os = "linux") {
        build_helper.arg(format!("-Wl,-rpath,{}", lib_dir.display()));
    }
    build_helper.arg("-o");
    build_helper.arg(&helper_path);

    run_command(&mut build_helper, "build Ghostty VT host helper");

    println!(
        "cargo:rustc-env=VTCODE_GHOSTTY_VT_HOST_BUILD={}",
        helper_path.display()
    );
}

fn find_asset_dir(workspace_root: &Path, target: &str) -> Option<PathBuf> {
    let env_dir = env::var_os("VTCODE_GHOSTTY_VT_ASSET_DIR").map(PathBuf::from);
    let fallback_dir = workspace_root.join("dist").join("ghostty-vt").join(target);

    [env_dir, Some(fallback_dir)]
        .into_iter()
        .flatten()
        .find_map(|base| {
            if has_asset_layout(&base) {
                Some(base)
            } else {
                let target_dir = base.join(target);
                has_asset_layout(&target_dir).then_some(target_dir)
            }
        })
}

fn has_asset_layout(path: &Path) -> bool {
    path.join("include").join("ghostty").join("vt.h").is_file() && path.join("lib").is_dir()
}

fn rerun_if_changed(path: &Path) {
    println!("cargo:rerun-if-changed={}", path.display());
}

fn run_command(command: &mut Command, description: &str) {
    let status = command
        .status()
        .unwrap_or_else(|error| panic!("{description}: {error}"));
    assert!(
        status.success(),
        "{description} failed with status {status}"
    );
}
