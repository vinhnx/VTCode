use std::path::Path;

/// Infer sensible default verify commands for a workspace by inspecting
/// well-known project manifests.
///
/// Returns one command per detected language toolchain. For monorepos with
/// multiple languages, all matching commands are returned so verification
/// covers the full stack. Priority order for single-language projects:
/// Rust, Python, Node.js, Go, Java, Ruby, .NET.
pub fn infer_default_verify_commands(workspace_root: &Path) -> Vec<String> {
    let mut commands = Vec::new();

    if workspace_root.join("Cargo.toml").exists() {
        commands.push("cargo check".to_string());
    }
    if workspace_root.join("pytest.ini").exists()
        || workspace_root.join("pyproject.toml").exists()
        || workspace_root.join("setup.py").exists()
    {
        commands.push("pytest".to_string());
    }
    if workspace_root.join("package.json").exists() {
        commands.push("npm test".to_string());
    }
    if workspace_root.join("go.mod").exists() {
        commands.push("go test ./...".to_string());
    }
    if workspace_root.join("pom.xml").exists() {
        commands.push("mvn test".to_string());
    } else if workspace_root.join("build.gradle").exists()
        || workspace_root.join("build.gradle.kts").exists()
    {
        commands.push("gradle test".to_string());
    }
    if workspace_root.join("Gemfile").exists() {
        if workspace_root.join("spec").is_dir() {
            commands.push("bundle exec rspec".to_string());
        } else {
            commands.push("bundle exec rake test".to_string());
        }
    }
    if has_dotnet_manifest(workspace_root) {
        commands.push("dotnet test".to_string());
    }

    commands
}

/// Check if the workspace contains .NET project indicators.
///
/// Looks for a `.sln` file at the workspace root (the standard top-level
/// artifact for .NET solutions) or a `.csproj` file in immediate
/// subdirectories (common for single-project repos).
fn has_dotnet_manifest(dir: &Path) -> bool {
    // .sln files are always at the solution root.
    if dir
        .read_dir()
        .ok()
        .and_then(|mut entries| {
            entries.find_map(|entry| {
                let entry = entry.ok()?;
                let name = entry.file_name();
                if name.to_string_lossy().ends_with(".sln") {
                    Some(())
                } else {
                    None
                }
            })
        })
        .is_some()
    {
        return true;
    }

    // Check immediate subdirectories for .csproj (single-project layout).
    let Ok(root_entries) = dir.read_dir() else {
        return false;
    };
    for entry in root_entries.flatten() {
        if !entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false) {
            continue;
        }
        let subdir = entry.path();
        if subdir
            .read_dir()
            .ok()
            .and_then(|mut entries| {
                entries.find_map(|e| {
                    let e = e.ok()?;
                    if e.file_name().to_string_lossy().ends_with(".csproj") {
                        Some(())
                    } else {
                        None
                    }
                })
            })
            .is_some()
        {
            return true;
        }
    }

    false
}
