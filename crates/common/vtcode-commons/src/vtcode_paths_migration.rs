//! Rollback-safe migration from the pre-XDG `~/.vtcode` layout.

use std::ffi::OsStr;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, anyhow, bail};
use serde::Serialize;

use super::{VtCodePaths, create_private_new_file, ensure_migration_dir, ensure_private_dir, open_no_follow};

/// Performs one idempotent migration using an immutable path-policy snapshot.
#[derive(Debug, Clone)]
pub struct LegacyMigrator {
    paths: VtCodePaths,
}

impl LegacyMigrator {
    /// Creates a migrator bound to one resolved global path policy.
    pub fn new(paths: VtCodePaths) -> Self {
        Self { paths }
    }

    /// Scans and copies eligible legacy content without modifying its source.
    pub fn run(&self) -> Result<MigrationReport> {
        let marker = self.paths.migration_marker_path();
        let mut report = MigrationReport::default();
        let marker_parent = marker.parent().ok_or_else(|| anyhow!("migration marker has no parent"))?;
        if let Err(error) = ensure_private_dir(marker_parent) {
            report.failures.push(MigrationFailure {
                path: marker_parent.to_path_buf(),
                error: error.to_string(),
            });
            persist_report_best_effort(&self.paths, &report);
            return Ok(report);
        }
        let marker_blocked = match fs::symlink_metadata(&marker) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                report.failures.push(MigrationFailure {
                    path: marker.clone(),
                    error: "migration marker is a symlink; refusing to follow it".to_string(),
                });
                true
            }
            Ok(metadata) if metadata.is_file() => match valid_migration_marker(&marker) {
                Ok(true) => {
                    return Ok(MigrationReport {
                        already_completed: true,
                        ..MigrationReport::default()
                    });
                }
                Ok(false) => {
                    report.failures.push(MigrationFailure {
                        path: marker.clone(),
                        error: "migration marker has invalid contents or permissions".to_string(),
                    });
                    true
                }
                Err(error) => {
                    report.failures.push(MigrationFailure {
                        path: marker.clone(),
                        error: format!("could not validate migration marker: {error}"),
                    });
                    true
                }
            },
            Ok(_) => {
                report.failures.push(MigrationFailure {
                    path: marker.clone(),
                    error: "migration marker is not a regular file".to_string(),
                });
                true
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => false,
            Err(error) => {
                report.failures.push(MigrationFailure {
                    path: marker.clone(),
                    error: format!("could not inspect migration marker: {error}"),
                });
                true
            }
        };

        let legacy_root = self.paths.legacy_home_dir();
        let legacy_root_is_safe = match fs::symlink_metadata(legacy_root) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                report.failures.push(MigrationFailure {
                    path: legacy_root.to_path_buf(),
                    error: "legacy root is a symlink; refusing to traverse it".to_string(),
                });
                false
            }
            Ok(metadata) if !metadata.is_dir() => {
                report.failures.push(MigrationFailure {
                    path: legacy_root.to_path_buf(),
                    error: "legacy root is not a directory".to_string(),
                });
                false
            }
            Ok(_) => true,
            Err(error) if error.kind() == io::ErrorKind::NotFound => true,
            Err(error) => {
                report.failures.push(MigrationFailure {
                    path: legacy_root.to_path_buf(),
                    error: format!("could not inspect legacy root: {error}"),
                });
                false
            }
        };

        if legacy_root_is_safe {
            let mappings = legacy_mappings(&self.paths);
            for mapping in &mappings {
                if mapping.source == mapping.destination {
                    continue;
                }
                if mapping.skip {
                    if fs::symlink_metadata(&mapping.source).is_ok() {
                        report.skipped.push(MigrationSkip {
                            path: mapping.source.clone(),
                            reason: MigrationSkipReason::Excluded,
                        });
                    }
                    continue;
                }
                if let Err(error) = copy_legacy_tree_with_exclusions(
                    &mapping.source,
                    &mapping.destination,
                    &mut report,
                    mapping.excluded_children,
                ) {
                    report.failures.push(MigrationFailure {
                        path: mapping.source.clone(),
                        error: error.to_string(),
                    });
                }
            }
            record_unmapped_entries(legacy_root, &mappings, &mut report);
        }

        if report.has_retryable_failures() || marker_blocked {
            persist_report_best_effort(&self.paths, &report);
            return Ok(report);
        }

        // Persist the complete scan report before publishing the completion
        // marker. If diagnostics cannot be recorded, retry the migration
        // rather than claiming a successful one-time migration.
        if let Err(error) = persist_report(&self.paths, &report) {
            report.failures.push(MigrationFailure {
                path: self.paths.migration_report_path(),
                error: error.to_string(),
            });
            persist_report_best_effort(&self.paths, &report);
            return Ok(report);
        }

        if let Err(error) = write_private_atomic(&marker, b"legacy migration completed\n") {
            if error
                .downcast_ref::<io::Error>()
                .is_some_and(|io_error| io_error.kind() == io::ErrorKind::AlreadyExists)
            {
                if valid_migration_marker(&marker).unwrap_or(false) {
                    report.already_completed = true;
                } else {
                    report.failures.push(MigrationFailure {
                        path: marker,
                        error: "migration marker appeared but is invalid".to_string(),
                    });
                }
                return Ok(report);
            }
            report
                .failures
                .push(MigrationFailure { path: marker, error: error.to_string() });
            persist_report_best_effort(&self.paths, &report);
            return Ok(report);
        }
        report.marker_written = true;
        if let Err(error) = persist_report(&self.paths, &report) {
            report.failures.push(MigrationFailure {
                path: self.paths.migration_report_path(),
                error: error.to_string(),
            });
        }
        Ok(report)
    }
}

/// Outcome of a legacy migration attempt.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize)]
pub struct MigrationReport {
    pub migrated: Vec<MigrationEntry>,
    pub skipped: Vec<MigrationSkip>,
    pub failures: Vec<MigrationFailure>,
    pub marker_written: bool,
    pub already_completed: bool,
}

impl MigrationReport {
    /// Returns whether a later startup should retry the migration.
    pub fn has_retryable_failures(&self) -> bool {
        !self.failures.is_empty()
    }
}

/// A source file copied by migration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MigrationEntry {
    pub source: PathBuf,
    pub destination: PathBuf,
}

/// A legacy item that was deliberately left untouched.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MigrationSkip {
    pub path: PathBuf,
    pub reason: MigrationSkipReason,
}

/// An individual migration failure; remaining entries are still scanned.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MigrationFailure {
    pub path: PathBuf,
    pub error: String,
}

/// Reason a legacy item was not copied.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MigrationSkipReason {
    DestinationExists,
    Symlink,
    SpecialFile,
    Excluded,
    Unmapped,
}

struct LegacyMapping {
    source: PathBuf,
    destination: PathBuf,
    skip: bool,
    excluded_children: &'static [&'static str],
}

fn legacy_mappings(paths: &VtCodePaths) -> Vec<LegacyMapping> {
    let legacy = paths.legacy_home_dir();
    let mut mappings = Vec::with_capacity(50);
    for name in [
        "vtcode.toml",
        "update.toml",
        "config.toml",
        "AGENTS.md",
        "AGENTS.override.md",
        "CLAUDE.md",
        "commands",
        "agents",
        "rules",
        "prompts",
        "tool-policy.json",
        "mcp.json",
        "mcp.toml",
        "mcp-config.json",
        "mcp-config.toml",
        "mcp",
        "auth",
        "output-styles",
        "output_styles",
    ] {
        add_mapping(&mut mappings, legacy, name, paths.config_dir().join(name));
    }
    add_mapping(&mut mappings, legacy, "auth.json", paths.auth_file());
    for name in [
        "plugins",
        "skills",
        "installed-skills",
        "assets",
        "durable-assets",
        "tools",
    ] {
        add_mapping(&mut mappings, legacy, name, paths.data_dir().join(name));
    }
    add_mapping(&mut mappings, legacy, "bin", paths.executable_dir().to_path_buf());
    for name in [
        "projects",
        "sessions",
        "history",
        "memory",
        "agent-memory",
        "audit",
        "logs",
        "scheduler",
        "pods",
        "checkpoints",
        "backups",
    ] {
        add_mapping(&mut mappings, legacy, name, paths.state_dir().join(name));
    }
    // Before the centralized path policy, DotManager kept cache, logs,
    // sessions, and backups directly below the configuration directory. On
    // native platforms that directory is still the current config root, so
    // those files are outside the legacy-home scan above and need explicit
    // compatibility mappings.
    for (name, destination) in [
        ("cache", paths.cache_dir().to_path_buf()),
        ("logs", paths.state_dir().join("logs")),
        ("sessions", paths.state_dir().join("sessions")),
        ("backups", paths.state_dir().join("backups")),
    ] {
        let source = paths.config_dir().join(name);
        if source != legacy.join(name) {
            mappings.push(LegacyMapping {
                source,
                destination,
                skip: false,
                excluded_children: &[],
            });
        }
    }

    for name in [
        "model-cache",
        "prompt-cache",
        "approval-data",
        "ast-grep",
        "ast-grep.lock",
        "web-fetch",
        "large-output",
    ] {
        add_mapping(&mut mappings, legacy, name, paths.cache_dir().join(name));
    }
    // Keep the pre-XDG configuration root ahead of the legacy home cache when
    // both layouts exist; it is the most recent location used by DotManager.
    add_mapping(&mut mappings, legacy, "cache", paths.cache_dir().to_path_buf());
    add_mapping(&mut mappings, legacy, ".cache", paths.cache_dir().to_path_buf());

    mappings.push(LegacyMapping {
        source: legacy.join("state"),
        destination: paths.state_dir().to_path_buf(),
        skip: false,
        // Migration metadata is owned by this protocol. Copying a legacy
        // marker could make an incomplete scan look completed.
        excluded_children: &["migration"],
    });
    mappings.push(LegacyMapping {
        source: legacy.join("tmp"),
        destination: paths.runtime_dir().join("tmp"),
        skip: true,
        excluded_children: &[],
    });
    mappings
}

fn add_mapping(mappings: &mut Vec<LegacyMapping>, legacy: &Path, name: &str, destination: PathBuf) {
    mappings.push(LegacyMapping {
        source: legacy.join(name),
        destination,
        skip: false,
        excluded_children: &[],
    });
}

fn record_unmapped_entries(legacy_root: &Path, mappings: &[LegacyMapping], report: &mut MigrationReport) {
    let entries = match fs::read_dir(legacy_root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return,
        Err(error) => {
            report.failures.push(MigrationFailure {
                path: legacy_root.to_path_buf(),
                error: format!("could not list legacy root: {error}"),
            });
            return;
        }
    };
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                report.failures.push(MigrationFailure {
                    path: legacy_root.to_path_buf(),
                    error: format!("could not inspect legacy entry: {error}"),
                });
                continue;
            }
        };
        if !mappings.iter().any(|mapping| mapping.source == entry.path()) {
            report.skipped.push(MigrationSkip {
                path: entry.path(),
                reason: MigrationSkipReason::Unmapped,
            });
        }
    }
}

fn copy_legacy_tree(source: &Path, destination: &Path, report: &mut MigrationReport) -> Result<()> {
    copy_legacy_tree_with_exclusions(source, destination, report, &[])
}

fn copy_legacy_tree_with_exclusions(
    source: &Path,
    destination: &Path,
    report: &mut MigrationReport,
    excluded_children: &[&str],
) -> Result<()> {
    let metadata = match fs::symlink_metadata(source) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error).with_context(|| format!("could not inspect legacy path {}", source.display())),
    };
    if metadata.file_type().is_symlink() {
        report.skipped.push(MigrationSkip {
            path: source.to_path_buf(),
            reason: MigrationSkipReason::Symlink,
        });
        return Ok(());
    }
    if metadata.is_file() {
        return copy_regular_file(source, destination, report);
    }
    if !metadata.is_dir() {
        report.skipped.push(MigrationSkip {
            path: source.to_path_buf(),
            reason: MigrationSkipReason::SpecialFile,
        });
        return Ok(());
    }
    ensure_migration_dir(destination)?;
    let entries =
        fs::read_dir(source).with_context(|| format!("could not read legacy directory {}", source.display()))?;
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                report.failures.push(MigrationFailure {
                    path: source.to_path_buf(),
                    error: format!("could not inspect legacy entry: {error}"),
                });
                continue;
            }
        };
        let child_source = entry.path();
        let child_destination = destination.join(entry.file_name());
        if excluded_children.iter().any(|name| entry.file_name() == OsStr::new(name)) {
            report.skipped.push(MigrationSkip {
                path: child_source,
                reason: MigrationSkipReason::Excluded,
            });
            continue;
        }
        if let Err(error) = copy_legacy_tree(&child_source, &child_destination, report) {
            report
                .failures
                .push(MigrationFailure { path: child_source, error: error.to_string() });
        }
    }
    Ok(())
}

fn copy_regular_file(source: &Path, destination: &Path, report: &mut MigrationReport) -> Result<()> {
    if let Ok(metadata) = fs::symlink_metadata(destination) {
        report.skipped.push(MigrationSkip {
            path: source.to_path_buf(),
            reason: if metadata.file_type().is_symlink() {
                MigrationSkipReason::Symlink
            } else {
                MigrationSkipReason::DestinationExists
            },
        });
        return Ok(());
    }
    let parent = destination
        .parent()
        .ok_or_else(|| anyhow!("migration destination {} has no parent", destination.display()))?;
    ensure_migration_dir(parent)?;
    let (temporary, mut output) = unique_private_file(parent, destination.file_name().unwrap_or(OsStr::new("file")))?;
    let mut input =
        open_no_follow(source).with_context(|| format!("could not safely open legacy file {}", source.display()))?;
    let result: Result<()> = (|| {
        let _bytes_copied = io::copy(&mut input, &mut output)?;
        output.sync_all()?;
        drop(output);
        match fs::hard_link(&temporary, destination) {
            Ok(()) => {
                fs::remove_file(&temporary)?;
                report.migrated.push(MigrationEntry {
                    source: source.to_path_buf(),
                    destination: destination.to_path_buf(),
                });
                Ok(())
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                report.skipped.push(MigrationSkip {
                    path: source.to_path_buf(),
                    reason: MigrationSkipReason::DestinationExists,
                });
                Ok(())
            }
            Err(error) => Err(error).with_context(|| format!("could not atomically publish {}", destination.display())),
        }
    })();
    if result.is_err() {
        remove_temporary_file(&temporary);
    }
    result
}

fn persist_report(paths: &VtCodePaths, report: &MigrationReport) -> Result<()> {
    let serialized = serde_json::to_vec_pretty(report).context("could not serialize legacy migration report")?;
    VtCodePaths::write_private_file_atomic(paths.migration_report_path(), &serialized)
        .with_context(|| format!("could not write migration report {}", paths.migration_report_path().display()))
}

fn persist_report_best_effort(paths: &VtCodePaths, report: &MigrationReport) {
    if let Err(error) = persist_report(paths, report) {
        tracing::debug!(error = %error, "could not persist legacy migration report");
    }
}

fn write_private_atomic(destination: &Path, contents: &[u8]) -> Result<()> {
    let parent = destination
        .parent()
        .ok_or_else(|| anyhow!("private file {} has no parent", destination.display()))?;
    ensure_private_dir(parent)?;
    let (temporary, mut file) = unique_private_file(parent, destination.file_name().unwrap_or(OsStr::new("file")))?;
    let result: io::Result<()> = (|| {
        file.write_all(contents)?;
        file.sync_all()?;
        drop(file);
        fs::hard_link(&temporary, destination)?;
        fs::remove_file(&temporary)?;
        Ok(())
    })();
    if result.is_err() {
        remove_temporary_file(&temporary);
    }
    result.with_context(|| format!("could not publish {}", destination.display()))
}

fn valid_migration_marker(path: &Path) -> io::Result<bool> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Ok(false);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Ok(false);
        }
    }
    let mut file = open_no_follow(path)?;
    let mut contents = Vec::new();
    let _bytes_read = file.read_to_end(&mut contents)?;
    Ok(contents == b"legacy migration completed\n")
}

fn unique_private_file(parent: &Path, stem: &OsStr) -> Result<(PathBuf, File)> {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let stem = stem.to_string_lossy();
    for attempt in 0..32u8 {
        let temporary = parent.join(format!(".{stem}.{}.{}.{}.migration", std::process::id(), timestamp, attempt));
        match create_private_new_file(&temporary) {
            Ok(file) => return Ok((temporary, file)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error).with_context(|| format!("could not create {}", temporary.display())),
        }
    }
    bail!("could not allocate a unique migration temporary file in {}", parent.display())
}

fn remove_temporary_file(path: &Path) {
    if let Err(error) = fs::remove_file(path)
        && error.kind() != io::ErrorKind::NotFound
    {
        tracing::debug!(path = %path.display(), %error, "failed to remove temporary migration file");
    }
}
