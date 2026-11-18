use super::*;
use serial_test::serial;

async fn discover_with_roots(workspace: &Path, home: Option<&Path>) -> Vec<PromptTemplate> {
    discover_prompt_templates_with_options(PromptResourceOptions {
        workspace_root: workspace,
        home_dir: home.map(Path::to_path_buf),
    })
    .await
}

async fn layers_with_roots(workspace: &Path, home: Option<&Path>) -> SystemPromptLayers {
    resolve_system_prompt_layers_with_options(PromptResourceOptions {
        workspace_root: workspace,
        home_dir: home.map(Path::to_path_buf),
    })
    .await
}

async fn find_with_roots(workspace: &Path, home: Option<&Path>, name: &str) -> Option<PromptTemplate> {
    find_prompt_template_with_options(
        PromptResourceOptions {
            workspace_root: workspace,
            home_dir: home.map(Path::to_path_buf),
        },
        name,
    )
    .await
}

#[tokio::test]
#[serial]
async fn system_layers_reuse_process_wide_cache_until_cleared() {
    clear_prompt_resource_caches();

    let workspace = tempfile::TempDir::new().expect("workspace");
    let home = tempfile::TempDir::new().expect("home");
    let workspace_prompts = workspace.path().join(PROMPTS_DIR);
    std::fs::create_dir_all(&workspace_prompts).expect("workspace prompts");
    std::fs::write(workspace_prompts.join(SYSTEM_PROMPT_FILENAME), "workspace system override")
        .expect("write workspace system");

    let first = layers_with_roots(workspace.path(), Some(home.path())).await;
    assert_eq!(first.override_body.as_deref(), Some("workspace system override"));

    std::fs::remove_file(workspace_prompts.join(SYSTEM_PROMPT_FILENAME)).expect("remove workspace system");

    let second = layers_with_roots(workspace.path(), Some(home.path())).await;
    assert_eq!(second.override_body.as_deref(), Some("workspace system override"));

    clear_prompt_resource_caches();

    let third = layers_with_roots(workspace.path(), Some(home.path())).await;
    assert_eq!(third.override_body, None);
}

#[tokio::test]
#[serial]
async fn prompt_template_discovery_reuses_process_wide_cache_until_cleared() {
    clear_prompt_resource_caches();

    let workspace = tempfile::TempDir::new().expect("workspace");
    let home = tempfile::TempDir::new().expect("home");
    let workspace_templates = workspace.path().join(PROMPTS_DIR).join(TEMPLATES_DIR);
    std::fs::create_dir_all(&workspace_templates).expect("workspace templates");
    std::fs::write(workspace_templates.join("cache-test.md"), "# Cache test\n\nBody")
        .expect("write workspace template");

    let first = discover_with_roots(workspace.path(), Some(home.path())).await;
    assert!(first.iter().any(|template| template.name == "cache-test"));

    std::fs::remove_file(workspace_templates.join("cache-test.md")).expect("remove workspace template");

    let second = discover_with_roots(workspace.path(), Some(home.path())).await;
    assert!(!second.iter().any(|template| template.name == "cache-test"));
    assert!(
        find_with_roots(workspace.path(), Some(home.path()), "cache-test")
            .await
            .is_none()
    );

    clear_prompt_resource_caches();

    let third = discover_with_roots(workspace.path(), Some(home.path())).await;
    assert!(!third.iter().any(|template| template.name == "cache-test"));
    assert!(
        find_with_roots(workspace.path(), Some(home.path()), "cache-test")
            .await
            .is_none()
    );
}

#[tokio::test]
async fn system_layers_prefer_workspace_override_and_append_user_then_workspace() {
    let workspace = tempfile::TempDir::new().expect("workspace");
    let home = tempfile::TempDir::new().expect("home");

    let user_prompts = home.path().join(PROMPTS_DIR);
    let workspace_prompts = workspace.path().join(PROMPTS_DIR);
    std::fs::create_dir_all(&user_prompts).expect("user prompts");
    std::fs::create_dir_all(&workspace_prompts).expect("workspace prompts");

    std::fs::write(user_prompts.join(SYSTEM_PROMPT_FILENAME), "user system override").expect("write user system");
    std::fs::write(workspace_prompts.join(SYSTEM_PROMPT_FILENAME), "workspace system override")
        .expect("write workspace system");
    std::fs::write(user_prompts.join(APPEND_SYSTEM_PROMPT_FILENAME), "user append").expect("write user append");
    std::fs::write(workspace_prompts.join(APPEND_SYSTEM_PROMPT_FILENAME), "workspace append")
        .expect("write workspace append");

    let layers = layers_with_roots(workspace.path(), Some(home.path())).await;
    assert_eq!(layers.override_body.as_deref(), Some("workspace system override"));
    assert_eq!(layers.append_bodies, vec!["user append".to_string(), "workspace append".to_string()]);

    let composed = apply_system_prompt_layers("fallback base", &layers);
    assert_eq!(composed, "workspace system override\n\nuser append\n\nworkspace append");
}

#[test]
fn applied_text_matches_spliced_layer_bodies() {
    let layers = SystemPromptLayers {
        override_body: Some("  workspace override  ".to_string()),
        append_bodies: vec![
            "   ".to_string(),
            "user append".to_string(),
            " workspace append ".to_string(),
        ],
    };
    assert_eq!(layers.applied_text(), "workspace override\n\nuser append\n\nworkspace append");
    // The override replaces the base, so composition is exactly the applied text.
    assert_eq!(apply_system_prompt_layers("fallback base", &layers), layers.applied_text());
}

#[test]
fn applied_text_appends_follow_base_without_override() {
    let layers = SystemPromptLayers {
        override_body: None,
        append_bodies: vec!["first append".to_string(), "second append".to_string()],
    };
    assert_eq!(layers.applied_text(), "first append\n\nsecond append");
    let composed = apply_system_prompt_layers("BASE", &layers);
    assert_eq!(composed, "BASE\n\nfirst append\n\nsecond append");
}

#[test]
fn applied_text_is_empty_without_effective_layers() {
    let layers = SystemPromptLayers {
        override_body: Some("   ".to_string()),
        append_bodies: vec!["  ".to_string()],
    };
    assert!(layers.applied_text().is_empty());
    assert_eq!(apply_system_prompt_layers("fallback base", &layers), "fallback base");
    assert!(SystemPromptLayers::default().applied_text().is_empty());
}

#[tokio::test]
async fn template_discovery_prefers_workspace_and_derives_descriptions() {
    let workspace = tempfile::TempDir::new().expect("workspace");
    let home = tempfile::TempDir::new().expect("home");
    let user_templates = home.path().join(PROMPTS_DIR).join(TEMPLATES_DIR);
    let workspace_templates = workspace.path().join(PROMPTS_DIR).join(TEMPLATES_DIR);
    std::fs::create_dir_all(&user_templates).expect("user templates");
    std::fs::create_dir_all(&workspace_templates).expect("workspace templates");

    std::fs::write(user_templates.join("review.md"), "---\ndescription: User review template\n---\nReview $1")
        .expect("user review");
    std::fs::write(workspace_templates.join("review.md"), "# Workspace review\n\nReview workspace $1")
        .expect("workspace review");
    std::fs::write(workspace_templates.join("audit.md"), "First non-empty line becomes description.\n\nAudit $@")
        .expect("workspace audit");

    let templates = discover_with_roots(workspace.path(), Some(home.path())).await;
    assert_eq!(templates.len(), 2);
    assert_eq!(templates[0].name, "audit");
    assert_eq!(templates[0].description, "First non-empty line becomes description.");
    assert_eq!(templates[1].name, "review");
    assert_eq!(templates[1].description, "Workspace review");
    assert_eq!(templates[1].body, "# Workspace review\n\nReview workspace $1");
}

#[tokio::test]
async fn direct_template_lookup_uses_workspace_precedence() {
    let workspace = tempfile::TempDir::new().expect("workspace");
    let home = tempfile::TempDir::new().expect("home");
    let user_templates = home.path().join(PROMPTS_DIR).join(TEMPLATES_DIR);
    let workspace_templates = workspace.path().join(PROMPTS_DIR).join(TEMPLATES_DIR);
    std::fs::create_dir_all(&user_templates).expect("user templates");
    std::fs::create_dir_all(&workspace_templates).expect("workspace templates");

    std::fs::write(user_templates.join("review.md"), "User review body").expect("user template");
    std::fs::write(workspace_templates.join("review.md"), "Workspace review body").expect("workspace template");

    let template = find_with_roots(workspace.path(), Some(home.path()), "review")
        .await
        .expect("template");
    assert_eq!(template.body, "Workspace review body");
}

#[tokio::test]
async fn direct_template_lookup_rejects_unsafe_names() {
    let workspace = tempfile::TempDir::new().expect("workspace");
    let home = tempfile::TempDir::new().expect("home");

    let template = find_with_roots(workspace.path(), Some(home.path()), "../escape").await;
    assert!(template.is_none());
}

#[test]
fn template_expansion_supports_positional_and_all_arguments() {
    let expanded = expand_prompt_template(
        "Review $1 against $2.\nArgs: $@\nAgain: $ARGUMENTS\nMissing: '$3'",
        &["src/lib.rs".to_string(), "main".to_string()],
    );

    assert_eq!(
        expanded,
        "Review src/lib.rs against main.\nArgs: src/lib.rs main\nAgain: src/lib.rs main\nMissing: ''"
    );
}

struct TestEntry {
    timestamp: SystemTime,
    expired: bool,
}

impl ExpirableCacheEntry for TestEntry {
    fn is_expired(&self) -> bool {
        self.expired
    }

    fn timestamp(&self) -> SystemTime {
        self.timestamp
    }
}

fn full_live_cache(now: SystemTime) -> HashMap<u32, TestEntry> {
    (0..PROMPT_RESOURCE_CACHE_MAX_ENTRIES as u32)
        .map(|i| {
            (
                i,
                TestEntry {
                    // Key 0 is newest, the last key is oldest.
                    timestamp: now - Duration::from_secs((i + 1) as u64 * 60),
                    expired: false,
                },
            )
        })
        .collect()
}

#[test]
fn evict_if_full_removes_oldest_live_entry_at_budget() {
    let now = SystemTime::now();
    let mut cache = full_live_cache(now);
    assert_eq!(cache.len(), PROMPT_RESOURCE_CACHE_MAX_ENTRIES);

    evict_if_full(&mut cache, &u32::MAX);
    assert_eq!(cache.len(), PROMPT_RESOURCE_CACHE_MAX_ENTRIES - 1);
    // Oldest (last key) evicted, newest (key 0) retained.
    assert!(!cache.contains_key(&(PROMPT_RESOURCE_CACHE_MAX_ENTRIES as u32 - 1)));
    assert!(cache.contains_key(&0));
}

#[test]
fn evict_if_full_prefers_expired_over_oldest_live_and_keeps_present_keys() {
    let now = SystemTime::now();
    let mut cache = full_live_cache(now);
    // Expire the newest entry: it must go before the oldest live one.
    cache.insert(0, TestEntry { timestamp: now, expired: true });

    evict_if_full(&mut cache, &u32::MAX);
    assert!(!cache.contains_key(&0));
    assert!(cache.contains_key(&(PROMPT_RESOURCE_CACHE_MAX_ENTRIES as u32 - 1)));

    // A present key at full budget means in-place update: no eviction.
    let mut full = full_live_cache(now);
    evict_if_full(&mut full, &0);
    assert_eq!(full.len(), PROMPT_RESOURCE_CACHE_MAX_ENTRIES);

    // Under budget: no-op for a new key.
    full.remove(&0);
    evict_if_full(&mut full, &u32::MAX);
    assert_eq!(full.len(), PROMPT_RESOURCE_CACHE_MAX_ENTRIES - 1);
    assert!(full.contains_key(&1));
}

#[test]
fn newest_mtime_is_order_independent_and_ignores_missing_paths() {
    let dir = tempfile::TempDir::new().expect("tempdir");
    let older = dir.path().join("older.md");
    let newer = dir.path().join("newer.md");
    std::fs::write(&older, "old").expect("write older");
    std::fs::write(&newer, "new").expect("write newer");
    let base = SystemTime::now();
    std::fs::File::options()
        .write(true)
        .open(&older)
        .expect("open older")
        .set_modified(base - Duration::from_secs(100))
        .expect("set older mtime");
    std::fs::File::options()
        .write(true)
        .open(&newer)
        .expect("open newer")
        .set_modified(base)
        .expect("set newer mtime");
    let missing = dir.path().join("missing.md");

    // Newer file wins regardless of argument order.
    let forward = newest_mtime([older.as_path(), newer.as_path()]);
    let reversed = newest_mtime([newer.as_path(), older.as_path()]);
    assert_eq!(forward, reversed);
    assert!(forward > base - Duration::from_secs(100));

    // Missing paths never contribute: same result with or without them,
    // and a lone missing path degrades to UNIX_EPOCH.
    assert_eq!(newest_mtime([missing.as_path(), older.as_path(), newer.as_path()]), forward);
    assert_eq!(newest_mtime([missing.as_path()]), SystemTime::UNIX_EPOCH);
    assert_eq!(newest_mtime(Vec::<&Path>::new()), SystemTime::UNIX_EPOCH);
}
