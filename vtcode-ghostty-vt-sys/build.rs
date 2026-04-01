use std::env;
use std::path::{Path, PathBuf};

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let workspace_root = manifest_dir.parent().expect("workspace root");
    let target = env::var("TARGET").expect("target");

    rerun_if_changed(&manifest_dir.join("ghostty-vt-manifest.toml"));
    rerun_if_changed(&workspace_root.join("dist").join("ghostty-vt"));
    println!("cargo:rerun-if-env-changed=VTCODE_GHOSTTY_VT_ASSET_DIR");
    println!("cargo:rustc-env=VTCODE_GHOSTTY_VT_TEST_ASSET_DIR=");

    if let Some(asset_dir) = find_asset_dir(workspace_root, &target) {
        println!(
            "cargo:rustc-env=VTCODE_GHOSTTY_VT_TEST_ASSET_DIR={}",
            asset_dir.display()
        );
    }
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
