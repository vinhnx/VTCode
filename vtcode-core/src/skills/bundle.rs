//! Skill bundle import/export
//!
//! Supports zip-based packaging of skills for distribution, version management,
//! and inline bundle injection. Follows OpenAI Skills API packaging patterns:
//! - Safe zip extraction with path traversal protection
//! - Size limits (50MB compressed, 25MB per file, 500 files max)
//! - Manifest validation after extraction
//! - Versioned storage layout

use anyhow::{Context, Result, bail};
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use tracing::{debug, info, warn};

/// Maximum compressed bundle size (50 MB)
pub const MAX_BUNDLE_SIZE: usize = 50 * 1024 * 1024;
/// Maximum uncompressed file size (25 MB)
pub const MAX_FILE_SIZE: usize = 25 * 1024 * 1024;
/// Maximum file count per bundle
pub const MAX_FILE_COUNT: usize = 500;

/// Result of importing a skill bundle
#[derive(Debug, Clone)]
pub struct ImportedSkillInfo {
    pub name: String,
    pub version: Option<String>,
    pub description: String,
    pub path: PathBuf,
    pub file_count: usize,
    pub total_size: u64,
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
        let mut zip_writer =
            zip::ZipWriter::new(cursor);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);

        add_dir_to_zip(&mut zip_writer, skill_root, skill_root, options)?;
        zip_writer.finish().context("Failed to finalize zip archive")?;
    }

    info!(
        "Exported skill bundle from {}: {} bytes",
        skill_root.display(),
        buf.len()
    );
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

        if path.is_dir() {
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
            let data = fs::read(&path)
                .with_context(|| format!("reading file {}", path.display()))?;
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
        bail!(
            "Bundle size {} bytes exceeds maximum {} bytes",
            zip_bytes.len(),
            MAX_BUNDLE_SIZE
        );
    }

    let temp_dir = tempfile::tempdir().context("Failed to create temp directory")?;
    let temp_path = temp_dir.path();

    extract_zip_safely(zip_bytes, temp_path)?;

    let skill_md_path = find_skill_md(temp_path)?;
    let skill_root = skill_md_path
        .parent()
        .unwrap_or(temp_path);

    validate_extracted_bundle(skill_root)?;

    let (manifest, _instructions) = crate::skills::manifest::parse_skill_content(
        &fs::read_to_string(&skill_md_path).context("Failed to read extracted SKILL.md")?,
    )?;

    let version = manifest
        .version
        .clone()
        .unwrap_or_else(|| "0.0.0".to_string());

    let dest_dir = dest_store.join(&manifest.name).join(&version);
    if dest_dir.exists() {
        warn!(
            "Overwriting existing skill version at {}",
            dest_dir.display()
        );
        fs::remove_dir_all(&dest_dir).context("Failed to remove existing version")?;
    }
    fs::create_dir_all(&dest_dir).context("Failed to create destination directory")?;

    let (file_count, total_size) = copy_dir_recursive(skill_root, &dest_dir)?;

    info!(
        "Imported skill '{}' v{} ({} files, {} bytes) to {}",
        manifest.name,
        version,
        file_count,
        total_size,
        dest_dir.display()
    );

    update_skill_index(dest_store, &manifest.name, &version)?;

    Ok(ImportedSkillInfo {
        name: manifest.name,
        version: Some(version),
        description: manifest.description,
        path: dest_dir,
        file_count,
        total_size,
    })
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
        bail!(
            "Zip contains {} entries, exceeds maximum {}",
            archive.len(),
            MAX_FILE_COUNT
        );
    }

    for i in 0..archive.len() {
        let mut file = archive
            .by_index(i)
            .with_context(|| format!("reading zip entry {i}"))?;
        let raw_name = file.name().to_string();

        if raw_name.contains("..") {
            bail!("Path traversal detected in zip entry: {raw_name}");
        }

        let out_path = dest.join(&raw_name);

        if !out_path.starts_with(dest) {
            bail!(
                "Zip entry escapes destination: {}",
                out_path.display()
            );
        }

        if file.is_dir() {
            fs::create_dir_all(&out_path)
                .with_context(|| format!("creating dir {}", out_path.display()))?;
        } else {
            if file.size() > MAX_FILE_SIZE as u64 {
                bail!(
                    "Zip entry '{}' ({} bytes) exceeds maximum {} bytes",
                    raw_name,
                    file.size(),
                    MAX_FILE_SIZE
                );
            }

            if let Some(parent) = out_path.parent() {
                fs::create_dir_all(parent)?;
            }

            let mut out_file = fs::File::create(&out_path)
                .with_context(|| format!("creating file {}", out_path.display()))?;
            std::io::copy(&mut file, &mut out_file)
                .with_context(|| format!("writing file {}", out_path.display()))?;
        }
    }

    Ok(())
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
        if entry.path().is_dir() {
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
        bail!(
            "Bundle contains {file_count} files, exceeds maximum {MAX_FILE_COUNT}"
        );
    }

    Ok(())
}

fn validate_dir_recursive(
    root: &Path,
    dir: &Path,
    file_count: &mut u64,
    total_size: &mut u64,
) -> Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();

        if path.is_symlink() {
            bail!("Symlinks not allowed in skill bundles: {}", path.display());
        }

        let canonical = path.canonicalize().unwrap_or_else(|_| path.clone());
        let root_canonical = root
            .canonicalize()
            .unwrap_or_else(|_| root.to_path_buf());
        if !canonical.starts_with(&root_canonical) {
            bail!(
                "Path traversal detected: {} escapes bundle root",
                path.display()
            );
        }

        if path.is_dir() {
            validate_dir_recursive(root, &path, file_count, total_size)?;
        } else {
            *file_count += 1;
            let size = entry.metadata()?.len();
            if size > MAX_FILE_SIZE as u64 {
                bail!(
                    "File {} ({size} bytes) exceeds maximum {MAX_FILE_SIZE} bytes",
                    path.display(),
                );
            }
            *total_size += size;
        }
    }
    Ok(())
}

/// Copy directory recursively, returning (file_count, total_bytes)
fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<(usize, u64)> {
    let mut count = 0usize;
    let mut size = 0u64;

    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let file_name = entry.file_name();
        let dst_path = dst.join(&file_name);

        if src_path.is_dir() {
            fs::create_dir_all(&dst_path)?;
            let (c, s) = copy_dir_recursive(&src_path, &dst_path)?;
            count += c;
            size += s;
        } else {
            fs::copy(&src_path, &dst_path)?;
            count += 1;
            size += entry.metadata()?.len();
        }
    }

    Ok((count, size))
}

/// Skill store index for tracking versions
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
pub struct SkillStoreIndex {
    pub skills: HashMap<String, SkillVersionIndex>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SkillVersionIndex {
    pub latest_version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_version: Option<String>,
    pub versions: Vec<String>,
}

/// Update the skill index after import
fn update_skill_index(store_path: &Path, skill_name: &str, version: &str) -> Result<()> {
    let index_path = store_path.join("index.json");

    let mut index: SkillStoreIndex = if index_path.exists() {
        let content = fs::read_to_string(&index_path)?;
        serde_json::from_str(&content).unwrap_or_default()
    } else {
        SkillStoreIndex::default()
    };

    let entry = index
        .skills
        .entry(skill_name.to_string())
        .or_insert_with(|| SkillVersionIndex {
            latest_version: version.to_string(),
            default_version: None,
            versions: Vec::new(),
        });

    if !entry.versions.contains(&version.to_string()) {
        entry.versions.push(version.to_string());
    }
    entry.latest_version = version.to_string();

    fs::create_dir_all(store_path)?;
    fs::write(&index_path, serde_json::to_string_pretty(&index)?)?;

    debug!("Updated skill index at {}", index_path.display());
    Ok(())
}

/// Load the skill store index
pub fn load_skill_index(store_path: &Path) -> Result<SkillStoreIndex> {
    let index_path = store_path.join("index.json");
    if !index_path.exists() {
        return Ok(SkillStoreIndex::default());
    }
    let content = fs::read_to_string(&index_path)?;
    serde_json::from_str(&content).context("Failed to parse skill store index")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_skill_store_index_default() {
        let index = SkillStoreIndex::default();
        assert!(index.skills.is_empty());
    }

    #[test]
    fn test_skill_store_index_roundtrip() {
        let mut index = SkillStoreIndex::default();
        index.skills.insert(
            "test-skill".to_string(),
            SkillVersionIndex {
                latest_version: "1.0.0".to_string(),
                default_version: Some("1.0.0".to_string()),
                versions: vec!["0.9.0".to_string(), "1.0.0".to_string()],
            },
        );
        let json = serde_json::to_string(&index).expect("serialize");
        let parsed: SkillStoreIndex = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed.skills["test-skill"].latest_version, "1.0.0");
        assert_eq!(parsed.skills["test-skill"].versions.len(), 2);
    }

    #[test]
    fn test_bundle_size_limit() {
        let oversized = vec![0u8; MAX_BUNDLE_SIZE + 1];
        let temp = tempfile::tempdir().expect("tempdir");
        let result = import_skill_bundle(&oversized, temp.path());
        assert!(result.is_err());
        assert!(
            result
                .expect_err("should fail")
                .to_string()
                .contains("exceeds maximum")
        );
    }
}
