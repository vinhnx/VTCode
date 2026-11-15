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
    assert!(result.expect_err("should fail").to_string().contains("exceeds maximum"));
}

/// Build an uncompressed zip archive from `(name, contents)` entries.
fn zip_bytes(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut buf = Vec::new();
    {
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
        let options = zip::write::SimpleFileOptions::default();
        for (name, data) in entries {
            writer.start_file(*name, options).expect("start_file");
            writer.write_all(data).expect("write_all");
        }
        writer.finish().expect("finish");
    }
    buf
}

#[test]
fn copy_bounded_accepts_exactly_the_limit() {
    let data = vec![7u8; 10];
    let mut out = Vec::new();
    let written = copy_bounded(&mut data.as_slice(), &mut out, 10).expect("exact limit is allowed");
    assert_eq!(written, 10);
    assert_eq!(out.len(), 10);
}

#[test]
fn copy_bounded_rejects_one_byte_over_the_limit() {
    let data = vec![7u8; 11];
    let mut out = Vec::new();
    let err = copy_bounded(&mut data.as_slice(), &mut out, 10).expect_err("over limit must fail");
    assert!(err.to_string().contains("exceeds maximum"), "unexpected error: {err}");
}

#[test]
fn within_total_budget_boundaries() {
    assert!(within_total_budget(0));
    assert!(within_total_budget(MAX_TOTAL_UNCOMPRESSED_SIZE));
    assert!(!within_total_budget(MAX_TOTAL_UNCOMPRESSED_SIZE + 1));
}

#[test]
fn extract_rejects_parent_directory_traversal() {
    let bytes = zip_bytes(&[("../evil.txt", b"pwned")]);
    let temp = tempfile::tempdir().expect("tempdir");
    let err = extract_zip_safely(&bytes, temp.path()).expect_err("traversal must fail");
    assert!(err.to_string().contains("Path traversal"), "unexpected error: {err}");
    assert!(!temp.path().join("../evil.txt").exists());
}

#[test]
fn extract_rejects_absolute_path_entries() {
    let bytes = zip_bytes(&[("/abs.txt", b"pwned")]);
    let temp = tempfile::tempdir().expect("tempdir");
    let err = extract_zip_safely(&bytes, temp.path()).expect_err("absolute path must fail");
    assert!(err.to_string().contains("Path traversal"), "unexpected error: {err}");
}

#[test]
fn extract_rejects_mid_path_traversal() {
    let bytes = zip_bytes(&[("skills/../../evil.txt", b"pwned")]);
    let temp = tempfile::tempdir().expect("tempdir");
    let err = extract_zip_safely(&bytes, temp.path()).expect_err("mid-path traversal must fail");
    assert!(err.to_string().contains("Path traversal"), "unexpected error: {err}");
    assert!(!temp.path().join("evil.txt").exists());
}

#[test]
fn extract_accepts_dot_dot_inside_a_single_component() {
    // `..` inside one component is a legitimate file name, not traversal;
    // the substring check this replaced used to reject it.
    let bytes = zip_bytes(&[("v1..2/README.md", b"body")]);
    let temp = tempfile::tempdir().expect("tempdir");
    extract_zip_safely(&bytes, temp.path()).expect("dot-dot component must extract");

    assert_eq!(fs::read(temp.path().join("v1..2/README.md")).expect("read"), b"body");
}

#[test]
fn import_round_trips_an_exported_bundle() {
    let source = tempfile::tempdir().expect("source tempdir");
    let skill_dir = source.path().join("roundtrip-skill");
    fs::create_dir_all(&skill_dir).expect("create skill dir");
    fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: roundtrip-skill\ndescription: Round trip test skill\nmetadata:\n  version: 1.2.3\n---\n# Body\n",
    )
    .expect("write SKILL.md");

    let bytes = export_skill_bundle(&skill_dir).expect("export");

    let store = tempfile::tempdir().expect("store tempdir");
    let imported = import_skill_bundle(&bytes, store.path()).expect("import");
    assert_eq!(imported.name, "roundtrip-skill");
    assert_eq!(imported.version.as_deref(), Some("1.2.3"));
    assert!(imported.file_count >= 1, "expected at least SKILL.md to be copied");
    assert!(imported.path.join("SKILL.md").exists(), "SKILL.md must land in the versioned store");
}

#[test]
fn import_keeps_distinct_metadata_versions() {
    let store = tempfile::tempdir().unwrap();
    let first =
        zip_bytes(&[("SKILL.md", b"---\nname: sample\ndescription: Example\nmetadata:\n  version: 1.2.3\n---\nfirst")]);
    let second = zip_bytes(&[(
        "SKILL.md",
        b"---\nname: sample\ndescription: Example\nmetadata:\n  version: 2.0.0\n---\nsecond",
    )]);
    let imported_first = import_skill_bundle(&first, store.path()).unwrap();
    let imported_second = import_skill_bundle(&second, store.path()).unwrap();
    assert_eq!(imported_first.version.as_deref(), Some("1.2.3"));
    assert_eq!(imported_second.version.as_deref(), Some("2.0.0"));
    assert!(
        fs::read_to_string(imported_first.path.join("SKILL.md"))
            .unwrap()
            .ends_with("first")
    );
    assert!(
        fs::read_to_string(imported_second.path.join("SKILL.md"))
            .unwrap()
            .ends_with("second")
    );
}

#[test]
fn import_without_version_uses_default() {
    let store = tempfile::tempdir().unwrap();
    let bytes = zip_bytes(&[("SKILL.md", b"---\nname: sample\ndescription: Example\n---\nbody")]);
    let imported = import_skill_bundle(&bytes, store.path()).unwrap();
    assert_eq!(imported.version.as_deref(), Some("0.0.0"));
    assert_eq!(imported.path, store.path().join("sample/0.0.0"));
}

#[test]
fn import_rejects_corrupt_index_before_replacing_existing_version() {
    let store = tempfile::tempdir().unwrap();
    let existing = store.path().join("sample/1.2.3");
    fs::create_dir_all(&existing).unwrap();
    fs::write(existing.join("SKILL.md"), "original").unwrap();
    fs::write(store.path().join("index.json"), "{invalid").unwrap();
    let bytes = zip_bytes(&[(
        "SKILL.md",
        b"---\nname: sample\ndescription: Example\nmetadata:\n  version: 1.2.3\n---\nreplacement",
    )]);
    let error = import_skill_bundle(&bytes, store.path()).unwrap_err();
    assert!(format!("{error:#}").contains("Failed to parse skill store index"));
    assert_eq!(fs::read_to_string(existing.join("SKILL.md")).unwrap(), "original");
    assert_eq!(fs::read_to_string(store.path().join("index.json")).unwrap(), "{invalid");
}

#[test]
fn import_preserves_other_skills_in_existing_index() {
    let store = tempfile::tempdir().unwrap();
    let first = zip_bytes(&[("SKILL.md", b"---\nname: first\ndescription: Example\n---\nfirst")]);
    let second = zip_bytes(&[("SKILL.md", b"---\nname: second\ndescription: Example\n---\nsecond")]);
    let _ = import_skill_bundle(&first, store.path()).unwrap();
    let _ = import_skill_bundle(&second, store.path()).unwrap();
    let index = load_skill_index(store.path()).unwrap();
    assert_eq!(index.skills["first"].versions, ["0.0.0"]);
    assert_eq!(index.skills["second"].versions, ["0.0.0"]);
}

#[test]
fn publication_failure_restores_existing_version_and_removes_new_version() {
    let root = tempfile::tempdir().unwrap();
    for had_previous in [true, false] {
        let destination = root.path().join(if had_previous { "existing" } else { "new" });
        if had_previous {
            fs::create_dir(&destination).unwrap();
            fs::write(destination.join("SKILL.md"), "old").unwrap();
        }
        let staging = tempfile::tempdir_in(root.path()).unwrap();
        fs::create_dir(staging.path().join("new")).unwrap();
        fs::write(staging.path().join("new/SKILL.md"), "replacement").unwrap();
        let error = publish_staged_skill(staging, &destination, || anyhow::bail!("index write failed")).unwrap_err();
        assert!(format!("{error:#}").contains("index write failed"));
        if had_previous {
            assert_eq!(fs::read_to_string(destination.join("SKILL.md")).unwrap(), "old");
        } else {
            assert!(!destination.exists());
        }
    }
}

#[test]
fn rollback_failure_retains_previous_version_for_recovery() {
    let root = tempfile::tempdir().unwrap();
    let destination = root.path().join("skill");
    fs::create_dir(&destination).unwrap();
    fs::write(destination.join("SKILL.md"), "old").unwrap();
    let staging = tempfile::tempdir_in(root.path()).unwrap();
    let staging_path = staging.path().to_path_buf();
    fs::create_dir(staging.path().join("new")).unwrap();
    let error = publish_staged_skill(staging, &destination, || {
        fs::remove_dir_all(&destination)?;
        fs::write(&destination, "obstruction")?;
        anyhow::bail!("index failure")
    })
    .unwrap_err();
    let message = format!("{error:#}");
    assert!(message.contains("index failure") && message.contains("rollback failed"));
    assert_eq!(fs::read_to_string(staging_path.join("previous/SKILL.md")).unwrap(), "old");
}

#[test]
fn concurrent_imports_keep_both_index_entries() {
    let store = tempfile::tempdir().unwrap();
    let first = zip_bytes(&[("SKILL.md", b"---\nname: first\ndescription: Example\n---\nfirst")]);
    let second = zip_bytes(&[("SKILL.md", b"---\nname: second\ndescription: Example\n---\nsecond")]);
    let barrier = std::sync::Barrier::new(2);
    std::thread::scope(|scope| {
        let handles: Vec<_> = [&first, &second]
            .into_iter()
            .map(|bytes| {
                let barrier = &barrier;
                let path = store.path();
                scope.spawn(move || {
                    barrier.wait();
                    import_skill_bundle(bytes, path).unwrap()
                })
            })
            .collect();
        for handle in handles {
            let _ = handle.join().unwrap();
        }
    });
    let index = load_skill_index(store.path()).unwrap();
    assert!(index.skills.contains_key("first"));
    assert!(index.skills.contains_key("second"));
    assert!(store.path().join("first/0.0.0/SKILL.md").is_file());
    assert!(store.path().join("second/0.0.0/SKILL.md").is_file());
}

#[test]
fn import_preserves_abandoned_transaction_and_existing_store() {
    let store = tempfile::tempdir().unwrap();
    let previous = store.path().join(".skill-import-interrupted/previous");
    fs::create_dir_all(&previous).unwrap();
    fs::write(previous.join("SKILL.md"), "recoverable").unwrap();
    fs::write(store.path().join("index.json"), "{\"skills\":{}}").unwrap();
    let bytes = zip_bytes(&[("SKILL.md", b"---\nname: sample\ndescription: Example\n---\nbody")]);
    let error = import_skill_bundle(&bytes, store.path()).unwrap_err();
    assert!(format!("{error:#}").contains("interrupted import"));
    assert_eq!(fs::read_to_string(previous.join("SKILL.md")).unwrap(), "recoverable");
    assert_eq!(fs::read_to_string(store.path().join("index.json")).unwrap(), "{\"skills\":{}}");
    assert!(!store.path().join("sample").exists());
}

#[test]
fn import_recovers_recorded_interruption_before_adding_another_skill() {
    let store = tempfile::tempdir().unwrap();
    let old_index = b"{\"skills\":{}}";
    let intended = b"{\"skills\":{\"first\":{\"latest_version\":\"1.2\",\"versions\":[\"1.2\"]}}}";
    fs::write(store.path().join("index.json"), old_index).unwrap();
    fs::create_dir(store.path().join("first")).unwrap();
    let staging = tempfile::Builder::new()
        .prefix(".skill-import-")
        .tempdir_in(store.path())
        .unwrap();
    fs::create_dir(staging.path().join("new")).unwrap();
    fs::write(staging.path().join("new/SKILL.md"), "recovered").unwrap();
    recovery::record_transaction(staging.path(), store.path(), "first", "1.2", false, intended).unwrap();
    let bytes = zip_bytes(&[("SKILL.md", b"---\nname: second\ndescription: Example\n---\nsecond")]);
    let _ = import_skill_bundle(&bytes, store.path()).unwrap();
    assert_eq!(fs::read_to_string(store.path().join("first/1.2/SKILL.md")).unwrap(), "recovered");
    let index = load_skill_index(store.path()).unwrap();
    assert!(index.skills.contains_key("first") && index.skills.contains_key("second"));
}

#[test]
fn index_load_recovers_interrupted_publication() {
    let store = tempfile::tempdir().unwrap();
    fs::write(store.path().join("index.json"), b"{\"skills\":{}}").unwrap();
    fs::create_dir(store.path().join("sample")).unwrap();
    let staging = tempfile::Builder::new()
        .prefix(".skill-import-")
        .tempdir_in(store.path())
        .unwrap();
    fs::create_dir(staging.path().join("new")).unwrap();
    fs::write(staging.path().join("new/SKILL.md"), "recovered").unwrap();
    let intended = b"{\"skills\":{\"sample\":{\"latest_version\":\"1.2\",\"versions\":[\"1.2\"]}}}";
    recovery::record_transaction(staging.path(), store.path(), "sample", "1.2", false, intended).unwrap();
    let index = load_skill_index(store.path()).unwrap();
    assert!(index.skills.contains_key("sample"));
    assert_eq!(fs::read_to_string(store.path().join("sample/1.2/SKILL.md")).unwrap(), "recovered");
}

#[test]
fn import_rejects_unsafe_version_before_store_changes() {
    let root = tempfile::tempdir().unwrap();
    let store = root.path().join("store");
    let sentinel = root.path().join("sentinel");
    fs::write(&sentinel, "untouched").unwrap();
    for version in ["../sentinel", "/absolute", "..", "a\\b", ""] {
        let skill = format!("---\nname: sample\ndescription: Example\nmetadata:\n  version: '{version}'\n---\nbody");
        let bytes = zip_bytes(&[("SKILL.md", skill.as_bytes())]);
        assert!(import_skill_bundle(&bytes, &store).is_err(), "{version:?}");
        assert!(!store.exists());
        assert_eq!(fs::read_to_string(&sentinel).unwrap(), "untouched");
    }
}
