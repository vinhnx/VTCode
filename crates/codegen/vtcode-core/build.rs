#![allow(
    missing_docs,
    reason = "Intentional compatibility, platform, or test-only suppression."
)]

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

const EMBEDDED_ASSETS: &[(&str, &str)] = &[
    ("docs/modules/vtcode_docs_map.md", "docs/vtcode_docs_map.md"),
    ("resources/icons/vtcode-app-light-128.png", "resources/icons/vtcode-app-light-128.png"),
    ("resources/icons/vtcode-app-dark-128.png", "resources/icons/vtcode-app-dark-128.png"),
    ("resources/icons/vtcode-app-clear-128.png", "resources/icons/vtcode-app-clear-128.png"),
    ("resources/icons/vtcode-app-tinted-128.png", "resources/icons/vtcode-app-tinted-128.png"),
];

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let is_docsrs = env::var_os("DOCS_RS").is_some();
    let is_nix_build = env::var_os("NIX_BUILD_TOP").is_some();

    if is_docsrs || is_nix_build {
        println!(
            "cargo:warning={} build detected, generating placeholder files",
            if is_docsrs { "docs.rs" } else { "nix" }
        );
        let out_dir = PathBuf::from(env::var("OUT_DIR")?);
        let assets_out_dir = out_dir.join("embedded_assets");
        fs::create_dir_all(&assets_out_dir)?;
        for (_, dest_relative) in EMBEDDED_ASSETS {
            let destination = assets_out_dir.join(dest_relative);
            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(destination, b"")?;
        }

        return Ok(());
    }

    println!("cargo:rerun-if-changed=build.rs");

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR")?);
    let workspace_dir = ancestor(&manifest_dir, 3)
        .map(Path::to_path_buf)
        .unwrap_or_else(|| manifest_dir.clone());
    let out_dir = PathBuf::from(env::var("OUT_DIR")?);
    let assets_out_dir = out_dir.join("embedded_assets");
    fs::create_dir_all(&assets_out_dir)?;

    for (relative, dest_relative) in EMBEDDED_ASSETS {
        let source = workspace_dir.join(relative);
        let destination = assets_out_dir.join(dest_relative);
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }

        if !source.exists() {
            // Published crates ship without workspace-root assets, so the
            // `include_bytes!`/`include_str!` call sites must still find a
            // (placeholder) file at the OUT_DIR destination. Without this,
            // `cargo package` verification compiles from the tarball and fails
            // with ENOENT on the missing asset.
            println!("cargo:warning=skipping missing embedded asset `{}`", relative);
            fs::write(&destination, b"")?;
            continue;
        }

        println!("cargo:rerun-if-changed={}", source.display());

        let _copied_bytes = fs::copy(&source, &destination)?;
    }

    Ok(())
}

fn ancestor(path: &Path, count: usize) -> Option<&Path> {
    let mut current = path;
    for _ in 0..count {
        current = current.parent()?;
    }
    Some(current)
}
