use anyhow::{Context, Result};
use indexmap::IndexMap;
use tempfile::tempdir;
use tokio::task;
use vtcode_core::markdown_storage::{
    MarkdownStorage, MarkdownStorageOptions, ProjectData, ProjectStorage, SimpleKVStorage,
};

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct SessionRecord {
    project: String,
    last_command: String,
    notes: Vec<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("=== Markdown storage workflow ===");

    let workspace = tempdir().context("failed to create temporary workspace")?;
    let ledger_root = workspace.path().join("ledger");
    let options = MarkdownStorageOptions::default().with_extension("ledger");

    let storage = MarkdownStorage::with_options(ledger_root.clone(), options.clone());
    storage.init()?;

    let session = SessionRecord {
        project: "sample-app".to_string(),
        last_command: "cargo test".to_string(),
        notes: vec!["Investigate failing snapshot".to_string()],
    };

    storage.store("session-001", &session, "Session 001")?;
    let restored: SessionRecord = storage.load("session-001")?;
    println!("Restored session for project: {}", restored.project);

    let kv_root = workspace.path().join("kv");
    let kv_storage = SimpleKVStorage::with_options(kv_root, options.clone());
    kv_storage.init()?;
    kv_storage.put("api_key", "example-secret")?;
    kv_storage.put("run_mode", "analysis")?;
    println!("Stored keys: {:?}", kv_storage.list_keys()?);

    let projects_root = workspace.path().join("projects");
    let project_storage = ProjectStorage::with_options(projects_root, options);
    project_storage.init()?;

    let mut project = ProjectData::new("sample-app");
    project.description = Some("Demo project for markdown storage".to_string());
    project.tags.push("example".to_string());
    project
        .metadata
        .insert("language".to_string(), "rust".to_string());

    save_project_async(project_storage.clone(), project.clone()).await?;
    let summary = load_project_metadata_async(project_storage.clone()).await?;
    println!("Project metadata fields: {:?}", summary);

    let ledger_entries = storage.list()?;
    println!("Available ledger entries: {:?}", ledger_entries);

    Ok(())
}

async fn save_project_async(storage: ProjectStorage, project: ProjectData) -> Result<()> {
    task::spawn_blocking(move || storage.save_project(&project))
        .await
        .context("project save task failed")?
}

async fn load_project_metadata_async(storage: ProjectStorage) -> Result<IndexMap<String, String>> {
    task::spawn_blocking(move || {
        let project = storage.load_project("sample-app")?;
        Ok(project.metadata)
    })
    .await
    .context("project load task failed")?
}
