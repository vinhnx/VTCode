use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Mutex;

use once_cell::sync::Lazy;
use regex::Regex;
use serde::Deserialize;
use tokio::process::Command;

use super::matcher::{normalise_text, seek_segment};
use super::{PatchChunk, PatchError};

const AST_GREP_BIN_ENV: &str = "VTCODE_AST_GREP_BIN";

static IDENTIFIER_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"[A-Za-z_][A-Za-z0-9_]*").expect("semantic identifier regex must compile")
});
static AST_GREP_OVERRIDE: Lazy<Mutex<AstGrepBinaryOverride>> =
    Lazy::new(|| Mutex::new(AstGrepBinaryOverride::System));

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SemanticMatch {
    pub(crate) start_idx: usize,
    pub(crate) old_segment: Vec<String>,
    pub(crate) new_segment: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SupportedLanguage {
    Rust,
    Python,
    JavaScript,
    TypeScript,
    Tsx,
    Go,
    Java,
}

#[derive(Debug, Clone, Default)]
enum AstGrepBinaryOverride {
    #[default]
    System,
    Missing,
    Path(PathBuf),
}

#[doc(hidden)]
#[must_use]
pub struct AstGrepBinaryOverrideGuard {
    previous: AstGrepBinaryOverride,
}

impl Drop for AstGrepBinaryOverrideGuard {
    fn drop(&mut self) {
        *AST_GREP_OVERRIDE
            .lock()
            .expect("ast-grep override mutex must not be poisoned") = self.previous.clone();
    }
}

#[doc(hidden)]
pub fn set_ast_grep_binary_override_for_tests(path: Option<PathBuf>) -> AstGrepBinaryOverrideGuard {
    let mut state = AST_GREP_OVERRIDE
        .lock()
        .expect("ast-grep override mutex must not be poisoned");
    let previous = state.clone();
    *state = match path {
        Some(path) => AstGrepBinaryOverride::Path(path),
        None => AstGrepBinaryOverride::Missing,
    };
    AstGrepBinaryOverrideGuard { previous }
}

impl SupportedLanguage {
    fn from_path(path: &Path) -> Option<Self> {
        match path.extension().and_then(|ext| ext.to_str()) {
            Some("rs") => Some(Self::Rust),
            Some("py") => Some(Self::Python),
            Some("js") => Some(Self::JavaScript),
            Some("ts") => Some(Self::TypeScript),
            Some("tsx") => Some(Self::Tsx),
            Some("go") => Some(Self::Go),
            Some("java") => Some(Self::Java),
            _ => None,
        }
    }

    fn ast_grep_lang(self) -> &'static str {
        match self {
            Self::Rust => "rust",
            Self::Python => "python",
            Self::JavaScript => "javascript",
            Self::TypeScript => "typescript",
            Self::Tsx => "tsx",
            Self::Go => "go",
            Self::Java => "java",
        }
    }
}

#[derive(Debug, Deserialize)]
struct AstGrepJsonMatch {
    text: String,
    range: AstGrepRange,
}

#[derive(Debug, Deserialize)]
struct AstGrepRange {
    start: AstGrepPoint,
    end: AstGrepPoint,
}

#[derive(Debug, Deserialize)]
struct AstGrepPoint {
    line: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct StructuralCandidate {
    start_line: usize,
    end_line: usize,
}

impl StructuralCandidate {
    fn from_match(
        entry: AstGrepJsonMatch,
        file_lines: &[String],
        primary_term: &str,
    ) -> Option<Self> {
        if file_lines.is_empty() {
            return None;
        }

        let normalized_text = normalise_text(&entry.text).to_lowercase();
        if !normalized_text.contains(primary_term) {
            return None;
        }

        let start_line = entry
            .range
            .start
            .line
            .min(file_lines.len().saturating_sub(1));
        let end_line = entry.range.end.line.min(file_lines.len().saturating_sub(1));
        if start_line > end_line {
            return None;
        }

        Some(Self {
            start_line,
            end_line,
        })
    }

    fn end_exclusive(self, file_len: usize) -> usize {
        self.end_line.saturating_add(1).min(file_len)
    }
}

pub(crate) async fn resolve_semantic_match(
    source_path: &Path,
    display_path: &str,
    original_lines: &[String],
    chunk: &PatchChunk,
    old_segment: Vec<String>,
    new_segment: Vec<String>,
) -> Result<SemanticMatch, PatchError> {
    let anchor = chunk
        .change_context()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| PatchError::SemanticResolutionFailed {
            path: display_path.to_string(),
            anchor: String::new(),
            reason: "missing semantic @@ anchor".to_string(),
        })?;

    let language = SupportedLanguage::from_path(source_path).ok_or_else(|| {
        PatchError::SemanticResolutionFailed {
            path: display_path.to_string(),
            anchor: anchor.to_string(),
            reason: "unsupported language for semantic fallback".to_string(),
        }
    })?;

    let ast_grep = resolve_ast_grep_binary(display_path, anchor)?;
    let primary_term =
        semantic_anchor_term(anchor).ok_or_else(|| PatchError::SemanticResolutionFailed {
            path: display_path.to_string(),
            anchor: anchor.to_string(),
            reason: "anchor does not contain a usable symbol name".to_string(),
        })?;

    let candidates = collect_candidates(
        &ast_grep,
        language,
        source_path,
        original_lines,
        &primary_term,
        display_path,
        anchor,
    )
    .await?;

    if candidates.is_empty() {
        return Err(PatchError::SemanticResolutionFailed {
            path: display_path.to_string(),
            anchor: anchor.to_string(),
            reason: "no structural candidates matched the semantic anchor".to_string(),
        });
    }

    let mut resolved = BTreeMap::new();

    for candidate in candidates {
        let end_exclusive = candidate.end_exclusive(original_lines.len());
        if candidate.start_line >= end_exclusive {
            continue;
        }

        let mut candidate_old = old_segment.clone();
        let mut candidate_new = new_segment.clone();
        let candidate_lines = &original_lines[candidate.start_line..end_exclusive];

        if let Some(local_start) = seek_segment(
            candidate_lines,
            &mut candidate_old,
            &mut candidate_new,
            0,
            chunk.is_end_of_file(),
        ) {
            resolved
                .entry(candidate.start_line + local_start)
                .or_insert(SemanticMatch {
                    start_idx: candidate.start_line + local_start,
                    old_segment: candidate_old,
                    new_segment: candidate_new,
                });
        }
    }

    match resolved.len() {
        0 => Err(PatchError::SemanticResolutionFailed {
            path: display_path.to_string(),
            anchor: anchor.to_string(),
            reason: "anchor resolved to structural candidates, but removal/context lines were not found safely inside them".to_string(),
        }),
        1 => Ok(resolved.into_values().next().expect("single semantic match must exist")),
        candidate_count => Err(PatchError::SemanticAmbiguous {
            path: display_path.to_string(),
            anchor: anchor.to_string(),
            candidate_count,
        }),
    }
}

fn resolve_ast_grep_binary(display_path: &str, anchor: &str) -> Result<PathBuf, PatchError> {
    match AST_GREP_OVERRIDE
        .lock()
        .expect("ast-grep override mutex must not be poisoned")
        .clone()
    {
        AstGrepBinaryOverride::System => {}
        AstGrepBinaryOverride::Missing => {
            return Err(PatchError::SemanticResolutionFailed {
                path: display_path.to_string(),
                anchor: anchor.to_string(),
                reason:
                    "ast-grep is not available; install `sg`/`ast-grep` or use exact context lines"
                        .to_string(),
            });
        }
        AstGrepBinaryOverride::Path(path) => return Ok(path),
    }

    if let Some(path) = std::env::var_os(AST_GREP_BIN_ENV).filter(|value| !value.is_empty()) {
        let path = PathBuf::from(path);
        if path.exists() {
            return Ok(path);
        }
    }

    which::which("sg")
        .or_else(|_| which::which("ast-grep"))
        .map_err(|_| PatchError::SemanticResolutionFailed {
            path: display_path.to_string(),
            anchor: anchor.to_string(),
            reason: "ast-grep is not available; install `sg`/`ast-grep` or use exact context lines"
                .to_string(),
        })
}

async fn collect_candidates(
    ast_grep: &Path,
    language: SupportedLanguage,
    source_path: &Path,
    original_lines: &[String],
    primary_term: &str,
    display_path: &str,
    anchor: &str,
) -> Result<Vec<StructuralCandidate>, PatchError> {
    let output = Command::new(ast_grep)
        .arg("run")
        .arg("--pattern")
        .arg("$A")
        .arg("--lang")
        .arg(language.ast_grep_lang())
        .arg("--json=stream")
        .arg(source_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .map_err(|err| PatchError::SemanticResolutionFailed {
            path: display_path.to_string(),
            anchor: anchor.to_string(),
            reason: format!("failed to run ast-grep: {err}"),
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let detail = stderr.trim();
        return Err(PatchError::SemanticResolutionFailed {
            path: display_path.to_string(),
            anchor: anchor.to_string(),
            reason: if detail.is_empty() {
                "ast-grep failed to analyze the file".to_string()
            } else {
                format!("ast-grep failed to analyze the file: {detail}")
            },
        });
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut candidates = Vec::new();

    for line in stdout.lines().filter(|line| !line.trim().is_empty()) {
        let parsed = serde_json::from_str::<AstGrepJsonMatch>(line).map_err(|err| {
            PatchError::SemanticResolutionFailed {
                path: display_path.to_string(),
                anchor: anchor.to_string(),
                reason: format!("failed to parse ast-grep output: {err}"),
            }
        })?;

        if let Some(candidate) =
            StructuralCandidate::from_match(parsed, original_lines, primary_term)
        {
            candidates.push(candidate);
        }
    }

    candidates.sort_unstable();
    candidates.dedup();
    Ok(candidates)
}

pub(crate) fn semantic_anchor_term(anchor: &str) -> Option<String> {
    const STOPWORDS: &[&str] = &[
        "async",
        "class",
        "const",
        "crate",
        "def",
        "enum",
        "export",
        "fn",
        "for",
        "function",
        "impl",
        "interface",
        "let",
        "mod",
        "module",
        "private",
        "protected",
        "pub",
        "public",
        "self",
        "super",
        "static",
        "struct",
        "trait",
        "type",
        "void",
        "where",
    ];

    IDENTIFIER_RE
        .find_iter(anchor)
        .map(|m| m.as_str().to_ascii_lowercase())
        .find(|term| !STOPWORDS.iter().any(|stopword| term == stopword))
}

#[cfg(test)]
mod tests {
    use super::semantic_anchor_term;

    #[test]
    fn semantic_anchor_term_skips_rust_visibility_noise() {
        assert_eq!(
            semantic_anchor_term("pub(crate) fn second() -> usize"),
            Some("second".to_string())
        );
    }

    #[test]
    fn semantic_anchor_term_prefers_symbol_name_after_keywords() {
        assert_eq!(
            semantic_anchor_term("impl ToolDefinition for ProviderTool"),
            Some("tooldefinition".to_string())
        );
    }
}
