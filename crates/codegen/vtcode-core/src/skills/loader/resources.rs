use std::fs;
use std::path::Path;

use anyhow::Result;

pub fn load_skill_resources(skill_path: &Path) -> Result<Vec<crate::skills::types::SkillResource>> {
    let mut resources = Vec::new();
    let resource_dir = skill_path.join("scripts");

    if resource_dir.exists() {
        for entry in fs::read_dir(&resource_dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.is_file() {
                let rel_path = path
                    .strip_prefix(skill_path)
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_default();

                let resource_type = match path.extension().and_then(|e| e.to_str()) {
                    Some("py") | Some("sh") | Some("bash") => crate::skills::types::ResourceType::Script,
                    Some("md") => crate::skills::types::ResourceType::Markdown,
                    Some("json") | Some("yaml") | Some("yml") => crate::skills::types::ResourceType::Reference,
                    _ => crate::skills::types::ResourceType::Other(format!("{:?}", path.extension())),
                };

                resources.push(crate::skills::types::SkillResource { path: rel_path, resource_type, content: None });
            }
        }
    }

    // Check for references/ directory
    let references_dir = skill_path.join("references");
    if references_dir.exists() {
        for entry in fs::read_dir(&references_dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.is_file() {
                let rel_path = path
                    .strip_prefix(skill_path)
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_default();

                let resource_type = match path.extension().and_then(|e| e.to_str()) {
                    Some("md") => crate::skills::types::ResourceType::Reference,
                    Some("json") | Some("yaml") | Some("yml") | Some("txt") | Some("csv") => {
                        crate::skills::types::ResourceType::Reference
                    }
                    _ => crate::skills::types::ResourceType::Other(format!("{:?}", path.extension())),
                };

                resources.push(crate::skills::types::SkillResource { path: rel_path, resource_type, content: None });
            }
        }
    }

    // Check for assets/ directory
    let assets_dir = skill_path.join("assets");
    if assets_dir.exists() {
        for entry in fs::read_dir(&assets_dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.is_file() {
                let rel_path = path
                    .strip_prefix(skill_path)
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_default();

                let resource_type = match path.extension().and_then(|e| e.to_str()) {
                    Some("png") | Some("jpg") | Some("jpeg") | Some("gif") | Some("svg") => {
                        crate::skills::types::ResourceType::Asset
                    }
                    Some("json") | Some("yaml") | Some("yml") | Some("txt") | Some("csv") => {
                        crate::skills::types::ResourceType::Asset
                    }
                    _ => crate::skills::types::ResourceType::Asset,
                };

                resources.push(crate::skills::types::SkillResource { path: rel_path, resource_type, content: None });
            }
        }
    }

    Ok(resources)
}

pub(crate) fn is_executable_file(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = path.metadata() {
            return meta.permissions().mode() & 0o111 != 0;
        }
    }
    #[cfg(windows)]
    {
        if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
            return matches!(ext.to_lowercase().as_str(), "exe" | "bat" | "cmd");
        }
    }
    false
}
