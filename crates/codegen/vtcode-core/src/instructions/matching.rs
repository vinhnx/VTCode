use super::*;

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

pub(super) fn pattern_matches_candidate(pattern: &Pattern, candidate: &MatchCandidate) -> bool {
    if pattern_matches_path(pattern, candidate) {
        return true;
    }

    zero_directory_pattern_variants(pattern.as_str()).into_iter().any(|variant| {
        Pattern::new(&variant)
            .ok()
            .is_some_and(|variant_pattern| pattern_matches_path(&variant_pattern, candidate))
    })
}

fn pattern_matches_path(pattern: &Pattern, candidate: &MatchCandidate) -> bool {
    let path = Path::new(candidate.relative_path.as_str());
    if pattern.matches_path(path) || pattern.matches(candidate.relative_path.as_str()) {
        return true;
    }

    if candidate.is_dir {
        let probe_path = path.join(IMPORT_PROBE_NAME);
        return pattern.matches_path(&probe_path);
    }

    false
}

fn zero_directory_pattern_variants(pattern: &str) -> Vec<String> {
    let mut variants = Vec::new();
    let mut seen = HashSet::new();
    collect_zero_directory_variants(pattern, &mut seen, &mut variants);
    variants
}

fn collect_zero_directory_variants(pattern: &str, seen: &mut HashSet<String>, variants: &mut Vec<String>) {
    let mut search_start = 0usize;
    while let Some(relative_index) = pattern[search_start..].find("**/") {
        let index = search_start + relative_index;
        let variant = format!("{}{}", &pattern[..index], &pattern[index + 3..]);
        if seen.insert(variant.clone()) {
            variants.push(variant.clone());
            collect_zero_directory_variants(&variant, seen, variants);
        }
        search_start = index + 3;
    }
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
        roots.push(
            canonicalize_async(home)
                .await
                .with_context(|| format!("Failed to canonicalize home import root {}", home.display()))?,
        );

        for user_root in user_global_instruction_roots(home) {
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
