use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const EMBEDDED_ASSETS: &[(&str, &str)] =
    &[("docs/modules/vtcode_docs_map.md", "docs/vtcode_docs_map.md")];

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let is_docsrs = env::var_os("DOCS_RS").is_some();
    let is_nix_build = env::var_os("NIX_BUILD_TOP").is_some();

    if is_docsrs || is_nix_build {
        // When building on docs.rs, generate empty placeholder files to prevent compilation errors
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
    let workspace_dir = manifest_dir
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| manifest_dir.clone());
    let out_dir = PathBuf::from(env::var("OUT_DIR")?);
    let assets_out_dir = out_dir.join("embedded_assets");
    fs::create_dir_all(&assets_out_dir)?;

    let mut resolved_assets = Vec::new();
    for (relative, dest_relative) in EMBEDDED_ASSETS {
        match locate_asset(&manifest_dir, &workspace_dir, relative) {
            Ok(source) => {
                println!("cargo:rerun-if-changed={}", source.display());

                let fallback = fallback_path(&manifest_dir, relative);
                if fallback.exists() && fallback != source {
                    println!("cargo:rerun-if-changed={}", fallback.display());
                }

                resolved_assets.push((Some(source), *dest_relative));
            }
            Err(error) if can_fallback_to_placeholder(&error) => {
                println!(
                    "cargo:warning=using placeholder embedded asset for `{relative}`: {error}"
                );
                resolved_assets.push((None, *dest_relative));
            }
            Err(error) => return Err(error.into()),
        }
    }

    for (source, dest_relative) in resolved_assets {
        let destination = assets_out_dir.join(dest_relative);
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        match source {
            Some(source) => {
                fs::copy(&source, &destination)?;
            }
            None => {
                fs::write(&destination, b"")?;
            }
        }
    }

    Ok(())
}

fn can_fallback_to_placeholder(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::NotFound | io::ErrorKind::PermissionDenied
    )
}

fn locate_asset(manifest_dir: &Path, workspace_dir: &Path, relative: &str) -> io::Result<PathBuf> {
    let workspace_candidate = workspace_dir.join(relative);
    if workspace_candidate.exists() {
        ensure_fallback_in_sync(manifest_dir, relative, &workspace_candidate)?;
        return Ok(workspace_candidate);
    }

    let fallback = fallback_path(manifest_dir, relative);
    if fallback.exists() {
        return Ok(fallback);
    }

    Err(io::Error::new(
        io::ErrorKind::NotFound,
        format!(
            "failed to locate embedded asset `{}` (looked in `{}` and `{}`)",
            relative,
            workspace_candidate.display(),
            fallback.display()
        ),
    ))
}

fn ensure_fallback_in_sync(
    manifest_dir: &Path,
    relative: &str,
    canonical: &Path,
) -> io::Result<()> {
    let fallback = fallback_path(manifest_dir, relative);
    if fallback.exists() {
        let canonical_bytes = fs::read(canonical)?;
        let fallback_bytes = fs::read(&fallback)?;
        if canonical_bytes != fallback_bytes {
            return Err(io::Error::other(format!(
                "embedded asset `{}` is out of sync. Update `{}` to match `{}`",
                relative,
                fallback.display(),
                canonical.display(),
            )));
        }
    }
    Ok(())
}

fn fallback_path(manifest_dir: &Path, relative: &str) -> PathBuf {
    manifest_dir.join("embedded_assets_source").join(relative)
}
