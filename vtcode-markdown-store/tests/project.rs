use tempfile::TempDir;
use vtcode_markdown_store::{SimpleCache, SimpleProjectManager};

#[test]
fn test_simple_project_manager_initialization() {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let manager = SimpleProjectManager::new(temp_dir.path().to_path_buf());

    manager
        .init()
        .expect("Failed to initialize project manager");

    let data_dir = manager.project_data_dir("sample");
    assert!(data_dir.starts_with(temp_dir.path()));
    assert!(manager.workspace_root().starts_with(temp_dir.path()));
}

#[test]
fn test_create_and_load_project() {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let manager = SimpleProjectManager::new(temp_dir.path().to_path_buf());
    manager
        .init()
        .expect("Failed to initialize project manager");

    manager
        .create_project("demo", Some("Demo project"))
        .expect("Failed to create project");

    let project = manager
        .load_project("demo")
        .expect("Failed to load created project");
    assert_eq!(project.name, "demo");
    assert_eq!(project.description.as_deref(), Some("Demo project"));

    let projects = manager.list_projects().expect("Failed to list projects");
    assert!(projects.contains(&"demo".to_string()));
}

#[test]
fn test_project_identification_helpers() {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let manager = SimpleProjectManager::new(temp_dir.path().to_path_buf());
    manager
        .init()
        .expect("Failed to initialize project manager");

    let inferred = manager
        .identify_current_project()
        .expect("Failed to infer project name");
    assert_eq!(
        inferred,
        temp_dir.path().file_name().unwrap().to_str().unwrap()
    );

    manager
        .set_current_project("custom-project")
        .expect("Failed to set current project");
    let updated = manager
        .identify_current_project()
        .expect("Failed to read marker file");
    assert_eq!(updated, "custom-project");
}

#[test]
fn test_simple_cache_lifecycle() {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let cache_dir = temp_dir.path().join("cache");
    let cache = SimpleCache::new(cache_dir.clone());

    cache.init().expect("Failed to initialize cache");
    assert!(cache_dir.exists());

    cache
        .store("greeting", "hello world")
        .expect("Failed to store data");
    assert!(cache.exists("greeting"));

    let loaded = cache.load("greeting").expect("Failed to load data");
    assert_eq!(loaded, "hello world");

    let entries = cache.list().expect("Failed to list entries");
    assert!(entries.contains(&"greeting".to_string()));

    cache.clear().expect("Failed to clear cache");
    assert!(cache.list().unwrap().is_empty());
}

#[test]
fn test_project_update_flow() {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let manager = SimpleProjectManager::new(temp_dir.path().to_path_buf());
    manager
        .init()
        .expect("Failed to initialize project manager");

    manager
        .create_project("demo", Some("Original"))
        .expect("Failed to create project");

    let mut project = manager
        .load_project("demo")
        .expect("Failed to load project");
    project.description = Some("Updated description".to_string());
    project.tags.push("agent".to_string());
    project
        .metadata
        .insert("language".to_string(), "Rust".to_string());

    manager
        .update_project(&project)
        .expect("Failed to persist project changes");

    let reloaded = manager
        .load_project("demo")
        .expect("Failed to reload project");
    assert_eq!(reloaded.description.as_deref(), Some("Updated description"));
    assert!(reloaded.tags.contains(&"agent".to_string()));
    assert_eq!(reloaded.metadata.get("language"), Some(&"Rust".to_string()));
}
