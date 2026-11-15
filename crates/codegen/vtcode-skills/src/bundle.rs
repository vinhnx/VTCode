//! Skill bundle import/export
//!
//! Supports zip-based packaging of skills for distribution, version management,
//! and inline bundle injection. Follows OpenAI Skills API packaging patterns:
//! - Safe zip extraction with path traversal protection
//! - Size limits (50MB compressed, 25MB per file, 200MB total uncompressed, 500 files max)
//! - Manifest validation after extraction
//! - Versioned storage layout

use anyhow::{Context, Result, bail};
use hashbrown::HashMap;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use tracing::{debug, info, warn};
use vtcode_commons::safe_archive_relative_path;

mod recovery;
use vtcode_commons::{
    VtCodePaths, canonicalize, fs::rename_synced, fs::sync_dir, validate_no_escaping_symlink_ancestors,
};

/// Maximum compressed bundle size (50 MB)
const MAX_BUNDLE_SIZE: usize = 50 * 1024 * 1024;
/// Maximum uncompressed file size (25 MB)
const MAX_FILE_SIZE: usize = 25 * 1024 * 1024;
/// Maximum total uncompressed size (200 MB)
///
/// `MAX_BUNDLE_SIZE` bounds the *compressed* input, but DEFLATE can expand by
/// ~1000x, so an uncompressed budget must be enforced while writing. Without it
/// a single small bundle can fill the disk (zip bomb).
const MAX_TOTAL_UNCOMPRESSED_SIZE: u64 = 200 * 1024 * 1024;
/// Maximum file count per bundle
const MAX_FILE_COUNT: usize = 500;

/// Whether `total` uncompressed bytes fit within the bundle budget.
///
/// Shared by extraction (incremental, per entry) and post-extraction
/// validation so the two enforcement points cannot drift.
fn within_total_budget(total: u64) -> bool {
    total <= MAX_TOTAL_UNCOMPRESSED_SIZE
}

/// Result of importing a skill bundle
#[derive(Debug, Clone)]
pub struct ImportedSkillInfo {
    name: String,
    version: Option<String>,
    description: String,
    path: PathBuf,
    file_count: usize,
    total_size: u64,
}

/// Export a skill directory to a zip bundle (bytes)
///
/// Walks the skill directory and creates a zip archive.
/// The archive preserves the directory structure with the skill name as root.
pub fn export_skill_bundle(skill_root: &Path) -> Result<Vec<u8>> {
    let skill_md = skill_root.join("SKILL.md");
    if !skill_md.exists() {
        bail!("No SKILL.md found at {}", skill_root.display());
    }

    let mut buf = Vec::new();
    {
        let cursor = std::io::Cursor::new(&mut buf);
        let mut zip_writer = zip::ZipWriter::new(cursor);
        let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

        add_dir_to_zip(&mut zip_writer, skill_root, skill_root, options)?;
        zip_writer.finish().context("Failed to finalize zip archive")?;
    }

    info!("Exported skill bundle from {}: {} bytes", skill_root.display(), buf.len());
    Ok(buf)
}

fn add_dir_to_zip<W: Write + std::io::Seek>(
    zip_writer: &mut zip::ZipWriter<W>,
    root: &Path,
    dir: &Path,
    options: zip::write::SimpleFileOptions,
) -> Result<()> {
    for entry in fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        let rel = path
            .strip_prefix(root)
            .with_context(|| format!("stripping prefix from {}", path.display()))?;

        // Trust the dirent type rather than stat-ing each entry again.
        if vtcode_commons::walk::entry_is_dir(&entry) {
            let dir_name = format!("{}/", rel.to_string_lossy());
            zip_writer
                .add_directory(&dir_name, options)
                .with_context(|| format!("adding directory {dir_name}"))?;
            add_dir_to_zip(zip_writer, root, &path, options)?;
        } else {
            let name = rel.to_string_lossy().to_string();
            zip_writer
                .start_file(&name, options)
                .with_context(|| format!("starting file {name}"))?;
            let data = fs::read(&path).with_context(|| format!("reading file {}", path.display()))?;
            zip_writer.write_all(&data)?;
        }
    }
    Ok(())
}

/// Import a skill bundle (zip bytes) into the skill store
///
/// Extracts the zip, validates SKILL.md, and moves to versioned storage.
/// Storage layout: `<dest_store>/<skill-name>/<version>/...`
pub fn import_skill_bundle(zip_bytes: &[u8], dest_store: &Path) -> Result<ImportedSkillInfo> {
    if zip_bytes.len() > MAX_BUNDLE_SIZE {
        bail!("Bundle size {} bytes exceeds maximum {} bytes", zip_bytes.len(), MAX_BUNDLE_SIZE);
    }

    let temp_dir = tempfile::tempdir().context("Failed to create temp directory")?;
    let temp_path = temp_dir.path();

    extract_zip_safely(zip_bytes, temp_path)?;

    let skill_md_path = find_skill_md(temp_path)?;
    let skill_root = skill_md_path.parent().unwrap_or(temp_path);

    validate_extracted_bundle(skill_root)?;

    let (manifest, _instructions) = crate::manifest::parse_skill_content(
        &fs::read_to_string(&skill_md_path).context("Failed to read extracted SKILL.md")?,
    )?;

    if let Some(value) = manifest.metadata.as_ref().and_then(|metadata| metadata.get("version")) {
        let _ = value.as_str().context("skill metadata.version must be a string")?;
    }
    let version = crate::versioning::effective_version(&manifest).unwrap_or("0.0.0");
    if !safe_version(version) {
        bail!("skill metadata.version must be a safe version identifier");
    }

    let dest_dir = dest_store.join(&manifest.name).join(version);
    let (file_count, total_size) = VtCodePaths::with_private_file_lock(dest_store.join("index.json"), || {
        recovery::recover_abandoned_imports(dest_store)?;
        let index = load_skill_index_unlocked(dest_store)?;
        validate_no_escaping_symlink_ancestors(&dest_dir, true).context("validate skill destination")?;
        let staging = tempfile::Builder::new().prefix(".skill-import-").tempdir_in(dest_store)?;
        let staged = staging.path().join("new");
        fs::create_dir(&staged).context("create staged skill directory")?;
        let counts = copy_dir_recursive(skill_root, &staged)?;
        let parent = dest_dir.parent().context("skill destination has no parent")?;
        fs::create_dir_all(parent).context("create skill directory")?;
        let index_json = updated_skill_index(index, &manifest.name, version)?;
        let had_previous = recovery::directory_exists(&dest_dir)?;
        recovery::record_transaction(staging.path(), dest_store, &manifest.name, version, had_previous, &index_json)?;
        publish_staged_skill(staging, &dest_dir, || publish_skill_index(dest_store, &index_json))?;
        Ok(counts)
    })?;

    info!(
        "Imported skill '{}' v{} ({} files, {} bytes) to {}",
        manifest.name,
        version,
        file_count,
        total_size,
        dest_dir.display()
    );

    let version = version.to_owned();

    Ok(ImportedSkillInfo {
        name: manifest.name,
        version: Some(version),
        description: manifest.description,
        path: dest_dir,
        file_count,
        total_size,
    })
}

fn safe_version(version: &str) -> bool {
    !(version.is_empty()
        || version.len() > 128
        || version == "."
        || version == ".."
        || !version
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_' | b'+')))
}

fn publish_staged_skill(
    staging: tempfile::TempDir,
    dest: &Path,
    publish_index: impl FnOnce() -> Result<()>,
) -> Result<()> {
    let staged = staging.path().join("new");
    let backup = staging.path().join("previous");
    let had_previous = match fs::symlink_metadata(dest) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
            warn!("Overwriting existing skill version at {}", dest.display());
            rename_synced(dest, &backup).context("back up existing skill version")?;
            true
        }
        Ok(_) => bail!("skill destination is not a regular directory: {}", dest.display()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(error) => return Err(error).context("inspect existing skill version"),
    };

    let result = rename_synced(&staged, dest)
        .context("publish staged skill version")
        .and_then(|()| publish_index());
    if let Err(error) = result {
        let rollback = (|| -> Result<()> {
            if !staged.exists() {
                fs::remove_dir_all(dest).context("remove failed skill version")?;
            }
            if had_previous {
                rename_synced(&backup, dest).context("restore previous skill version")?;
            }
            Ok(())
        })();
        if let Err(rollback_error) = rollback {
            let retained = staging.keep();
            return Err(error.context(format!(
                "rollback failed: {rollback_error:#}; recovery files retained at {}",
                retained.display()
            )));
        }
        return Err(error);
    }
    Ok(())
}

/// Import a skill from base64-encoded zip (inline bundle)
pub fn import_inline_bundle(base64_data: &str, dest_store: &Path) -> Result<ImportedSkillInfo> {
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(base64_data)
        .context("Failed to decode base64 bundle")?;
    import_skill_bundle(&bytes, dest_store)
}

/// Safely extract a zip archive with path-traversal and size protections
fn extract_zip_safely(zip_bytes: &[u8], dest: &Path) -> Result<()> {
    let cursor = std::io::Cursor::new(zip_bytes);
    let mut archive = zip::ZipArchive::new(cursor).context("Failed to open zip archive")?;

    if archive.len() > MAX_FILE_COUNT {
        bail!("Zip contains {} entries, exceeds maximum {}", archive.len(), MAX_FILE_COUNT);
    }

    let mut total_uncompressed: u64 = 0;

    for i in 0..archive.len() {
        let mut file = archive.by_index(i).with_context(|| format!("reading zip entry {i}"))?;
        let raw_name = file
            .name()
            .with_context(|| format!("zip entry {i} has a non-UTF-8 name"))?
            .into_owned();

        let safe_name = safe_archive_relative_path(Path::new(&raw_name))
            .with_context(|| format!("Path traversal detected in zip entry: {raw_name}"))?;
        if safe_name.as_os_str().is_empty() {
            continue;
        }

        let out_path = dest.join(safe_name);

        if file.is_dir() {
            fs::create_dir_all(&out_path).with_context(|| format!("creating dir {}", out_path.display()))?;
        } else {
            if file.size() > MAX_FILE_SIZE as u64 {
                bail!("Zip entry '{}' ({} bytes) exceeds maximum {} bytes", raw_name, file.size(), MAX_FILE_SIZE);
            }

            if let Some(parent) = out_path.parent() {
                fs::create_dir_all(parent)?;
            }

            let mut out_file =
                fs::File::create(&out_path).with_context(|| format!("creating file {}", out_path.display()))?;
            // Bound by bytes actually written: the archive's declared `file.size()`
            // cannot be trusted, and the cumulative budget stops zip bombs.
            let written = copy_bounded(&mut file, &mut out_file, MAX_FILE_SIZE as u64)
                .with_context(|| format!("writing file {}", out_path.display()))?;
            total_uncompressed += written;
            if !within_total_budget(total_uncompressed) {
                bail!(
                    "Zip uncompressed size {total_uncompressed} bytes exceeds maximum {MAX_TOTAL_UNCOMPRESSED_SIZE} bytes"
                );
            }
        }
    }

    Ok(())
}

/// Copy at most `limit` bytes from `reader` into `writer`.
///
/// Fails rather than silently truncating when the reader yields more than
/// `limit`, so untrusted input whose declared size lies is still capped by the
/// bytes actually written.
fn copy_bounded<R: Read, W: Write>(reader: &mut R, writer: &mut W, limit: u64) -> Result<u64> {
    let mut bounded = reader.take(limit.saturating_add(1));
    let written = std::io::copy(&mut bounded, writer)?;
    if written > limit {
        bail!("entry exceeds maximum {limit} bytes");
    }
    Ok(written)
}

/// Find SKILL.md in extracted directory (handles nested structures)
fn find_skill_md(dir: &Path) -> Result<PathBuf> {
    let direct = dir.join("SKILL.md");
    if direct.exists() {
        return Ok(direct);
    }
    let direct_lower = dir.join("skill.md");
    if direct_lower.exists() {
        return Ok(direct_lower);
    }

    for entry in fs::read_dir(dir).context("Failed to read extracted directory")? {
        let entry = entry?;
        if vtcode_commons::walk::entry_is_dir(&entry) {
            let nested = entry.path().join("SKILL.md");
            if nested.exists() {
                return Ok(nested);
            }
            let nested_lower = entry.path().join("skill.md");
            if nested_lower.exists() {
                return Ok(nested_lower);
            }
        }
    }

    bail!("No SKILL.md found in extracted bundle")
}

/// Validate extracted bundle for security
fn validate_extracted_bundle(skill_root: &Path) -> Result<()> {
    let mut file_count = 0u64;
    let mut total_size = 0u64;

    validate_dir_recursive(skill_root, skill_root, &mut file_count, &mut total_size)?;

    if file_count > MAX_FILE_COUNT as u64 {
        bail!("Bundle contains {file_count} files, exceeds maximum {MAX_FILE_COUNT}");
    }

    if !within_total_budget(total_size) {
        bail!("Bundle uncompressed size {total_size} bytes exceeds maximum {MAX_TOTAL_UNCOMPRESSED_SIZE} bytes");
    }

    Ok(())
}

fn validate_dir_recursive(root: &Path, dir: &Path, file_count: &mut u64, total_size: &mut u64) -> Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();

        if path.is_symlink() {
            bail!("Symlinks not allowed in skill bundles: {}", path.display());
        }

        let canonical = canonicalize(&path).unwrap_or_else(|_| path.clone());
        let root_canonical = canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
        if !canonical.starts_with(&root_canonical) {
            bail!("Path traversal detected: {} escapes bundle root", path.display());
        }

        if path.is_dir() {
            validate_dir_recursive(root, &path, file_count, total_size)?;
        } else {
            *file_count += 1;
            let size = entry.metadata()?.len();
            if size > MAX_FILE_SIZE as u64 {
                bail!("File {} ({size} bytes) exceeds maximum {MAX_FILE_SIZE} bytes", path.display(),);
            }
            *total_size += size;
        }
    }
    Ok(())
}

/// Copy directory recursively, returning (file_count, total_bytes).
///
/// Each copied file is synced before the destination directory is synced, so
/// a staged skill tree is crash-durable leaf-up once this returns.
fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<(usize, u64)> {
    let mut count = 0usize;
    let mut size = 0u64;

    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let file_name = entry.file_name();
        let dst_path = dst.join(&file_name);

        if vtcode_commons::walk::entry_is_dir(&entry) {
            fs::create_dir_all(&dst_path)?;
            let (c, s) = copy_dir_recursive(&src_path, &dst_path)?;
            count += c;
            size += s;
        } else {
            fs::copy(&src_path, &dst_path)
                .with_context(|| format!("failed to copy {} to {}", src_path.display(), dst_path.display()))?;
            fs::File::open(&dst_path)
                .with_context(|| format!("failed to open copied file {}", dst_path.display()))?
                .sync_all()
                .with_context(|| format!("failed to sync copied file {}", dst_path.display()))?;
            count += 1;
            size += entry
                .metadata()
                .with_context(|| format!("failed to stat {}", src_path.display()))?
                .len();
        }
    }

    sync_dir(dst).with_context(|| format!("failed to sync copied directory {}", dst.display()))?;

    Ok((count, size))
}

/// Skill store index for tracking versions
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
pub struct SkillStoreIndex {
    skills: HashMap<String, SkillVersionIndex>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SkillVersionIndex {
    latest_version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    default_version: Option<String>,
    versions: Vec<String>,
}

/// Update the skill index after import
fn updated_skill_index(mut index: SkillStoreIndex, skill_name: &str, version: &str) -> Result<Vec<u8>> {
    let entry = index.skills.entry(skill_name.to_string()).or_insert_with(|| SkillVersionIndex {
        latest_version: version.to_string(),
        default_version: None,
        versions: Vec::new(),
    });

    if !entry.versions.contains(&version.to_string()) {
        entry.versions.push(version.to_string());
    }
    entry.latest_version = version.to_string();

    serde_json::to_vec_pretty(&index).with_context(|| format!("failed to serialize index for {skill_name}"))
}

fn publish_skill_index(store_path: &Path, index_json: &[u8]) -> Result<()> {
    let index_path = store_path.join("index.json");
    VtCodePaths::write_private_file_atomic(&index_path, index_json)
        .with_context(|| format!("failed to write index at {}", index_path.display()))?;
    sync_dir(store_path).with_context(|| format!("failed to sync store at {}", store_path.display()))?;

    debug!("Updated skill index at {}", index_path.display());
    Ok(())
}

/// Load the skill store index
pub fn load_skill_index(store_path: &Path) -> Result<SkillStoreIndex> {
    match fs::metadata(store_path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(SkillStoreIndex::default()),
        Err(error) => return Err(error).context("Failed to inspect skill store"),
        Ok(_) => {}
    }
    VtCodePaths::with_private_file_lock(store_path.join("index.json"), || {
        recovery::recover_abandoned_imports(store_path)?;
        load_skill_index_unlocked(store_path)
    })
}

fn load_skill_index_unlocked(store_path: &Path) -> Result<SkillStoreIndex> {
    let Some(content) = recovery::read_index(store_path)? else {
        return Ok(SkillStoreIndex::default());
    };
    serde_json::from_slice(&content).context("Failed to parse skill store index")
}

#[cfg(test)]
mod tests;
