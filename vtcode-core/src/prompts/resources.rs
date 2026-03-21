//! Shared prompt resource discovery for system prompt layers and prompt templates.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use tokio::fs;
use tracing::warn;

const PROMPTS_DIR: &str = ".vtcode/prompts";
const TEMPLATES_DIR: &str = "templates";
const SYSTEM_PROMPT_FILENAME: &str = "system.md";
const APPEND_SYSTEM_PROMPT_FILENAME: &str = "append-system.md";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptTemplate {
    pub name: String,
    pub description: String,
    pub body: String,
    pub path: PathBuf,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SystemPromptLayers {
    pub override_body: Option<String>,
    pub append_bodies: Vec<String>,
}

#[derive(Debug, Clone, Copy)]
enum PromptResourceScope {
    User,
    Workspace,
}

#[derive(Debug, Clone)]
struct PromptResourceOptions<'a> {
    workspace_root: &'a Path,
    home_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct PromptTemplateFrontmatter {
    description: Option<String>,
}

pub async fn resolve_system_prompt_layers(workspace_root: &Path) -> SystemPromptLayers {
    resolve_system_prompt_layers_with_options(PromptResourceOptions::new(workspace_root)).await
}

pub async fn discover_prompt_templates(workspace_root: &Path) -> Vec<PromptTemplate> {
    discover_prompt_templates_with_options(PromptResourceOptions::new(workspace_root)).await
}

pub async fn find_prompt_template(workspace_root: &Path, name: &str) -> Option<PromptTemplate> {
    let normalized = name.trim();
    if normalized.is_empty() {
        return None;
    }

    find_prompt_template_with_options(PromptResourceOptions::new(workspace_root), normalized).await
}

pub fn apply_system_prompt_layers(base_prompt: &str, layers: &SystemPromptLayers) -> String {
    let mut prompt = String::new();

    if let Some(override_body) = layers.override_body.as_deref().map(str::trim)
        && !override_body.is_empty()
    {
        prompt.push_str(override_body);
    } else {
        prompt.push_str(base_prompt);
    }

    for append_body in &layers.append_bodies {
        let trimmed = append_body.trim();
        if trimmed.is_empty() {
            continue;
        }
        if !prompt.is_empty() {
            prompt.push_str("\n\n");
        }
        prompt.push_str(trimmed);
    }

    prompt
}

pub fn expand_prompt_template(body: &str, args: &[String]) -> String {
    let joined_args = args.join(" ");
    let mut expanded = String::with_capacity(body.len() + joined_args.len());
    let chars: Vec<char> = body.chars().collect();
    let mut index = 0;

    while index < chars.len() {
        if chars[index] != '$' {
            expanded.push(chars[index]);
            index += 1;
            continue;
        }

        if index + 1 >= chars.len() {
            expanded.push('$');
            index += 1;
            continue;
        }

        match chars[index + 1] {
            '@' => {
                expanded.push_str(&joined_args);
                index += 2;
            }
            'A' => {
                const ARGUMENTS_TOKEN: &str = "ARGUMENTS";
                let remaining: String = chars[index + 1..].iter().collect();
                if remaining.starts_with(ARGUMENTS_TOKEN) {
                    expanded.push_str(&joined_args);
                    index += ARGUMENTS_TOKEN.chars().count() + 1;
                } else {
                    expanded.push('$');
                    index += 1;
                }
            }
            digit if digit.is_ascii_digit() => {
                let mut cursor = index + 1;
                while cursor < chars.len() && chars[cursor].is_ascii_digit() {
                    cursor += 1;
                }
                let ordinal: String = chars[index + 1..cursor].iter().collect();
                let replacement = ordinal
                    .parse::<usize>()
                    .ok()
                    .and_then(|value| value.checked_sub(1))
                    .and_then(|position| args.get(position))
                    .map(String::as_str)
                    .unwrap_or("");
                expanded.push_str(replacement);
                index = cursor;
            }
            _ => {
                expanded.push('$');
                index += 1;
            }
        }
    }

    expanded
}

impl<'a> PromptResourceOptions<'a> {
    fn new(workspace_root: &'a Path) -> Self {
        #[cfg(test)]
        let home_dir = None;

        #[cfg(not(test))]
        let home_dir = dirs::home_dir();

        Self {
            workspace_root,
            home_dir,
        }
    }
}

async fn resolve_system_prompt_layers_with_options(
    options: PromptResourceOptions<'_>,
) -> SystemPromptLayers {
    let mut layers = SystemPromptLayers::default();

    let user_system_path = options
        .home_dir
        .as_ref()
        .map(|home| home.join(PROMPTS_DIR).join(SYSTEM_PROMPT_FILENAME));
    let workspace_system_path = options
        .workspace_root
        .join(PROMPTS_DIR)
        .join(SYSTEM_PROMPT_FILENAME);

    if let Some(path) = user_system_path.as_ref() {
        layers.override_body = read_optional_markdown(path).await;
    }

    if let Some(workspace_override) = read_optional_markdown(&workspace_system_path).await {
        layers.override_body = Some(workspace_override);
    }

    if let Some(path) = options
        .home_dir
        .as_ref()
        .map(|home| home.join(PROMPTS_DIR).join(APPEND_SYSTEM_PROMPT_FILENAME))
        && let Some(contents) = read_optional_markdown(&path).await
    {
        layers.append_bodies.push(contents);
    }

    let workspace_append = options
        .workspace_root
        .join(PROMPTS_DIR)
        .join(APPEND_SYSTEM_PROMPT_FILENAME);
    if let Some(contents) = read_optional_markdown(&workspace_append).await {
        layers.append_bodies.push(contents);
    }

    layers
}

async fn discover_prompt_templates_with_options(
    options: PromptResourceOptions<'_>,
) -> Vec<PromptTemplate> {
    let mut discovered = BTreeMap::new();

    if let Some(home) = options.home_dir.as_deref() {
        let user_templates = home.join(PROMPTS_DIR).join(TEMPLATES_DIR);
        merge_prompt_templates(&mut discovered, &user_templates, PromptResourceScope::User).await;
    }

    let workspace_templates = options.workspace_root.join(PROMPTS_DIR).join(TEMPLATES_DIR);
    merge_prompt_templates(
        &mut discovered,
        &workspace_templates,
        PromptResourceScope::Workspace,
    )
    .await;

    discovered.into_values().collect()
}

async fn find_prompt_template_with_options(
    options: PromptResourceOptions<'_>,
    name: &str,
) -> Option<PromptTemplate> {
    let template_path = |root: &Path| {
        root.join(PROMPTS_DIR)
            .join(TEMPLATES_DIR)
            .join(format!("{name}.md"))
    };

    if !is_safe_template_name(name) {
        return None;
    }

    let workspace_path = template_path(options.workspace_root);
    if let Some(template) = load_prompt_template(&workspace_path, name.to_string()).await {
        return Some(template);
    }

    if let Some(home) = options.home_dir.as_deref() {
        let user_path = template_path(home);
        if let Some(template) = load_prompt_template(&user_path, name.to_string()).await {
            return Some(template);
        }
    }

    None
}

async fn merge_prompt_templates(
    discovered: &mut BTreeMap<String, PromptTemplate>,
    directory: &Path,
    scope: PromptResourceScope,
) {
    let Ok(mut entries) = fs::read_dir(directory).await else {
        return;
    };

    let mut markdown_files = Vec::new();
    loop {
        match entries.next_entry().await {
            Ok(Some(entry)) => {
                let path = entry.path();
                if path.extension().and_then(|ext| ext.to_str()) == Some("md") {
                    markdown_files.push(path);
                }
            }
            Ok(None) => break,
            Err(err) => {
                warn!(
                    "failed to read prompt templates directory {}: {}",
                    directory.display(),
                    err
                );
                break;
            }
        }
    }

    markdown_files.sort();

    for path in markdown_files {
        let Some(name) = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .map(str::trim)
            .filter(|stem| !stem.is_empty())
            .map(str::to_string)
        else {
            continue;
        };

        match load_prompt_template(&path, name.clone()).await {
            Some(template) => {
                if matches!(scope, PromptResourceScope::Workspace)
                    || !discovered.contains_key(&name)
                {
                    discovered.insert(name, template);
                }
            }
            None => continue,
        }
    }
}

async fn load_prompt_template(path: &Path, name: String) -> Option<PromptTemplate> {
    let raw = read_optional_markdown(path).await?;
    let normalized = normalize_newlines(&raw);
    let (frontmatter, body) = parse_frontmatter(&normalized);
    let description = frontmatter
        .description
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| derive_template_description(&body, &name));

    Some(PromptTemplate {
        name,
        description,
        body: body.trim().to_string(),
        path: path.to_path_buf(),
    })
}

async fn read_optional_markdown(path: &Path) -> Option<String> {
    match fs::read_to_string(path).await {
        Ok(contents) => Some(contents),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
        Err(err) => {
            warn!("failed to read prompt resource {}: {}", path.display(), err);
            None
        }
    }
}

fn parse_frontmatter(content: &str) -> (PromptTemplateFrontmatter, String) {
    if !content.starts_with("---\n") {
        return (PromptTemplateFrontmatter::default(), content.to_string());
    }

    let Some(frontmatter_end) = content[4..].find("\n---\n").map(|idx| idx + 4) else {
        return (PromptTemplateFrontmatter::default(), content.to_string());
    };

    let yaml = &content[4..frontmatter_end];
    let body_start = frontmatter_end + 5;
    let body = if body_start < content.len() {
        content[body_start..].to_string()
    } else {
        String::new()
    };

    let metadata = match serde_yaml::from_str::<PromptTemplateFrontmatter>(yaml.trim()) {
        Ok(value) => value,
        Err(err) => {
            warn!("failed to parse prompt template frontmatter: {}", err);
            PromptTemplateFrontmatter::default()
        }
    };

    (metadata, body)
}

fn derive_template_description(body: &str, name: &str) -> String {
    for line in body.lines().map(str::trim) {
        if line.is_empty() {
            continue;
        }
        if let Some(heading) = line.strip_prefix('#') {
            let trimmed = heading.trim_start_matches('#').trim();
            if !trimmed.is_empty() {
                return trimmed.to_string();
            }
        }
        return line.to_string();
    }

    format!("Prompt template `{}`", name)
}

fn normalize_newlines(content: &str) -> String {
    content.replace("\r\n", "\n")
}

fn is_safe_template_name(name: &str) -> bool {
    !name.is_empty() && !name.contains('/') && !name.contains('\\') && !name.contains("..")
}

#[cfg(test)]
mod tests {
    use super::*;

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

    async fn find_with_roots(
        workspace: &Path,
        home: Option<&Path>,
        name: &str,
    ) -> Option<PromptTemplate> {
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
    async fn system_layers_prefer_workspace_override_and_append_user_then_workspace() {
        let workspace = tempfile::TempDir::new().expect("workspace");
        let home = tempfile::TempDir::new().expect("home");

        let user_prompts = home.path().join(PROMPTS_DIR);
        let workspace_prompts = workspace.path().join(PROMPTS_DIR);
        std::fs::create_dir_all(&user_prompts).expect("user prompts");
        std::fs::create_dir_all(&workspace_prompts).expect("workspace prompts");

        std::fs::write(
            user_prompts.join(SYSTEM_PROMPT_FILENAME),
            "user system override",
        )
        .expect("write user system");
        std::fs::write(
            workspace_prompts.join(SYSTEM_PROMPT_FILENAME),
            "workspace system override",
        )
        .expect("write workspace system");
        std::fs::write(
            user_prompts.join(APPEND_SYSTEM_PROMPT_FILENAME),
            "user append",
        )
        .expect("write user append");
        std::fs::write(
            workspace_prompts.join(APPEND_SYSTEM_PROMPT_FILENAME),
            "workspace append",
        )
        .expect("write workspace append");

        let layers = layers_with_roots(workspace.path(), Some(home.path())).await;
        assert_eq!(
            layers.override_body.as_deref(),
            Some("workspace system override")
        );
        assert_eq!(
            layers.append_bodies,
            vec!["user append".to_string(), "workspace append".to_string()]
        );

        let composed = apply_system_prompt_layers("fallback base", &layers);
        assert_eq!(
            composed,
            "workspace system override\n\nuser append\n\nworkspace append"
        );
    }

    #[tokio::test]
    async fn template_discovery_prefers_workspace_and_derives_descriptions() {
        let workspace = tempfile::TempDir::new().expect("workspace");
        let home = tempfile::TempDir::new().expect("home");
        let user_templates = home.path().join(PROMPTS_DIR).join(TEMPLATES_DIR);
        let workspace_templates = workspace.path().join(PROMPTS_DIR).join(TEMPLATES_DIR);
        std::fs::create_dir_all(&user_templates).expect("user templates");
        std::fs::create_dir_all(&workspace_templates).expect("workspace templates");

        std::fs::write(
            user_templates.join("review.md"),
            "---\ndescription: User review template\n---\nReview $1",
        )
        .expect("user review");
        std::fs::write(
            workspace_templates.join("review.md"),
            "# Workspace review\n\nReview workspace $1",
        )
        .expect("workspace review");
        std::fs::write(
            workspace_templates.join("audit.md"),
            "First non-empty line becomes description.\n\nAudit $@",
        )
        .expect("workspace audit");

        let templates = discover_with_roots(workspace.path(), Some(home.path())).await;
        assert_eq!(templates.len(), 2);
        assert_eq!(templates[0].name, "audit");
        assert_eq!(
            templates[0].description,
            "First non-empty line becomes description."
        );
        assert_eq!(templates[1].name, "review");
        assert_eq!(templates[1].description, "Workspace review");
        assert_eq!(
            templates[1].body,
            "# Workspace review\n\nReview workspace $1"
        );
    }

    #[tokio::test]
    async fn direct_template_lookup_uses_workspace_precedence() {
        let workspace = tempfile::TempDir::new().expect("workspace");
        let home = tempfile::TempDir::new().expect("home");
        let user_templates = home.path().join(PROMPTS_DIR).join(TEMPLATES_DIR);
        let workspace_templates = workspace.path().join(PROMPTS_DIR).join(TEMPLATES_DIR);
        std::fs::create_dir_all(&user_templates).expect("user templates");
        std::fs::create_dir_all(&workspace_templates).expect("workspace templates");

        std::fs::write(user_templates.join("review.md"), "User review body")
            .expect("user template");
        std::fs::write(
            workspace_templates.join("review.md"),
            "Workspace review body",
        )
        .expect("workspace template");

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
}
