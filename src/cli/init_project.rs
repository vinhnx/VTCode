//! Init-project command implementation

use anyhow::{Context, Result};
use std::fs;
use std::path::Path;
use vtcode_core::utils::colors::style;
use vtcode_core::{ProjectData, SimpleProjectManager};
use walkdir::WalkDir;

/// Handle the init-project command
pub async fn handle_init_project_command(
    name: Option<String>,
    force: bool,
    migrate: bool,
) -> Result<()> {
    println!(
        "{}",
        style("Initialize project with dot-folder structure")
            .blue()
            .bold()
    );

    // Initialize project manager
    let project_manager = SimpleProjectManager::new(std::env::current_dir()?);
    project_manager.init()?;

    // Determine project name
    let project_name = if let Some(name) = name {
        name
    } else {
        // Use current directory name
        let current_dir = std::env::current_dir()?;
        current_dir
            .file_name()
            .and_then(|name| name.to_str())
            .map(|name| name.to_string())
            .unwrap_or_else(|| "unnamed-project".to_string())
    };

    println!("Project name: {}", project_name);

    // Check if project already exists
    let project_dir = project_manager.project_data_dir(&project_name);
    if project_dir.exists() && !force {
        println!(
            "{} Project directory already exists: {}",
            style("Warning").yellow(),
            project_dir.display()
        );
        println!("Use --force to overwrite existing project structure.");
        return Ok(());
    }

    // Create project structure
    project_manager.create_project(&project_name, Some("VTCode project"))?;
    println!(
        "{} Created project structure in: {}",
        style("Success").green(),
        project_dir.display()
    );

    // Create or update project metadata
    let current_dir = std::env::current_dir()?;
    let mut metadata = ProjectData::new(&project_name);
    metadata.description = Some("VTCode project".to_string());
    project_manager.update_project(&metadata)?;
    println!("{} Created project metadata", style("Success").green());

    // Migrate existing files if requested
    if migrate {
        migrate_existing_files(&project_manager, &project_name, &current_dir).await?;
    }

    println!(
        "\n{} Project initialization completed!",
        style("Success").green().bold()
    );
    println!("Project structure created at: {}", project_dir.display());
    println!(
        "Configuration directory: {}",
        project_manager.config_dir(&project_name).display()
    );
    println!(
        "Cache directory: {}",
        project_manager.cache_dir(&project_name).display()
    );

    Ok(())
}

/// Migrate existing config/cache files to the new project structure
async fn migrate_existing_files(
    _project_manager: &SimpleProjectManager,
    _project_name: &str,
    current_dir: &Path,
) -> Result<()> {
    println!(
        "\n{} Checking for existing config/cache files to migrate...",
        style("Info").blue()
    );

    let mut files_to_migrate = Vec::new();

    // Check for vtcode.toml in current directory
    let local_config = current_dir.join("vtcode.toml");
    if local_config.exists() {
        files_to_migrate.push(("vtcode.toml", local_config.clone()));
    }

    // Check for .vtcode directory
    let local_vtcode = current_dir.join(".vtcode");
    if local_vtcode.exists() && local_vtcode.is_dir() {
        // Look for config files in .vtcode directory
        let vtcode_config = local_vtcode.join("vtcode.toml");
        if vtcode_config.exists() {
            files_to_migrate.push(("vtcode.toml (from .vtcode)", vtcode_config));
        }

        let vtcode_gitignore = local_vtcode.join(".vtcodegitignore");
        if vtcode_gitignore.exists() {
            files_to_migrate.push((".vtcodegitignore (from .vtcode)", vtcode_gitignore));
        }
    }

    // Check for common cache directories
    let cache_dirs = ["cache", ".cache"];
    for cache_dir_name in &cache_dirs {
        let cache_dir = current_dir.join(cache_dir_name);
        if cache_dir.exists() && cache_dir.is_dir() {
            files_to_migrate.push((cache_dir_name, cache_dir));
        }
    }

    // Check for common config directories
    let config_dirs = ["config", ".config"];
    for config_dir_name in &config_dirs {
        let config_dir = current_dir.join(config_dir_name);
        if config_dir.exists() && config_dir.is_dir() {
            files_to_migrate.push((config_dir_name, config_dir));
        }
    }

    if files_to_migrate.is_empty() {
        println!("No existing config/cache files found to migrate.");
        return Ok(());
    }

    println!("Found {} items to migrate:", files_to_migrate.len());
    for (name, path) in &files_to_migrate {
        println!("  - {} ({})", name, path.display());
    }

    for (name, path) in files_to_migrate {
        let destination = if name.contains("cache") {
            _project_manager.cache_dir(_project_name).join(name)
        } else if name.contains("vtcode.toml") {
            _project_manager
                .config_dir(_project_name)
                .join("vtcode.toml")
        } else {
            _project_manager.project_data_dir(_project_name).join(name)
        };

        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!(
                    "failed to create destination directory {}",
                    parent.display()
                )
            })?;
        }

        if destination.exists() {
            let backup = destination.with_extension("bak");
            fs::rename(&destination, &backup)
                .with_context(|| format!("failed to backup {}", destination.display()))?;
        }

        if path.is_dir() {
            for entry in WalkDir::new(&path).into_iter().filter_map(|e| e.ok()) {
                let file_path = entry.path();
                let relative = file_path.strip_prefix(&path).unwrap_or(file_path);
                let dest_path = destination.join(relative);
                if entry.file_type().is_dir() {
                    fs::create_dir_all(&dest_path).with_context(|| {
                        format!("failed to create directory {}", dest_path.display())
                    })?;
                } else {
                    if let Some(parent) = dest_path.parent() {
                        fs::create_dir_all(parent).with_context(|| {
                            format!("failed to create directory {}", parent.display())
                        })?;
                    }
                    fs::copy(file_path, &dest_path).with_context(|| {
                        format!(
                            "failed to copy {} to {}",
                            file_path.display(),
                            dest_path.display()
                        )
                    })?;
                }
            }
        } else {
            fs::copy(&path, &destination).with_context(|| {
                format!(
                    "failed to copy {} to {}",
                    path.display(),
                    destination.display()
                )
            })?;
        }
    }

    println!("Migration completed successfully.");
    Ok(())
}
