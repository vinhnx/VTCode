use crate::config::constants::prompts;
use crate::config::core::AgentCustomPromptsConfig;
use anyhow::{Context, Result, anyhow};
use serde::Deserialize;
use shell_words::split as shell_split;
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use tracing::warn;

const PROMPTS_SUBDIR: &str = "prompts";

#[derive(Debug, Clone)]
pub struct CustomPromptRegistry {
    enabled: bool,
    directories: Vec<PathBuf>,
    prompts: BTreeMap<String, CustomPrompt>,
}

impl Default for CustomPromptRegistry {
    fn default() -> Self {
        Self {
            enabled: false,
            directories: Vec::new(),
            prompts: BTreeMap::new(),
        }
    }
}

impl CustomPromptRegistry {
    pub fn load(config: Option<&AgentCustomPromptsConfig>, workspace: &Path) -> Result<Self> {
        let settings = config.cloned().unwrap_or_default();
        let directories = resolve_directories(&settings, workspace);

        if !settings.enabled {
            return Ok(Self {
                enabled: false,
                directories,
                prompts: BTreeMap::new(),
            });
        }

        let max_bytes = if settings.max_file_size_kb == 0 {
            usize::MAX
        } else {
            settings.max_file_size_kb.saturating_mul(1024)
        };

        let mut prompts = BTreeMap::new();
        for directory in &directories {
            if !directory.exists() {
                continue;
            }
            if !directory.is_dir() {
                warn!(
                    "custom prompt path `{}` is not a directory - skipping",
                    directory.display()
                );
                continue;
            }

            match fs::read_dir(directory) {
                Ok(entries) => {
                    for entry in entries {
                        let entry = match entry {
                            Ok(value) => value,
                            Err(err) => {
                                warn!("failed to read entry in `{}`: {err}", directory.display());
                                continue;
                            }
                        };

                        let path = entry.path();
                        if !path.is_file() || !is_markdown_file(&path) {
                            continue;
                        }

                        match CustomPrompt::from_file(&path, max_bytes) {
                            Ok(Some(prompt)) => {
                                let key = prompt.name.to_ascii_lowercase();
                                if prompts.contains_key(&key) {
                                    warn!(
                                        "duplicate custom prompt `{}` detected at {}; keeping first occurrence",
                                        prompt.name,
                                        path.display()
                                    );
                                    continue;
                                }
                                prompts.insert(key, prompt);
                            }
                            Ok(None) => {}
                            Err(err) => {
                                warn!(
                                    "failed to load custom prompt from {}: {err:#}",
                                    path.display()
                                );
                            }
                        }
                    }
                }
                Err(err) => {
                    warn!(
                        "failed to read custom prompt directory `{}`: {err}",
                        directory.display()
                    );
                }
            }
        }

        Ok(Self {
            enabled: true,
            directories,
            prompts,
        })
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn is_empty(&self) -> bool {
        self.prompts.is_empty()
    }

    pub fn directories(&self) -> &[PathBuf] {
        &self.directories
    }

    pub fn iter(&self) -> impl Iterator<Item = &CustomPrompt> {
        self.prompts.values()
    }

    pub fn get(&self, name: &str) -> Option<&CustomPrompt> {
        self.prompts.get(&name.to_ascii_lowercase())
    }
}

#[derive(Debug, Clone)]
pub struct CustomPrompt {
    pub name: String,
    pub description: Option<String>,
    pub argument_hint: Option<String>,
    pub path: PathBuf,
    segments: Vec<PromptSegment>,
    required_named: BTreeSet<String>,
    required_positionals: usize,
}

impl CustomPrompt {
    fn from_file(path: &Path, max_bytes: usize) -> Result<Option<Self>> {
        let Some(stem) = path.file_stem().and_then(|value| value.to_str()) else {
            warn!(
                "skipping custom prompt with non-UTF-8 filename: {}",
                path.display()
            );
            return Ok(None);
        };

        if stem.trim().is_empty() {
            warn!(
                "skipping custom prompt with empty name at {}",
                path.display()
            );
            return Ok(None);
        }

        if stem.chars().any(|ch| ch.is_whitespace() || ch == ':') {
            warn!(
                "custom prompt names must not contain whitespace or colons; `{}` skipped",
                stem
            );
            return Ok(None);
        }

        let metadata = fs::metadata(path)
            .with_context(|| format!("failed to read metadata for {}", path.display()))?;
        if metadata.len() as usize > max_bytes {
            warn!(
                "custom prompt `{}` exceeds max_file_size_kb ({:.1} KB) - skipping",
                stem,
                metadata.len() as f64 / 1024.0
            );
            return Ok(None);
        }

        let contents = fs::read_to_string(path)
            .with_context(|| format!("failed to read custom prompt from {}", path.display()))?;

        let (frontmatter, body) = split_frontmatter(&contents)
            .with_context(|| format!("failed to parse frontmatter in {}", path.display()))?;
        let body = body.trim_start_matches(|ch| ch == '\n' || ch == '\r');
        if body.trim().is_empty() {
            warn!(
                "custom prompt `{}` has no content after frontmatter; skipping",
                stem
            );
            return Ok(None);
        }

        let (segments, required_named, required_positionals) = parse_segments(body, stem, path)?;

        let prompt = CustomPrompt {
            name: stem.to_string(),
            description: frontmatter.as_ref().and_then(|fm| fm.description.clone()),
            argument_hint: frontmatter.as_ref().and_then(|fm| fm.argument_hint.clone()),
            path: path.to_path_buf(),
            segments,
            required_named,
            required_positionals,
        };

        Ok(Some(prompt))
    }

    pub fn expand(&self, invocation: &PromptInvocation) -> Result<String> {
        if invocation.positional.len() < self.required_positionals {
            return Err(anyhow!(
                "`/prompts:{}` expects at least {} positional argument(s); received {}",
                self.name,
                self.required_positionals,
                invocation.positional.len()
            ));
        }

        for required in &self.required_named {
            if !invocation.named.contains_key(required) {
                return Err(anyhow!(
                    "missing required argument `{}` for `/prompts:{}`",
                    required,
                    self.name
                ));
            }
        }

        let mut output = String::new();
        for segment in &self.segments {
            match segment {
                PromptSegment::Literal(text) => output.push_str(text),
                PromptSegment::Positional(index) => {
                    output.push_str(&invocation.positional[*index]);
                }
                PromptSegment::Named(name) => {
                    let value = invocation
                        .named
                        .get(name)
                        .expect("missing named argument despite validation");
                    output.push_str(value);
                }
                PromptSegment::AllArguments => {
                    if let Some(joined) = invocation.all_arguments() {
                        output.push_str(joined);
                    }
                }
            }
        }

        Ok(output)
    }
}

#[derive(Debug, Clone, Default)]
pub struct PromptInvocation {
    positional: Vec<String>,
    named: BTreeMap<String, String>,
    all_arguments: Option<String>,
}

impl PromptInvocation {
    pub fn parse(raw: &str) -> Result<Self> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Ok(Self::default());
        }

        let tokens = shell_split(trimmed)
            .with_context(|| "failed to parse custom prompt arguments".to_string())?;

        let mut positional = Vec::new();
        let mut named = BTreeMap::new();
        for token in tokens {
            if let Some((key, value)) = token.split_once('=') {
                let key_trimmed = key.trim();
                if key_trimmed.is_empty() {
                    positional.push(token);
                } else {
                    named.insert(key_trimmed.to_string(), value.to_string());
                }
            } else {
                positional.push(token);
            }
        }

        let all_arguments = if positional.is_empty() {
            None
        } else {
            Some(positional.join(" "))
        };

        Ok(Self {
            positional,
            named,
            all_arguments,
        })
    }

    pub fn all_arguments(&self) -> Option<&str> {
        self.all_arguments.as_deref()
    }

    pub fn positional(&self) -> &[String] {
        &self.positional
    }

    pub fn named(&self) -> &BTreeMap<String, String> {
        &self.named
    }
}

#[derive(Debug, Deserialize)]
struct CustomPromptFrontmatter {
    #[serde(default)]
    description: Option<String>,
    #[serde(default, alias = "argument_hint", alias = "argument-hint")]
    argument_hint: Option<String>,
}

#[derive(Debug, Clone)]
enum PromptSegment {
    Literal(String),
    Positional(usize),
    Named(String),
    AllArguments,
}

fn resolve_directories(config: &AgentCustomPromptsConfig, workspace: &Path) -> Vec<PathBuf> {
    let mut resolved: BTreeSet<PathBuf> = BTreeSet::new();

    if let Ok(env_path) = env::var(prompts::CUSTOM_PROMPTS_ENV_VAR) {
        let trimmed = env_path.trim();
        if !trimmed.is_empty() {
            resolved.insert(PathBuf::from(trimmed).join(PROMPTS_SUBDIR));
        }
    }

    resolved.insert(resolve_directory(&config.directory, workspace));
    for extra in &config.extra_directories {
        resolved.insert(resolve_directory(extra, workspace));
    }

    resolved.into_iter().collect()
}

fn resolve_directory(value: &str, workspace: &Path) -> PathBuf {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return resolve_directory(prompts::DEFAULT_CUSTOM_PROMPTS_DIR, workspace);
    }

    if let Some(stripped) = trimmed
        .strip_prefix("~/")
        .or_else(|| trimmed.strip_prefix("~\\"))
    {
        if let Some(home) = dirs::home_dir() {
            return home.join(stripped);
        }
        return PathBuf::from(stripped);
    }

    let candidate = Path::new(trimmed);
    if candidate.is_absolute() {
        candidate.to_path_buf()
    } else {
        workspace.join(candidate)
    }
}

fn is_markdown_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.eq_ignore_ascii_case("md"))
        .unwrap_or(false)
}

fn split_frontmatter<'a>(contents: &'a str) -> Result<(Option<CustomPromptFrontmatter>, &'a str)> {
    let Some(remaining) = contents.strip_prefix("---") else {
        return Ok((None, contents));
    };

    let remainder = if let Some(rest) = remaining.strip_prefix("\r\n") {
        rest
    } else if let Some(rest) = remaining.strip_prefix('\n') {
        rest
    } else {
        return Ok((None, contents));
    };

    let mut end_offset = None;
    let mut consumed = 0usize;
    for line in remainder.split_inclusive(['\n']) {
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed == "---" {
            end_offset = Some(consumed);
            consumed += line.len();
            break;
        }
        consumed += line.len();
    }

    let Some(end) = end_offset else {
        return Ok((None, contents));
    };

    let frontmatter_raw = &remainder[..end];
    let body_start = consumed;
    let body = &remainder[body_start..];
    let frontmatter: CustomPromptFrontmatter =
        serde_yaml::from_str(frontmatter_raw).context("invalid YAML frontmatter")?;
    Ok((Some(frontmatter), body))
}

fn parse_segments(
    body: &str,
    name: &str,
    path: &Path,
) -> Result<(Vec<PromptSegment>, BTreeSet<String>, usize)> {
    let mut segments = Vec::new();
    let mut literal = String::new();
    let mut required_named = BTreeSet::new();
    let mut required_positionals = 0usize;
    let mut chars = body.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch != '$' {
            literal.push(ch);
            continue;
        }

        let Some(next) = chars.peek().copied() else {
            literal.push('$');
            break;
        };

        match next {
            '$' => {
                literal.push('$');
                chars.next();
            }
            '1'..='9' => {
                flush_literal(&mut literal, &mut segments);
                chars.next();
                let index = next as usize - '1' as usize;
                required_positionals = required_positionals.max(index + 1);
                segments.push(PromptSegment::Positional(index));
            }
            c if is_identifier_start(c) => {
                flush_literal(&mut literal, &mut segments);
                let mut name_buf = String::new();
                while let Some(candidate) = chars.peek().copied() {
                    if is_identifier_continue(candidate) {
                        name_buf.push(candidate);
                        chars.next();
                    } else {
                        break;
                    }
                }

                if name_buf.is_empty() {
                    literal.push('$');
                    continue;
                }

                if name_buf == "ARGUMENTS" {
                    segments.push(PromptSegment::AllArguments);
                } else {
                    required_named.insert(name_buf.clone());
                    segments.push(PromptSegment::Named(name_buf));
                }
            }
            _ => {
                literal.push('$');
            }
        }
    }

    flush_literal(&mut literal, &mut segments);

    if segments.is_empty() {
        return Err(anyhow!(
            "custom prompt `{}` from {} produced no output segments",
            name,
            path.display()
        ));
    }

    Ok((segments, required_named, required_positionals))
}

fn flush_literal(buffer: &mut String, segments: &mut Vec<PromptSegment>) {
    if !buffer.is_empty() {
        segments.push(PromptSegment::Literal(buffer.clone()));
        buffer.clear();
    }
}

fn is_identifier_start(ch: char) -> bool {
    ch == '_' || ch.is_ascii_alphabetic()
}

fn is_identifier_continue(ch: char) -> bool {
    ch == '_' || ch.is_ascii_alphanumeric()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn prompt_invocation_parses_named_and_positional_arguments() {
        let invocation = PromptInvocation::parse("one two FILE=path focus=main").unwrap();
        assert_eq!(invocation.positional(), &["one", "two"]);
        assert_eq!(invocation.named().get("FILE").unwrap(), "path");
        assert_eq!(invocation.named().get("focus").unwrap(), "main");
        assert_eq!(invocation.all_arguments().unwrap(), "one two");
    }

    #[test]
    fn custom_prompt_expands_placeholders() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("review.md");
        fs::write(
            &path,
            "---\ndescription: Review helper\nargument-hint: FILE=<path>\n---\nReview $FILE with focus on $1.\nAll args: $ARGUMENTS\n",
        )
        .unwrap();

        let prompt = CustomPrompt::from_file(&path, 8 * 1024).unwrap().unwrap();
        let invocation = PromptInvocation::parse("critical FILE=src/lib.rs").unwrap();
        let expanded = prompt.expand(&invocation).unwrap();
        assert!(expanded.contains("src/lib.rs"));
        assert!(expanded.contains("critical"));
        assert!(expanded.contains("All args: critical"));
        assert_eq!(prompt.description.as_deref(), Some("Review helper"));
        assert_eq!(prompt.argument_hint.as_deref(), Some("FILE=<path>"));
    }

    #[test]
    fn custom_prompt_registry_loads_from_directory() {
        let temp = tempdir().unwrap();
        let prompts_dir = temp.path().join("prompts");
        fs::create_dir_all(&prompts_dir).unwrap();
        fs::write(prompts_dir.join("draft.md"), "Draft PR for $1").unwrap();

        let mut cfg = AgentCustomPromptsConfig::default();
        cfg.directory = prompts_dir.to_string_lossy().to_string();
        let registry = CustomPromptRegistry::load(Some(&cfg), temp.path()).expect("load registry");
        assert!(registry.enabled());
        assert!(!registry.is_empty());
        let prompt = registry.get("draft").unwrap();
        let invocation = PromptInvocation::parse("feature").unwrap();
        let expanded = prompt.expand(&invocation).unwrap();
        assert_eq!(expanded.trim(), "Draft PR for feature");
    }
}
