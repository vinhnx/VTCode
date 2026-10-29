use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use glob::Pattern;
use tracing::warn;
use vtcode_commons::canonicalize;
use vtcode_commons::canonicalize_async;

use super::IMPORT_PROBE_NAME;
use super::discovery::user_global_instruction_roots;
use super::types::{MatchCandidate, RuleFrontmatter};

pub(super) fn parse_rule_frontmatter(contents: &str, path: &Path) -> Result<RuleFrontmatter> {
    let Some((frontmatter, _body)) = split_frontmatter(contents) else {
        return Ok(RuleFrontmatter::default());
    };

    serde_saphyr::from_str(frontmatter)
        .with_context(|| format!("Failed to parse YAML frontmatter for instruction rule {}", path.display()))
}

pub(super) fn strip_rule_frontmatter(contents: &str) -> String {
    split_frontmatter(contents)
        .map(|(_, body)| body.to_string())
        .unwrap_or_else(|| contents.to_string())
}

fn split_frontmatter(contents: &str) -> Option<(&str, &str)> {
    let mut lines = contents.split_inclusive('\n');
    let first = lines.next()?;
    if first.trim_end() != "---" {
        return None;
    }

    let mut offset = first.len();
    for line in lines {
        let trimmed = line.trim_end();
        if trimmed == "---" || trimmed == "..." {
            let body_start = offset + line.len();
            let frontmatter = &contents[first.len()..offset];
            let body = contents.get(body_start..).unwrap_or_default();
            return Some((frontmatter, body));
        }
        offset += line.len();
    }

    None
}

enum RuleToken {
    Literal(char),
    Any,
    Star,
    Recursive,
    Class(Pattern),
}

pub(super) struct RulePattern(Vec<RuleToken>);

impl RulePattern {
    pub(super) fn new(pattern: &str) -> Result<Self> {
        // Reuse glob's parser for syntax validation and character-class semantics.
        Pattern::new(pattern)?;
        let mut chars = pattern.chars().peekable();
        let mut tokens = Vec::new();
        while let Some(ch) = chars.next() {
            match ch {
                '?' => tokens.push(RuleToken::Any),
                '*' if chars.peek() == Some(&'*') => {
                    let _ignored = chars.next();
                    if chars.peek().is_some_and(|ch| std::path::is_separator(*ch)) {
                        let _ignored = chars.next();
                    }
                    tokens.push(RuleToken::Recursive);
                }
                '*' => tokens.push(RuleToken::Star),
                '[' => {
                    let mut class = String::from("[");
                    let mut negated = false;
                    for (content_len, ch) in chars.by_ref().enumerate() {
                        if ch == ']' && content_len > usize::from(negated) {
                            class.push(ch);
                            break;
                        }
                        if content_len == 0 && ch == '!' {
                            negated = true;
                        }
                        class.push(ch);
                    }
                    tokens.push(RuleToken::Class(Pattern::new(&class)?));
                }
                _ => tokens.push(RuleToken::Literal(ch)),
            }
        }
        Ok(Self(tokens))
    }

    fn matches(&self, value: &str) -> bool {
        let chars = value.chars().collect::<Vec<_>>();
        let mut reachable = vec![false; chars.len() + 1];
        if let Some(start) = reachable.first_mut() {
            *start = true;
        }
        for (token_index, token) in self.0.iter().enumerate() {
            let mut next = Vec::with_capacity(reachable.len());
            match token {
                RuleToken::Star | RuleToken::Recursive => {
                    let mut seen = false;
                    for (position, previous) in reachable.iter().copied().enumerate() {
                        seen |= previous;
                        let recursive_end = position
                            .checked_sub(1)
                            .and_then(|index| chars.get(index))
                            .is_some_and(|ch| std::path::is_separator(*ch));
                        next.push(
                            previous
                                || (seen
                                    && (matches!(token, RuleToken::Star)
                                        || token_index + 1 == self.0.len()
                                        || recursive_end)),
                        );
                    }
                }
                _ => {
                    next.push(false);
                    for (previous, ch) in reachable.iter().zip(&chars) {
                        let matched = match token {
                            RuleToken::Literal(expected) => {
                                *expected == *ch
                                    || (cfg!(windows)
                                        && std::path::is_separator(*expected)
                                        && std::path::is_separator(*ch))
                            }
                            RuleToken::Any => true,
                            RuleToken::Class(class) => class.matches(&ch.to_string()),
                            RuleToken::Star | RuleToken::Recursive => false,
                        };
                        next.push(*previous && matched);
                    }
                }
            }
            reachable = next;
        }
        reachable.last().copied().unwrap_or(false)
    }
}

pub(super) fn pattern_matches_candidate(pattern: &RulePattern, candidate: &MatchCandidate) -> bool {
    pattern.matches(&candidate.relative_path)
        || (candidate.is_dir
            && pattern.matches(&Path::new(&candidate.relative_path).join(IMPORT_PROBE_NAME).to_string_lossy()))
}

pub(super) fn rule_specificity(pattern: &str) -> usize {
    pattern
        .chars()
        .filter(|ch| !matches!(ch, '*' | '?' | '[' | ']' | '{' | '}' | ','))
        .count()
}

pub(super) fn contains_glob_meta(pattern: &str) -> bool {
    pattern.chars().any(|ch| matches!(ch, '*' | '?' | '[' | ']' | '{' | '}'))
}

pub(super) fn strip_html_comments(contents: &str) -> String {
    let mut output = String::with_capacity(contents.len());
    let mut in_code_block = false;
    let mut in_comment = false;

    for line in contents.lines() {
        let trimmed = line.trim_start();
        if !in_comment && is_fence_line(trimmed) {
            in_code_block = !in_code_block;
            output.push_str(line);
            output.push('\n');
            continue;
        }

        if in_code_block {
            output.push_str(line);
            output.push('\n');
            continue;
        }

        let mut cursor = line;
        let mut rendered = String::new();
        loop {
            if in_comment {
                if let Some(end) = cursor.find("-->") {
                    cursor = &cursor[end + 3..];
                    in_comment = false;
                    continue;
                }
                cursor = "";
                break;
            }

            let Some(start) = cursor.find("<!--") else {
                rendered.push_str(cursor);
                cursor = "";
                break;
            };

            rendered.push_str(&cursor[..start]);
            cursor = &cursor[start + 4..];
            if let Some(end) = cursor.find("-->") {
                cursor = &cursor[end + 3..];
                continue;
            }

            in_comment = true;
            cursor = "";
            break;
        }

        if !rendered.is_empty() || !cursor.is_empty() {
            output.push_str(rendered.trim_end_matches('\r'));
        }
        output.push('\n');
    }

    output
}

pub(super) fn is_fence_line(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.starts_with("```") || trimmed.starts_with("~~~")
}

pub(super) fn collect_imports(contents: &str) -> Vec<String> {
    let mut imports = Vec::new();
    let mut in_code_block = false;

    for line in contents.lines() {
        let trimmed = line.trim_start();
        if is_fence_line(trimmed) {
            in_code_block = !in_code_block;
            continue;
        }
        if in_code_block {
            continue;
        }

        for token in line.split_whitespace() {
            let Some(candidate) = token.strip_prefix('@') else {
                continue;
            };
            let trimmed =
                candidate.trim_matches(|ch: char| matches!(ch, ')' | '(' | '[' | ']' | '{' | '}' | ',' | ';' | ':'));
            let trimmed = trimmed.trim_end_matches('.');
            if trimmed.is_empty() {
                continue;
            }
            imports.push(trimmed.to_string());
        }
    }

    imports
}

pub(super) fn resolve_import_path(
    import: &str,
    containing_file: &Path,
    allowed_roots: &[PathBuf],
) -> Result<Option<PathBuf>> {
    let parent = containing_file
        .parent()
        .ok_or_else(|| anyhow!("Instruction file {} has no parent", containing_file.display()))?;
    let home_dir = dirs::home_dir();

    let candidate = if let Some(stripped) = import.strip_prefix("~/") {
        let Some(home_dir) = home_dir else {
            warn!("Skipping instruction import `@{import}` because the home directory is unavailable");
            return Ok(None);
        };
        home_dir.join(stripped)
    } else {
        let import_path = Path::new(import);
        if import_path.is_absolute() {
            import_path.to_path_buf()
        } else {
            parent.join(import_path)
        }
    };

    let canonical = match canonicalize(&candidate) {
        Ok(path) => path,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            warn!("Skipping missing instruction import `@{}` referenced from {}", import, containing_file.display());
            return Ok(None);
        }
        Err(err) => {
            return Err(err).with_context(|| {
                format!("Failed to resolve instruction import `@{}` from {}", import, containing_file.display())
            });
        }
    };

    let allowed = allowed_roots.iter().any(|root| canonical.starts_with(root));
    if !allowed {
        warn!("Skipping instruction import `{}` because it is outside the allowed roots", canonical.display());
        return Ok(None);
    }

    Ok(Some(canonical))
}

pub(super) async fn allowed_import_roots(project_root: &Path, home_dir: Option<&Path>) -> Result<Vec<PathBuf>> {
    let mut roots = Vec::new();
    roots.push(
        canonicalize_async(project_root)
            .await
            .with_context(|| format!("Failed to canonicalize project root import root {}", project_root.display()))?,
    );

    if let Some(home) = home_dir {
        for user_root in user_global_instruction_roots(home) {
            if user_root == home {
                continue;
            }
            if tokio::fs::try_exists(&user_root).await.unwrap_or(false) {
                roots.push(canonicalize_async(&user_root).await.with_context(|| {
                    format!("Failed to canonicalize VT Code user import root {}", user_root.display())
                })?);
            }
        }
    }

    roots.sort();
    roots.dedup();
    Ok(roots)
}
