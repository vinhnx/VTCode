use super::*;
use crate::tools::grep_file::GrepSearchManager;
use crate::tools::traits::Tool;
use crate::tools::types::ListInput;
use std::collections::BTreeSet;
use std::fs;
use tempfile::TempDir;

fn make_tool(workspace: &TempDir) -> FileOpsTool {
    FileOpsTool::new(
        workspace.path().to_path_buf(),
        std::sync::Arc::new(GrepSearchManager::new(workspace.path().to_path_buf())),
    )
}

fn names(result: &Value) -> BTreeSet<String> {
    result["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["name"].as_str().unwrap().to_string())
        .collect()
}

#[tokio::test]
async fn basic_list_filters_and_classification_survive_cache_hits() {
    let workspace = TempDir::new().unwrap();
    let directory = workspace.path().join("listing");
    fs::create_dir(&directory).unwrap();
    for name in ["Alpha.rs", "alphabet.txt", "beta.rs", ".hidden.rs"] {
        fs::write(directory.join(name), "fixture").unwrap();
    }
    fs::create_dir(directory.join("Alpha_dir")).unwrap();
    let tool = make_tool(&workspace);
    let cases = [
        (json!({}), vec!["Alpha.rs", "alphabet.txt", "beta.rs", "Alpha_dir"]),
        (json!({"include_hidden": true, "pattern": "*.rs"}), vec!["Alpha.rs", "beta.rs", ".hidden.rs"]),
        (json!({"pattern": "*.rs", "name_pattern": "no-match"}), vec!["Alpha.rs", "beta.rs"]),
        (json!({"name_pattern": "Alpha"}), vec!["Alpha.rs", "Alpha_dir"]),
        (
            json!({"name_pattern": "alpha", "case_sensitive": false}),
            vec!["Alpha.rs", "alphabet.txt", "Alpha_dir"],
        ),
        (json!({"name_pattern": "alpha", "case_sensitive": true}), vec!["alphabet.txt"]),
    ];
    for (extra, expected) in cases {
        let mut args = json!({"path": "listing", "mode": "list", "max_items": 50});
        args.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        let result = tool.execute(args.clone()).await.unwrap();
        assert_eq!(names(&result), expected.into_iter().map(str::to_string).collect());
        assert_eq!(result["total"], names(&result).len());
        for item in result["items"].as_array().unwrap() {
            let name = item["name"].as_str().unwrap();
            assert_eq!(item["path"], format!("listing/{name}"));
            assert_eq!(item["type"], if name == "Alpha_dir" { "directory" } else { "file" });
        }
        assert_eq!(tool.execute(args).await.unwrap(), result);
    }
}

#[tokio::test]
async fn basic_list_pagination_crosses_read_dir_batch_boundary() {
    for count in [31_usize, 32, 33, 65] {
        let workspace = TempDir::new().unwrap();
        fs::create_dir(workspace.path().join("listing")).unwrap();
        let expected: BTreeSet<_> = (0..count).map(|index| format!("item_{index:02}.txt")).collect();
        for name in &expected {
            fs::write(workspace.path().join("listing").join(name), "fixture").unwrap();
        }
        let tool = make_tool(&workspace);
        let mut seen = BTreeSet::new();
        for page in 1..=count.div_ceil(32) + 1 {
            let result = tool
                .execute(json!({"path": "listing", "max_items": 100, "per_page": 32, "page": page}))
                .await
                .unwrap();
            let page_names = names(&result);
            assert!(seen.is_disjoint(&page_names));
            seen.extend(page_names);
            assert_eq!(result["total"], count);
            assert_eq!(result["count"], count.saturating_sub((page - 1) * 32).min(32));
            assert_eq!(result["has_more"], page * 32 < count);
            assert_eq!(result["page"], page);
        }
        assert_eq!(seen, expected);
        let capped = tool
            .execute(json!({"path": "listing", "max_items": 32, "per_page": 20, "page": 2}))
            .await
            .unwrap();
        assert_eq!(capped["total"], count.min(32));
        assert_eq!(capped["count"], count.min(32) - 20);
        assert_eq!(capped["has_more"], false);
        if count > 32 {
            assert_eq!(capped["message"], format!("[+{} more items]", count - 32));
        }
    }
}

#[test]
fn basic_list_cache_keys_separate_all_response_dimensions() {
    let workspace = TempDir::new().unwrap();
    let tool = make_tool(&workspace);
    let mut keys = BTreeSet::new();
    for extra in [
        json!({}),
        json!({"path": "different"}),
        json!({"include_hidden": true}),
        json!({"pattern": "*.rs"}),
        json!({"name_pattern": "alpha"}),
        json!({"case_sensitive": false}),
        json!({"max_items": 9}),
        json!({"page": 2}),
        json!({"per_page": 7}),
        json!({"response_format": "detailed"}),
    ] {
        let mut args = json!({"path": "listing"});
        args.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        let input: ListInput = serde_json::from_value(args).unwrap();
        assert!(keys.insert(tool.directory_cache_key(&input)));
    }
    let other_workspace = TempDir::new().unwrap();
    assert!(keys.insert(
        make_tool(&other_workspace).directory_cache_key(&serde_json::from_value(json!({"path": "listing"})).unwrap())
    ));
}

#[tokio::test]
async fn basic_list_file_filters_missing_path_and_workspace_alias() {
    let workspace = TempDir::new().unwrap();
    fs::create_dir(workspace.path().join("listing")).unwrap();
    fs::write(workspace.path().join("listing/Only.rs"), "fixture").unwrap();
    let tool = make_tool(&workspace);
    let normal = tool.execute(json!({"path": "listing"})).await.unwrap();
    let alias = tool.execute(json!({"path": "/workspace/listing"})).await.unwrap();
    assert_eq!(alias, normal);
    assert_eq!(names(&normal), BTreeSet::from(["Only.rs".to_string()]));
    let file = tool
        .execute(json!({"path": "listing/Only.rs", "pattern": "*.rs"}))
        .await
        .unwrap();
    assert_eq!(file["items"][0]["type"], "file");
    assert_eq!(file["total"], 1);
    let filtered = tool
        .execute(json!({"path": "listing/Only.rs", "pattern": "*.txt"}))
        .await
        .unwrap();
    assert_eq!(filtered["total"], 0);
    assert_eq!(filtered["items"], json!([]));
    let missing = tool.execute(json!({"path": "absent-dir"})).await.unwrap_err();
    assert!(missing.to_string().contains("does not exist"));
    let invalid = tool.execute(json!({"path": "listing", "pattern": "["})).await.unwrap_err();
    assert!(invalid.to_string().contains("invalid list glob"));
}

#[cfg(unix)]
#[tokio::test]
async fn basic_list_preserves_symlink_classification_and_rejects_escape_targets() {
    let workspace = TempDir::new().unwrap();
    let outside = TempDir::new().unwrap();
    let listing = workspace.path().join("listing");
    fs::create_dir(&listing).unwrap();
    fs::create_dir(workspace.path().join("target-dir")).unwrap();
    fs::write(outside.path().join("secret.txt"), "secret").unwrap();
    std::os::unix::fs::symlink(workspace.path().join("target-dir"), listing.join("inside")).unwrap();
    std::os::unix::fs::symlink(outside.path(), listing.join("escape")).unwrap();
    std::os::unix::fs::symlink("missing-target", listing.join("dangling")).unwrap();
    let tool = make_tool(&workspace);
    let result = tool.execute(json!({"path": "listing"})).await.unwrap();
    assert_eq!(names(&result), BTreeSet::from(["inside".into(), "escape".into(), "dangling".into()]));
    for item in result["items"].as_array().unwrap() {
        assert_eq!(item["type"], "file");
        match item["name"].as_str().unwrap() {
            "inside" => assert_eq!(item["path"], "target-dir"),
            name => assert_eq!(
                item["path"],
                dunce::canonicalize(&listing).unwrap().join(name).to_string_lossy().into_owned()
            ),
        }
    }
    for path in ["listing/escape", "listing/escape/secret.txt", "listing/dangling"] {
        assert!(tool.execute(json!({"path": path})).await.is_err(), "{path} must be rejected");
    }
    assert_eq!(tool.execute(json!({"path": "listing/inside"})).await.unwrap()["total"], 0);
}

#[tokio::test]
async fn missing_list_path_suggests_similar_workspace_path() {
    let temp_dir = TempDir::new().expect("workspace tempdir");
    fs::create_dir_all(temp_dir.path().join("src/agent")).expect("create agent dir");
    let grep_manager = std::sync::Arc::new(GrepSearchManager::new(temp_dir.path().to_path_buf()));
    let file_ops = FileOpsTool::new(temp_dir.path().to_path_buf(), grep_manager);

    let err = file_ops
        .execute_basic_list(&ListInput {
            path: "src/agnt".to_string(),
            max_items: 20,
            page: None,
            per_page: None,
            response_format: None,
            include_hidden: false,
            glob_pattern: None,
            mode: None,
            name_pattern: None,
            content_pattern: None,
            file_extensions: None,
            case_sensitive: None,
        })
        .await
        .expect_err("missing path should fail")
        .to_string();

    assert!(err.contains("Did you mean"));
    assert!(err.contains("src/agent"));
}

#[tokio::test]
async fn basic_list_cache_respects_filter_shape() {
    let temp_dir = TempDir::new().expect("workspace tempdir");
    let list_dir = temp_dir.path().join("cache-key-shape");
    fs::create_dir_all(&list_dir).expect("create list dir");
    fs::write(list_dir.join("alpha.rs"), "alpha").expect("write alpha");
    fs::write(list_dir.join("beta.rs"), "beta").expect("write beta");

    let grep_manager = std::sync::Arc::new(GrepSearchManager::new(temp_dir.path().to_path_buf()));
    let file_ops = FileOpsTool::new(temp_dir.path().to_path_buf(), grep_manager);
    let base_input = |name_pattern| ListInput {
        path: "cache-key-shape".to_string(),
        max_items: 20,
        page: None,
        per_page: None,
        response_format: None,
        include_hidden: false,
        glob_pattern: None,
        mode: None,
        name_pattern,
        content_pattern: None,
        file_extensions: None,
        case_sensitive: None,
    };

    let alpha = file_ops
        .execute_basic_list(&base_input(Some("alpha".to_string())))
        .await
        .expect("alpha list should succeed");
    assert_eq!(alpha["items"][0]["name"], "alpha.rs");

    let beta = file_ops
        .execute_basic_list(&base_input(Some("beta".to_string())))
        .await
        .expect("beta list should succeed");
    assert_eq!(beta["items"][0]["name"], "beta.rs");
}

fn matcher_input(name_pattern: Option<&str>, case_sensitive: Option<bool>) -> ListInput {
    ListInput {
        path: String::new(),
        max_items: 20,
        page: None,
        per_page: None,
        response_format: None,
        include_hidden: false,
        glob_pattern: None,
        mode: None,
        name_pattern: name_pattern.map(str::to_string),
        content_pattern: None,
        file_extensions: None,
        case_sensitive,
    }
}

#[test]
fn name_matcher_wildcard_and_empty_pattern_match_everything() {
    for input in [
        matcher_input(None, None),
        matcher_input(None, Some(false)),
        matcher_input(Some("*"), None),
        matcher_input(Some("*"), Some(false)),
        // `str::contains("")` is always true; pin that an empty pattern matches all.
        matcher_input(Some(""), None),
        matcher_input(Some(""), Some(false)),
    ] {
        let matcher = ListNameMatcher::new(&input);
        assert!(matcher.matches_name("Alpha.rs"));
        assert!(matcher.matches_name("alphabet.txt"));
        assert!(matcher.matches_name(""));
    }
}

#[test]
fn name_matcher_case_sensitive_contains_is_exact_case() {
    let input = matcher_input(Some("Alpha"), Some(true));
    let matcher = ListNameMatcher::new(&input);
    assert!(matcher.matches_name("Alpha.rs"));
    assert!(matcher.matches_name("Alpha_dir"));
    assert!(!matcher.matches_name("alphabet.txt"));

    // Asymmetric direction: a lowercase pattern must not match uppercase names.
    let input = matcher_input(Some("alpha"), None);
    let matcher = ListNameMatcher::new(&input);
    assert!(!matcher.matches_name("Alpha.rs"));
    assert!(matcher.matches_name("alphabet.txt"));
}

#[test]
fn name_matcher_case_insensitive_contains_ignores_case_once_per_call() {
    for pattern in ["alpha", "ALPHA", "AlPhA"] {
        let input = matcher_input(Some(pattern), Some(false));
        let matcher = ListNameMatcher::new(&input);
        // The lowered pattern is precomputed at construction, not per entry.
        assert_eq!(matcher.lowered_pattern.as_deref(), Some("alpha"));
        assert!(matcher.matches_name("Alpha.rs"));
        assert!(matcher.matches_name("alphabet.txt"));
        assert!(matcher.matches_name("Alpha_dir"));
        assert!(!matcher.matches_name("beta.rs"));
    }
}

#[test]
fn name_matcher_case_insensitive_unicode_keeps_full_lowercase_semantics() {
    let input = matcher_input(Some("ÉCOLE"), Some(false));
    let matcher = ListNameMatcher::new(&input);
    assert!(matcher.matches_name("école.txt"));
    assert!(!matcher.matches_name("school.txt"));

    // Asymmetric direction with the same folded form.
    let input = matcher_input(Some("école"), Some(false));
    let matcher = ListNameMatcher::new(&input);
    assert!(matcher.matches_name("ÉCOLE.txt"));

    // Case-sensitive matching must not fold at all.
    let input = matcher_input(Some("ÉCOLE"), Some(true));
    let matcher = ListNameMatcher::new(&input);
    assert!(!matcher.matches_name("école.txt"));
    assert!(matcher.matches_name("ÉCOLE.txt"));
}

#[tokio::test]
async fn find_by_name_matches_exact_case_and_folds_when_insensitive() {
    let workspace = TempDir::new().unwrap();
    fs::write(workspace.path().join("Cargo.toml"), "fixture").unwrap();
    fs::write(workspace.path().join("README.md"), "fixture").unwrap();
    let tool = make_tool(&workspace);

    let find = |name_pattern: &str, case_sensitive: Option<bool>| {
        let mut args = json!({"path": ".", "mode": "find_name", "name_pattern": name_pattern});
        if let Some(case_sensitive) = case_sensitive {
            args["case_sensitive"] = json!(case_sensitive);
        }
        tool.execute(args)
    };

    // Exact case-sensitive hit (default).
    let hit = find("Cargo.toml", None).await.unwrap();
    assert_eq!(hit["found"], true);
    assert_eq!(hit["name"], "Cargo.toml");

    // Wrong case misses when sensitive...
    let miss = find("cargo.toml", Some(true)).await.unwrap();
    assert_eq!(miss["found"], false);

    // ...but hits in both directions when insensitive.
    for pattern in ["cargo.toml", "CARGO.TOML"] {
        let folded = find(pattern, Some(false)).await.unwrap();
        assert_eq!(folded["found"], true, "pattern: {pattern}");
        assert_eq!(folded["name"], "Cargo.toml");
    }

    // Unknown name misses.
    let unknown = find("nope.txt", None).await.unwrap();
    assert_eq!(unknown["found"], false);
}
