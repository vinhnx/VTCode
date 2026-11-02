use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use std::fs::{self, File};
use std::io;
use vtcode_commons::safe_archive_relative_path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ArchiveKind {
    TarGz,
    Zip,
}

pub(super) fn extract_binary(
    archive_path: &Path,
    archive_kind: ArchiveKind,
    destination: &Path,
    binary_name: &str,
) -> Result<PathBuf> {
    fs::create_dir_all(destination)
        .with_context(|| format!("failed to create extraction directory {}", destination.display()))?;
    let output_path = destination.join(format!(".{binary_name}.staged"));
    let mut matched = false;

    match archive_kind {
        ArchiveKind::TarGz => {
            let archive = File::open(archive_path)
                .with_context(|| format!("failed to open tar-gzip archive {}", archive_path.display()))?;
            let decoder = flate2::read::GzDecoder::new(archive);
            let mut archive = tar::Archive::new(decoder);
            for entry in archive.entries().context("failed to read tar-gzip archive entries")? {
                let mut entry = entry.context("failed to read tar-gzip archive entry")?;
                let path = safe_archive_relative_path(&entry.path().context("failed to read tar archive path")?)?;
                let entry_type = entry.header().entry_type();
                if entry_type.is_symlink() || entry_type.is_hard_link() {
                    bail!("unsafe archive link: {}", path.display());
                }
                if !entry_type.is_file() || path.file_name().and_then(|name| name.to_str()) != Some(binary_name) {
                    continue;
                }
                if matched {
                    bail!("archive contains multiple {binary_name} binaries");
                }
                let mut output =
                    File::create(&output_path).with_context(|| format!("failed to stage {binary_name}"))?;
                io::copy(&mut entry, &mut output).context("failed to extract VT Code binary")?;
                set_executable_permissions(&output_path, entry.header().mode().unwrap_or(0o755))?;
                matched = true;
            }
        }
        ArchiveKind::Zip => {
            let archive = File::open(archive_path)
                .with_context(|| format!("failed to open zip archive {}", archive_path.display()))?;
            let mut archive = zip::ZipArchive::new(archive).context("failed to read zip archive")?;
            for index in 0..archive.len() {
                let mut entry = archive.by_index(index).context("failed to read zip archive entry")?;
                let path = entry
                    .enclosed_name()
                    .context("unsafe archive path: zip entry escapes extraction directory")?;
                let path = safe_archive_relative_path(&path)?;
                if is_zip_symlink(&entry) {
                    bail!("unsafe archive link: {}", path.display());
                }
                if entry.is_dir() || path.file_name().and_then(|name| name.to_str()) != Some(binary_name) {
                    continue;
                }
                if matched {
                    bail!("archive contains multiple {binary_name} binaries");
                }
                let mut output =
                    File::create(&output_path).with_context(|| format!("failed to stage {binary_name}"))?;
                io::copy(&mut entry, &mut output).context("failed to extract VT Code binary")?;
                set_executable_permissions(&output_path, zip_mode(&entry).unwrap_or(0o755))?;
                matched = true;
            }
        }
    }

    if !matched {
        bail!("could not find {binary_name} in update archive");
    }
    Ok(output_path)
}

#[cfg(unix)]
fn set_executable_permissions(path: &Path, mode: u32) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let executable_mode = if mode & 0o111 == 0 { 0o755 } else { mode & 0o777 };
    fs::set_permissions(path, fs::Permissions::from_mode(executable_mode))
        .with_context(|| format!("failed to set executable permissions on {}", path.display()))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_executable_permissions(_path: &Path, _mode: u32) -> Result<()> {
    Ok(())
}

#[cfg(unix)]
fn zip_mode(entry: &zip::read::ZipFile<'_, File>) -> Option<u32> {
    entry.unix_mode()
}

#[cfg(not(unix))]
fn zip_mode(_entry: &zip::read::ZipFile<'_, File>) -> Option<u32> {
    None
}

#[cfg(unix)]
fn is_zip_symlink(entry: &zip::read::ZipFile<'_, File>) -> bool {
    entry.unix_mode().is_some_and(|mode| mode & 0o170000 == 0o120000)
}

#[cfg(not(unix))]
fn is_zip_symlink(_entry: &zip::read::ZipFile<'_, File>) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::{Compression, write::GzEncoder};
    use std::fs;
    use std::io::Write;
    use tar::{Builder, Header};
    use tempfile::tempdir;
    use zip::{ZipWriter, write::SimpleFileOptions};

    fn write_tar_gz(path: &Path, entry_name: &str, contents: &[u8]) {
        let file = File::create(path).expect("archive");
        let encoder = GzEncoder::new(file, Compression::default());
        let mut builder = Builder::new(encoder);
        let mut header = Header::new_gnu();
        header.set_path(entry_name).expect("path");
        header.set_size(contents.len() as u64);
        header.set_mode(0o755);
        header.set_cksum();
        builder.append(&header, contents).expect("entry");
        builder.into_inner().expect("tar").finish().expect("gzip");
    }

    fn write_zip(path: &Path, entry_name: &str, contents: &[u8]) {
        let file = File::create(path).expect("archive");
        let mut writer = ZipWriter::new(file);
        writer
            .start_file(entry_name, SimpleFileOptions::default().unix_permissions(0o755))
            .expect("entry");
        writer.write_all(contents).expect("contents");
        writer.finish().expect("zip");
    }

    #[test]
    fn extracts_tar_gz_binary_and_preserves_executable_permission() {
        let temp = tempdir().expect("temp");
        let archive = temp.path().join("vtcode.tar.gz");
        write_tar_gz(&archive, "vtcode-0.1.0/vtcode", b"binary");

        let extracted = extract_binary(&archive, ArchiveKind::TarGz, temp.path(), "vtcode").expect("extract");

        assert_eq!(fs::read(&extracted).expect("read"), b"binary");
        #[cfg(unix)]
        assert_eq!(fs::metadata(extracted).expect("metadata").permissions().mode() & 0o111, 0o111);
    }

    #[test]
    fn extracts_zip_binary() {
        let temp = tempdir().expect("temp");
        let archive = temp.path().join("vtcode.zip");
        write_zip(&archive, "nested/vtcode.exe", b"binary");

        let extracted = extract_binary(&archive, ArchiveKind::Zip, temp.path(), "vtcode.exe").expect("extract");

        assert_eq!(fs::read(extracted).expect("read"), b"binary");
    }

    #[test]
    fn rejects_archive_path_traversal() {
        let temp = tempdir().expect("temp");
        let archive = temp.path().join("vtcode.zip");
        write_zip(&archive, "../../vtcode", b"binary");

        let error = extract_binary(&archive, ArchiveKind::Zip, temp.path(), "vtcode").expect_err("traversal");

        assert!(error.to_string().contains("unsafe archive path"));
        assert!(!temp.path().parent().expect("parent").join("vtcode").exists());
    }

    #[test]
    fn rejects_missing_or_ambiguous_binary() {
        let temp = tempdir().expect("temp");
        let archive = temp.path().join("vtcode.tar.gz");
        write_tar_gz(&archive, "nested/not-vtcode", b"binary");

        let error = extract_binary(&archive, ArchiveKind::TarGz, temp.path(), "vtcode").expect_err("missing");

        assert!(error.to_string().contains("could not find"));
    }

    #[test]
    fn rejects_multiple_matching_binaries() {
        let temp = tempdir().expect("temp");
        let archive = temp.path().join("vtcode.zip");
        let file = File::create(&archive).expect("archive");
        let mut writer = ZipWriter::new(file);
        for path in ["first/vtcode", "second/vtcode"] {
            writer
                .start_file(path, SimpleFileOptions::default().unix_permissions(0o755))
                .expect("entry");
            writer.write_all(b"binary").expect("contents");
        }
        writer.finish().expect("zip");

        let error = extract_binary(&archive, ArchiveKind::Zip, temp.path(), "vtcode").expect_err("ambiguous");

        assert!(error.to_string().contains("multiple vtcode binaries"));
    }

    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
}
