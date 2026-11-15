//! Evidence needed to identify an interrupted skill import.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use vtcode_commons::{VtCodePaths, fs::rename_synced, fs::sync_dir, validate_no_escaping_symlink_ancestors};

#[derive(Debug, Serialize, Deserialize)]
struct TransactionRecord {
    format_version: u32,
    skill_name: String,
    version: String,
    had_previous: bool,
    previous_index: Option<Vec<u8>>,
    intended_index: Vec<u8>,
}

/// Record exact index states and the validated target before any rename.
pub(super) fn record_transaction(
    staging: &Path,
    store: &Path,
    skill_name: &str,
    version: &str,
    had_previous: bool,
    intended_index: &[u8],
) -> Result<()> {
    let previous_index = read_index(store).context("read previous skill index for transaction")?;
    let record = TransactionRecord {
        format_version: 2,
        skill_name: skill_name.to_owned(),
        version: version.to_owned(),
        had_previous,
        previous_index,
        intended_index: intended_index.to_vec(),
    };
    let data = serde_json::to_vec(&record).context("serialize skill import transaction")?;
    VtCodePaths::write_private_file_atomic(staging.join("transaction.json"), &data)
        .context("record skill import transaction")?;
    sync_dir(staging).context("sync skill import transaction directory")
}

pub(super) fn read_index(store: &Path) -> Result<Option<Vec<u8>>> {
    let path = store.join("index.json");
    match fs::symlink_metadata(&path) {
        Ok(metadata) => {
            ensure!(metadata.is_file() && !metadata.file_type().is_symlink(), "skill index is not a regular file");
            Ok(Some(fs::read(path).context("read skill index")?))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).context("inspect skill index"),
    }
}

pub(super) fn directory_exists(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => Ok(true),
        Ok(_) => bail!("expected a regular directory: {}", path.display()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error).with_context(|| format!("inspect {}", path.display())),
    }
}

/// Called with the store lock held. Unknown or inconsistent transactions are preserved.
pub(super) fn recover_abandoned_imports(store: &Path) -> Result<()> {
    for entry in fs::read_dir(store).context("inspect skill store for interrupted imports")? {
        let entry = entry.context("inspect skill store entry")?;
        if !entry.file_name().to_string_lossy().starts_with(".skill-import-") {
            continue;
        }
        recover_one(store, &entry.path())
            .with_context(|| format!("interrupted import at {}; preserve it for recovery", entry.path().display()))?;
    }
    Ok(())
}

fn recover_one(store: &Path, staging: &Path) -> Result<()> {
    ensure!(directory_exists(staging)?, "transaction directory is missing");
    let journal = staging.join("transaction.json");
    let metadata = fs::symlink_metadata(&journal).context("inspect transaction record")?;
    ensure!(metadata.is_file() && !metadata.file_type().is_symlink(), "invalid transaction record");
    let record: TransactionRecord = serde_json::from_slice(&fs::read(&journal)?).context("parse transaction record")?;
    ensure!(record.format_version == 2, "unsupported transaction record version");
    let manifest = crate::types::SkillManifest {
        name: record.skill_name.clone(),
        description: "recovery".to_owned(),
        ..Default::default()
    };
    manifest.validate().context("invalid transaction skill name")?;
    ensure!(super::safe_version(&record.version), "invalid transaction version");
    let intended: super::SkillStoreIndex =
        serde_json::from_slice(&record.intended_index).context("invalid intended index")?;
    let previous: super::SkillStoreIndex = match &record.previous_index {
        Some(bytes) => serde_json::from_slice(bytes).context("invalid previous index")?,
        None => Default::default(),
    };
    let expected = super::updated_skill_index(previous, &record.skill_name, &record.version)?;
    let expected: super::SkillStoreIndex = serde_json::from_slice(&expected)?;
    ensure!(intended == expected, "transaction index changes more than the target version");
    let current = read_index(store).context("read current skill index")?;
    let committed = current.as_deref() == Some(record.intended_index.as_slice());
    ensure!(committed || current == record.previous_index, "skill index changed since interrupted import");
    let dest = store.join(&record.skill_name).join(&record.version);
    validate_no_escaping_symlink_ancestors(&dest, true).context("validate recovery destination")?;
    let parent = dest.parent().context("recovery destination has no parent")?;
    ensure!(directory_exists(parent)?, "recovery destination parent is missing");
    let staged = staging.join("new");
    let backup = staging.join("previous");
    let has_staged = directory_exists(&staged)?;
    let has_backup = directory_exists(&backup)?;
    let has_destination = directory_exists(&dest)?;
    ensure!(record.had_previous || !has_backup, "unexpected previous version");
    if has_staged {
        ensure!(
            !committed || record.previous_index.as_deref() == Some(record.intended_index.as_slice()),
            "index committed before version publication"
        );
        if record.had_previous && !has_backup && has_destination {
            rename_synced(&dest, &backup).context("back up previous version during recovery")?;
            #[cfg(test)]
            pause_recovery_for_test("backup")?;
        } else {
            ensure!(!has_destination && (has_backup == record.had_previous), "ambiguous transaction directories");
        }
        rename_synced(&staged, &dest).context("publish staged version during recovery")?;
        #[cfg(test)]
        pause_recovery_for_test("version")?;
    } else {
        ensure!(has_destination, "published version is missing");
        ensure!(!record.had_previous || has_backup || committed, "previous version is missing before index commit");
    }
    if !committed {
        VtCodePaths::write_private_file_atomic(store.join("index.json"), &record.intended_index)
            .context("publish recovered skill index")?;
        sync_dir(store).context("sync skill store after recovered index publish")?;
        #[cfg(test)]
        pause_recovery_for_test("index")?;
    }
    fs::remove_dir_all(staging).context("remove recovered transaction")?;
    Ok(())
}

#[cfg(test)]
fn pause_recovery_for_test(phase: &str) -> Result<()> {
    if std::env::var("VTCODE_TEST_RECOVERY_KILL_PHASE").ok().as_deref() == Some(phase) {
        let ready = std::env::var_os("VTCODE_TEST_RECOVERY_KILL_READY").context("missing test signal path")?;
        fs::write(ready, phase)?;
        loop {
            std::thread::park();
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn killed_recovery_process_can_be_retried() {
        const STORE_ENV: &str = "VTCODE_TEST_RECOVERY_KILL_STORE";
        if let Some(store) = std::env::var_os(STORE_ENV) {
            let _ = super::super::load_skill_index(Path::new(&store)).unwrap();
            return;
        }
        let previous = b"{\"skills\":{\"sample\":{\"latest_version\":\"0.9\",\"versions\":[\"0.9\",\"1.2\"]}}}";
        let intended = b"{\"skills\":{\"sample\":{\"latest_version\":\"1.2\",\"versions\":[\"0.9\",\"1.2\"]}}}";
        for phase in ["backup", "version", "index"] {
            let root = tempfile::tempdir().unwrap();
            let store = root.path().join("store");
            let dest = store.join("sample/1.2");
            fs::create_dir_all(&dest).unwrap();
            fs::write(dest.join("SKILL.md"), "old").unwrap();
            fs::write(store.join("index.json"), previous).unwrap();
            let staging = tempfile::Builder::new().prefix(".skill-import-").tempdir_in(&store).unwrap();
            fs::create_dir(staging.path().join("new")).unwrap();
            fs::write(staging.path().join("new/SKILL.md"), "new").unwrap();
            record_transaction(staging.path(), &store, "sample", "1.2", true, intended).unwrap();
            let ready = root.path().join("ready");
            let mut child = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "bundle::recovery::tests::killed_recovery_process_can_be_retried",
                    "--nocapture",
                ])
                .env(STORE_ENV, &store)
                .env("VTCODE_TEST_RECOVERY_KILL_PHASE", phase)
                .env("VTCODE_TEST_RECOVERY_KILL_READY", &ready)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .unwrap();
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
            while !ready.exists() && std::time::Instant::now() < deadline {
                assert!(child.try_wait().unwrap().is_none(), "child exited before {phase} boundary");
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            let observed = ready.exists();
            child.kill().unwrap();
            let status = child.wait().unwrap();
            assert!(observed && !status.success(), "child did not stop at {phase} boundary");
            let index = super::super::load_skill_index(&store).unwrap();
            assert_eq!(index.skills["sample"].latest_version, "1.2");
            assert_eq!(fs::read(dest.join("SKILL.md")).unwrap(), b"new");
            assert_eq!(fs::read(store.join("index.json")).unwrap(), intended);
            assert!(!staging.path().exists());
        }
    }

    #[test]
    fn killed_import_process_recovers_on_index_load() {
        const CHILD_STORE: &str = "VTCODE_TEST_SKILL_KILL_STORE";
        const CHILD_PHASE: &str = "VTCODE_TEST_SKILL_KILL_PHASE";
        const INTENDED: &[u8] =
            b"{\"skills\":{\"sample\":{\"latest_version\":\"1.2\",\"versions\":[\"0.9\",\"1.2\"]}}}";
        if let Some(store) = std::env::var_os(CHILD_STORE) {
            let store = std::path::PathBuf::from(store);
            let phase = std::env::var(CHILD_PHASE).unwrap();
            let destination = store.join("sample/1.2");
            let ready = store.parent().unwrap().join("ready");
            VtCodePaths::with_private_file_lock(store.join("index.json"), || {
                let staging = tempfile::Builder::new().prefix(".skill-import-").tempdir_in(&store)?;
                fs::create_dir(staging.path().join("new"))?;
                fs::write(staging.path().join("new/SKILL.md"), "new")?;
                record_transaction(staging.path(), &store, "sample", "1.2", true, INTENDED)?;
                super::super::publish_staged_skill(staging, &destination, || {
                    if phase == "committed" {
                        VtCodePaths::write_private_file_atomic(store.join("index.json"), INTENDED)?;
                    }
                    fs::write(&ready, "ready")?;
                    loop {
                        std::thread::park();
                    }
                })
            })
            .unwrap();
            return;
        }

        for phase in ["published", "committed"] {
            let root = tempfile::tempdir().unwrap();
            let store = root.path().join("store");
            let destination = store.join("sample/1.2");
            fs::create_dir_all(&destination).unwrap();
            fs::write(destination.join("SKILL.md"), "old").unwrap();
            fs::write(
                store.join("index.json"),
                b"{\"skills\":{\"sample\":{\"latest_version\":\"0.9\",\"versions\":[\"0.9\",\"1.2\"]}}}",
            )
            .unwrap();
            let mut child = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "bundle::recovery::tests::killed_import_process_recovers_on_index_load",
                    "--nocapture",
                ])
                .env(CHILD_STORE, &store)
                .env(CHILD_PHASE, phase)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .unwrap();
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
            while !root.path().join("ready").exists() && std::time::Instant::now() < deadline {
                assert!(child.try_wait().unwrap().is_none(), "child exited before {phase} boundary");
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            let ready = root.path().join("ready").exists();
            child.kill().unwrap();
            let status = child.wait().unwrap();
            assert!(ready && !status.success(), "child did not stop at {phase} boundary");
            let index = super::super::load_skill_index(&store).unwrap();
            assert_eq!(index.skills["sample"].latest_version, "1.2");
            assert_eq!(fs::read(destination.join("SKILL.md")).unwrap(), b"new");
            assert_eq!(fs::read(store.join("index.json")).unwrap(), INTENDED);
            assert!(
                fs::read_dir(&store).unwrap().all(|entry| !entry
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".skill-import-"))
            );
        }
    }

    #[test]
    fn records_distinct_prior_and_intended_index_bytes() {
        let store = tempfile::tempdir().unwrap();
        let staging = tempfile::tempdir_in(store.path()).unwrap();
        fs::write(store.path().join("index.json"), b"old index\n").unwrap();
        record_transaction(staging.path(), store.path(), "sample", "2.0", true, b"new index").unwrap();
        let bytes = fs::read(staging.path().join("transaction.json")).unwrap();
        let record: TransactionRecord = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(record.format_version, 2);
        assert_eq!(record.skill_name, "sample");
        assert_eq!(record.version, "2.0");
        assert_eq!(record.previous_index.as_deref(), Some(b"old index\n".as_slice()));
        assert_eq!(record.intended_index, b"new index");
    }

    #[test]
    fn records_absent_previous_index() {
        let store = tempfile::tempdir().unwrap();
        let staging = tempfile::tempdir_in(store.path()).unwrap();
        record_transaction(staging.path(), store.path(), "other", "0.0.0", false, b"{}").unwrap();
        let record: TransactionRecord =
            serde_json::from_slice(&fs::read(staging.path().join("transaction.json")).unwrap()).unwrap();
        assert!(record.previous_index.is_none());
        assert_eq!(record.intended_index, b"{}");
    }

    #[test]
    fn interrupted_publication_recovers_each_state() {
        let old_index = b"{\"skills\":{}}";
        let new_index = b"{\"skills\":{\"sample\":{\"latest_version\":\"1.2\",\"versions\":[\"1.2\"]}}}";
        for had_previous in [false, true] {
            for phase in 0..=3 {
                if !had_previous && phase == 1 {
                    continue;
                }
                let store = tempfile::tempdir().unwrap();
                fs::write(store.path().join("index.json"), old_index).unwrap();
                let parent = store.path().join("sample");
                fs::create_dir(&parent).unwrap();
                let dest = parent.join("1.2");
                if had_previous {
                    fs::create_dir(&dest).unwrap();
                    fs::write(dest.join("SKILL.md"), "old").unwrap();
                }
                let staging = tempfile::Builder::new()
                    .prefix(".skill-import-")
                    .tempdir_in(store.path())
                    .unwrap();
                fs::create_dir(staging.path().join("new")).unwrap();
                fs::write(staging.path().join("new/SKILL.md"), "new").unwrap();
                record_transaction(staging.path(), store.path(), "sample", "1.2", had_previous, new_index).unwrap();
                if had_previous && phase >= 1 {
                    fs::rename(&dest, staging.path().join("previous")).unwrap();
                }
                if phase >= 2 {
                    fs::rename(staging.path().join("new"), &dest).unwrap();
                }
                if phase == 3 {
                    fs::write(store.path().join("index.json"), new_index).unwrap();
                }
                recover_abandoned_imports(store.path()).unwrap();
                assert_eq!(
                    fs::read_to_string(dest.join("SKILL.md")).unwrap(),
                    "new",
                    "phase={phase} previous={had_previous}"
                );
                assert_eq!(fs::read(store.path().join("index.json")).unwrap(), new_index);
                recover_abandoned_imports(store.path()).unwrap();
            }
        }
    }

    #[test]
    fn recovery_rejects_changed_index_without_touching_transaction() {
        let store = tempfile::tempdir().unwrap();
        fs::write(store.path().join("index.json"), b"{\"skills\":{}}").unwrap();
        fs::create_dir(store.path().join("sample")).unwrap();
        let staging = tempfile::Builder::new()
            .prefix(".skill-import-")
            .tempdir_in(store.path())
            .unwrap();
        fs::create_dir(staging.path().join("new")).unwrap();
        let intended = b"{\"skills\":{\"sample\":{\"latest_version\":\"1.2\",\"versions\":[\"1.2\"]}}}";
        record_transaction(staging.path(), store.path(), "sample", "1.2", false, intended).unwrap();
        fs::write(store.path().join("index.json"), "changed").unwrap();
        assert!(format!("{:#}", recover_abandoned_imports(store.path()).unwrap_err()).contains("index changed"));
        assert!(staging.path().join("new").is_dir());
        assert_eq!(fs::read_to_string(store.path().join("index.json")).unwrap(), "changed");
        assert!(!store.path().join("sample/1.2").exists());
    }

    #[test]
    fn recovery_rejects_untrusted_record_paths() {
        let store = tempfile::tempdir().unwrap();
        let staging = tempfile::Builder::new()
            .prefix(".skill-import-")
            .tempdir_in(store.path())
            .unwrap();
        let record = TransactionRecord {
            format_version: 2,
            skill_name: "../outside".to_owned(),
            version: "1.2".to_owned(),
            had_previous: false,
            previous_index: None,
            intended_index: b"{\"skills\":{}}".to_vec(),
        };
        fs::write(staging.path().join("transaction.json"), serde_json::to_vec(&record).unwrap()).unwrap();
        assert!(
            format!("{:#}", recover_abandoned_imports(store.path()).unwrap_err())
                .contains("invalid transaction skill name")
        );
        assert!(staging.path().join("transaction.json").exists());
    }

    #[test]
    fn recovery_rejects_index_that_removes_another_skill() {
        let store = tempfile::tempdir().unwrap();
        let previous = br#"{"skills":{"other":{"latest_version":"9","versions":["9"]}}}"#;
        let malicious = br#"{"skills":{"sample":{"latest_version":"1.2","versions":["1.2"]}}}"#;
        fs::write(store.path().join("index.json"), previous).unwrap();
        fs::create_dir(store.path().join("sample")).unwrap();
        let staging = tempfile::Builder::new()
            .prefix(".skill-import-")
            .tempdir_in(store.path())
            .unwrap();
        fs::create_dir(staging.path().join("new")).unwrap();
        record_transaction(staging.path(), store.path(), "sample", "1.2", false, malicious).unwrap();
        assert!(format!("{:#}", recover_abandoned_imports(store.path()).unwrap_err()).contains("more than the target"));
        assert_eq!(fs::read(store.path().join("index.json")).unwrap(), previous);
        assert!(staging.path().join("new").is_dir());
    }

    #[cfg(unix)]
    #[test]
    fn transaction_refuses_symlinked_index() {
        let store = tempfile::tempdir().unwrap();
        let staging = tempfile::tempdir_in(store.path()).unwrap();
        let target = store.path().join("target");
        fs::write(&target, "unchanged").unwrap();
        std::os::unix::fs::symlink(&target, store.path().join("index.json")).unwrap();
        assert!(record_transaction(staging.path(), store.path(), "sample", "1.2", false, b"{}").is_err());
        assert_eq!(fs::read_to_string(target).unwrap(), "unchanged");
        assert!(!staging.path().join("transaction.json").exists());
    }

    #[test]
    fn recovery_rejects_unexpected_backup_without_previous() {
        let store = tempfile::tempdir().unwrap();
        let old_index = b"{\"skills\":{}}";
        fs::write(store.path().join("index.json"), old_index).unwrap();
        fs::create_dir(store.path().join("sample")).unwrap();
        let staging = tempfile::Builder::new()
            .prefix(".skill-import-")
            .tempdir_in(store.path())
            .unwrap();
        fs::create_dir(staging.path().join("new")).unwrap();
        fs::create_dir(staging.path().join("previous")).unwrap();
        let intended = b"{\"skills\":{\"sample\":{\"latest_version\":\"1.2\",\"versions\":[\"1.2\"]}}}";
        record_transaction(staging.path(), store.path(), "sample", "1.2", false, intended).unwrap();
        assert!(
            format!("{:#}", recover_abandoned_imports(store.path()).unwrap_err())
                .contains("unexpected previous version")
        );
        assert!(staging.path().join("new").is_dir());
        assert!(staging.path().join("previous").is_dir());
        assert_eq!(fs::read(store.path().join("index.json")).unwrap(), old_index);
        assert!(!store.path().join("sample/1.2").exists());
    }

    #[test]
    fn recovery_rejects_existing_destination_for_first_import() {
        let store = tempfile::tempdir().unwrap();
        let old_index = b"{\"skills\":{}}";
        fs::write(store.path().join("index.json"), old_index).unwrap();
        let dest = store.path().join("sample/1.2");
        fs::create_dir_all(&dest).unwrap();
        fs::write(dest.join("SKILL.md"), "squat").unwrap();
        let staging = tempfile::Builder::new()
            .prefix(".skill-import-")
            .tempdir_in(store.path())
            .unwrap();
        fs::create_dir(staging.path().join("new")).unwrap();
        fs::write(staging.path().join("new/SKILL.md"), "new").unwrap();
        let intended = b"{\"skills\":{\"sample\":{\"latest_version\":\"1.2\",\"versions\":[\"1.2\"]}}}";
        record_transaction(staging.path(), store.path(), "sample", "1.2", false, intended).unwrap();
        assert!(
            format!("{:#}", recover_abandoned_imports(store.path()).unwrap_err()).contains("ambiguous transaction")
        );
        assert_eq!(fs::read_to_string(dest.join("SKILL.md")).unwrap(), "squat");
        assert!(staging.path().join("new").is_dir());
        assert_eq!(fs::read(store.path().join("index.json")).unwrap(), old_index);
    }

    #[test]
    fn recovery_rejects_missing_directories_after_publication() {
        let store = tempfile::tempdir().unwrap();
        let old_index = b"{\"skills\":{}}";
        fs::write(store.path().join("index.json"), old_index).unwrap();
        fs::create_dir(store.path().join("sample")).unwrap();
        let staging = tempfile::Builder::new()
            .prefix(".skill-import-")
            .tempdir_in(store.path())
            .unwrap();
        let intended = b"{\"skills\":{\"sample\":{\"latest_version\":\"1.2\",\"versions\":[\"1.2\"]}}}";
        record_transaction(staging.path(), store.path(), "sample", "1.2", false, intended).unwrap();
        assert!(
            format!("{:#}", recover_abandoned_imports(store.path()).unwrap_err())
                .contains("published version is missing")
        );
        assert_eq!(fs::read(store.path().join("index.json")).unwrap(), old_index);
        assert!(!store.path().join("sample/1.2").exists());
    }

    #[test]
    fn recovery_rejects_missing_backup_before_index_commit() {
        let store = tempfile::tempdir().unwrap();
        let previous = b"{\"skills\":{\"sample\":{\"latest_version\":\"0.9\",\"versions\":[\"0.9\"]}}}";
        let intended = b"{\"skills\":{\"sample\":{\"latest_version\":\"1.2\",\"versions\":[\"0.9\",\"1.2\"]}}}";
        fs::write(store.path().join("index.json"), previous).unwrap();
        let dest = store.path().join("sample/1.2");
        fs::create_dir_all(&dest).unwrap();
        fs::write(dest.join("SKILL.md"), "new").unwrap();
        let staging = tempfile::Builder::new()
            .prefix(".skill-import-")
            .tempdir_in(store.path())
            .unwrap();
        record_transaction(staging.path(), store.path(), "sample", "1.2", true, intended).unwrap();
        assert!(
            format!("{:#}", recover_abandoned_imports(store.path()).unwrap_err())
                .contains("previous version is missing")
        );
        assert_eq!(fs::read_to_string(dest.join("SKILL.md")).unwrap(), "new");
        assert_eq!(fs::read(store.path().join("index.json")).unwrap(), previous);
    }

    #[test]
    fn recovery_rejects_committed_index_with_unpublished_staging() {
        let store = tempfile::tempdir().unwrap();
        let old_index = b"{\"skills\":{}}";
        let intended = b"{\"skills\":{\"sample\":{\"latest_version\":\"1.2\",\"versions\":[\"1.2\"]}}}";
        fs::write(store.path().join("index.json"), old_index).unwrap();
        fs::create_dir(store.path().join("sample")).unwrap();
        let staging = tempfile::Builder::new()
            .prefix(".skill-import-")
            .tempdir_in(store.path())
            .unwrap();
        fs::create_dir(staging.path().join("new")).unwrap();
        fs::write(staging.path().join("new/SKILL.md"), "new").unwrap();
        record_transaction(staging.path(), store.path(), "sample", "1.2", false, intended).unwrap();
        fs::write(store.path().join("index.json"), intended).unwrap();
        assert!(
            format!("{:#}", recover_abandoned_imports(store.path()).unwrap_err())
                .contains("committed before version publication")
        );
        assert!(staging.path().join("new").is_dir());
        assert_eq!(fs::read(store.path().join("index.json")).unwrap(), intended);
        assert!(!store.path().join("sample/1.2").exists());
    }

    #[test]
    fn recovery_rejects_invalid_record_version() {
        let store = tempfile::tempdir().unwrap();
        let staging = tempfile::Builder::new()
            .prefix(".skill-import-")
            .tempdir_in(store.path())
            .unwrap();
        let record = TransactionRecord {
            format_version: 2,
            skill_name: "sample".to_owned(),
            version: "../evil".to_owned(),
            had_previous: false,
            previous_index: None,
            intended_index: b"{\"skills\":{}}".to_vec(),
        };
        fs::write(staging.path().join("transaction.json"), serde_json::to_vec(&record).unwrap()).unwrap();
        assert!(
            format!("{:#}", recover_abandoned_imports(store.path()).unwrap_err())
                .contains("invalid transaction version")
        );
        assert!(staging.path().join("transaction.json").exists());
    }

    #[test]
    fn recovery_rejects_missing_destination_parent() {
        let store = tempfile::tempdir().unwrap();
        let old_index = b"{\"skills\":{}}";
        let intended = b"{\"skills\":{\"sample\":{\"latest_version\":\"1.2\",\"versions\":[\"1.2\"]}}}";
        fs::write(store.path().join("index.json"), old_index).unwrap();
        let staging = tempfile::Builder::new()
            .prefix(".skill-import-")
            .tempdir_in(store.path())
            .unwrap();
        fs::create_dir(staging.path().join("new")).unwrap();
        record_transaction(staging.path(), store.path(), "sample", "1.2", false, intended).unwrap();
        assert!(
            format!("{:#}", recover_abandoned_imports(store.path()).unwrap_err())
                .contains("destination parent is missing")
        );
        assert!(staging.path().join("new").is_dir());
        assert_eq!(fs::read(store.path().join("index.json")).unwrap(), old_index);
    }
}
