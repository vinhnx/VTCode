use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use hashbrown::HashSet;
use tracing::warn;
use vtcode_commons::canonicalize_async;

use crate::utils::file_utils::canonicalize_with_context;

use super::discovery::discover_instruction_sources;
use super::matching::{
    allowed_import_roots, collect_imports, is_fence_line, resolve_import_path, strip_html_comments,
    strip_rule_frontmatter,
};
use super::types::{
    InstructionBundle, InstructionDiscoveryOptions, InstructionScope, InstructionSegment, InstructionSourceKind,
};
use super::{AGENTS_FILENAME, AGENTS_OVERRIDE_FILENAME};

pub async fn read_instruction_bundle(
    options: &InstructionDiscoveryOptions<'_>,
    max_bytes: usize,
) -> Result<Option<InstructionBundle>> {
    if max_bytes == 0 {
        return Ok(None);
    }

    let sources = discover_instruction_sources(options).await?;
    if sources.is_empty() {
        return Ok(None);
    }

    let allowed_import_roots = allowed_import_roots(options.project_root, options.home_dir).await?;
    let workspace_import_root = canonicalize_async(options.project_root)
        .await
        .with_context(|| format!("Failed to canonicalize project root {}", options.project_root.display()))?;
    let import_max_depth = options.import_max_depth.max(1);

    // expand_instruction_contents performs blocking filesystem I/O (read_to_string,
    // canonicalize) for each instruction file and its recursive imports. Offload
    // the entire expansion loop to the blocking thread pool.
    let (segments, truncated, bytes_read) = tokio::task::spawn_blocking(move || {
        let mut remaining = max_bytes;
        let mut segments = Vec::with_capacity(sources.len());
        let mut truncated = false;
        let mut bytes_read = 0usize;
        let mut seen_imports = HashSet::new();

        for source in sources {
            if remaining == 0 {
                truncated = true;
                break;
            }

            let import_policy = ImportPolicy {
                workspace_root: &workspace_import_root,
                allowed_roots: if source.scope == InstructionScope::User
                    && !source.path.starts_with(&workspace_import_root)
                {
                    &allowed_import_roots
                } else {
                    std::slice::from_ref(&workspace_import_root)
                },
            };
            let contents = match expand_instruction_contents(
                &source.path,
                &source.kind,
                import_policy,
                import_max_depth,
                &mut seen_imports,
                0,
                &mut Vec::new(),
            )? {
                Some(contents) => contents,
                None => continue,
            };

            if contents.len() > remaining {
                truncated = true;
            }

            let slice_len = contents.len().min(remaining);
            let visible = String::from_utf8_lossy(&contents.as_bytes()[..slice_len]).to_string();
            if visible.trim().is_empty() {
                remaining = remaining.saturating_sub(slice_len);
                continue;
            }

            bytes_read += slice_len;
            remaining = remaining.saturating_sub(slice_len);
            segments.push(InstructionSegment { source, contents: visible });
        }

        Ok::<_, anyhow::Error>((segments, truncated, bytes_read))
    })
    .await
    .context("Instruction expansion task panicked")??;

    if segments.is_empty() {
        Ok(None)
    } else {
        Ok(Some(InstructionBundle { segments, truncated, bytes_read }))
    }
}

#[derive(Clone, Copy)]
struct ImportPolicy<'a> {
    workspace_root: &'a PathBuf,
    allowed_roots: &'a [PathBuf],
}

fn expand_instruction_contents(
    path: &Path,
    kind: &InstructionSourceKind,
    import_policy: ImportPolicy<'_>,
    max_depth: usize,
    seen_imports: &mut HashSet<PathBuf>,
    depth: usize,
    stack: &mut Vec<PathBuf>,
) -> Result<Option<String>> {
    let canonical = canonicalize_with_context(path, "instruction source")?;
    if depth > 0 {
        if stack.contains(&canonical) {
            warn!("Skipping cyclic instruction import `{}` while expanding `{}`", canonical.display(), path.display());
            return Ok(None);
        }
        if depth > max_depth {
            warn!(
                "Skipping instruction import `{}` because it exceeds the max depth of {}",
                canonical.display(),
                max_depth
            );
            return Ok(None);
        }
        if !seen_imports.insert(canonical.clone()) {
            return Ok(None);
        }
    }

    stack.push(canonical.clone());

    let raw = match std::fs::read_to_string(&canonical) {
        Ok(contents) => contents,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            stack.pop();
            return Ok(None);
        }
        Err(err) => {
            stack.pop();
            return Err(err).with_context(|| format!("Failed to open instruction file {}", canonical.display()));
        }
    };
    let contents_without_frontmatter = match kind {
        InstructionSourceKind::Rule => strip_rule_frontmatter(&raw),
        InstructionSourceKind::Agents | InstructionSourceKind::Extra => raw,
    };
    let sanitized = strip_html_comments(&contents_without_frontmatter);
    let output = expand_inline_imports(&sanitized, &canonical, import_policy, max_depth, seen_imports, depth, stack)?;

    stack.pop();

    if output.trim().is_empty() {
        Ok(None)
    } else {
        Ok(Some(output))
    }
}

fn expand_inline_imports(
    contents: &str,
    containing_file: &Path,
    import_policy: ImportPolicy<'_>,
    max_depth: usize,
    seen_imports: &mut HashSet<PathBuf>,
    depth: usize,
    stack: &mut Vec<PathBuf>,
) -> Result<String> {
    let mut output = String::new();
    let mut in_code_block = false;

    for line in contents.lines() {
        let trimmed = line.trim_start();
        if is_fence_line(trimmed) {
            in_code_block = !in_code_block;
            output.push_str(line);
            output.push('\n');
            continue;
        }

        output.push_str(line);
        output.push('\n');

        if in_code_block {
            continue;
        }

        let imports = collect_imports(line);
        for import in imports {
            let Some(import_path) = resolve_import_path(&import, containing_file, import_policy.allowed_roots)? else {
                continue;
            };

            let import_kind = infer_instruction_kind(&import_path);
            let child_policy = if import_path.starts_with(import_policy.workspace_root) {
                ImportPolicy {
                    allowed_roots: std::slice::from_ref(import_policy.workspace_root),
                    ..import_policy
                }
            } else {
                import_policy
            };
            let imported = expand_instruction_contents(
                &import_path,
                &import_kind,
                child_policy,
                max_depth,
                seen_imports,
                depth + 1,
                stack,
            )?;
            let Some(imported) = imported else {
                continue;
            };

            if imported.trim().is_empty() {
                continue;
            }

            let _ = writeln!(output, "[Imported from {}]", import_path.display());
            output.push_str(imported.trim());
            output.push_str("\n\n");
        }
    }

    Ok(output.trim().to_string())
}

fn infer_instruction_kind(path: &Path) -> InstructionSourceKind {
    if path.components().any(|component| component.as_os_str() == "rules") {
        InstructionSourceKind::Rule
    } else if path.file_name().and_then(|value| value.to_str()) == Some(AGENTS_FILENAME)
        || path.file_name().and_then(|value| value.to_str()) == Some(AGENTS_OVERRIDE_FILENAME)
    {
        InstructionSourceKind::Agents
    } else {
        InstructionSourceKind::Extra
    }
}
