use std::fs;
use std::num::NonZero;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tempfile::TempDir;
use vtcode_file_search::{run, FileSearchConfig};

#[test]
fn test_multiple_matches() -> anyhow::Result<()> {
    let temp = TempDir::new()?;
    fs::write(temp.path().join("main.rs"), "")?;
    fs::write(temp.path().join("test.rs"), "")?;
    fs::write(temp.path().join("utils.rs"), "")?;
    fs::write(temp.path().join("file.txt"), "")?;

    let results = run(FileSearchConfig {
        pattern_text: ".rs".to_string(),
        limit: NonZero::new(10).unwrap(),
        search_directory: temp.path().to_path_buf(),
        exclude: vec![],
        threads: NonZero::new(2).unwrap(),
        cancel_flag: Arc::new(AtomicBool::new(false)),
        compute_indices: false,
        respect_gitignore: false,
    })?;

    assert_eq!(results.matches.len(), 3);
    assert!(results.matches.iter().all(|m| m.path.ends_with(".rs")));
    Ok(())
}

#[test]
fn test_exclusion_patterns() -> anyhow::Result<()> {
    let temp = TempDir::new()?;
    fs::write(temp.path().join("keep.rs"), "")?;
    fs::create_dir(temp.path().join("target"))?;
    fs::write(temp.path().join("target/ignore.rs"), "")?;

    let results = run(FileSearchConfig {
        pattern_text: "rs".to_string(),
        limit: NonZero::new(10).unwrap(),
        search_directory: temp.path().to_path_buf(),
        exclude: vec!["target/**".to_string()],
        threads: NonZero::new(2).unwrap(),
        cancel_flag: Arc::new(AtomicBool::new(false)),
        compute_indices: false,
        respect_gitignore: false,
    })?;

    assert_eq!(results.matches.len(), 1);
    assert!(results.matches[0].path.contains("keep.rs"));
    Ok(())
}

#[test]
fn test_nested_directories() -> anyhow::Result<()> {
    let temp = TempDir::new()?;
    fs::create_dir(temp.path().join("src"))?;
    fs::create_dir(temp.path().join("src/utils"))?;
    fs::write(temp.path().join("src/main.rs"), "")?;
    fs::write(temp.path().join("src/utils/helper.rs"), "")?;

    let results = run(FileSearchConfig {
        pattern_text: "rs".to_string(),
        limit: NonZero::new(10).unwrap(),
        search_directory: temp.path().to_path_buf(),
        exclude: vec![],
        threads: NonZero::new(2).unwrap(),
        cancel_flag: Arc::new(AtomicBool::new(false)),
        compute_indices: false,
        respect_gitignore: false,
    })?;

    assert_eq!(results.matches.len(), 2);
    Ok(())
}

#[test]
fn test_fuzzy_matching() -> anyhow::Result<()> {
    let temp = TempDir::new()?;
    fs::write(temp.path().join("main.rs"), "")?;
    fs::write(temp.path().join("maintest.rs"), "")?;
    fs::write(temp.path().join("other.txt"), "")?;

    let results = run(FileSearchConfig {
        pattern_text: "main".to_string(),
        limit: NonZero::new(10).unwrap(),
        search_directory: temp.path().to_path_buf(),
        exclude: vec![],
        threads: NonZero::new(2).unwrap(),
        cancel_flag: Arc::new(AtomicBool::new(false)),
        compute_indices: false,
        respect_gitignore: false,
    })?;

    // Both main.rs and maintest.rs should match
    assert!(results.matches.len() >= 1);
    Ok(())
}

#[test]
fn test_limit_respects_count() -> anyhow::Result<()> {
    let temp = TempDir::new()?;
    for i in 0..20 {
        fs::write(temp.path().join(format!("file{}.rs", i)), "")?;
    }

    let results = run(FileSearchConfig {
        pattern_text: "file".to_string(),
        limit: NonZero::new(5).unwrap(),
        search_directory: temp.path().to_path_buf(),
        exclude: vec![],
        threads: NonZero::new(2).unwrap(),
        cancel_flag: Arc::new(AtomicBool::new(false)),
        compute_indices: false,
        respect_gitignore: false,
    })?;

    assert_eq!(results.matches.len(), 5);
    assert!(results.total_match_count >= 5);
    Ok(())
}

#[test]
fn test_cancellation_stops_search() -> anyhow::Result<()> {
    let temp = TempDir::new()?;
    for i in 0..100 {
        fs::write(temp.path().join(format!("file{}.rs", i)), "")?;
    }

    let cancel_flag = Arc::new(AtomicBool::new(true));
    let results = run(FileSearchConfig {
        pattern_text: "file".to_string(),
        limit: NonZero::new(100).unwrap(),
        search_directory: temp.path().to_path_buf(),
        exclude: vec![],
        threads: NonZero::new(1).unwrap(),
        cancel_flag,
        compute_indices: false,
        respect_gitignore: false,
    })?;

    // Should return early due to cancellation
    assert!(results.matches.is_empty());
    Ok(())
}
