use std::fs::{self, File};
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::tools::ast_grep_binary::{alias_ast_grep_binary_name, canonical_ast_grep_binary_name};
use anyhow::{Context, Result, anyhow, bail};
use flate2::read::GzDecoder;
use tar::Archive;
use tempfile::TempDir;
use zip::ZipArchive;

use super::state::InstallPaths;

pub(super) fn install_archive(
    paths: &InstallPaths,
    archive_name: &str,
    archive_bytes: &[u8],
) -> Result<()> {
    fs::create_dir_all(&paths.bin_dir)
        .with_context(|| format!("Failed to create {}", paths.bin_dir.display()))?;

    let temp_dir =
        TempDir::new().context("Failed to create temp directory for ast-grep install")?;
    let extract_dir = temp_dir.path().join("extract");
    fs::create_dir_all(&extract_dir)
        .with_context(|| format!("Failed to create {}", extract_dir.display()))?;

    extract_archive(archive_name, archive_bytes, &extract_dir)?;
    let extracted_binary = find_extracted_binary(&extract_dir)?;

    fs::copy(&extracted_binary, &paths.binary_path).with_context(|| {
        format!(
            "Failed to install ast-grep to {}",
            paths.binary_path.display()
        )
    })?;
    set_executable_permissions(&paths.binary_path)?;

    if cfg!(target_os = "linux") {
        let stale_alias = paths.bin_dir.join("sg");
        if stale_alias.exists() {
            let _ = fs::remove_file(stale_alias);
        }
    } else if let Some(alias_path) = &paths.alias_path {
        fs::copy(&paths.binary_path, alias_path).with_context(|| {
            format!(
                "Failed to install ast-grep alias to {}",
                alias_path.display()
            )
        })?;
        set_executable_permissions(alias_path)?;
    }

    Ok(())
}

pub(super) fn ast_grep_version(binary: &Path) -> Result<String> {
    let output = Command::new(binary)
        .arg("--version")
        .output()
        .with_context(|| format!("Failed to run {}", binary.display()))?;
    if !output.status.success() {
        bail!(
            "{} --version exited with status {}",
            binary.display(),
            output.status
        );
    }

    let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if version.is_empty() {
        bail!("{} --version returned empty output", binary.display());
    }
    Ok(version)
}

fn extract_archive(archive_name: &str, archive_bytes: &[u8], destination: &Path) -> Result<()> {
    if archive_name.ends_with(".tar.gz") {
        let decoder = GzDecoder::new(Cursor::new(archive_bytes));
        let mut archive = Archive::new(decoder);
        archive
            .unpack(destination)
            .with_context(|| format!("Failed to unpack {}", archive_name))?;
        return Ok(());
    }

    if archive_name.ends_with(".zip") {
        let cursor = Cursor::new(archive_bytes);
        let mut archive =
            ZipArchive::new(cursor).with_context(|| format!("Failed to open {}", archive_name))?;
        for index in 0..archive.len() {
            let mut file = archive
                .by_index(index)
                .with_context(|| format!("Failed to read {} entry {}", archive_name, index))?;
            let outpath = destination.join(file.mangled_name());

            if file.is_dir() {
                fs::create_dir_all(&outpath)
                    .with_context(|| format!("Failed to create {}", outpath.display()))?;
                continue;
            }

            if let Some(parent) = outpath.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("Failed to create {}", parent.display()))?;
            }

            let mut outfile = File::create(&outpath)
                .with_context(|| format!("Failed to create {}", outpath.display()))?;
            std::io::copy(&mut file, &mut outfile)
                .with_context(|| format!("Failed to extract {}", outpath.display()))?;
        }
        return Ok(());
    }

    bail!("Unsupported ast-grep archive format: {}", archive_name);
}

fn find_extracted_binary(root: &Path) -> Result<PathBuf> {
    let alias_name = alias_ast_grep_binary_name();
    walkdir::WalkDir::new(root)
        .into_iter()
        .filter_map(std::result::Result::ok)
        .filter(|entry| entry.file_type().is_file())
        .find_map(|entry| {
            let name = entry.file_name().to_string_lossy();
            if name == canonical_ast_grep_binary_name()
                || alias_name.is_some_and(|alias| name == alias)
            {
                Some(entry.into_path())
            } else {
                None
            }
        })
        .ok_or_else(|| anyhow!("ast-grep binary not found in release archive"))
}

fn set_executable_permissions(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let mut permissions = fs::metadata(path)
            .with_context(|| format!("Failed to inspect {}", path.display()))?
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions)
            .with_context(|| format!("Failed to update permissions for {}", path.display()))?;
    }

    #[cfg(not(unix))]
    {
        let _ = path;
    }

    Ok(())
}
