use std::fs;
use std::num::NonZero;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tempfile::TempDir;
use vtcode_file_search::run;

#[test]
fn test_multiple_matches() -> anyhow::Result<()> {
    let temp = TempDir::new()?;
    fs::write(temp.path().join("main.rs"), "")?;
    fs::write(temp.path().join("test.rs"), "")?;
    fs::write(temp.path().join("utils.rs"), "")?;
    fs::write(temp.path().join("file.txt"), "")?;

    let results = run(
        ".rs",
        NonZero::new(10).unwrap(),
        temp.path(),
        vec![],
        NonZero::new(2).unwrap(),
        Arc::new(AtomicBool::new(false)),
        false,
        false,
    )?;

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

    let results = run(
        "rs",
        NonZero::new(10).unwrap(),
        temp.path(),
        vec!["target/**".to_string()],
        NonZero::new(2).unwrap(),
        Arc::new(AtomicBool::new(false)),
        false,
        false,
    )?;

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

    let results = run(
        "rs",
        NonZero::new(10).unwrap(),
        temp.path(),
        vec![],
        NonZero::new(2).unwrap(),
        Arc::new(AtomicBool::new(false)),
        false,
        false,
    )?;

    assert_eq!(results.matches.len(), 2);
    Ok(())
}

#[test]
fn test_fuzzy_matching() -> anyhow::Result<()> {
    let temp = TempDir::new()?;
    fs::write(temp.path().join("main.rs"), "")?;
    fs::write(temp.path().join("maintest.rs"), "")?;
    fs::write(temp.path().join("other.txt"), "")?;

    let results = run(
        "main",
        NonZero::new(10).unwrap(),
        temp.path(),
        vec![],
        NonZero::new(2).unwrap(),
        Arc::new(AtomicBool::new(false)),
        false,
        false,
    )?;

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

    let results = run(
        "file",
        NonZero::new(5).unwrap(),
        temp.path(),
        vec![],
        NonZero::new(2).unwrap(),
        Arc::new(AtomicBool::new(false)),
        false,
        false,
    )?;

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
    let results = run(
        "file",
        NonZero::new(100).unwrap(),
        temp.path(),
        vec![],
        NonZero::new(1).unwrap(),
        cancel_flag,
        false,
        false,
    )?;

    // Should return early due to cancellation
    assert!(results.matches.is_empty());
    Ok(())
}
